//! Lo que la interfaz puede hacer con el sistema: cada operación lee o
//! actúa a través del `Store`, el bus, la capa de acciones o la bóveda y
//! devuelve una vista de `crate::vistas`. Nada de HTTP aquí; los errores son
//! de dominio y `http` decide el código de estado.

use ea_acciones::{ErrorAccion, SistemaReal};
use ea_bus::{Bus, ErrorBus};
use ea_store::StoreError;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::AppState;

pub mod acciones;
pub mod ajustes;
pub mod boveda;
pub mod diario;
pub mod estado;
pub mod explorador;
pub mod memoria;
pub mod propuestas;
pub mod rutinas;
pub mod servicios;
pub mod trabajos;

const ACTOR: &str = "usuario";

pub type Resultado<T> = Result<T, ErrorWeb>;

#[derive(Debug)]
pub enum ErrorWeb {
    NoEncontrado(String),
    Invalido(String),
    Conflicto(String),
    Prohibido(String),
    NoDisponible(String),
    Tiempo(String),
    Interno(String),
}

impl From<StoreError> for ErrorWeb {
    fn from(e: StoreError) -> Self {
        let msg = e.to_string();
        match e {
            StoreError::EventNotFound(_) | StoreError::PropuestaNotFound(_) => Self::NoEncontrado(msg),
            StoreError::InvalidStatus(_) | StoreError::InvalidOutcome(_) | StoreError::NotaInvalida(_) => {
                Self::Invalido(msg)
            }
            _ => Self::Interno(msg),
        }
    }
}

impl From<ErrorAccion> for ErrorWeb {
    fn from(e: ErrorAccion) -> Self {
        let msg = e.to_string();
        match &e {
            ErrorAccion::Desconocida(_) | ErrorAccion::NoSupervisada(_) => Self::NoEncontrado(msg),
            ErrorAccion::Argumentos(_) => Self::Invalido(msg),
            ErrorAccion::NoAplica(_) => Self::Conflicto(msg),
            ErrorAccion::Prohibida(_) => Self::Prohibido(msg),
            ErrorAccion::Sistema(_) => Self::Interno(msg),
            ErrorAccion::Store(StoreError::PropuestaYaDecidida(_)) => Self::Conflicto(msg),
            ErrorAccion::Store(StoreError::EventNotFound(_) | StoreError::PropuestaNotFound(_)) => {
                Self::NoEncontrado(msg)
            }
            ErrorAccion::Store(_) => Self::Interno(msg),
        }
    }
}

impl From<ErrorBus> for ErrorWeb {
    fn from(e: ErrorBus) -> Self {
        let msg = e.to_string();
        match &e {
            ErrorBus::NoExiste(_) => Self::NoEncontrado(msg),
            ErrorBus::TransicionInvalida { .. } | ErrorBus::Ocupado => Self::Conflicto(msg),
            ErrorBus::Argumentos(_) | ErrorBus::Skills(_) => Self::Invalido(msg),
            _ => Self::Interno(msg),
        }
    }
}

/// Directo, sin pasar por `ErrorAccion`: la bóveda distingue lo que no
/// existe (404) de lo que choca con lo que hay (409).
impl From<ea_boveda::ErrorBoveda> for ErrorWeb {
    fn from(e: ea_boveda::ErrorBoveda) -> Self {
        use ea_boveda::ErrorBoveda as B;
        let m = e.to_string();
        match e {
            B::NoExiste(_) => Self::NoEncontrado(m),
            B::YaExiste(_) | B::Cambio(_) | B::Ocupada => Self::Conflicto(m),
            B::Ruta(_) | B::Argumentos(_) | B::Cabecera(_) => Self::Invalido(m),
            B::Prohibida(_) | B::FueraDeLaBoveda(_) => Self::Prohibido(m),
            B::Store(_) | B::Io(_) | B::EventoPerdido(..) => Self::Interno(m),
        }
    }
}

// ----------------------------------------------------------------- helpers

fn ts(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_default()
}

/// El sistema real para las acciones que confirma el usuario desde esta
/// página. `ea-web` no puede reiniciarse a sí mismo a mitad de la petición.
fn sistema(s: &AppState) -> SistemaReal {
    SistemaReal {
        rutinas_dir: s.config.rutinas_dir.clone(),
        boveda: s.boveda.clone(),
        propio: Some("ea-web"),
    }
}

/// Bus de solo lectura, sobre la raíz de estado configurada.
fn bus_lectura(s: &AppState) -> Resultado<Bus> {
    Ok(Bus::abrir_en(&s.config.estado_dir)?)
}

/// Bus con el historial y el diario de SQLite, para las acciones del usuario.
async fn bus_con_store(s: &AppState) -> Resultado<Bus> {
    let store = s.acciones.lock().await.duplicar()?;
    Ok(bus_lectura(s)?.con_actor(ACTOR).con_store(store))
}

// --------------------------------------------------------------- validación

fn parse_fecha(s: &str) -> Resultado<OffsetDateTime> {
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(dt);
    }
    time::Date::parse(s, &format_description!("[year]-[month]-[day]"))
        .map(|d| d.midnight().assume_utc())
        .map_err(|_| ErrorWeb::Invalido(format!("fecha inválida: '{s}' (usa RFC3339 o AAAA-MM-DD)")))
}

fn limite(pedido: Option<i64>, defecto: i64, maximo: i64) -> i64 {
    pedido.unwrap_or(defecto).clamp(1, maximo)
}

/// Los slugs acaban en una ruta de fichero (`<dir>/<slug>.toml`) y en el
/// nombre de una unidad de systemd: nada que no sea `[A-Za-z0-9_-]`.
fn slug_valido(slug: &str) -> Resultado<()> {
    let ok = !slug.is_empty()
        && slug.len() <= 64
        && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok { Ok(()) } else { Err(ErrorWeb::Invalido("slug de rutina inválido".into())) }
}

fn texto_no_vacio(valor: &str, campo: &str, max: usize) -> Resultado<String> {
    let v = valor.trim();
    if v.is_empty() {
        return Err(ErrorWeb::Invalido(format!("'{campo}' no puede estar vacío")));
    }
    if v.chars().count() > max {
        return Err(ErrorWeb::Invalido(format!("'{campo}' supera {max} caracteres")));
    }
    Ok(v.to_string())
}
