//! API del bus: colas por agente, estado de los trabajos y su historial.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ea_store::{EventOutcome, FilaFase, FilaRutina, FilaTrabajo, NewEvent, Store};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::config::ConfigOrquestador;
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::{escribir_atomico, escribir_json};
use crate::rutina::{Rutina, MAX_DEFINICION};
use crate::skills::{pide_skills, ConfigSkills};
use crate::tipos::{
    transicion, EjecutadoPor, Estado, EstadoTrabajo, Evento, FaseInforme, Informe, Modo, Orden, ResultadoInforme, Rol, RolEquipo,
};

pub const ENV_ESTADO: &str = "EA_ESTADO_DIR";
static CONTADOR_ID: AtomicU64 = AtomicU64::new(0);

/// `$EA_ESTADO_DIR`, si no `rutas.estado` de los ajustes (por defecto
/// `~/.local/state/ecosistema-asistente/`).
pub fn raiz_estado() -> PathBuf {
    if let Ok(v) = std::env::var(ENV_ESTADO) && !v.trim().is_empty() {
        return PathBuf::from(v.trim());
    }
    ea_ajustes::cargar().rutas.estado
}

pub(crate) fn ahora_rfc3339() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default()
}

/// Id ordenable por tiempo: `20261014T193000-0003ab12`. El contador evita que
/// dos ids del mismo segundo se ordenen al azar (las colas son FIFO).
pub(crate) fn nuevo_id() -> String {
    let t = OffsetDateTime::now_utc();
    let n = CONTADOR_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let h = ea_plataforma::fs::fnv64(format!("{}-{n}-{nanos}", std::process::id()).as_bytes()) & 0xffff;
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}-{:04x}{h:04x}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        n & 0xffff
    )
}

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

/// Entrada de una cola.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntradaCola {
    pub id: String,
    pub trabajo: String,
    pub motivo: String,
    pub encolado_en: String,
}

pub struct Bus {
    pub(crate) raiz: PathBuf,
    pub(crate) store: Option<Store>,
    pub(crate) actor: String,
    /// Configuración de skills con la que se validan las órdenes al crear.
    /// `None`: la de los ajustes efectivos, leída solo si la orden pide skills.
    skills: Option<ConfigSkills>,
}

impl Bus {
    /// Sobre la raíz de estado estándar.
    pub fn abrir() -> Resultado<Self> {
        Self::abrir_en(&raiz_estado())
    }

    pub fn abrir_en(raiz: &Path) -> Resultado<Self> {
        for rol in Rol::TODOS {
            fs::create_dir_all(raiz.join("colas").join(rol.as_str()))?;
        }
        for d in ["trabajos", "semaforos", "equipos", "colas/agentes", "colas/revision"] {
            fs::create_dir_all(raiz.join(d))?;
        }
        Ok(Self { raiz: raiz.to_path_buf(), store: None, actor: "orquestador".into(), skills: None })
    }

    /// Con historial y diario en SQLite.
    pub fn con_store(mut self, store: Store) -> Self {
        self.store = Some(store);
        self
    }

    /// Actor con el que se registran los eventos del diario.
    pub fn con_actor(mut self, actor: &str) -> Self {
        self.actor = actor.into();
        self
    }

    /// Valida las skills de las órdenes con esta configuración en vez de con
    /// la de los ajustes (pruebas o un llamador que ya la tiene cargada).
    pub fn con_skills(mut self, skills: ConfigSkills) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    fn dir_cola(&self, a: Rol) -> PathBuf {
        self.raiz.join("colas").join(a.as_str())
    }

    /// `colas/agentes/<agente>/`. El id tiene que ser del catálogo: así no se
    /// abre ninguna ruta arbitraria (spec 019, RF-005).
    pub(crate) fn dir_cola_agente(&self, agente: &str) -> Resultado<PathBuf> {
        if ea_ajustes::ficha(agente).is_none() {
            return Err(ErrorBus::Argumentos(format!("{agente:?} no es un agente del catálogo")));
        }
        Ok(self.raiz.join("colas").join("agentes").join(agente))
    }

    /// `colas/revision/<agente>/`: las entregas que un revisor tiene que mirar.
    /// Va aparte de `colas/agentes/<agente>/` (órdenes asignadas) para que un
    /// ejecutor que también sea revisor no tome una revisión como si fuera
    /// trabajo (spec 019, RF-006).
    fn dir_cola_revision(&self, agente: &str) -> Resultado<PathBuf> {
        if ea_ajustes::ficha(agente).is_none() {
            return Err(ErrorBus::Argumentos(format!("{agente:?} no es un agente del catálogo")));
        }
        Ok(self.raiz.join("colas").join("revision").join(agente))
    }

