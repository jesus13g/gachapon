//! Escritura de `ajustes.toml`: el único fichero de configuración que el
//! sistema escribe. Todo o nada, con control de versión, sin perder los
//! comentarios ni las claves ajenas (`toml_edit`) y de forma atómica.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::cargar::{cargar_de, resolver, Origen};
use crate::claves::{clave, Aplica, Canal, Clave, Edicion, Tipo};
use crate::error::ErrorAjustes;
use crate::fuentes::Fuentes;
use crate::validar::validar_valor;

static CONTADOR: AtomicU64 = AtomicU64::new(0);

/// Un cambio ya escrito.
#[derive(Debug, Clone, PartialEq)]
pub struct Aplicado {
    pub clave: &'static str,
    pub antes: Value,
    pub despues: Value,
    pub aplica: Aplica,
}

/// FNV-1a de 64 bits en hexadecimal (como la versión de la memoria en
/// ea-web); "" para un fichero que no existe.
pub fn huella(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Temporal oculto en el mismo directorio, `fsync` y renombrado (mismo
/// patrón que `fs_atomico` de ea-bus).
pub fn escribir_atomico(destino: &Path, contenido: &[u8]) -> std::io::Result<()> {
    let dir = destino
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "ruta sin directorio"))?;
    fs::create_dir_all(dir)?;
    let nombre = destino.file_name().and_then(|n| n.to_str()).unwrap_or("fichero");
    let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
    let temporal = dir.join(format!(".{nombre}.{}.{n}.tmp", std::process::id()));
    let resultado = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&temporal)?;
        f.write_all(contenido)?;
        f.sync_all()?;
        fs::rename(&temporal, destino)
    })();
    if let Err(e) = resultado {
        let _ = fs::remove_file(&temporal);
        return Err(e);
    }
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Convierte el texto de la terminal (o de una variable de entorno) al tipo
/// de la clave y lo valida: `true`/`false` (también sí/no), enteros y listas
/// separadas por comas.
pub fn interpretar(c: &Clave, texto: &str) -> Result<Value, ErrorAjustes> {
    let t = texto.trim();
    let invalido = |motivo: String| ErrorAjustes::ValorInvalido { clave: c.nombre.into(), motivo };
    let v = match c.tipo {
        Tipo::Booleano => match t.to_lowercase().as_str() {
            "true" | "sí" | "si" | "1" | "yes" => Value::Bool(true),
            "false" | "no" | "0" => Value::Bool(false),
            _ => return Err(invalido(format!("{t:?} no es true ni false"))),
        },
        Tipo::Entero { .. } => Value::from(t.parse::<i64>().map_err(|_| invalido(format!("{t:?} no es un entero")))?),
        Tipo::Lista { .. } => {
            Value::from(t.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>())
        }
        Tipo::Texto { .. } | Tipo::Url | Tipo::Ruta => Value::String(t.to_string()),
    };
    validar_valor(c, &v).map_err(invalido)
}

fn a_toml(v: &Value) -> toml_edit::Value {
    match v {
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => n.as_i64().unwrap_or_default().into(),
        Value::Array(l) => {
            let mut a = toml_edit::Array::new();
            for e in l {
                a.push(e.as_str().unwrap_or_default());
            }
            toml_edit::Value::Array(a)
        }
        otro => otro.as_str().unwrap_or_default().into(),
    }
}

/// Baja por las tablas de `partes`, creándolas (implícitas) si `crear`.
fn tabla<'a>(raiz: &'a mut dyn TableLike, partes: &[&str], crear: bool) -> Result<Option<&'a mut dyn TableLike>, ErrorAjustes> {
    let Some((primera, resto)) = partes.split_first() else {
        return Ok(Some(raiz));
    };
    if raiz.get(primera).is_none() {
        if !crear {
            return Ok(None);
        }
        let mut t = Table::new();
        t.set_implicit(true);
        raiz.insert(primera, Item::Table(t));
    }
    let sub = raiz
        .get_mut(primera)
        .and_then(|i| i.as_table_like_mut())
        .ok_or_else(|| ErrorAjustes::FicheroInvalido(format!("{primera:?} debería ser una tabla")))?;
    tabla(sub, resto, crear)
}

fn poner(doc: &mut DocumentMut, nombre: &str, valor: Option<&Value>) -> Result<(), ErrorAjustes> {
    let partes: Vec<&str> = nombre.split('.').collect();
    let (hoja, secciones) = partes.split_last().expect("una clave tiene al menos una parte");
    match valor {
        Some(v) => {
            let t = tabla(doc.as_table_mut(), secciones, true)?.expect("se acaba de crear");
            let mut nuevo = a_toml(v);
            // Se reemplaza en el sitio: se conservan el comentario de encima
            // (va con la clave) y el del final de la línea (va con el valor).
            match t.get_mut(hoja) {
                Some(viejo) => {
                    if let Some(v) = viejo.as_value() {
                        *nuevo.decor_mut() = v.decor().clone();
                    }
                    *viejo = Item::Value(nuevo);
                }
                None => {
                    t.insert(hoja, Item::Value(nuevo));
                }
            }
        }
        None => {
            if let Some(t) = tabla(doc.as_table_mut(), secciones, false)? {
                t.remove(hoja);
            }
        }
    }
    Ok(())
}

