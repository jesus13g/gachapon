//! La máquina de estados de los trabajos aplicada al bus: cada transición
//! se valida, se escribe y se registra bajo el bloqueo del trabajo. Aquí van
//! también la entrega según el modo, la confirmación de una rutina en espera
//! y la reformulación de lo que está en revisión.

use std::fs;

use ea_store::Store;

use super::{Bus, Cola, ahora_rfc3339, espejo};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::tipos::{EjecutadoPor, Estado, EstadoFase, EstadoTrabajo, Evento, Informe, InformeFase, Modo, Orden, ResultadoInforme, Rol, transicion};

/// Lo que hizo [`Bus::entregar`] con el trabajo, según su modo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entrega {
    /// Misión u obra: en revisión, en la cola del planificador.
    EnRevision,
    /// Rutina que no pasa: en espera, con su paquete de corrección.
    EnEspera,
    /// Rutina que pasa: cerrada sola.
    Cerrado,
}

impl Bus {
    /// Valida la transición, escribe `estado.json`, añade una línea a
    /// `historial.jsonl` y registra el evento en SQLite (si hay `Store`).
    /// `Entregar` guarda además el informe.
    ///
    /// La lectura, la validación y la escritura van bajo el bloqueo del
    /// trabajo ([`Bus::bloquear_estado`]): una cancelación que llegue a la vez
    /// que el ejecutor escribe no se pierde.
    pub fn aplicar(&self, id: &str, evento: Evento) -> Resultado<Estado> {
        let _bloqueo = self.bloquear_estado(id)?;
        self.aplicar_bloqueado(id, evento, None, None)
    }

    /// `Tomar` anotando quién lo ejecuta (spec 017, RF-008): el agente y el
    /// modelo quedan en `estado.json` y en el historial, y la entrega los
    /// copia al informe. El trabajo conserva ese ejecutor hasta que se
    /// vuelva a tomar, aunque cambie `roles.ejecutor` mientras corre.
    pub fn tomar_con(&self, id: &str, por: EjecutadoPor) -> Resultado<Estado> {
        let _bloqueo = self.bloquear_estado(id)?;
        self.aplicar_bloqueado(id, Evento::Tomar, None, Some(por))
    }