    /// Todas las colas: la de cada rol, la de cada agente y la de revisión de
    /// cada agente que tenga carpeta.
    fn dirs_colas(&self) -> Vec<(Option<String>, PathBuf)> {
        let mut v: Vec<(Option<String>, PathBuf)> = Rol::TODOS.iter().map(|r| (None, self.dir_cola(*r))).collect();
        for sub in ["agentes", "revision"] {
            if let Ok(rd) = fs::read_dir(self.raiz.join("colas").join(sub)) {
                let mut agentes: Vec<(String, PathBuf)> = rd
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok().map(|n| (n, e.path())))
                    .filter(|(n, _)| ea_ajustes::ficha(n).is_some())
                    .collect();
                agentes.sort();
                v.extend(agentes.into_iter().map(|(n, p)| (Some(n), p)));
            }
        }
        v
    }

    pub(crate) fn dir_trabajo(&self, id: &str) -> Resultado<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ErrorBus::Argumentos(format!("id de trabajo inválido: {id:?}")));
        }
        Ok(self.raiz.join("trabajos").join(id))
    }

    // --- trabajos ----------------------------------------------------------

    /// Escribe `orden.json` y `estado.json` y encola en `modelo-local`. Si la
    /// orden trae el id vacío se le asigna uno nuevo. Antes de escribir nada
    /// valida la estructura (`validar_estructura`: worktree absoluto, fases
    /// 1..N), los criterios y las skills (llave, existencia y topes), de modo
    /// que cualquier punto de entrada (CLI, ea-web, rutinas) lo hereda. Que el
    /// worktree exista y sea un repo con commits lo comprueban la CLI y la web
    /// con [`crate::validar_worktree`] (aquí no se ejecuta git).
    ///
    /// Con `equipo` (spec 019, RF-004) comprueba además que el equipo existe y
    /// está despierto, que `asignado_a` es un ejecutor suyo, que `revisor` es
    /// un revisor o planificador suyo y distinto de `asignado_a`. Con
    /// `asignado_a` la orden entra en la cola de ese agente; si no, en la del
    /// rol ejecutor. Los planificadores del equipo tienen la llave de las
    /// skills; los revisores no. Esa validación, el encolado y la anotación
    /// en el equipo van bajo el bloqueo de los equipos: un `dormir_equipo` a
    /// la vez no deja la orden en la cola de un agente que ya nadie atiende.
    ///
    /// El espejo en SQLite (filas de `trabajos` y `trabajo_fases`) se escribe
    /// después de encolar y es de mejor esfuerzo, como el diario: si falla,
    /// el trabajo queda creado y en su cola.
    pub fn crear_trabajo(&self, mut orden: Orden) -> Resultado<String> {
        validar_estructura(&orden)?;
        validar_criterios(&orden)?;
        let bloqueo_equipos = if orden.equipo.is_some() { Some(self.bloquear_equipos()?) } else { None };
        let equipo = self.validar_equipo_de_orden(&orden)?;
        if pide_skills(&orden) {
            let mut cfg = match &self.skills {
                Some(cfg) => cfg.clone(),
                None => ConfigOrquestador::cargar().skills,
            };
            if equipo.as_ref().is_some_and(|e| e.con_rol(&orden.creada_por, RolEquipo::Planificador)) {
                cfg.pueden_elegir.push(orden.creada_por.clone());
            }
            cfg.validar_orden(&orden)?;
        }
        if orden.id.is_empty() {
            orden.id = nuevo_id();
        }
        if orden.creada_en.is_empty() {
            orden.creada_en = ahora_rfc3339();
        }
        let id = orden.id.clone();
        let dir = self.dir_trabajo(&id)?;
        if dir.join("orden.json").exists() {
            return Err(ErrorBus::Argumentos(format!("el trabajo {id} ya existe")));
        }
        escribir_json(&dir.join("orden.json"), &orden)?;
        let estado = Estado {
            estado: EstadoTrabajo::EnCola,
            fase_actual: orden.fases.first().map(|f| f.numero),
            iteracion: 0,
            actualizado_en: ahora_rfc3339(),
            detalle: None,
            ejecutado_por: None,
        };
        escribir_json(&dir.join("estado.json"), &estado)?;
        self.historial(&id, "crear", &estado)?;
        if let Some(s) = &self.store {
            self.diario(s, &id, "bus_crear", true, serde_json::json!({ "modo": orden.modo.as_str(), "titulo": orden.titulo }));
        }
        match &orden.asignado_a {
            Some(agente) => self.encolar_agente(agente, &id, "orden nueva")?,
            None => self.encolar(Rol::Ejecutor, &id, "orden nueva")?,
        };
        if let Some(e) = &orden.equipo {
            self.anotar_trabajo_en_equipo_bloqueado(e, &id)?;
        }
        drop(bloqueo_equipos);
        if let Some(s) = &self.store {
            espejo(
                &id,
                "insertar el trabajo",
                s.insertar_trabajo(&FilaTrabajo {
                    id: id.clone(),
                    modo: orden.modo.as_str().into(),
                    titulo: orden.titulo.clone(),
                    worktree: orden.worktree.clone(),
                    estado: estado.estado.as_str().into(),
                    creado_en: orden.creada_en.clone(),
                    actualizado_en: estado.actualizado_en.clone(),
                    creado_por: orden.creada_por.clone(),
                    rutina_slug: orden.rutina.as_ref().map(|r| r.slug.clone()),
                    rutina_version: orden.rutina.as_ref().map(|r| i64::from(r.version)),
                }),
            );
            for f in &orden.fases {
                let fila = FilaFase {
                    trabajo_id: id.clone(),
                    numero: i64::from(f.numero),
                    titulo: f.titulo.clone(),
                    estado: "pendiente".into(),
                    informe: None,
                    cerrado_en: None,
                };
                espejo(&id, "guardar la fase", s.guardar_fase(&fila));
            }
        }
        Ok(id)
    }

    pub fn leer_orden(&self, id: &str) -> Resultado<Orden> {
        self.leer(id, "orden.json")
    }

    pub fn leer_estado(&self, id: &str) -> Resultado<Estado> {
        self.leer(id, "estado.json")
    }

    pub fn leer_informe(&self, id: &str) -> Resultado<Option<Informe>> {
        let p = self.dir_trabajo(id)?.join("informe.json");
        match fs::read(&p) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn escribir_informe(&self, id: &str, informe: &Informe) -> Resultado<()> {
        let dir = self.dir_trabajo(id)?;
        if !dir.exists() {
            return Err(ErrorBus::NoExiste(id.into()));
        }
        escribir_json(&dir.join("informe.json"), informe)
    }

    /// Ids de todos los trabajos, del más antiguo al más reciente.
    pub fn listar_trabajos(&self) -> Resultado<Vec<String>> {
        let mut v: Vec<String> = fs::read_dir(self.raiz.join("trabajos"))?
            .flatten()
            .filter(|e| e.path().join("orden.json").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        v.sort();
        Ok(v)
    }

    fn leer<T: serde::de::DeserializeOwned>(&self, id: &str, fichero: &str) -> Resultado<T> {
        let p = self.dir_trabajo(id)?.join(fichero);
        match fs::read(&p) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorBus::NoExiste(id.into())),
            Err(e) => Err(e.into()),
        }
    }

    // --- colas -------------------------------------------------------------

    pub fn encolar(&self, agente: Rol, trabajo: &str, motivo: &str) -> Resultado<String> {
        encolar_en(&self.dir_cola(agente), trabajo, motivo)
    }

    /// Como [`Bus::encolar`] en la cola de un agente concreto
    /// (`colas/agentes/<agente>/`, spec 019, RF-005).
    pub fn encolar_agente(&self, agente: &str, trabajo: &str, motivo: &str) -> Resultado<String> {
        encolar_en(&self.dir_cola_agente(agente)?, trabajo, motivo)
    }

    /// Encola una entrega para que la revise `agente` (`colas/revision/<agente>/`).
    pub fn encolar_revision(&self, agente: &str, trabajo: &str, motivo: &str) -> Resultado<String> {
        encolar_en(&self.dir_cola_revision(agente)?, trabajo, motivo)
    }

    /// Entregas pendientes de revisar por `agente`; vacía si no hay ninguna.
    pub fn listar_cola_revision(&self, agente: &str) -> Resultado<Vec<EntradaCola>> {
        listar_en(&self.dir_cola_revision(agente)?)
    }

    /// Entradas visibles de la cola, de la más antigua a la más nueva.
    pub fn listar_cola(&self, agente: Rol) -> Resultado<Vec<EntradaCola>> {
        listar_en(&self.dir_cola(agente))
    }

    /// Entradas de la cola de un agente; vacía si nunca se le encoló nada.
    pub fn listar_cola_agente(&self, agente: &str) -> Resultado<Vec<EntradaCola>> {
        listar_en(&self.dir_cola_agente(agente)?)
    }

    /// Toma el trabajo más antiguo de la cola. Reclama la entrada
    /// renombrándola (atómico): de dos procesos, solo uno lo consigue. No
    /// cambia el estado del trabajo; para eso, [`Bus::aplicar`] con `Tomar`.
    pub fn tomar(&self, agente: Rol) -> Resultado<Option<String>> {
        tomar_en(&self.dir_cola(agente), &|_| true)
    }

    /// Lo que toma un trabajador de un agente ejecutor: primero la cola de su
    /// agente y, si está vacía, la del rol `modelo-local`. Mismo reclamo por
    /// `rename` que [`Bus::tomar`] (spec 019, RF-005).
    pub fn tomar_para(&self, agente: &str) -> Resultado<Option<String>> {
        self.tomar_para_con(agente, &|_| true)
    }

    /// Como [`Bus::tomar_para`], pero de la cola de rol solo toma las órdenes
    /// que pasan `filtro`; las demás se quedan donde estaban (se lee la orden
    /// antes de reclamar la entrada). La cola propia del agente se toma
    /// entera. Una orden ilegible pasa el filtro, para que se descarte.
    pub fn tomar_para_con(&self, agente: &str, filtro: &dyn Fn(&Orden) -> bool) -> Resultado<Option<String>> {
        if let Some(t) = tomar_en(&self.dir_cola_agente(agente)?, &|_| true)? {
            return Ok(Some(t));
        }
        tomar_en(&self.dir_cola(Rol::Ejecutor), &|id| self.leer_orden(id).map(|o| filtro(&o)).unwrap_or(true))
    }

    // --- transiciones ------------------------------------------------------

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
                            self.encolar_revision(r, id, motivo)?;
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
    fn bloquear_estado(&self, id: &str) -> Resultado<fs::File> {
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

    /// Quita de TODAS las colas las entradas de un trabajo.
    pub fn retirar_de_colas(&self, id: &str) -> Resultado<usize> {
        let mut n = 0;
        for (_, dir) in self.dirs_colas() {
            for e in fs::read_dir(dir)?.flatten() {
                let p = e.path();
                let nombre = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if nombre.starts_with('.') || !nombre.ends_with(".json") {
                    continue;
                }
                let de_este = fs::read(&p)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<EntradaCola>(&b).ok())
                    .is_some_and(|en| en.trabajo == id);
                if de_este && fs::remove_file(&p).is_ok() {
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Anota en SQLite el resultado de una fase de obra (`hecha`, `bloqueada`…).
    pub fn registrar_fase(&self, id: &str, numero: u32, estado: &str, informe: &serde_json::Value) -> Resultado<()> {
        let Some(s) = &self.store else { return Ok(()) };
        if let Some(mut f) = s.listar_fases(id)?.into_iter().find(|f| f.numero == i64::from(numero)) {
            f.estado = estado.into();
            f.informe = Some(informe.to_string());
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
            use std::io::Write;
            let ruta = self.dir_trabajo(id)?.join("paquete-correccion.md");
            let mut f = fs::OpenOptions::new().create(true).append(true).open(ruta)?;
            writeln!(f, "\n## Contexto del usuario\n\n{}", contexto.trim())?;
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
        let ruta_paquete = self.dir_trabajo(id)?.join("paquete-correccion.md");
        self.aplicar(id, Evento::Entregar(informe))?;
        let (evento, entrega, que) = match paquete {
            Some(texto) => {
                if let Err(e) = escribir_atomico(&ruta_paquete, texto.as_bytes()) {
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

    /// Devuelve a su cola un trabajo `EnCola` cuya entrada se reclamó y no se
    /// llegó a tomar (el worktree estaba ocupado, se reformuló, se perdió el
    /// reclamo): la del agente asignado si su equipo sigue despierto y él en
    /// él; si no, la del rol ejecutor. Elegir y encolar van bajo el bloqueo de
    /// los equipos, para que no entre en la cola de un agente cuyo equipo se
    /// está durmiendo. Devuelve el id de la entrada.
    pub fn devolver(&self, id: &str, motivo: &str) -> Resultado<String> {
        let _bloqueo = self.bloquear_equipos()?;
        self.devolver_bloqueado(id, motivo)
    }

    /// [`Bus::devolver`] para quien ya tiene el bloqueo de los equipos. Una
    /// orden ilegible va a la cola del rol, donde el ejecutor la descarta.
    pub(crate) fn devolver_bloqueado(&self, id: &str, motivo: &str) -> Resultado<String> {
        match self.leer_orden(id).ok().and_then(|o| self.asignado_vigente(&o)) {
            Some(agente) => self.encolar_agente(&agente, id, motivo),
            None => self.encolar(Rol::Ejecutor, id, motivo),
        }
    }

    // --- rutinas -----------------------------------------------------------

    fn store_rutinas(&self) -> Resultado<&Store> {
        self.store.as_ref().ok_or_else(|| ErrorBus::Argumentos("las rutinas necesitan la base de datos".into()))
    }

    /// Valida y registra una rutina con [`comprobar_registro`] (tamaño,
    /// formato y versión mayor que la última registrada). Guarda la versión
    /// en SQLite y copia el fichero a `dir` (`<slug>.toml`). Si la rutina
    /// estaba retirada, registrarla la vuelve a poner en uso: la copia vieja
    /// de la papelera se borra (sus versiones siguen en SQLite).
    pub fn registrar_rutina(&self, texto: &str, dir: &Path) -> Resultado<Rutina> {
        let s = self.store_rutinas()?;
        let (r, _) = comprobar_registro(s, texto)?;
        s.guardar_rutina(&FilaRutina {
            slug: r.rutina.slug.clone(),
            version: i64::from(r.rutina.version),
            definicion: texto.into(),
            creada_en: ahora_rfc3339(),
            creada_por: self.actor.clone(),
        })?;
        fs::create_dir_all(dir)?;
        escribir_atomico(&dir.join(format!("{}.toml", r.rutina.slug)), texto.as_bytes())?;
        crate::papelera::quitar_de_papelera(dir, &r.rutina.slug)?;
        Ok(r)
    }

    /// Última versión registrada de una rutina.
    pub fn rutina(&self, slug: &str) -> Resultado<Rutina> {
        let fila = self
            .store_rutinas()?
            .ultima_rutina(slug)?
            .ok_or_else(|| ErrorBus::Argumentos(format!("la rutina {slug:?} no está registrada")))?;
        Rutina::parsear(&fila.definicion)
    }

    /// Texto TOML de una versión concreta.
    pub fn definicion_rutina(&self, slug: &str, version: u32) -> Option<String> {
        self.store.as_ref()?.obtener_rutina(slug, i64::from(version)).ok().flatten().map(|f| f.definicion)
    }

    /// Última versión de cada rutina registrada.
    pub fn listar_rutinas(&self) -> Resultado<Vec<Rutina>> {
        self.store_rutinas()?.listar_rutinas()?.iter().map(|f| Rutina::parsear(&f.definicion)).collect()
    }

    /// Crea un trabajo de modo rutina con la última versión registrada. Una
    /// rutina tiene como mucho una ejecución activa (en cola o ejecutándose).
    /// Comprueba, por este orden: que está registrada, que las entradas valen
    /// ([`Rutina::a_orden`]) y, bajo un `flock` de `<raiz>/.rutinas.lock` que
    /// dura hasta crear el trabajo, que no hay otra en curso
    /// ([`ErrorBus::EnCurso`]). Es la base de [`Bus::lanzar_rutina_en`] (CLI,
    /// timer y web) y de la aplicación de `ea_encolar_rutina`.
    pub fn lanzar_rutina(&self, slug: &str, entradas: &[(String, String)]) -> Resultado<String> {
        let orden = self.rutina(slug)?.a_orden(entradas)?;
        let _bloqueo = self.bloquear_rutinas()?;
        if !self.trabajos_activos_de_rutina(slug)?.is_empty() {
            return Err(ErrorBus::EnCurso(slug.into()));
        }
        self.crear_trabajo(orden)
    }

    /// `flock` exclusivo sobre `<raiz>/.rutinas.lock`: ordena entre procesos
    /// mirar si una rutina tiene una ejecución en curso y crear la nueva. Se
    /// toma antes que el de los equipos (que `crear_trabajo` pediría con una
    /// orden de equipo) y nunca con otro en la mano. No es reentrante.
    fn bloquear_rutinas(&self) -> Resultado<fs::File> {
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.raiz.join(".rutinas.lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Como [`Bus::lanzar_rutina`], pero antes mira el fichero en
    /// `rutinas_dir` ([`crate::comprobar_lanzable`]): una rutina retirada o
    /// sin fichero no se lanza. Es lo que usan la CLI (y con ella el timer)
    /// y la web; `ea_encolar_rutina` lo comprueba al simular.
    pub fn lanzar_rutina_en(&self, dir: &Path, slug: &str, entradas: &[(String, String)]) -> Resultado<String> {
        crate::papelera::comprobar_lanzable(dir, slug)?;
        self.lanzar_rutina(slug, entradas)
    }

    /// Trabajos de una rutina que aún no han terminado.
    pub fn trabajos_activos_de_rutina(&self, slug: &str) -> Resultado<Vec<String>> {
        let mut v = Vec::new();
        for id in self.listar_trabajos()? {
            let (Ok(o), Ok(e)) = (self.leer_orden(&id), self.leer_estado(&id)) else { continue };
            if o.rutina.is_some_and(|r| r.slug == slug) && matches!(e.estado, EstadoTrabajo::EnCola | EstadoTrabajo::Ejecutando) {
                v.push(id);
            }
        }
        Ok(v)
    }

    /// Trabajos de una rutina, del más reciente al más antiguo, con su estado.
    pub fn trabajos_de_rutina(&self, slug: &str) -> Resultado<Vec<(String, Estado)>> {
        let mut v = Vec::new();
        for id in self.listar_trabajos()?.into_iter().rev() {
            let (Ok(o), Ok(e)) = (self.leer_orden(&id), self.leer_estado(&id)) else { continue };
            if o.rutina.is_some_and(|r| r.slug == slug) {
                v.push((id, e));
            }
        }
        Ok(v)
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
        let p = self.dir_trabajo(id)?.join("fases-informe.json");
        if let Ok(b) = fs::read(&p) {
            let mut v: Vec<FaseInforme> = serde_json::from_slice(&b).unwrap_or_default();
            v.retain(|f| f.numero < desde);
            escribir_json(&p, &v)?;
        }
        // El espejo en SQLite, de mejor esfuerzo: la orden y el informe de
        // fases ya están escritos.
        if let Some(s) = &self.store {
            let filas = espejo(id, "leer las fases", s.listar_fases(id)).unwrap_or_default();
            for mut f in filas.into_iter().filter(|f| f.numero >= i64::from(desde)) {
                if f.estado != "pendiente" || f.informe.is_some() {
                    f.estado = "pendiente".into();
                    f.informe = None;
                    f.cerrado_en = None;
                    espejo(id, "guardar la fase", s.guardar_fase(&f));
                }
            }
        }
        Ok(())
    }

    /// Para llamar al arrancar (es idempotente). Devuelve a la cola los
    /// reclamos `.tomada-*` huérfanos cuyo trabajo sigue `EnCola`; reconcilia
    /// por estado lo que sigue `EnCola` sin entrada en ninguna cola de las
    /// que se toman órdenes (la del rol ejecutor y las de los agentes: un
    /// reclamo que se borró y no se llegó a tomar porque el proceso cayó
    /// entre medias), que vuelve a la suya con [`Bus::devolver`] (motivo
    /// «recuperado»); y entrega con [`Bus::entregar`], como Bloqueado
    /// («interrumpido»), lo que se quedó en `Ejecutando` (una rutina queda en
    /// espera con su paquete). Devuelve los ids recuperados.
    pub fn recuperar(&self) -> Resultado<Vec<String>> {
        let mut recuperados = Vec::new();
        for (_, dir) in self.dirs_colas() {
            for e in fs::read_dir(&dir)?.flatten() {
                let p = e.path();
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !n.starts_with('.') || !n.contains(".tomada-") {
                    continue;
                }
                let entrada: Option<EntradaCola> = fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if let Some(en) = entrada {
                    // En colas/revision lo pendiente es una entrega, no una orden.
                    let esperado =
                        if dir.starts_with(self.raiz.join("colas").join("revision")) { EstadoTrabajo::EnRevision } else { EstadoTrabajo::EnCola };
                    let en_cola = self.leer_estado(&en.trabajo).is_ok_and(|s| s.estado == esperado);
                    let ya_en_cola = listar_en(&dir)?.iter().any(|x| x.trabajo == en.trabajo);
                    if en_cola && !ya_en_cola {
                        encolar_en(&dir, &en.trabajo, "recuperado")?;
                        recuperados.push(en.trabajo);
                    }
                }
                let _ = fs::remove_file(&p);
            }
        }
        // Entradas de trabajos ya cerrados o cancelados (restos de antes de
        // que Cerrar y Cancelar limpiaran las colas).
        for id in self.listar_trabajos()? {
            if self.leer_estado(&id).is_ok_and(|s| matches!(s.estado, EstadoTrabajo::Cerrado | EstadoTrabajo::Cancelado)) {
                self.retirar_de_colas(&id)?;
            }
        }
        let en_cola = |id: &str| self.leer_estado(id).is_ok_and(|s| s.estado == EstadoTrabajo::EnCola);
        let encolados = self.encolados_para_ejecutar()?;
        for id in self.listar_trabajos()? {
            if !en_cola(&id) || encolados.contains(&id) {
                continue;
            }
            // Se vuelve a mirar bajo el bloqueo del trabajo: un `Reformular`
            // a la vez lo deja en cola y lo encola sin soltarlo.
            let _bloqueo = self.bloquear_estado(&id)?;
            if en_cola(&id) && !self.encolados_para_ejecutar()?.contains(&id) {
                self.devolver(&id, "recuperado")?;
                recuperados.push(id);
            }
        }
        for id in self.listar_trabajos()? {
            if self.leer_estado(&id).is_ok_and(|s| s.estado == EstadoTrabajo::Ejecutando) {
                let inf = Informe { motivo: Some("interrumpido".into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) };
                self.entregar(&id, inf)?;
                recuperados.push(id);
            }
        }
        Ok(recuperados)
    }

    /// Los trabajos con alguna entrada en una cola de las que se toman
    /// órdenes: la del rol ejecutor y las de los agentes.
    fn encolados_para_ejecutar(&self) -> Resultado<std::collections::HashSet<String>> {
        let agentes = self.raiz.join("colas").join("agentes");
        let mut v = std::collections::HashSet::new();
        for (_, dir) in self.dirs_colas() {
            if dir == self.dir_cola(Rol::Ejecutor) || dir.starts_with(&agentes) {
                v.extend(listar_en(&dir)?.into_iter().map(|e| e.trabajo));
            }
        }
        Ok(v)
    }

    fn sincronizar_fases(&self, s: &Store, id: &str, evento: &Evento, nuevo: &Estado) -> Resultado<()> {
        let fases = s.listar_fases(id)?;
        match evento {
            Evento::Entregar(inf) => {
                let json = serde_json::to_string(inf)?;
                for mut f in fases.into_iter().filter(|f| inf.fases_hechas > 0 && f.numero <= i64::from(inf.fases_hechas)) {
                    if f.estado != "hecha" {
                        f.estado = "hecha".into();
                        f.informe = Some(json.clone());
                        f.cerrado_en = Some(nuevo.actualizado_en.clone());
                        s.guardar_fase(&f)?;
                    }
                }
            }
            Evento::Cancelar => {
                for mut f in fases.into_iter().filter(|f| f.estado == "pendiente") {
                    f.estado = "cancelada".into();
                    f.cerrado_en = Some(nuevo.actualizado_en.clone());
                    s.guardar_fase(&f)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn historial(&self, id: &str, evento: &str, estado: &Estado) -> Resultado<()> {
        use std::io::Write;
        let mut linea = serde_json::json!({
            "ts": estado.actualizado_en,
            "evento": evento,
            "estado": estado.estado.as_str(),
            "iteracion": estado.iteracion,
        });
        if let Some(por) = &estado.ejecutado_por {
            linea["ejecutado_por"] = serde_json::json!(por);
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.dir_trabajo(id)?.join("historial.jsonl"))?;
        writeln!(f, "{linea}")?;
        f.sync_all()?;
        Ok(())
    }

    /// El diario es secundario: un fallo de SQLite no deshace el bus.
    fn diario(&self, s: &Store, id: &str, accion: &str, ok: bool, detalle: serde_json::Value) {
        let mut detalle = detalle;
        detalle["trabajo"] = serde_json::Value::String(id.into());
        let _ = s.append_event(NewEvent {
            actor: self.actor.clone(),
            action: accion.into(),
            // En la columna, no solo en el detalle: así `--trabajo` y el
            // filtro del diario encuentran los `bus_*` del trabajo.
            trabajo: Some(id.into()),
            outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
            duration_ms: None,
            detail: Some(detalle.to_string()),
        });
    }
}

/// El espejo en SQLite es secundario, como el diario: un fallo no deshace el
/// bus (que ya está escrito) ni hace fallar la operación; queda en el
/// registro.
fn espejo<T, E: std::fmt::Display>(id: &str, que: &str, r: Result<T, E>) -> Option<T> {
    r.map_err(|e| tracing::warn!(%id, error = %e, "espejo SQLite: no se pudo {que}")).ok()
}

fn encolar_en(dir: &Path, trabajo: &str, motivo: &str) -> Resultado<String> {
    let entrada = EntradaCola { id: nuevo_id(), trabajo: trabajo.into(), motivo: motivo.into(), encolado_en: ahora_rfc3339() };
    escribir_json(&dir.join(format!("{}.json", entrada.id)), &entrada)?;
    Ok(entrada.id)
}

fn listar_en(dir: &Path) -> Resultado<Vec<EntradaCola>> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut nombres: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !n.starts_with('.') && n.ends_with(".json")
        })
        .collect();
    nombres.sort();
    Ok(nombres.iter().filter_map(|p| fs::read(p).ok()).filter_map(|b| serde_json::from_slice(&b).ok()).collect())
}

pub(crate) fn tomar_en(dir: &Path, admite: &dyn Fn(&str) -> bool) -> Resultado<Option<String>> {
    fn listar(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut v: Vec<PathBuf> = fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !n.starts_with('.') && n.ends_with(".json")
            })
            .collect();
        v.sort();
        Ok(v)
    }
    // Sondeo barato: con la cola vacía (o sin directorio) no se crea ni se
    // abre el cerrojo.
    match listar(dir) {
        Ok(v) if v.is_empty() => return Ok(None),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    // Un solo reclamante a la vez en esta cola (entre hilos y procesos): el
    // `rename` basta en Unix, pero en Windows dos reclamos simultáneos del
    // mismo fichero no son fiables.
    let _cerrojo = {
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".tomar.lock"))?;
        f.lock()?;
        f
    };
    // Dentro del cerrojo se vuelve a listar: otro pudo llevarse las candidatas.
    let candidatas = match listar(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    for p in candidatas {
        // Con filtro, se mira la entrada antes de reclamarla; si no pasa, se queda.
        if let Some(e) = fs::read(&p).ok().and_then(|b| serde_json::from_slice::<EntradaCola>(&b).ok())
            && !admite(&e.trabajo)
        {
            continue;
        }
        let nombre = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        // Único por reclamo: dos hilos del mismo proceso no pueden pisarse.
        static RECLAMOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = RECLAMOS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reclamada = dir.join(format!(".{nombre}.tomada-{}-{n}", std::process::id()));
        if fs::rename(&p, &reclamada).is_err() {
            continue; // otro proceso se adelantó
        }
        let entrada: Option<EntradaCola> = fs::read(&reclamada).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let _ = fs::remove_file(&reclamada);
        if let Some(e) = entrada {
            return Ok(Some(e.trabajo));
        }
    }
    Ok(None)
}

/// Lo que comprueba registrar una rutina, sin escribir nada: la política
/// única de la CLI, la web y `ea_registrar_rutina`. La definición no pasa de
/// [`MAX_DEFINICION`] bytes, es una rutina válida ([`Rutina::parsear`]) y su
/// versión es mayor que la última registrada. Devuelve la rutina y la última
/// versión registrada, si la hay.
pub fn comprobar_registro(store: &Store, texto: &str) -> Resultado<(Rutina, Option<FilaRutina>)> {
    if texto.len() > MAX_DEFINICION {
        return Err(ErrorBus::Argumentos(format!(
            "la definición es demasiado larga ({} bytes; como mucho {MAX_DEFINICION})",
            texto.len()
        )));
    }
    let r = Rutina::parsear(texto)?;
    let anterior = store.ultima_rutina(&r.rutina.slug)?;
    if let Some(ult) = &anterior && i64::from(r.rutina.version) <= ult.version {
        return Err(ErrorBus::Argumentos(format!(
            "la rutina {} ya está en la versión {}; la nueva debe ser mayor (es la {})",
            r.rutina.slug, ult.version, r.rutina.version
        )));
    }
    Ok((r, anterior))
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
            return Err(ErrorBus::Argumentos(format!(
                "solo una obra con fases se reformula desde una fase; este trabajo es una {}",
                orden.modo.as_str()
            )));
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

/// Lo que toda orden cumple, la cree quien la cree, sin mirar el disco ni
/// ejecutar git: el worktree es una ruta absoluta (una relativa se
/// resolvería contra el directorio del proceso que la ejecuta, el `$HOME`
/// del servicio) y las fases de una obra se numeran 1, 2, …, N en el orden en
/// que van (el ejecutor reanuda por número, y con 10/20/30 se saltaba fases).
fn validar_estructura(o: &Orden) -> Resultado<()> {
    if !Path::new(&o.worktree).is_absolute() {
        return Err(ErrorBus::Argumentos("'worktree' debe ser una ruta absoluta".into()));
    }
    if o.modo == Modo::Obra && o.fases.iter().enumerate().any(|(i, f)| f.numero as usize != i + 1) {
        let llegan: Vec<String> = o.fases.iter().map(|f| f.numero.to_string()).collect();
        return Err(ErrorBus::Argumentos(format!(
            "las fases deben numerarse 1, 2, …, N, en el orden en que van (esta obra trae {})",
            llegan.join(", ")
        )));
    }
    Ok(())
}

/// El criterio de éxito es obligatorio: sin él, nadie puede decir que algo
/// está hecho. Misión y rutina lo llevan en la orden; la obra, en cada fase
/// (o en la orden, si no tiene fases).
fn validar_criterios(o: &Orden) -> Resultado<()> {
    let vacio = |c: &Option<String>| c.as_deref().is_none_or(|c| c.trim().is_empty());
    match o.modo {
        Modo::Obra if !o.fases.is_empty() => {
            if let Some(f) = o.fases.iter().find(|f| f.criterio.trim().is_empty()) {
                return Err(ErrorBus::Argumentos(format!("la fase {} no tiene criterio", f.numero)));
            }
        }
        _ if vacio(&o.criterio_exito) => {
            return Err(ErrorBus::Argumentos("falta criterio_exito: es obligatorio".into()));
        }
        _ => {}
    }
    Ok(())
}
