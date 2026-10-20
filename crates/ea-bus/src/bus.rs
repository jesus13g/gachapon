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
use crate::fs_atomico::{escribir_json, fnv};
use crate::rutina::Rutina;
use crate::skills::{pide_skills, ConfigSkills};
use crate::tipos::{
    transicion, Agente, Estado, EstadoTrabajo, Evento, Informe, Modo, Orden, ResultadoInforme,
};

pub const ENV_ESTADO: &str = "EA_ESTADO_DIR";
static CONTADOR_ID: AtomicU64 = AtomicU64::new(0);

/// `$EA_ESTADO_DIR`, si no `rutas.estado` de los ajustes (por defecto
/// `~/.local/state/ecosistema-asistente/`).
pub fn raiz_estado() -> PathBuf {
    if let Ok(v) = std::env::var(ENV_ESTADO) {
        if !v.trim().is_empty() {
            return PathBuf::from(v.trim());
        }
    }
    ea_ajustes::cargar().rutas.estado
}

pub(crate) fn ahora_rfc3339() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default()
}

/// Id ordenable por tiempo: `20261014T193000-0003ab12`. El contador evita que
/// dos ids del mismo segundo se ordenen al azar (las colas son FIFO).
fn nuevo_id() -> String {
    let t = OffsetDateTime::now_utc();
    let n = CONTADOR_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let h = fnv(&format!("{}-{n}-{nanos}", std::process::id())) & 0xffff;
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

/// Entrada de una cola.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntradaCola {
    pub id: String,
    pub trabajo: String,
    pub motivo: String,
    pub encolado_en: String,
}

