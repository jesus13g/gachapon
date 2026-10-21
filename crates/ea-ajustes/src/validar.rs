//! Validaciones: rutas con `~`, la URL del modelo (solo loopback para lo
//! local), los tipos del registro de claves y la validación cruzada de
//! perfiles, agentes y roles (`validar_roles`).

use std::path::PathBuf;

use serde_json::Value;

use crate::agentes::{ficha, id_perfil_valido, Rol};
use crate::claves::{Clave, Tipo};
use crate::fuentes::{Ajustes, PerfilModelo};

/// Expande `~` y `~/…` al directorio personal. Es la ÚNICA función del
/// ecosistema que lo hace (los TOML y las variables los escribe una persona).
pub fn expandir_tilde(ruta: &str) -> PathBuf {
    let casa = || directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    if ruta == "~"
        && let Some(c) = casa() {
            return c;
        }
    if let Some(resto) = ruta.strip_prefix("~/")
        && let Some(c) = casa() {
            return c.join(resto);
        }
    PathBuf::from(ruta)
}

/// La URL del servidor del modelo: `http(s)://` y un host de loopback exacto
/// (`127.0.0.1`, `localhost` o `[::1]`), puerto numérico opcional y sin
/// credenciales. Nada de la red local ni de `0.0.0.0`.
pub fn validar_base_url(url: &str) -> Result<(), String> {
    let host = host_de(url)?;
    if matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]") {
        Ok(())
    } else {
        Err(format!("el host {host:?} no es local: solo 127.0.0.1, localhost o [::1]"))
    }
}

/// URL de un perfil remoto: `http(s)://`, con host, sin credenciales.
pub fn validar_url_remota(url: &str) -> Result<(), String> {
    host_de(url).and_then(|h| if h.is_empty() { Err("falta el host".into()) } else { Ok(()) })
}

/// El host (en minúsculas) de una URL `http(s)://` sin credenciales y con un
/// puerto válido, si lo lleva.
fn host_de(url: &str) -> Result<String, String> {
    let resto = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or("debe empezar por http:// o https://")?;
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("no puede llevar espacios ni caracteres de control".into());
    }
    let autoridad = resto.split(['/', '?', '#']).next().unwrap_or("");
    if autoridad.contains('@') {
        return Err("no puede llevar credenciales (@)".into());
    }
    let (host, puerto) = if let Some(r) = autoridad.strip_prefix('[') {
        let (h, tras) = r.split_once(']').ok_or("host IPv6 sin cerrar")?;
        (format!("[{h}]"), tras.strip_prefix(':'))
    } else {
        match autoridad.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (autoridad.to_string(), None),
        }
    };
    if let Some(p) = puerto
        && (p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) || p.parse::<u32>().map_or(true, |n| n > 65535)) {
            return Err(format!("puerto inválido: {p:?}"));
        }
    Ok(host.to_ascii_lowercase())
}

fn texto_limpio(v: &Value, max: usize) -> Result<String, String> {
    let t = v.as_str().ok_or("se espera un texto")?.trim();
    if t.is_empty() {
        return Err("no puede estar vacío".into());
    }
    if t.chars().count() > max {
        return Err(format!("supera {max} caracteres"));
    }
    if t.chars().any(char::is_control) {
        return Err("no puede llevar caracteres de control".into());
    }
    Ok(t.to_string())
}

