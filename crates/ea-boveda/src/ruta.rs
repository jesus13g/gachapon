//! Regiones de la bóveda y validación léxica de rutas. Aquí solo se mira el
//! texto de la ruta; la comprobación contra enlaces simbólicos (que exige
//! tocar el disco) está en `Boveda::resolver`.

use crate::error::{ErrorBoveda, Resultado};

/// Región del modelo local: libertad total de escritura (decisión 8).
pub const REGION_LOCAL: &str = "modelo-local";
/// Región de Claude Code: partición por autor, no por tema (§8.3).
pub const REGION_CLAUDE: &str = "claude-code";
/// Región compartida: solo admite contenido por promoción del usuario
/// (decisión 9).
pub const REGION_COMPARTIDA: &str = "compartida";

pub const REGIONES: [&str; 3] = [REGION_LOCAL, REGION_CLAUDE, REGION_COMPARTIDA];

/// Autores que pueden escribir (cada uno en su región).
pub const AGENTES: [&str; 2] = [REGION_LOCAL, REGION_CLAUDE];
/// Autores válidos en una cabecera. `usuario` no escribe por la API de
/// agentes; figura para las notas que el usuario trae a la región compartida.
pub const AUTORES: [&str; 3] = [REGION_LOCAL, REGION_CLAUDE, "usuario"];

pub const MAX_RUTA: usize = 200;
pub const MAX_COMPONENTE: usize = 100;

pub fn es_region(s: &str) -> bool {
    REGIONES.contains(&s)
}

/// La región de escritura de un actor, o `None` si ese actor no escribe en
/// la bóveda (el orquestador desatendido, el usuario por esta vía, etc.).
pub fn region_de_agente(actor: &str) -> Option<&'static str> {
    AGENTES.iter().copied().find(|a| *a == actor)
}

/// Carpeta oculta de la raíz donde van las notas borradas por el usuario
/// (`<raíz>/.papelera/<marca>/<ruta>`). Ninguna ruta válida llega a ella.
pub const PAPELERA: &str = ".papelera";

/// Comprobaciones comunes a notas y carpetas: relativa, sin `..` ni `.`,
/// sin componentes vacíos u ocultos, sin barras invertidas ni caracteres de
/// control, y que empiece por una región. Devuelve los componentes.
fn validar_componentes(ruta: &str, minimo: usize) -> Resultado<Vec<&str>> {
    let mal = |m: &str| Err(ErrorBoveda::Ruta(format!("'{ruta}': {m}")));
    if ruta.is_empty() {
        return mal("vacía");
    }
    if ruta.len() > MAX_RUTA {
        return mal("demasiado larga");
    }
    if ruta.starts_with('/') || ruta.starts_with('~') || ruta.contains(':') && ruta.as_bytes().get(1) == Some(&b':') {
        return mal("debe ser relativa a la bóveda");
    }
    if ruta.contains('\\') {
        return mal("no admite barras invertidas");
    }
    if ruta.chars().any(|c| c.is_control()) {
        return mal("no admite caracteres de control");
    }
    let partes: Vec<&str> = ruta.split('/').collect();
    for p in &partes {
        match *p {
            "" => return mal("componente vacío (barras repetidas o final en '/')"),
            "." | ".." => return mal("no admite '.' ni '..'"),
            p if p.starts_with('.') => return mal("no admite componentes ocultos"),
            p if p.len() > MAX_COMPONENTE => return mal("componente demasiado largo"),
            _ => {}
        }
    }
    if partes.len() < minimo {
        return mal("debe ser 'región/nota.md'");
    }
    if !es_region(partes[0]) {
        return Err(ErrorBoveda::Ruta(format!(
            "'{ruta}': la primera carpeta debe ser una región ({})",
            REGIONES.join(", ")
        )));
    }
    Ok(partes)
}

/// Valida una ruta de nota relativa a la raíz: `region/…/nota.md`. Rechaza
/// lo absoluto, `..`, `.`, componentes vacíos u ocultos, barras invertidas,
/// caracteres de control, y exige que empiece por una región y acabe en
/// `.md`. Devuelve los componentes.
pub fn validar_ruta(ruta: &str) -> Resultado<Vec<&str>> {
    let mal = |m: &str| Err(ErrorBoveda::Ruta(format!("'{ruta}': {m}")));
    let partes = validar_componentes(ruta, 2)?;
    if !partes[partes.len() - 1].ends_with(".md") || partes[partes.len() - 1] == ".md" {
        return mal("debe acabar en .md");
    }
    Ok(partes)
}

/// Valida una ruta de carpeta: `region` o `region/…/carpeta`, con las mismas
/// reglas que una nota salvo la extensión.
pub fn validar_carpeta(ruta: &str) -> Resultado<Vec<&str>> {
    let partes = validar_componentes(ruta, 1)?;
    if partes[partes.len() - 1].ends_with(".md") {
        return Err(ErrorBoveda::Ruta(format!("'{ruta}': una carpeta no acaba en .md")));
    }
    Ok(partes)
}

/// ¿Las dos rutas están en la misma región?
pub fn misma_region(a: &str, b: &str) -> bool {
    region_de(a) == region_de(b)
}

/// Ruta dentro de una región (sin el prefijo de región), lo que escribe un
/// agente: se valida y se le antepone su región. Añade `.md` si falta.
pub fn en_region(region: &str, relativa: &str) -> Resultado<String> {
    let mut r = relativa.trim().to_string();
    if !r.ends_with(".md") {
        r.push_str(".md");
    }
    let completa = format!("{region}/{r}");
    validar_ruta(&completa)?;
    Ok(completa)
}

/// Región de una ruta ya validada.
pub fn region_de(ruta: &str) -> &str {
    ruta.split('/').next().unwrap_or("")
}

/// Un nombre de fichero legible a partir de un título: minúsculas, sin
/// acentos, guiones.
pub fn slug(titulo: &str) -> String {
    let mut s = String::new();
    for c in titulo.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-');
    let s: String = s.chars().take(60).collect();
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "nota".to_string() } else { s }
}
