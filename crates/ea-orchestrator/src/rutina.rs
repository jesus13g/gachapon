//! Parseo y validación del formato declarativo de rutinas (§9.2 de la
//! especificación): un fichero TOML escrito por el usuario, grafo de pasos
//! lineal con ramificación, sin bucles. El modelo local solo interviene en
//! los pasos `decision`, eligiendo entre un conjunto cerrado de ramas — la
//! secuencia de acciones en sí está fijada de antemano por el autor.
//!
//! Alcance deliberadamente acotado a esta fase: el catálogo de herramientas
//! sigue siendo de solo lectura (fase 6 trae mutaciones), así que un paso
//! `accion` solo admite las cuatro herramientas de consulta ya existentes.
//! Una rutina que nombre cualquier otra falla al cargar — no se ejecuta
//! parcialmente. La única forma de expresar "esto necesita al usuario" es
//! que el propio autor diseñe una rama que termine en un paso `propuesta`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

/// Herramientas de solo lectura que un paso `accion` puede invocar en esta
/// fase — el mismo catálogo reducido que ya usa `ea-proxy`.
pub const HERRAMIENTAS_PERMITIDAS: &[&str] =
    &["ea_status", "ea_listar_tareas", "ea_ver_tarea", "ea_listar_diario"];

#[derive(Debug, Error)]
pub enum RutinaError {
    #[error("no se pudo leer la rutina '{0}': {1}")]
    Io(String, std::io::Error),
    #[error("rutina '{0}' con TOML inválido: {1}")]
    Toml(String, toml::de::Error),
    #[error("el paso inicial '{0}' no existe entre los pasos declarados")]
    InicioDesconocido(String),
    #[error("el paso '{0}' aparece más de una vez")]
    PasoDuplicado(String),
    #[error("el paso '{0}' referencia un paso desconocido '{1}'")]
    PasoDesconocido(String, String),
    #[error("el paso '{0}' invoca la herramienta '{1}', fuera del catálogo de solo lectura permitido en rutinas")]
    HerramientaNoPermitida(String, String),
}

#[derive(Debug, Deserialize)]
struct RutinaFile {
    rutina: Cabecera,
    #[serde(rename = "pasos")]
    pasos: Vec<PasoRaw>,
}

#[derive(Debug, Deserialize)]
struct Cabecera {
    descripcion: String,
    inicio: String,
    max_duracion_s: u64,
    max_pasos: u32,
    max_llamadas_modelo: u32,
    #[serde(default = "default_ventana_inicial_dias")]
    ventana_inicial_dias: i64,
}

fn default_ventana_inicial_dias() -> i64 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum PasoRaw {
    Accion {
        id: String,
        herramienta: String,
        #[serde(default)]
        argumentos: serde_json::Value,
        siguiente: String,
    },
    Decision {
        id: String,
        contexto: String,
        ramas: HashMap<String, String>,
    },
    Propuesta {
        id: String,
        descripcion: String,
        siguiente: Option<String>,
    },
    Fin {
        id: String,
    },
}

#[derive(Debug, Clone)]
pub enum Paso {
    Accion {
        herramienta: String,
        argumentos: serde_json::Value,
        siguiente: String,
    },
    Decision {
        contexto: String,
        ramas: HashMap<String, String>,
    },
    Propuesta {
        descripcion: String,
        siguiente: Option<String>,
    },
    Fin,
}

#[derive(Debug, Clone)]
pub struct Rutina {
    pub slug: String,
    pub descripcion: String,
    pub inicio: String,
    pub max_duracion_s: u64,
    pub max_pasos: u32,
    pub max_llamadas_modelo: u32,
    pub ventana_inicial_dias: i64,
    pub pasos: HashMap<String, Paso>,
}

/// Carga y valida `<dir>/<slug>.toml`.
pub fn cargar(dir: &Path, slug: &str) -> Result<Rutina, RutinaError> {
    let path = dir.join(format!("{slug}.toml"));
    let contenido = std::fs::read_to_string(&path).map_err(|e| RutinaError::Io(slug.to_string(), e))?;
    let raw: RutinaFile =
        toml::from_str(&contenido).map_err(|e| RutinaError::Toml(slug.to_string(), e))?;
    validar(slug, raw)
}

/// Lista los slugs de rutina disponibles (nombres de fichero `*.toml`, sin
/// validar su contenido — eso lo hace `cargar`).
pub fn listar_slugs(dir: &Path) -> std::io::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut slugs: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    slugs.sort();
    Ok(slugs)
}

