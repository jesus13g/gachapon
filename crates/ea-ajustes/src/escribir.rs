//! Escritura de `ajustes.toml`: el único fichero de configuración que el
//! sistema escribe. Todo o nada, con control de versión, sin perder los
//! comentarios ni las claves ajenas (`toml_edit`) y de forma atómica.

use std::fs;
use std::path::Path;

use ea_plataforma::fs::Permisos;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::cargar::{cargar_de, resolver, Origen};
use crate::claves::{clave, heredadas, sin_alias, Aplica, Canal, Clave, Edicion, Tipo, AJ_VIEJO};
use crate::error::ErrorAjustes;
use crate::fuentes::Fuentes;
use crate::validar::validar_valor;

/// Un cambio ya escrito.
#[derive(Debug, Clone, PartialEq)]
pub struct Aplicado {
    pub clave: String,
    pub antes: Value,
    pub despues: Value,
    pub aplica: Aplica,
}

/// La huella FNV-1a de `ea-plataforma` (como la versión de la memoria en
/// ea-web); "" para un fichero que no existe.
pub fn huella(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    ea_plataforma::fs::huella(bytes)
}

/// La escritura atómica de `ea-plataforma` creando antes el directorio
/// (como `fs_atomico` de ea-bus).
pub fn escribir_atomico(destino: &Path, contenido: &[u8]) -> std::io::Result<()> {
    let dir = destino
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "ruta sin directorio"))?;
    fs::create_dir_all(dir)?;
    ea_plataforma::fs::escribir_atomico(destino, contenido, Permisos::Heredados)
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
        Tipo::Mapa => serde_json::from_str::<Value>(t).map_err(|e| invalido(format!("se espera JSON de texto a texto: {e}")))?,
        Tipo::Lista { .. } | Tipo::Rutas => {
            Value::from(t.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>())
        }
        Tipo::Texto { .. } | Tipo::Url | Tipo::Ruta | Tipo::Eleccion { .. } | Tipo::UrlPerfil | Tipo::Perfil | Tipo::Distro => {
            Value::String(t.to_string())
        }
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
        Value::Object(m) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, v) in m {
                t.insert(k, v.as_str().unwrap_or_default().into());
            }
            toml_edit::Value::InlineTable(t)
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

