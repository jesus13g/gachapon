//! Lectura de los ajustes efectivos. Precedencia por clave: entorno >
//! `ajustes.toml` > TOML heredado > valor por defecto. Un valor inválido en
//! cualquier capa deja un aviso y cae a la capa siguiente; nunca un pánico ni
//! un servicio que no arranca.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::agentes::id_perfil_valido;
use crate::claves::{AJ_VIEJO, ALIAS, Clave, aplicar, clave, concretas, heredadas, valor_de};
use crate::escribir::{huella, interpretar};
use crate::fuentes::{Ajustes, Fuentes};
use crate::validar::{validar_escribibles, validar_perfil, validar_roles, validar_valor};

/// De dónde sale el valor efectivo de una clave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Origen {
    Defecto,
    Ajustes,
    /// Un TOML antiguo, o la sección `[modelo]` antigua de `ajustes.toml`.
    Heredado {
        fichero: &'static str,
    },
    Entorno {
        variable: &'static str,
    },
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
    /// Las claves concretas (fijas e instancias de patrones), en orden.
    pub claves: Vec<(String, &'static Clave)>,
    pub valores: BTreeMap<String, Value>,
    pub origenes: BTreeMap<String, Origen>,
    pub defectos: BTreeMap<String, Value>,
    pub avisos: Vec<String>,
    /// Los avisos de la validación cruzada (puertos, perfiles, roles): un
    /// cambio que añade uno nuevo no se escribe.
    pub conflictos: Vec<String>,
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
        let conocida = clave(&nombre).is_some() || ALIAS.iter().any(|(viejo, _)| *viejo == nombre);
        match v.as_table() {
            Some(sub) if !conocida => desconocidas(sub, &nombre, salida),
            _ if !conocida => salida.push(nombre),
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

    // Perfiles de modelo: los de por defecto y los que nombra ajustes.toml.
    let mut perfiles: Vec<String> = defecto.modelos.keys().cloned().collect();
    if let Some(t) = propia.as_ref().and_then(|t| t.get("modelos")).and_then(|m| m.as_table()) {
        for id in t.keys() {
            if id_perfil_valido(id) && !perfiles.contains(id) {
                perfiles.push(id.clone());
            }
        }
    }
    let lista = concretas(&perfiles);

    let mut valores = BTreeMap::new();
    let mut origenes = BTreeMap::new();
    let mut defectos = BTreeMap::new();
    for (nombre, c) in &lista {
        let por_defecto = match valor_de(&defecto, nombre) {
            // Un perfil que no está en los valores por defecto.
            Value::Null => validar_valor(c, &Value::String(String::new())).or_else(|_| validar_valor(c, &Value::Bool(false))).unwrap_or(Value::Null),
            v => v,
        };
        defectos.insert(nombre.clone(), por_defecto.clone());
        let mut elegido: Option<(Value, Origen)> = None;

        if let Some(var) = c.entorno
            && let Some(texto) = f.entorno.var(var)
        {
            match interpretar(c, &texto) {
                Ok(v) => elegido = Some((v, Origen::Entorno { variable: var })),
                Err(e) => avisos.push(format!("{var}: {e}; se ignora")),
            }
        }
        if elegido.is_none()
            && let Some(v) = propia.as_ref().and_then(|t| buscar(t, nombre))
        {
            match serde_json::to_value(v).map_err(|e| e.to_string()).and_then(|j| validar_valor(c, &j)) {
                Ok(v) => elegido = Some((v, Origen::Ajustes)),
                Err(e) => avisos.push(format!("ajustes.toml: {nombre}: {e}; se ignora")),
            }
        }
        for (fichero, ruta) in heredadas(nombre, c) {
            if elegido.is_some() {
                break;
            }
            let tabla = if *fichero == AJ_VIEJO { propia.as_ref() } else { heredados.get(fichero).and_then(|t| t.as_ref()) };
            if let Some(v) = tabla.and_then(|t| buscar(t, ruta)) {
                match serde_json::to_value(v).map_err(|e| e.to_string()).and_then(|j| validar_valor(c, &j)) {
                    Ok(v) => elegido = Some((v, Origen::Heredado { fichero })),
                    Err(e) => avisos.push(format!("{fichero}: {ruta}: {e}; se ignora")),
                }
            }
        }
        let (v, o) = elegido.unwrap_or((por_defecto, Origen::Defecto));
        valores.insert(nombre.clone(), v);
        origenes.insert(nombre.clone(), o);
    }

    let mut conflictos = Vec::new();
    let revertir = |nombre: &str, valores: &mut BTreeMap<String, Value>, origenes: &mut BTreeMap<String, Origen>| {
        let cambia = origenes.get(nombre).is_some_and(|o| *o != Origen::Defecto);
        if let Some(d) = defectos.get(nombre) {
            valores.insert(nombre.to_string(), d.clone());
            origenes.insert(nombre.to_string(), Origen::Defecto);
        }
        cambia
    };

    // Validación cruzada: los dos servicios no pueden compartir puerto.
    for perdedora in ["red.puerto_mcp", "red.puerto_web"] {
        if valores["red.puerto_web"] == valores["red.puerto_mcp"] {
            conflictos.push(format!("red.puerto_web y red.puerto_mcp coinciden ({}); {perdedora} vuelve a su valor por defecto", valores["red.puerto_web"]));
            revertir(perdedora, &mut valores, &mut origenes);
        }
    }

    let construir = |valores: &BTreeMap<String, Value>, fuera: &[String]| {
        let mut a = defecto.clone();
        for (nombre, v) in valores {
            if !fuera.iter().any(|id| nombre.starts_with(&format!("modelos.{id}."))) {
                aplicar(&mut a, nombre, v);
            }
        }
        a
    };

    // Perfiles incompletos o «locales» con una URL que no es de loopback: el
    // de por defecto vuelve a su valor; uno nuevo se descarta.
    let mut descartados: Vec<String> = Vec::new();
    let a = construir(&valores, &descartados);
    for (id, p) in &a.modelos {
        if let Err(e) = validar_perfil(p) {
            let prefijo = format!("modelos.{id}.");
            if defecto.modelos.contains_key(id) {
                conflictos.push(format!("modelos.{id}: {e}; vuelve a su valor por defecto"));
                for (n, _) in lista.iter().filter(|(n, _)| n.starts_with(&prefijo)) {
                    revertir(n, &mut valores, &mut origenes);
                }
            } else {
                conflictos.push(format!("modelos.{id}: {e}; se descarta el perfil"));
                descartados.push(id.clone());
            }
        }
    }

    // Agentes y roles: se vuelve atrás la clave culpable hasta que todo
    // cuadre (los valores por defecto cuadran siempre).
    for _ in 0..16 {
        let a = construir(&valores, &descartados);
        let Err((culpable, motivo)) = validar_roles(&a) else { break };
        if revertir(&culpable, &mut valores, &mut origenes) {
            conflictos.push(format!("{culpable}: {motivo}; vuelve a su valor por defecto"));
        } else {
            conflictos.push(format!("{culpable}: {motivo}; los roles vuelven a sus valores por defecto"));
            // El ejecutor de por defecto, su modelo y su perfil, tal como
            // vienen de fábrica.
            let ej = &defecto.roles.ejecutor;
            let perfil = format!("modelos.{}.", defecto.agentes[ej].modelo);
            let mut volver = vec!["roles.planificador".to_string(), "roles.ejecutor".to_string(), format!("agentes.{ej}.modelo")];
            volver.extend(lista.iter().map(|(n, _)| n).filter(|n| n.starts_with(&perfil)).cloned());
            for n in volver {
                revertir(&n, &mut valores, &mut origenes);
            }
        }
    }

    // Las rutas escribibles del criterio: que existan y no abran el
    // directorio personal ni los del ecosistema (con los de este proceso).
    let escribibles = "agente.criterio.escribibles";
    let a = construir(&valores, &descartados);
    if let Err(e) = validar_escribibles(&a.agente.criterio.escribibles, f, &a) {
        conflictos.push(format!("{escribibles}: {e}; vuelve a su valor por defecto"));
        revertir(escribibles, &mut valores, &mut origenes);
    }

    for id in &descartados {
        let prefijo = format!("modelos.{id}.");
        valores.retain(|n, _| !n.starts_with(&prefijo));
        origenes.retain(|n, _| !n.starts_with(&prefijo));
        defectos.retain(|n, _| !n.starts_with(&prefijo));
    }
    let claves: Vec<(String, &'static Clave)> = lista.into_iter().filter(|(n, _)| valores.contains_key(n)).collect();
    let ajustes = construir(&valores, &descartados);
    debug_assert!(validar_roles(&ajustes).is_ok());
    avisos.extend(conflictos.iter().cloned());
    Efectivos { ajustes, claves, valores, origenes, defectos, avisos, conflictos, fichero: f.fichero(), existe, version }
}