/// Comprueba un valor contra el tipo de su clave y lo devuelve normalizado
/// (textos sin espacios en los extremos, listas sin repetidos).
pub fn validar_valor(c: &Clave, v: &Value) -> Result<Value, String> {
    match c.tipo {
        Tipo::Booleano => v.as_bool().map(Value::Bool).ok_or_else(|| "se espera true o false".into()),
        Tipo::Entero { min, max } => {
            let n = v.as_i64().ok_or("se espera un número entero")?;
            if n < min || n > max {
                return Err(format!("{n} está fuera del rango {min}–{max}"));
            }
            Ok(Value::from(n))
        }
        Tipo::Texto { max } => texto_limpio(v, max).map(Value::String),
        Tipo::Url => {
            let t = texto_limpio(v, 500)?;
            validar_base_url(&t)?;
            Ok(Value::String(t))
        }
        Tipo::Ruta => texto_limpio(v, 4096).map(Value::String),
        Tipo::Eleccion { permitidos } => {
            let t = texto_limpio(v, 64)?;
            if !permitidos.contains(&t.as_str()) {
                return Err(format!("{t:?} no es una opción (solo {})", permitidos.join(", ")));
            }
            Ok(Value::String(t))
        }
        Tipo::UrlPerfil => {
            let t = v.as_str().ok_or("se espera un texto")?.trim();
            if !t.is_empty() {
                validar_url_remota(t)?;
                if t.len() > 500 {
                    return Err("supera 500 caracteres".into());
                }
            }
            Ok(Value::String(t.to_string()))
        }
        Tipo::Perfil => {
            let t = v.as_str().ok_or("se espera un texto")?.trim();
            if !t.is_empty() && !id_perfil_valido(t) {
                return Err(format!("{t:?} no es un id de perfil (minúsculas, dígitos, '-' y '_')"));
            }
            Ok(Value::String(t.to_string()))
        }
        Tipo::Lista { permitidos } => {
            let lista = v.as_array().ok_or("se espera una lista de textos")?;
            if lista.len() > 100 {
                return Err("la lista supera 100 elementos".into());
            }
            let mut salida: Vec<String> = Vec::new();
            for e in lista {
                let t = e.as_str().ok_or("cada elemento debe ser un texto")?.trim();
                let ok = !t.is_empty()
                    && t.len() <= 64
                    && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
                if !ok {
                    return Err(format!("elemento inválido: {t:?} (solo letras, dígitos, '-', '_' y '.')"));
                }
                if let Some(p) = permitidos
                    && !p.contains(&t) {
                        return Err(format!("{t:?} no está permitido (solo {p:?})"));
                    }
                if !salida.iter().any(|s| s == t) {
                    salida.push(t.to_string());
                }
            }
            Ok(Value::from(salida))
        }
    }
}

/// Un perfil de modelo completo: con nombre, y si no es remoto, con un
/// `base_url` de loopback.
pub fn validar_perfil(p: &PerfilModelo) -> Result<(), String> {
    if p.nombre.trim().is_empty() {
        return Err("no tiene nombre de modelo".into());
    }
    if !p.remoto {
        if p.base_url.is_empty() {
            return Err("no es remoto y no tiene base_url".into());
        }
        validar_base_url(&p.base_url).map_err(|e| format!("no es remoto y su base_url no vale: {e}"))?;
    }
    Ok(())
}

/// Validación cruzada de agentes y roles (RF-006), sobre perfiles ya
/// validados. Devuelve la clave culpable y el motivo del primer problema:
/// - cada agente apunta a un perfil que existe (o a ninguno);
/// - cada rol lo ocupa un agente del catálogo que lo admite;
/// - el planificador y el ejecutor son agentes distintos;
/// - el ejecutor tiene perfil, y ese perfil no es remoto (art. VII).
pub fn validar_roles(a: &Ajustes) -> Result<(), (String, String)> {
    for (id, g) in &a.agentes {
        if !g.modelo.is_empty() && !a.modelos.contains_key(&g.modelo) {
            return Err((format!("agentes.{id}.modelo"), format!("no existe el perfil de modelo {:?}", g.modelo)));
        }
    }
    for rol in Rol::TODOS {
        let id = match rol {
            Rol::Planificador => &a.roles.planificador,
            Rol::Ejecutor => &a.roles.ejecutor,
        };
        let clave = format!("roles.{}", rol.nombre());
        match ficha(id) {
            None => return Err((clave, format!("{id:?} no es un agente del catálogo"))),
            Some(f) if !f.admite(rol) => return Err((clave, format!("{} no puede ser {}", f.nombre, rol.nombre()))),
            _ => {}
        }
    }
    if a.roles.planificador == a.roles.ejecutor {
        return Err((
            "roles.planificador".into(),
            format!("{} no puede ser planificador y ejecutor a la vez: ningún agente se revisa a sí mismo", a.roles.ejecutor),
        ));
    }
    let ej = &a.roles.ejecutor;
    let clave = format!("agentes.{ej}.modelo");
    match a.agentes.get(ej).map(|g| g.modelo.as_str()).unwrap_or_default() {
        "" => Err((clave, format!("{ej} es el ejecutor y necesita un perfil de modelo local"))),
        m => match a.modelos.get(m) {
            Some(p) if p.remoto => {
                Err((clave, format!("el perfil {m:?} es remoto y {ej} es el ejecutor: el ejecutor solo usa modelos locales")))
            }
            _ => Ok(()),
        },
    }
}