fn validar(slug: &str, raw: RutinaFile) -> Result<Rutina, RutinaError> {
    let mut pasos = HashMap::with_capacity(raw.pasos.len());
    for paso_raw in raw.pasos {
        let (id, paso) = match paso_raw {
            PasoRaw::Accion { id, herramienta, argumentos, siguiente } => {
                if !HERRAMIENTAS_PERMITIDAS.contains(&herramienta.as_str()) {
                    return Err(RutinaError::HerramientaNoPermitida(id, herramienta));
                }
                (id, Paso::Accion { herramienta, argumentos, siguiente })
            }
            PasoRaw::Decision { id, contexto, ramas } => (id, Paso::Decision { contexto, ramas }),
            PasoRaw::Propuesta { id, descripcion, siguiente } => {
                (id, Paso::Propuesta { descripcion, siguiente })
            }
            PasoRaw::Fin { id } => (id, Paso::Fin),
        };
        if pasos.insert(id.clone(), paso).is_some() {
            return Err(RutinaError::PasoDuplicado(id));
        }
    }

    if !pasos.contains_key(&raw.rutina.inicio) {
        return Err(RutinaError::InicioDesconocido(raw.rutina.inicio));
    }

    // Todo destino declarado (siguiente/ramas) debe existir entre los pasos
    // — comprobado en la carga, no en tiempo de ejecución.
    for (id, paso) in &pasos {
        let destinos: Vec<&String> = match paso {
            Paso::Accion { siguiente, .. } => vec![siguiente],
            Paso::Decision { ramas, .. } => ramas.values().collect(),
            Paso::Propuesta { siguiente: Some(s), .. } => vec![s],
            Paso::Propuesta { siguiente: None, .. } | Paso::Fin => vec![],
        };
        for destino in destinos {
            if !pasos.contains_key(destino) {
                return Err(RutinaError::PasoDesconocido(id.clone(), destino.clone()));
            }
        }
    }

    Ok(Rutina {
        slug: slug.to_string(),
        descripcion: raw.rutina.descripcion,
        inicio: raw.rutina.inicio,
        max_duracion_s: raw.rutina.max_duracion_s,
        max_pasos: raw.rutina.max_pasos,
        max_llamadas_modelo: raw.rutina.max_llamadas_modelo,
        ventana_inicial_dias: raw.rutina.ventana_inicial_dias,
        pasos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EJEMPLO: &str = r#"
[rutina]
descripcion = "Revisa el diario desde la última ejecución"
inicio = "consultar_diario"
max_duracion_s = 120
max_pasos = 10
max_llamadas_modelo = 5

[[pasos]]
id = "consultar_diario"
tipo = "accion"
herramienta = "ea_listar_diario"
argumentos = { desde = "{ultima_ejecucion}", limite = 50 }
siguiente = "clasificar"

[[pasos]]
id = "clasificar"
tipo = "decision"
contexto = "¿Hay algún evento con outcome=error?\n{salida_previa}"
ramas = { hay_errores = "proponer_revision", sin_errores = "fin" }

[[pasos]]
id = "proponer_revision"
tipo = "propuesta"
descripcion = "Revisar incidencias detectadas desde {ultima_ejecucion}"

[[pasos]]
id = "fin"
tipo = "fin"
"#;

    #[test]
    fn parsea_y_valida_una_rutina_correcta() {
        let raw: RutinaFile = toml::from_str(EJEMPLO).expect("parsear TOML");
        let rutina = validar("revision-diaria", raw).expect("validar rutina");
        assert_eq!(rutina.pasos.len(), 4);
        assert_eq!(rutina.inicio, "consultar_diario");
        assert!(matches!(rutina.pasos["fin"], Paso::Fin));
    }

    #[test]
    fn rechaza_herramienta_fuera_del_catalogo_de_solo_lectura() {
        let toml_str = EJEMPLO.replace("ea_listar_diario", "ea_crear_tarea");
        let raw: RutinaFile = toml::from_str(&toml_str).expect("parsear TOML");
        let err = validar("revision-diaria", raw).unwrap_err();
        assert!(matches!(err, RutinaError::HerramientaNoPermitida(_, _)));
    }

    #[test]
    fn rechaza_destino_desconocido() {
        let toml_str = EJEMPLO.replace(
            "sin_errores = \"fin\"",
            "sin_errores = \"paso_inexistente\"",
        );
        let raw: RutinaFile = toml::from_str(&toml_str).expect("parsear TOML");
        let err = validar("revision-diaria", raw).unwrap_err();
        assert!(matches!(err, RutinaError::PasoDesconocido(_, _)));
    }

    #[test]
    fn la_rutina_de_ejemplo_enviada_con_el_crate_es_valida() {
        let contenido = include_str!("../rutinas-ejemplo/revision-diaria.toml");
        let raw: RutinaFile = toml::from_str(contenido).expect("parsear rutina de ejemplo");
        let rutina = validar("revision-diaria", raw).expect("validar rutina de ejemplo");
        assert_eq!(rutina.inicio, "consultar_diario");
        assert_eq!(rutina.pasos.len(), 4);
    }

    #[test]
    fn rechaza_inicio_desconocido() {
        let toml_str = EJEMPLO.replace(
            "inicio = \"consultar_diario\"",
            "inicio = \"no_existe\"",
        );
        let raw: RutinaFile = toml::from_str(&toml_str).expect("parsear TOML");
        let err = validar("revision-diaria", raw).unwrap_err();
        assert!(matches!(err, RutinaError::InicioDesconocido(_)));
    }
}