    /// [`Bus::aplicar`] para quien ya tiene el bloqueo del trabajo. Con
    /// `Reformular`, `fase` es la fase de obra desde la que se rehace.
    fn aplicar_bloqueado(&self, id: &str, evento: Evento, fase: Option<u32>, por: Option<EjecutadoPor>) -> Resultado<Estado> {
        let actual = self.leer_estado(id)?;
        let nuevo_estado = transicion(actual.estado, &evento)?;
        let mut nuevo = Estado { estado: nuevo_estado, actualizado_en: ahora_rfc3339(), ..actual };
        match &evento {
            Evento::Tomar if por.is_some() => nuevo.ejecutado_por = por,
            Evento::Entregar(inf) => {
                // El informe dice quién lo hizo aunque el ejecutor no lo anote.
                let mut inf = inf.clone();
                if inf.ejecutado_por.is_none() {
                    inf.ejecutado_por = nuevo.ejecutado_por.clone();
                }
                // El revisor es quien lo ejecutó: no se le encola la revisión
                // (más abajo) y el informe lo deja dicho.
                if let (Some(r), Some(por)) = (self.leer_orden(id)?.revisor, &inf.ejecutado_por)
                    && r == por.agente
                {
                    inf.aviso = Some(format!("el revisor {r} es quien ejecutó el trabajo: sin revisión independiente"));
                }
                self.escribir_informe(id, &inf)?;
                if inf.fases_hechas > 0 {
                    nuevo.fase_actual = Some(inf.fases_hechas + 1);
                }
                nuevo.detalle = inf.motivo.clone();
            }
            Evento::Reformular if fase.is_some() => nuevo.fase_actual = fase,
            _ => {}
        }
        let dir = self.dir_trabajo(id)?;
        escribir_json(&dir.join("estado.json"), &nuevo)?;
        self.historial(id, evento.nombre(), &nuevo)?;
        match &evento {
            Evento::Entregar(inf) => {
                let motivo = inf.resultado.as_str();
                // Una rutina vuelve a Claude Code solo si falla y el usuario lo
                // confirma (`Confirmar`); su informe no pasa por la cola.
                let orden = self.leer_orden(id)?;
                if orden.modo != Modo::Rutina {
                    self.encolar(Rol::Planificador, id, motivo)?;
                    // El revisor del equipo la recibe además, salvo que sea
                    // quien la ejecutó: nadie se revisa a sí mismo (RF-006).
                    if let Some(r) = &orden.revisor {
                        let ejecutor = nuevo.ejecutado_por.as_ref().map(|p| p.agente.as_str());
                        if ejecutor != Some(r.as_str()) {
                            self.encolar(Cola::Revision(r), id, motivo)?;
                        }
                    }
                }
            }
            Evento::Confirmar => {
                self.encolar(Rol::Planificador, id, "corrección de rutina")?;
            }
            Evento::Cerrar | Evento::Cancelar => {
                self.retirar_de_colas(id)?;
            }
            Evento::Reformular => {
                // Sin entradas rancias (p. ej. el «terminado» en claude-code si
                // se reformula sin haberlo tomado, como hace el panel web).
                self.retirar_de_colas(id)?;
                self.devolver(id, "reformulado")?;
            }
            _ => {}
        }
        // El espejo en SQLite es de mejor esfuerzo: el bus ya está escrito.
        if let Some(s) = &self.store {
            espejo(id, "actualizar el estado", s.actualizar_estado_trabajo(id, nuevo.estado.as_str(), &nuevo.actualizado_en));
            espejo(id, "sincronizar las fases", self.sincronizar_fases(s, id, &evento, &nuevo));
            let mut detalle = serde_json::json!({ "estado": nuevo.estado.as_str() });
            if let Some(n) = fase {
                detalle["fase"] = n.into();
            }
            self.diario(s, id, &format!("bus_{}", evento.nombre()), true, detalle);
        }
        Ok(nuevo)
    }

