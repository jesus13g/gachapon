//! API del bus: colas por agente, estado de los trabajos y su historial.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ea_store::{EventOutcome, FilaFase, FilaTrabajo, NewEvent, Store};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::{escribir_json, fnv};
use crate::tipos::{
    transicion, Agente, Estado, EstadoTrabajo, Evento, Informe, Orden, ResultadoInforme,
};

pub const ENV_ESTADO: &str = "EA_ESTADO_DIR";
static CONTADOR_ID: AtomicU64 = AtomicU64::new(0);

/// `$EA_ESTADO_DIR` o `~/.local/state/ecosistema-asistente/`.
pub fn raiz_estado() -> PathBuf {
    if let Ok(v) = std::env::var(ENV_ESTADO) {
        if !v.trim().is_empty() {
            return PathBuf::from(v.trim());
        }
    }
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|d| d.state_dir().unwrap_or_else(|| d.data_local_dir()).to_path_buf())
        .unwrap_or_else(|| PathBuf::from("estado"))
}

pub(crate) fn ahora_rfc3339() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default()
}

/// Id ordenable por tiempo: `20261014T193000-ab12`.
fn nuevo_id() -> String {
    let t = OffsetDateTime::now_utc();
    let n = CONTADOR_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let h = fnv(&format!("{}-{n}-{nanos}", std::process::id())) & 0xffff;
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}-{h:04x}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
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
        Ok(Self { raiz: raiz.to_path_buf(), store: None, actor: "orquestador".into() })
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
    /// orden trae el id vacío se le asigna uno nuevo.
    pub fn crear_trabajo(&self, mut orden: Orden) -> Resultado<String> {
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
            Evento::Tomar => nuevo.iteracion += 1,
            Evento::Entregar(inf) => {
                self.escribir_informe(id, inf)?;
                if inf.resultado == ResultadoInforme::Terminado && inf.fases_hechas > 0 {
                    nuevo.fase_actual = Some(inf.fases_hechas + 1);
                }
                nuevo.detalle = inf.motivo.clone();
            }
            _ => {}
        }
        let dir = self.dir_trabajo(id)?;
        escribir_json(&dir.join("estado.json"), &nuevo)?;
        self.historial(id, evento.nombre(), &nuevo)?;
        if let Some(s) = &self.store {
            s.actualizar_estado_trabajo(id, nuevo.estado.as_str(), &nuevo.actualizado_en)?;
            self.sincronizar_fases(s, id, &evento, &nuevo)?;
            self.diario(s, id, &format!("bus_{}", evento.nombre()), true, serde_json::json!({ "estado": nuevo.estado.as_str() }));
        }
        Ok(nuevo)
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
            task_id: None,
            outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
            duration_ms: None,
            detail: Some(detalle.to_string()),
        });
    }
}
