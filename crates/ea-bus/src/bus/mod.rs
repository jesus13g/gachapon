//! API del bus: colas por agente, estado de los trabajos y su historial.
//! Cada responsabilidad en su módulo: `colas` (encolar, listar, tomar,
//! retirar y devolver), `trabajos` (crear y leer), `transiciones` (la
//! máquina de estados aplicada bajo el bloqueo de cada trabajo, la entrega y
//! la reformulación), `rutinas` y `recuperar` (lo que se hace al arrancar).
//! El orden de los candados está en [`Bus::bloquear_estado`].

mod colas;
mod recuperar;
mod rutinas;
mod trabajos;
mod transiciones;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ea_store::{EventOutcome, NewEvent, Store};
use time::OffsetDateTime;

use crate::error::{ErrorBus, Resultado};
use crate::skills::ConfigSkills;
use crate::tipos::{Estado, Rol};

pub use colas::{Cola, EntradaCola};
pub use rutinas::comprobar_registro;
pub use transiciones::Entrega;

static CONTADOR_ID: AtomicU64 = AtomicU64::new(0);

/// `rutas.estado` de los ajustes, que resuelven `$EA_ESTADO_DIR` (por defecto
/// `~/.local/state/ecosistema-asistente/`). Un atajo para los binarios: las
/// bibliotecas reciben la raíz.
pub fn raiz_estado() -> PathBuf {
    ea_ajustes::cargar().rutas.estado
}

/// Las marcas de tiempo del bus, en RFC 3339 como las del diario.
pub(crate) use ea_store::ahora_rfc3339;

/// Id ordenable por tiempo: `20261014T193000-0003ab12`. El contador evita que
/// dos ids del mismo segundo se ordenen al azar (las colas son FIFO).
pub(crate) fn nuevo_id() -> String {
    let t = OffsetDateTime::now_utc();
    let n = CONTADOR_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let h = ea_plataforma::fs::fnv64(format!("{}-{n}-{nanos}", std::process::id()).as_bytes()) & 0xffff;
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}-{:04x}{h:04x}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(), t.second(), n & 0xffff)
}

/// Los ids de trabajo (y de equipo) acaban en una ruta del bus: nada que no
/// sea `[A-Za-z0-9-]`.
pub fn id_valido(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub struct Bus {
    pub(crate) raiz: PathBuf,
    pub(crate) store: Option<Store>,
    /// Quien firma en el diario lo que hace este bus. Llega con el store
    /// ([`Bus::con_store`]): sin base no se firma nada, y con ella no hay
    /// actor por defecto (U-06).
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
        Ok(Self { raiz: raiz.to_path_buf(), store: None, actor: String::new(), skills: None })
    }

    /// Con historial y diario en SQLite, a nombre de `actor`: quien llama dice
    /// siempre quién es (el orquestador, el usuario, el planificador…).
    pub fn con_store(mut self, store: Store, actor: &str) -> Self {
        self.store = Some(store);
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

    pub(crate) fn dir_trabajo(&self, id: &str) -> Resultado<PathBuf> {
        if !id_valido(id) {
            return Err(ErrorBus::Argumentos(format!("id de trabajo inválido: {id:?}")));
        }
        Ok(self.raiz.join("trabajos").join(id))
    }

    /// Una línea más en `historial.jsonl` (append-only, con `fsync`).
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
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.artefactos(id)?.ruta_historial())?;
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
            detail: Some(detalle.to_string()),
            ..Default::default()
        });
    }
}

/// El espejo en SQLite es secundario, como el diario: un fallo no deshace el
/// bus (que ya está escrito) ni hace fallar la operación; queda en el
/// registro.
fn espejo<T, E: std::fmt::Display>(id: &str, que: &str, r: Result<T, E>) -> Option<T> {
    r.map_err(|e| tracing::warn!(%id, error = %e, "espejo SQLite: no se pudo {que}")).ok()
}
