//! Equipos de agentes (spec 019): `equipos/<id>/equipo.json`, escrito con
//! `fs_atomico` y con un bloqueo para toda lectura-modificación-escritura.

use std::fs;
use std::path::PathBuf;

use ea_ajustes::{validar_equipo, Ajustes};
use ea_store::{EventOutcome, NewEvent};

use crate::bus::{ahora_rfc3339, nuevo_id, Bus};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::tipos::{Equipo, EstadoEquipo, Miembro, Orden, RolEquipo};

/// Lo que hace falta para despertar un equipo; el id, la fecha y el estado
/// los pone el sistema.
#[derive(Debug, Clone)]
pub struct NuevoEquipo {
    pub titulo: String,
    pub objetivo: String,
    pub worktree_base: String,
    pub miembros: Vec<Miembro>,
    pub creado_por: String,
}

impl Bus {
    fn dir_equipo(&self, id: &str) -> Resultado<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ErrorBus::Argumentos(format!("id de equipo inválido: {id:?}")));
        }
        Ok(self.raiz.join("equipos").join(id))
    }

    /// `flock` exclusivo sobre `equipos/.equipos.lock`: ordena entre procesos
    /// toda escritura de equipos (crear, dormir, anotar un trabajo) y la
    /// comprobación de «un agente, un equipo despierto». Quien solo lee no lo
    /// necesita. No es reentrante.
    fn bloquear_equipos(&self) -> Resultado<fs::File> {
        let dir = self.raiz.join("equipos");
        fs::create_dir_all(&dir)?;
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".equipos.lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Despierta un equipo: valida los miembros con [`validar_equipo`], rechaza
    /// a un agente que ya esté en otro equipo despierto, lo escribe y apunta
    /// `equipo_despertado` en el diario. La disponibilidad de los binarios la
    /// comprueba quien llama (`ea_ajustes::comprobar_disponibles`).
    pub fn crear_equipo(&self, nuevo: NuevoEquipo, ajustes: &Ajustes) -> Resultado<Equipo> {
        let texto = |v: &str, que: &str| -> Resultado<String> {
            let v = v.trim();
            if v.is_empty() || v.chars().any(char::is_control) {
                return Err(ErrorBus::Argumentos(format!("falta {que} (o lleva caracteres de control)")));
            }
            Ok(v.to_string())
        };
        let titulo = texto(&nuevo.titulo, "el título del equipo")?;
        let objetivo = texto(&nuevo.objetivo, "el objetivo del equipo")?;
        let worktree_base = texto(&nuevo.worktree_base, "el worktree base del equipo")?;
        validar_equipo(&nuevo.miembros, ajustes).map_err(ErrorBus::Argumentos)?;
        let _bloqueo = self.bloquear_equipos()?;
        // Todo ejecutor despierto necesita un trabajador: los ejecutores
        // distintos de los equipos despiertos y del nuevo caben en `paralelo`.
        // También el agente de `roles.ejecutor`, que siempre tiene trabajador.
        let mut ejecutores: Vec<String> = vec![ajustes.agente_de(ea_ajustes::Rol::Ejecutor).0.to_string()];
        ejecutores.extend(nuevo.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor).map(|m| m.agente.clone()));
        for otro in self.listar_equipos()?.iter().filter(|e| e.estado == EstadoEquipo::Despierto) {
            ejecutores.extend(otro.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor).map(|m| m.agente.clone()));
        }
        ejecutores.sort();
        ejecutores.dedup();
        let tope = ajustes.orquestador.paralelo as usize;
        if ejecutores.len() > tope {
            return Err(ErrorBus::Argumentos(format!(
                "habría {} ejecutores despiertos ({}; el agente de roles.ejecutor siempre cuenta) y orquestador.paralelo es {tope}: \
                 sube orquestador.paralelo (`ea ajustes establecer orquestador.paralelo {}`), duerme otro equipo o, con \
                 paralelo 1, usa como único ejecutor al de roles.ejecutor",
                ejecutores.len(),
                ejecutores.join(", "),
                ejecutores.len()
            )));
        }
        for otro in self.listar_equipos()?.iter().filter(|e| e.estado == EstadoEquipo::Despierto) {
            if let Some(m) = nuevo.miembros.iter().find(|m| otro.miembro(&m.agente).is_some()) {
                return Err(ErrorBus::Argumentos(format!(
                    "{} ya está en el equipo despierto {} ({}): un agente solo está en un equipo a la vez",
                    m.agente, otro.id, otro.titulo
                )));
            }
        }
        let equipo = Equipo {
            id: nuevo_id(),
            titulo,
            objetivo,
            worktree_base,
            miembros: nuevo.miembros,
            estado: EstadoEquipo::Despierto,
            creado_en: ahora_rfc3339(),
            creado_por: nuevo.creado_por,
            trabajos: vec![],
        };
        escribir_json(&self.dir_equipo(&equipo.id)?.join("equipo.json"), &equipo)?;
        self.diario_equipo("equipo_despertado", &equipo);
        Ok(equipo)
    }

    pub fn leer_equipo(&self, id: &str) -> Resultado<Equipo> {
        match fs::read(self.dir_equipo(id)?.join("equipo.json")) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorBus::NoExiste(format!("equipo {id}"))),
            Err(e) => Err(e.into()),
        }
    }

    /// Todos los equipos, del más antiguo al más reciente. Salta los ficheros
    /// ilegibles.
    pub fn listar_equipos(&self) -> Resultado<Vec<Equipo>> {
        let mut ids: Vec<String> = fs::read_dir(self.raiz.join("equipos"))?
            .flatten()
            .filter(|e| e.path().join("equipo.json").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        ids.sort();
        Ok(ids.iter().filter_map(|id| self.leer_equipo(id).ok()).collect())
    }

    /// Duerme el equipo. Lo pendiente en la cola de cada uno de sus
    /// ejecutores vuelve a la cola del rol `modelo-local` (motivo «equipo
    /// dormido»), para que no quede huérfano: lo toma cualquier ejecutor.
    /// Dormir uno dormido no hace nada.
    pub fn dormir_equipo(&self, id: &str) -> Resultado<Equipo> {
        let _bloqueo = self.bloquear_equipos()?;
        let mut equipo = self.leer_equipo(id)?;
        if equipo.estado == EstadoEquipo::Dormido {
            return Ok(equipo);
        }
        equipo.estado = EstadoEquipo::Dormido;
        escribir_json(&self.dir_equipo(id)?.join("equipo.json"), &equipo)?;
        for m in equipo.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor) {
            let dir = self.dir_cola_agente(&m.agente)?;
            while let Some(trabajo) = crate::bus::tomar_en(&dir, &|_| true)? {
                self.encolar(ea_ajustes::Rol::Ejecutor, &trabajo, "equipo dormido")?;
            }
        }
        // Las sesiones de planificadores y revisores siguen abiertas en su
        // terminal, pero ya no están en el equipo: se les quita la presencia.
        // La de los ejecutores es del trabajador del orquestador, que la quita
        // al reconciliar.
        for m in equipo.miembros.iter().filter(|m| m.rol != RolEquipo::Ejecutor) {
            if self.presencia_de(&m.agente)?.is_some_and(|p| p.equipo.as_deref() == Some(id)) {
                self.quitar_presencia(&m.agente)?;
            }
        }
        self.diario_equipo("equipo_dormido", &equipo);
        Ok(equipo)
    }

    /// Añade el trabajo a la lista del equipo (sin repetir).
    pub fn anotar_trabajo_en_equipo(&self, equipo: &str, trabajo: &str) -> Resultado<()> {
        let _bloqueo = self.bloquear_equipos()?;
        let mut e = self.leer_equipo(equipo)?;
        if !e.trabajos.iter().any(|t| t == trabajo) {
            e.trabajos.push(trabajo.into());
            escribir_json(&self.dir_equipo(equipo)?.join("equipo.json"), &e)?;
        }
        Ok(())
    }

    /// El agente al que va la cola de un trabajo reasignable: `asignado_a`
    /// solo si su equipo sigue despierto y el agente sigue en él.
    pub(crate) fn asignado_vigente(&self, o: &Orden) -> Option<String> {
        let (a, e) = (o.asignado_a.as_ref()?, o.equipo.as_ref()?);
        let equipo = self.leer_equipo(e).ok()?;
        (equipo.estado == EstadoEquipo::Despierto && equipo.con_rol(a, RolEquipo::Ejecutor)).then(|| a.clone())
    }

    /// Las comprobaciones de `crear_trabajo` que dependen del equipo (RF-004).
    /// Devuelve el equipo, si la orden trae uno.
    pub(crate) fn validar_equipo_de_orden(&self, o: &Orden) -> Resultado<Option<Equipo>> {
        let Some(id) = &o.equipo else {
            if o.asignado_a.is_some() || o.revisor.is_some() {
                return Err(ErrorBus::Argumentos("asignado_a y revisor necesitan un equipo".into()));
            }
            return Ok(None);
        };
        let equipo = self.leer_equipo(id)?;
        if equipo.estado != EstadoEquipo::Despierto {
            return Err(ErrorBus::Argumentos(format!("el equipo {id} está dormido")));
        }
        if let Some(a) = &o.asignado_a
            && !equipo.con_rol(a, RolEquipo::Ejecutor)
        {
            return Err(ErrorBus::Argumentos(format!("{a} no es un ejecutor del equipo {id}")));
        }
        if let Some(r) = &o.revisor {
            if !(equipo.con_rol(r, RolEquipo::Revisor) || equipo.con_rol(r, RolEquipo::Planificador)) {
                return Err(ErrorBus::Argumentos(format!("{r} no es un revisor ni un planificador del equipo {id}")));
            }
            if o.asignado_a.as_deref() == Some(r.as_str()) {
                return Err(ErrorBus::Argumentos(format!("{r} no puede revisar lo que ejecuta él mismo")));
            }
        }
        Ok(Some(equipo))
    }

    /// El diario es secundario: un fallo de SQLite no deshace el equipo.
    fn diario_equipo(&self, accion: &str, e: &Equipo) {
        let Some(s) = &self.store else { return };
        let miembros: Vec<String> = e.miembros.iter().map(|m| format!("{}:{}", m.agente, m.rol.as_str())).collect();
        let _ = s.append_event(NewEvent {
            actor: self.actor.clone(),
            action: accion.into(),
            trabajo: None,
            outcome: Some(EventOutcome::Ok),
            duration_ms: None,
            detail: Some(serde_json::json!({ "equipo": e.id, "titulo": e.titulo, "miembros": miembros }).to_string()),
        });
    }
}
