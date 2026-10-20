//! Lectura de los ajustes efectivos. Precedencia por clave: entorno >
//! `ajustes.toml` > TOML heredado > valor por defecto. Un valor inválido en
//! cualquier capa deja un aviso y cae a la capa siguiente; nunca un pánico ni
//! un servicio que no arranca.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::claves::{clave, valor_de, aplicar, CLAVES};
use crate::escribir::{huella, interpretar};
use crate::fuentes::{Ajustes, Fuentes};
use crate::validar::validar_valor;

/// De dónde sale el valor efectivo de una clave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Origen {
    Defecto,
    Ajustes,
    Heredado { fichero: &'static str },
    Entorno { variable: &'static str },
}

impl Origen {
    pub fn legible(&self) -> String {
        match self {
            Origen::Defecto => "por defecto".into(),
            Origen::Ajustes => "ajustes.toml".into(),
            Origen::Heredado { fichero } => format!("heredado: {fichero}"),
            Origen::Entorno { variable } => format!("entorno: {variable}"),
        }
    }
}

/// Los ajustes efectivos con el origen de cada valor y lo que hubo que avisar.
#[derive(Debug, Clone)]
pub struct Efectivos {
    pub ajustes: Ajustes,
    pub valores: BTreeMap<&'static str, Value>,
    pub origenes: BTreeMap<&'static str, Origen>,
    pub defectos: BTreeMap<&'static str, Value>,
    pub avisos: Vec<String>,
    /// `ajustes.toml`.
    pub fichero: PathBuf,
    pub existe: bool,
    /// Huella del contenido de `ajustes.toml` ("" si no existe).
    pub version: String,
}

/// Los ajustes del sistema (entorno del proceso y directorios de siempre).
pub fn cargar() -> Ajustes {
    cargar_de(&Fuentes::sistema()).ajustes
}

pub fn cargar_de(f: &Fuentes) -> Efectivos {
    let fichero = f.fichero();
    let (existe, bytes) = match std::fs::read(&fichero) {
        Ok(b) => (true, b),
        Err(_) => (false, Vec::new()),
    };
    let texto = existe.then(|| String::from_utf8_lossy(&bytes).into_owned());
    resolver(f, texto.as_deref(), huella(if existe { &bytes } else { &[] }), existe)
}

/// Lee un TOML opcional: `None` si no existe; si no es TOML, aviso y `None`.
fn leer_toml(ruta: &Path, nombre: &str, avisos: &mut Vec<String>) -> Option<toml::Table> {
    let texto = std::fs::read_to_string(ruta).ok()?;
    parsear(&texto, nombre, avisos)
}

fn parsear(texto: &str, nombre: &str, avisos: &mut Vec<String>) -> Option<toml::Table> {
    match texto.parse::<toml::Table>() {
        Ok(t) => Some(t),
        Err(e) => {
            avisos.push(format!("{nombre} no es TOML válido y se ignora: {}", e.message()));
            None
        }
    }
}

fn buscar<'a>(t: &'a toml::Table, ruta: &str) -> Option<&'a toml::Value> {
    let mut partes = ruta.split('.');
    let mut actual = t.get(partes.next()?)?;
    for p in partes {
        actual = actual.as_table()?.get(p)?;
    }
    Some(actual)
}

/// Claves de `ajustes.toml` que no están en el registro (erratas, sobre todo).
fn desconocidas(t: &toml::Table, prefijo: &str, salida: &mut Vec<String>) {
    for (k, v) in t {
        let nombre = if prefijo.is_empty() { k.clone() } else { format!("{prefijo}.{k}") };
        match v.as_table() {
            Some(sub) if clave(&nombre).is_none() => desconocidas(sub, &nombre, salida),
            _ if clave(&nombre).is_none() => salida.push(nombre),
            _ => {}
        }
    }
}

/// Resuelve con el contenido de `ajustes.toml` dado (`None`: no existe). Lo
/// usa también la escritura para comprobar el resultado antes de guardarlo.
pub(crate) fn resolver(f: &Fuentes, ajustes_toml: Option<&str>, version: String, existe: bool) -> Efectivos {
    let mut avisos = Vec::new();
    let defecto = Ajustes::por_defecto(f);
    let propia = ajustes_toml.and_then(|t| parsear(t, "ajustes.toml", &mut avisos));
    if let Some(t) = &propia {
        let mut malas = Vec::new();
        desconocidas(t, "", &mut malas);
        for m in malas {
            avisos.push(format!("ajustes.toml: clave desconocida {m:?}; se ignora"));
        }
    }
    let mut heredados: BTreeMap<&'static str, Option<toml::Table>> = BTreeMap::new();
    for (nombre, ruta) in f.heredados() {
        heredados.insert(nombre, leer_toml(&ruta, nombre, &mut avisos));
    }

    let mut valores = BTreeMap::new();
    let mut origenes = BTreeMap::new();
    let mut defectos = BTreeMap::new();
    for c in CLAVES {
        let por_defecto = valor_de(&defecto, c.nombre);
        defectos.insert(c.nombre, por_defecto.clone());
        let mut elegido: Option<(Value, Origen)> = None;

        if let Some(var) = c.entorno
            && let Some(texto) = f.entorno.var(var) {
                match interpretar(c, &texto) {
                    Ok(v) => elegido = Some((v, Origen::Entorno { variable: var })),
                    Err(e) => avisos.push(format!("{var}: {e}; se ignora")),
                }
            }
        if elegido.is_none()
            && let Some(v) = propia.as_ref().and_then(|t| buscar(t, c.nombre)) {
                match serde_json::to_value(v).map_err(|e| e.to_string()).and_then(|j| validar_valor(c, &j)) {
                    Ok(v) => elegido = Some((v, Origen::Ajustes)),
                    Err(e) => avisos.push(format!("ajustes.toml: {}: {e}; se ignora", c.nombre)),
                }
            }
        if elegido.is_none()
            && let Some((fichero, ruta)) = c.heredada
                && let Some(v) = heredados.get(fichero).and_then(|t| t.as_ref()).and_then(|t| buscar(t, ruta)) {
                    match serde_json::to_value(v).map_err(|e| e.to_string()).and_then(|j| validar_valor(c, &j)) {
                        Ok(v) => elegido = Some((v, Origen::Heredado { fichero })),
                        Err(e) => avisos.push(format!("{fichero}: {ruta}: {e}; se ignora")),
                    }
                }
        let (v, o) = elegido.unwrap_or((por_defecto, Origen::Defecto));
        valores.insert(c.nombre, v);
        origenes.insert(c.nombre, o);
    }

    // Validación cruzada: los dos servicios no pueden compartir puerto.
    for perdedora in ["red.puerto_mcp", "red.puerto_web"] {
        if valores["red.puerto_web"] == valores["red.puerto_mcp"] {
            avisos.push(format!(
                "red.puerto_web y red.puerto_mcp coinciden ({}); {perdedora} vuelve a su valor por defecto",
                valores["red.puerto_web"]
            ));
            valores.insert(perdedora, defectos[perdedora].clone());
            origenes.insert(perdedora, Origen::Defecto);
        }
    }

    let mut ajustes = defecto;
    for (nombre, v) in &valores {
        aplicar(&mut ajustes, nombre, v);
    }
    Efectivos { ajustes, valores, origenes, defectos, avisos, fichero: f.fichero(), existe, version }
}