/// ¿`nuevo` es igual o más estricto que `actual`? Números: menor o igual;
/// listas: subconjunto.
fn no_afloja(nuevo: &Value, actual: &Value) -> bool {
    match (nuevo, actual) {
        (Value::Number(n), Value::Number(a)) => n.as_i64() <= a.as_i64(),
        (Value::Array(n), Value::Array(a)) => n.iter().all(|e| a.contains(e)),
        _ => nuevo == actual,
    }
}

/// Aplica un lote de cambios a `ajustes.toml` (`None` quita la clave y deja
/// que mande la capa siguiente). Comprueba, por este orden: que la clave
/// existe, que no la fija el entorno, que el canal puede editarla, el tipo y
/// el rango, que desde la web lo endurecible no se afloja, la validación
/// cruzada y la versión. Si algo falla no se escribe nada.
pub fn establecer(
    f: &Fuentes,
    canal: Canal,
    version: Option<&str>,
    cambios: &[(String, Option<Value>)],
) -> Result<Vec<Aplicado>, ErrorAjustes> {
    if cambios.is_empty() {
        return Err(ErrorAjustes::SinCambios);
    }
    let fichero = f.fichero();
    let bytes = match fs::read(&fichero) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let actual_version = huella(bytes.as_deref().unwrap_or_default());
    let texto = bytes.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let mut doc: DocumentMut = texto.parse().map_err(|e: toml_edit::TomlError| ErrorAjustes::FicheroInvalido(e.message().to_string()))?;
    let antes = cargar_de(f);

    let mut claves: Vec<&'static Clave> = Vec::new();
    for (nombre, valor) in cambios {
        let c = clave(nombre).ok_or_else(|| ErrorAjustes::ClaveDesconocida(nombre.clone()))?;
        if let Some(var) = c.entorno
            && f.entorno.var(var).is_some() {
                return Err(ErrorAjustes::FijadaPorEntorno { clave: c.nombre.into(), variable: var.into() });
            }
        if canal == Canal::Web && c.edicion == Edicion::Terminal {
            return Err(ErrorAjustes::NoEditable { clave: c.nombre.into(), canal: "la web".into() });
        }
        let valor = match valor {
            Some(v) => Some(validar_valor(c, v).map_err(|motivo| ErrorAjustes::ValorInvalido { clave: c.nombre.into(), motivo })?),
            None => None,
        };
        poner(&mut doc, c.nombre, valor.as_ref())?;
        claves.push(c);
    }

    // El resultado se evalúa como si ya estuviera escrito.
    let nuevo_texto = doc.to_string();
    let despues = resolver(f, Some(&nuevo_texto), huella(nuevo_texto.as_bytes()), true);
    for c in &claves {
        let (n, a) = (&despues.valores[c.nombre], &antes.valores[c.nombre]);
        if canal == Canal::Web && c.endurecible && !no_afloja(n, a) {
            return Err(ErrorAjustes::SoloEndurecer { clave: c.nombre.into(), actual: a.to_string() });
        }
        // Lo pedido debe ser lo que queda (salvo al quitar la clave): si la
        // validación cruzada (puertos) lo ha descartado, no se escribe.
        if despues.origenes[c.nombre] != Origen::Ajustes && cambios.iter().any(|(k, v)| k == c.nombre && v.is_some()) {
            let motivo = despues
                .avisos
                .iter()
                .find(|a| a.contains(c.nombre))
                .cloned()
                .unwrap_or_else(|| "el valor no se puede aplicar".into());
            return Err(ErrorAjustes::ValorInvalido { clave: c.nombre.into(), motivo });
        }
    }
    let choque = despues.avisos.iter().any(|a| a.starts_with("red.puerto_web y red.puerto_mcp coinciden"));
    if choque && claves.iter().any(|c| c.nombre.starts_with("red.")) {
        return Err(ErrorAjustes::ValorInvalido {
            clave: "red".into(),
            motivo: "red.puerto_web y red.puerto_mcp no pueden coincidir".into(),
        });
    }
    if let Some(v) = version
        && v != actual_version {
            return Err(ErrorAjustes::Version);
        }

    escribir_atomico(&fichero, nuevo_texto.as_bytes())?;
    let mut aplicados = Vec::new();
    for c in claves {
        if aplicados.iter().any(|a: &Aplicado| a.clave == c.nombre) {
            continue;
        }
        aplicados.push(Aplicado {
            clave: c.nombre,
            antes: antes.valores[c.nombre].clone(),
            despues: despues.valores[c.nombre].clone(),
            aplica: c.aplica,
        });
    }
    Ok(aplicados)
}