/// Quita las tablas que se han quedado sin nada (p. ej. `[modelo]` al pasar
/// sus claves a `modelos.local`), salvo que lleven un comentario.
fn quitar_tablas_vacias(doc: &mut DocumentMut) {
    fn vacia(i: &Item) -> bool {
        match i.as_table() {
            Some(t) => {
                let comentada = t.decor().prefix().and_then(|p| p.as_str()).is_some_and(|p| p.contains('#'));
                !comentada && t.iter().all(|(_, v)| vacia(v))
            }
            None => false,
        }
    }
    let raiz = doc.as_table_mut();
    let sobran: Vec<String> = raiz.iter().filter(|(_, v)| vacia(v)).map(|(k, _)| k.to_string()).collect();
    for k in sobran {
        raiz.remove(&k);
    }
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

    let mut claves: Vec<(String, &'static Clave)> = Vec::new();
    let mut pedidas: Vec<(String, bool)> = Vec::new();
    for (nombre, valor) in cambios {
        // `modelo.nombre` y compañía siguen valiendo: se escriben con su
        // nombre de ahora.
        let nombre = sin_alias(nombre.trim()).to_string();
        let c = clave(&nombre).ok_or_else(|| ErrorAjustes::ClaveDesconocida(nombre.clone()))?;
        if let Some(var) = c.entorno
            && f.entorno.var(var).is_some() {
                return Err(ErrorAjustes::FijadaPorEntorno { clave: nombre, variable: var.into() });
            }
        if canal == Canal::Web && c.edicion == Edicion::Terminal {
            return Err(ErrorAjustes::NoEditable { clave: nombre, canal: "la web".into() });
        }
        let valor = match valor {
            Some(v) => Some(validar_valor(c, v).map_err(|motivo| ErrorAjustes::ValorInvalido { clave: nombre.clone(), motivo })?),
            None => None,
        };
        poner(&mut doc, &nombre, valor.as_ref())?;
        // Al escribir la clave nueva se quita la antigua de `[modelo]`, para
        // que el fichero no diga dos cosas.
        if valor.is_some() {
            for (fichero, ruta) in heredadas(&nombre, c) {
                if *fichero == AJ_VIEJO {
                    poner(&mut doc, ruta, None)?;
                }
            }
        }
        pedidas.push((nombre.clone(), valor.is_some()));
        claves.push((nombre, c));
    }
    quitar_tablas_vacias(&mut doc);

    // El resultado se evalúa como si ya estuviera escrito.
    let nuevo_texto = doc.to_string();
    let despues = resolver(f, Some(&nuevo_texto), huella(nuevo_texto.as_bytes()), true);
    // Un conflicto nuevo de la validación cruzada (puertos, perfiles, roles)
    // impide escribir: lo pedido no es lo que quedaría.
    if let Some(motivo) = despues.conflictos.iter().find(|c| !antes.conflictos.contains(c)) {
        let clave = claves
            .iter()
            .map(|(n, _)| n)
            .find(|n| motivo.contains(n.as_str()))
            .or_else(|| claves.first().map(|(n, _)| n))
            .cloned()
            .unwrap_or_default();
        return Err(ErrorAjustes::ValorInvalido { clave, motivo: motivo.clone() });
    }
    let nulo = Value::Null;
    for (nombre, c) in &claves {
        let n = despues.valores.get(nombre).unwrap_or(&nulo);
        let a = antes.valores.get(nombre).unwrap_or(&nulo);
        if canal == Canal::Web && c.endurecible && !no_afloja(n, a) {
            return Err(ErrorAjustes::SoloEndurecer { clave: nombre.clone(), actual: a.to_string() });
        }
        // Lo pedido debe ser lo que queda (salvo al quitar la clave).
        if despues.origenes.get(nombre) != Some(&Origen::Ajustes) && pedidas.iter().any(|(k, puesta)| k == nombre && *puesta) {
            let motivo = despues
                .avisos
                .iter()
                .find(|a| a.contains(nombre.as_str()))
                .cloned()
                .unwrap_or_else(|| "el valor no se puede aplicar".into());
            return Err(ErrorAjustes::ValorInvalido { clave: nombre.clone(), motivo });
        }
    }
    if let Some(v) = version
        && v != actual_version {
            return Err(ErrorAjustes::Version);
        }

    escribir_atomico(&fichero, nuevo_texto.as_bytes())?;
    let mut aplicados = Vec::new();
    for (nombre, c) in claves {
        if aplicados.iter().any(|a: &Aplicado| a.clave == nombre) {
            continue;
        }
        aplicados.push(Aplicado {
            antes: antes.valores.get(&nombre).cloned().unwrap_or(Value::Null),
            despues: despues.valores.get(&nombre).cloned().unwrap_or(Value::Null),
            clave: nombre,
            aplica: c.aplica,
        });
    }
    Ok(aplicados)
}

#[cfg(test)]
mod tests {
    use super::huella;

    /// Prueba dorada: la huella de antes de usar la de `ea-plataforma`,
    /// copiada tal cual, da lo mismo (también el "" del fichero que no existe).
    #[test]
    fn la_huella_es_la_de_siempre() {
        fn antigua(bytes: &[u8]) -> String {
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
        for t in ["", "a", "[web]\nsondeo_ms = 3000\n", "# ñandú — 漢字 🦀\n"] {
            assert_eq!(huella(t.as_bytes()), antigua(t.as_bytes()), "{t:?}");
        }
        assert_eq!(huella(&[0xff, 0x00, 0xfe]), antigua(&[0xff, 0x00, 0xfe]));
    }
}