    /// `flock` exclusivo sobre `trabajos/<id>/.estado.lock`, que se suelta al
    /// soltar el fichero devuelto. Ordena entre procesos (CLI, ea-web, el
    /// ejecutor) toda lectura-modificación-escritura de `estado.json`; la
    /// escritura en sí sigue siendo atómica (`fs_atomico`), así que quien solo
    /// lee no necesita el bloqueo. No es reentrante: quien lo tiene no debe
    /// llamar a otra función que lo pida.
    ///
    /// Orden de adquisición cuando hacen falta varios: primero este, luego el
    /// de los equipos ([`Bus::devolver`] al reformular o al recuperar) y por
    /// último el cerrojo de una cola (`dormir_equipo` la vacía con el de los
    /// equipos tomado). Nunca al revés: nadie pide el de un trabajo con el de
    /// los equipos o el de una cola en la mano.
    ///
    /// El candado de instancia del orquestador (`<raiz>/orquestador.lock`,
    /// `ea_orchestrator::trabajadores::Instancia`) no se cruza con estos: solo
    /// lo pide el orquestador, una vez al arrancar y sin otro en la mano, y lo
    /// tiene toda su vida; los de aquí se toman siempre dentro de él.
    pub(super) fn bloquear_estado(&self, id: &str) -> Resultado<fs::File> {
        let dir = self.dir_trabajo(id)?;
        if !dir.is_dir() {
            return Err(ErrorBus::NoExiste(id.into()));
        }
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".estado.lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Sube el contador de iteraciones (una por llamada al agente). Solo si
    /// el trabajo sigue `Ejecutando`: si no (p. ej. se canceló), no escribe
    /// nada y devuelve `None`, para no devolver a la vida lo cancelado.
    pub fn anotar_iteracion(&self, id: &str) -> Resultado<Option<u32>> {
        let _bloqueo = self.bloquear_estado(id)?;
        let mut e = self.leer_estado(id)?;
        if e.estado != EstadoTrabajo::Ejecutando {
            return Ok(None);
        }
        e.iteracion += 1;
        e.actualizado_en = ahora_rfc3339();
        escribir_json(&self.dir_trabajo(id)?.join("estado.json"), &e)?;
        Ok(Some(e.iteracion))
    }

    /// `true` si el trabajo está `Cancelado` (un fallo al leer cuenta como
    /// no cancelado: el ejecutor sigue y la entrega dirá lo que haya).
    pub fn esta_cancelado(&self, id: &str) -> bool {
        self.leer_estado(id).is_ok_and(|e| e.estado == EstadoTrabajo::Cancelado)
    }

    /// El ejecutor dejó de trabajar en un trabajo cancelado mientras corría:
    /// una línea `cancelado_en_curso` en `historial.jsonl` y el evento
    /// `bus_cancelado_en_curso` en el diario, con la iteración y la fase en
    /// que se cortó. No cambia el estado (ya es `Cancelado`).
    pub fn anotar_cancelacion_en_curso(&self, id: &str, iteracion: u32, fase: Option<u32>) -> Resultado<()> {
        let estado = self.leer_estado(id)?;
        self.historial(id, "cancelado_en_curso", &Estado { actualizado_en: ahora_rfc3339(), iteracion, ..estado })?;
        if let Some(s) = &self.store {
            self.diario(s, id, "bus_cancelado_en_curso", true, serde_json::json!({ "iteracion": iteracion, "fase": fase }));
        }
        Ok(())
    }

    /// Anota en SQLite el resultado de una fase de obra (`hecha`, `bloqueada`…).
    pub fn registrar_fase(&self, id: &str, numero: u32, estado: EstadoFase, informe: &InformeFase) -> Resultado<()> {
        let Some(s) = &self.store else { return Ok(()) };
        if let Some(mut f) = s.listar_fases(id)?.into_iter().find(|f| f.numero == i64::from(numero)) {
            f.estado = estado.as_str().into();
            f.informe = Some(serde_json::to_string(informe)?);
            f.cerrado_en = Some(ahora_rfc3339());
            s.guardar_fase(&f)?;
        }
        Ok(())
    }

    /// Confirma el paquete de corrección de una rutina en espera: añade el
    /// contexto del usuario al paquete y lo encola en `claude-code`. Validar,
    /// añadir y aplicar van bajo el bloqueo del trabajo: de dos
    /// confirmaciones a la vez, solo una añade su contexto.
    pub fn confirmar(&self, id: &str, contexto: &str) -> Resultado<Estado> {
        let _bloqueo = self.bloquear_estado(id)?;
        let estado = self.leer_estado(id)?;
        transicion(estado.estado, &Evento::Confirmar)?;
        if !contexto.trim().is_empty() {
            self.artefactos(id)?.anadir_al_paquete_correccion(&format!("\n## Contexto del usuario\n\n{}\n", contexto.trim()))?;
        }
        self.aplicar_bloqueado(id, Evento::Confirmar, None, None)
    }

    /// Entrega el informe de un trabajo que se está ejecutando, según su
    /// modo (lo usan el ejecutor y [`Bus::recuperar`]):
    /// - misión y obra: `Entregar`, que lo deja en revisión y en la cola del
    ///   planificador (y en la del revisor del equipo);
    /// - rutina que pasa: `Entregar` y `Cerrar`, sin pasar por ninguna cola;
    /// - rutina que no pasa: `paquete-correccion.md`, `Entregar` y
    ///   `PonerEnEspera`, hasta que el usuario lo confirme ([`Bus::confirmar`]).
    ///
    /// Un fallo de `Entregar` se devuelve tal cual: si el trabajo se canceló
    /// mientras tanto (p. ej. mientras corría el criterio), no es un fallo, y
    /// quien llama lo distingue con [`Bus::esta_cancelado`]. El segundo paso
    /// de una rutina no deshace el primero: si falla, queda en el registro
    /// (salvo que se cancelara entre medias) y el trabajo sigue en revisión, a
    /// la vista. Devuelve la política aplicada.
    pub fn entregar(&self, id: &str, informe: Informe) -> Resultado<Entrega> {
        let orden = self.leer_orden(id)?;
        if orden.modo != Modo::Rutina {
            self.aplicar(id, Evento::Entregar(informe))?;
            return Ok(Entrega::EnRevision);
        }
        let paquete = (informe.resultado != ResultadoInforme::Terminado).then(|| self.paquete_correccion(id, &orden, &informe));
        let artefactos = self.artefactos(id)?;
        self.aplicar(id, Evento::Entregar(informe))?;
        let (evento, entrega, que) = match paquete {
            Some(texto) => {
                if let Err(e) = artefactos.guardar_paquete_correccion(&texto) {
                    tracing::error!(%id, error = %e, "no se pudo escribir el paquete de corrección");
                }
                (Evento::PonerEnEspera, Entrega::EnEspera, "no se pudo poner en espera")
            }
            None => (Evento::Cerrar, Entrega::Cerrado, "no se pudo cerrar la rutina"),
        };
        if let Err(e) = self.aplicar(id, evento)
            && !self.esta_cancelado(id)
        {
            tracing::error!(%id, error = %e, "{que}");
        }
        Ok(entrega)
    }

    /// Lo que necesita quien corrija una rutina que falló: la definición, la
    /// ejecución tal como se le dio al agente, el criterio y la evidencia.
    fn paquete_correccion(&self, id: &str, orden: &Orden, inf: &Informe) -> String {
        let (slug, version) = orden.rutina.as_ref().map(|r| (r.slug.clone(), r.version)).unwrap_or_default();
        let definicion = self.definicion_rutina(&slug, version).unwrap_or_else(|| "(definición no disponible)".into());
        format!(
            "# Paquete de corrección: rutina {slug} (versión {version})\n\n\
             Trabajo: {id}\n\
             Motivo: {}\n\n\
             ## Definición\n\n```toml\n{}\n```\n\n\
             ## Ejecución (entradas y pasos tal como se le dieron al agente)\n\n{}\n\n\
             ## Criterio de éxito\n\n`{}`\n\n\
             ## Salida y evidencia del error\n\n```\n{}\n```\n\n\
             ## Qué se espera\n\n\
             Corrige la definición de la rutina y sube su versión (`ea rutina registrar <fichero>`).\n",
            inf.motivo.as_deref().unwrap_or("el criterio no pasa"),
            definicion.trim(),
            orden.objetivo.trim(),
            orden.criterio_exito.as_deref().unwrap_or(""),
            inf.evidencia.trim(),
        )
    }

    /// Reformula un trabajo en revisión: añade las notas de la revisión a
    /// `orden.json` (`notas_revision`) y aplica `Reformular`, que lo vuelve
    /// a encolar en `modelo-local`.
    ///
    /// En una obra, `fase` es la fase desde la que se rehace; sin ella, la
    /// pendiente (`fase_actual`; en una obra completa, la última). Esa fase y
    /// las siguientes vuelven a quedar pendientes: `fase_actual` apunta a
    /// ella y se recortan `fases-informe.json` y las filas de fase de SQLite.
    /// Una fase que no existe en la obra, o una fase en una misión o rutina,
    /// es un error de argumentos y no escribe nada. Todo va bajo el bloqueo
    /// del trabajo.
    pub fn reformular(&self, id: &str, notas: &str, fase: Option<u32>) -> Resultado<Estado> {
        let _bloqueo = self.bloquear_estado(id)?;
        let estado = self.leer_estado(id)?;
        transicion(estado.estado, &Evento::Reformular)?; // validar antes de tocar la orden
        let mut orden = self.leer_orden(id)?;
        let rehacer = fase_a_rehacer(&orden, &estado, fase)?;
        if !notas.trim().is_empty() {
            orden.notas_revision.push(notas.trim().into());
            escribir_json(&self.dir_trabajo(id)?.join("orden.json"), &orden)?;
        }
        if let Some(n) = rehacer {
            self.recortar_fases(id, n)?;
        }
        self.aplicar_bloqueado(id, Evento::Reformular, rehacer, None)
    }

    /// Deja pendientes la fase `desde` y las siguientes: las quita de
    /// `fases-informe.json` (si existe) y devuelve sus filas de SQLite a
    /// `pendiente`, sin informe (que llevaba el árbol de la fase).
    fn recortar_fases(&self, id: &str, desde: u32) -> Resultado<()> {
        let artefactos = self.artefactos(id)?;
        if let Some(mut v) = artefactos.fases_informe() {
            v.retain(|f| f.numero < desde);
            artefactos.guardar_fases_informe(&v)?;
        }
        // El espejo en SQLite, de mejor esfuerzo: la orden y el informe de
        // fases ya están escritos.
        if let Some(s) = &self.store {
            let filas = espejo(id, "leer las fases", s.listar_fases(id)).unwrap_or_default();
            let pendiente = EstadoFase::Pendiente.as_str();
            for mut f in filas.into_iter().filter(|f| f.numero >= i64::from(desde)) {
                if f.estado != pendiente || f.informe.is_some() {
                    f.estado = pendiente.into();
                    f.informe = None;
                    f.cerrado_en = None;
                    espejo(id, "guardar la fase", s.guardar_fase(&f));
                }
            }
        }
        Ok(())
    }

    fn sincronizar_fases(&self, s: &Store, id: &str, evento: &Evento, nuevo: &Estado) -> Resultado<()> {
        let fases = s.listar_fases(id)?;
        match evento {
            Evento::Entregar(inf) => {
                let json = serde_json::to_string(inf)?;
                let hecha = EstadoFase::Hecha.as_str();
                for mut f in fases.into_iter().filter(|f| inf.fases_hechas > 0 && f.numero <= i64::from(inf.fases_hechas)) {
                    if f.estado != hecha {
                        f.estado = hecha.into();
                        f.informe = Some(json.clone());
                        f.cerrado_en = Some(nuevo.actualizado_en.clone());
                        s.guardar_fase(&f)?;
                    }
                }
            }
            Evento::Cancelar => {
                for mut f in fases.into_iter().filter(|f| f.estado == EstadoFase::Pendiente.as_str()) {
                    f.estado = EstadoFase::Cancelada.as_str().into();
                    f.cerrado_en = Some(nuevo.actualizado_en.clone());
                    s.guardar_fase(&f)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// La fase de obra desde la que se rehace al reformular. Solo una obra con
/// fases tiene una: la pedida, si existe en la obra; si no se pide, la
/// pendiente (`fase_actual`), y en una obra completa (`fase_actual` pasada
/// de la última) la última. Pedir fase en una misión o rutina, o una que la
/// obra no tiene (0, más allá de la última), es un error de argumentos.
fn fase_a_rehacer(orden: &Orden, estado: &Estado, pedida: Option<u32>) -> Resultado<Option<u32>> {
    let numeros: Vec<u32> = orden.fases.iter().map(|f| f.numero).collect();
    let extremos = numeros.iter().min().zip(numeros.iter().max());
    let (primera, ultima) = match (orden.modo, extremos) {
        (Modo::Obra, Some((&p, &u))) => (p, u),
        _ if pedida.is_some() => {
            return Err(ErrorBus::Argumentos(format!("solo una obra con fases se reformula desde una fase; este trabajo es una {}", orden.modo.as_str())));
        }
        _ => return Ok(None),
    };
    match pedida {
        Some(n) if !numeros.contains(&n) => {
            Err(ErrorBus::Argumentos(format!("la fase {n} está fuera de rango: la obra va de la fase {primera} a la {ultima}")))
        }
        Some(n) => Ok(Some(n)),
        None => Ok(Some(estado.fase_actual.filter(|n| numeros.contains(n)).unwrap_or(ultima))),
    }
}