pub struct Bus {
    raiz: PathBuf,
    store: Option<Store>,
    actor: String,
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
        for d in ["colas/modelo-local", "colas/claude-code", "trabajos", "semaforos"] {
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

    fn dir_cola(&self, a: Agente) -> PathBuf {
        self.raiz.join("colas").join(a.as_str())
    }

    fn dir_trabajo(&self, id: &str) -> Resultado<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ErrorBus::Argumentos(format!("id de trabajo inválido: {id:?}")));
        }
        Ok(self.raiz.join("trabajos").join(id))
    }

    // --- trabajos ----------------------------------------------------------

    /// Escribe `orden.json` y `estado.json` y encola en `modelo-local`. Si la
    /// orden trae el id vacío se le asigna uno nuevo. Antes de escribir nada
    /// valida los criterios y las skills (llave, existencia y topes), de modo
    /// que cualquier punto de entrada (CLI, ea-web, rutinas) lo hereda.
    pub fn crear_trabajo(&self, mut orden: Orden) -> Resultado<String> {
        validar_criterios(&orden)?;
        if pide_skills(&orden) {
            match &self.skills {
                Some(cfg) => cfg.validar_orden(&orden)?,
                None => ConfigOrquestador::cargar().skills.validar_orden(&orden)?,
            }
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
        };
        escribir_json(&dir.join("estado.json"), &estado)?;
        self.historial(&id, "crear", &estado)?;
        if let Some(s) = &self.store {
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
            })?;
            for f in &orden.fases {
                s.guardar_fase(&FilaFase {
                    trabajo_id: id.clone(),
                    numero: i64::from(f.numero),
                    titulo: f.titulo.clone(),
                    estado: "pendiente".into(),
                    informe: None,
                    cerrado_en: None,
                })?;
            }
            self.diario(s, &id, "bus_crear", true, serde_json::json!({ "modo": orden.modo.as_str(), "titulo": orden.titulo }));
        }
        self.encolar(Agente::ModeloLocal, &id, "orden nueva")?;
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

    pub fn encolar(&self, agente: Agente, trabajo: &str, motivo: &str) -> Resultado<String> {
        let entrada = EntradaCola {
            id: nuevo_id(),
            trabajo: trabajo.into(),
            motivo: motivo.into(),
            encolado_en: ahora_rfc3339(),
        };
        escribir_json(&self.dir_cola(agente).join(format!("{}.json", entrada.id)), &entrada)?;
        Ok(entrada.id)
    }

    /// Entradas visibles de la cola, de la más antigua a la más nueva.
    pub fn listar_cola(&self, agente: Agente) -> Resultado<Vec<EntradaCola>> {
        let mut nombres: Vec<PathBuf> = fs::read_dir(self.dir_cola(agente))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !n.starts_with('.') && n.ends_with(".json")
            })
            .collect();
        nombres.sort();
        Ok(nombres
            .iter()
            .filter_map(|p| fs::read(p).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect())
    }

    /// Toma el trabajo más antiguo de la cola. Reclama la entrada
    /// renombrándola (atómico): de dos procesos, solo uno lo consigue. No
    /// cambia el estado del trabajo; para eso, [`Bus::aplicar`] con `Tomar`.
    pub fn tomar(&self, agente: Agente) -> Resultado<Option<String>> {
        let dir = self.dir_cola(agente);
        let mut candidatas: Vec<PathBuf> = fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !n.starts_with('.') && n.ends_with(".json")
            })
            .collect();
        candidatas.sort();
        for p in candidatas {
            let nombre = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let reclamada = dir.join(format!(".{nombre}.tomada-{}", std::process::id()));
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

    // --- transiciones ------------------------------------------------------

    /// Valida la transición, escribe `estado.json`, añade una línea a
    /// `historial.jsonl` y registra el evento en SQLite (si hay `Store`).
    /// `Entregar` guarda además el informe.
    pub fn aplicar(&self, id: &str, evento: Evento) -> Resultado<Estado> {
        let actual = self.leer_estado(id)?;
        let nuevo_estado = transicion(actual.estado, &evento)?;
        let mut nuevo = Estado { estado: nuevo_estado, actualizado_en: ahora_rfc3339(), ..actual };
        match &evento {
            Evento::Entregar(inf) => {
                self.escribir_informe(id, inf)?;
                if inf.fases_hechas > 0 {
                    nuevo.fase_actual = Some(inf.fases_hechas + 1);
                }
                nuevo.detalle = inf.motivo.clone();
            }
            _ => {}
        }
        let dir = self.dir_trabajo(id)?;
        escribir_json(&dir.join("estado.json"), &nuevo)?;
        self.historial(id, evento.nombre(), &nuevo)?;
        match &evento {
            Evento::Entregar(inf) => {
                let motivo = match inf.resultado {
                    ResultadoInforme::Terminado => "terminado",
                    ResultadoInforme::Bloqueado => "bloqueado",
                };
                // Una rutina vuelve a Claude Code solo si falla y el usuario lo
                // confirma (`Confirmar`); su informe no pasa por la cola.
                if self.leer_orden(id)?.modo != Modo::Rutina {
                    self.encolar(Agente::ClaudeCode, id, motivo)?;
                }
            }
            Evento::Confirmar => {
                self.encolar(Agente::ClaudeCode, id, "corrección de rutina")?;
            }
            Evento::Cerrar | Evento::Cancelar => {
                self.retirar_de_colas(id)?;
            }
            Evento::Reformular => {
                // Sin entradas rancias (p. ej. el «terminado» en claude-code si
                // se reformula sin haberlo tomado, como hace el panel web).
                self.retirar_de_colas(id)?;
                self.encolar(Agente::ModeloLocal, id, "reformulado")?;
            }
            _ => {}
        }
        if let Some(s) = &self.store {
            s.actualizar_estado_trabajo(id, nuevo.estado.as_str(), &nuevo.actualizado_en)?;
            self.sincronizar_fases(s, id, &evento, &nuevo)?;
            self.diario(s, id, &format!("bus_{}", evento.nombre()), true, serde_json::json!({ "estado": nuevo.estado.as_str() }));
        }
        Ok(nuevo)
    }

    /// Sube el contador de iteraciones (una por llamada al agente).
    pub fn anotar_iteracion(&self, id: &str) -> Resultado<u32> {
        let mut e = self.leer_estado(id)?;
        e.iteracion += 1;
        e.actualizado_en = ahora_rfc3339();
        escribir_json(&self.dir_trabajo(id)?.join("estado.json"), &e)?;
        Ok(e.iteracion)
    }

    /// Quita de TODAS las colas las entradas de un trabajo.
    pub fn retirar_de_colas(&self, id: &str) -> Resultado<usize> {
        let mut n = 0;
        for a in [Agente::ModeloLocal, Agente::ClaudeCode] {
            for e in fs::read_dir(self.dir_cola(a))?.flatten() {
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
    /// contexto del usuario al paquete y lo encola en `claude-code`.
    pub fn confirmar(&self, id: &str, contexto: &str) -> Resultado<Estado> {
        let estado = self.leer_estado(id)?;
        transicion(estado.estado, &Evento::Confirmar)?;
        if !contexto.trim().is_empty() {
            use std::io::Write;
            let ruta = self.dir_trabajo(id)?.join("paquete-correccion.md");
            let mut f = fs::OpenOptions::new().create(true).append(true).open(ruta)?;
            writeln!(f, "\n## Contexto del usuario\n\n{}", contexto.trim())?;
        }
        self.aplicar(id, Evento::Confirmar)
    }

    // --- rutinas -----------------------------------------------------------

    fn store_rutinas(&self) -> Resultado<&Store> {
        self.store.as_ref().ok_or_else(|| ErrorBus::Argumentos("las rutinas necesitan la base de datos".into()))
    }

    /// Valida y registra una rutina; la versión debe ser mayor que la última
    /// registrada. Copia el fichero a `dir` (`<slug>.toml`).
    pub fn registrar_rutina(&self, texto: &str, dir: &Path) -> Resultado<Rutina> {
        let r = Rutina::parsear(texto)?;
        let s = self.store_rutinas()?;
        if let Some(ult) = s.ultima_rutina(&r.rutina.slug)? {
            if i64::from(r.rutina.version) <= ult.version {
                return Err(ErrorBus::Argumentos(format!(
                    "la rutina {} ya está en la versión {}; la nueva debe ser mayor (es la {})",
                    r.rutina.slug, ult.version, r.rutina.version
                )));
            }
        }
        s.guardar_rutina(&FilaRutina {
            slug: r.rutina.slug.clone(),
            version: i64::from(r.rutina.version),
            definicion: texto.into(),
            creada_en: ahora_rfc3339(),
            creada_por: self.actor.clone(),
        })?;
        fs::create_dir_all(dir)?;
        crate::fs_atomico::escribir_atomico(&dir.join(format!("{}.toml", r.rutina.slug)), texto.as_bytes())?;
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

    /// Crea un trabajo de modo rutina con la última versión registrada.
    pub fn lanzar_rutina(&self, slug: &str, entradas: &[(String, String)]) -> Resultado<String> {
        self.crear_trabajo(self.rutina(slug)?.a_orden(entradas)?)
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
    pub fn reformular(&self, id: &str, notas: &str) -> Resultado<Estado> {
        let estado = self.leer_estado(id)?;
        transicion(estado.estado, &Evento::Reformular)?; // validar antes de tocar la orden
        let mut orden = self.leer_orden(id)?;
        if !notas.trim().is_empty() {
            orden.notas_revision.push(notas.trim().into());
            escribir_json(&self.dir_trabajo(id)?.join("orden.json"), &orden)?;
        }
        self.aplicar(id, Evento::Reformular)
    }

    /// Para llamar al arrancar. Devuelve a la cola los reclamos `.tomada-*`
    /// huérfanos cuyo trabajo sigue `EnCola`, y entrega como Bloqueado
    /// («interrumpido») lo que se quedó en `Ejecutando`. Devuelve los ids
    /// recuperados.
    pub fn recuperar(&self) -> Resultado<Vec<String>> {
        let mut recuperados = Vec::new();
        for agente in [Agente::ModeloLocal, Agente::ClaudeCode] {
            let dir = self.dir_cola(agente);
            for e in fs::read_dir(&dir)?.flatten() {
                let p = e.path();
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !n.starts_with('.') || !n.contains(".tomada-") {
                    continue;
                }
                let entrada: Option<EntradaCola> = fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if let Some(en) = entrada {
                    let en_cola = self.leer_estado(&en.trabajo).is_ok_and(|s| s.estado == EstadoTrabajo::EnCola);
                    let ya_en_cola = self.listar_cola(agente)?.iter().any(|x| x.trabajo == en.trabajo);
                    if en_cola && !ya_en_cola {
                        self.encolar(agente, &en.trabajo, "recuperado")?;
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
        for id in self.listar_trabajos()? {
            if self.leer_estado(&id).is_ok_and(|s| s.estado == EstadoTrabajo::Ejecutando) {
                let inf = Informe { motivo: Some("interrumpido".into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) };
                self.aplicar(&id, Evento::Entregar(inf))?;
                recuperados.push(id);
            }
        }
        Ok(recuperados)
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
        let linea = serde_json::json!({
            "ts": estado.actualizado_en,
            "evento": evento,
            "estado": estado.estado.as_str(),
            "iteracion": estado.iteracion,
        });
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.dir_trabajo(id)?.join("historial.jsonl"))?;
        writeln!(f, "{linea}")?;
        f.sync_all()?;
        Ok(())
    }

    /// El diario es secundario: un fallo de SQLite no deshace el bus.
    fn diario(&self, s: &Store, _id: &str, accion: &str, ok: bool, detalle: serde_json::Value) {
        let mut detalle = detalle;
        detalle["trabajo"] = serde_json::Value::String(_id.into());
        let _ = s.append_event(NewEvent {
            actor: self.actor.clone(),
            action: accion.into(),
            trabajo: None,
            outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
            duration_ms: None,
            detail: Some(detalle.to_string()),
        });
    }
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
