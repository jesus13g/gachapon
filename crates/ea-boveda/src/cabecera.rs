//! Contrato de metadatos de cabecera (§8.3): un bloque YAML al principio de
//! cada nota que escribe el ecosistema, para que lo que escribe un agente lo
//! interprete el otro. Es un subconjunto deliberadamente pequeño de YAML
//! (claves planas, cadenas, y una lista en línea para `tags`) con un
//! analizador propio: no se añade una dependencia de YAML para siete campos.
//!
//! ```yaml
//! ---
//! contrato: 1                         # versión del contrato
//! titulo: "Cómo se reinicia ea-mcp"
//! autor: modelo-local                 # modelo-local | claude-code | usuario
//! region: modelo-local                # modelo-local | claude-code | compartida
//! creada: 2026-10-10T12:00:00Z        # RFC 3339, UTC
//! actualizada: 2026-10-10T12:00:00Z
//! origen: "trabajo:m-12"                 # trabajo:<id> | rutina:<slug> | manual
//! promocion: ninguna                  # ninguna | promovida
//! tags: [servicios, mcp]              # opcional
//! # solo tras promover:
//! promovida_de: "modelo-local/x.md"   # en la copia compartida
//! promovida_a: "compartida/x.md"      # en el original
//! promovida_por: usuario
//! promovida_en: 2026-10-10T13:00:00Z
//! ---
//! ```
//!
//! Las notas que escribe el ecosistema se validan de forma estricta; las que
//! trae el usuario (sin cabecera o con otra) se leen con tolerancia.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::{ErrorBoveda, Resultado};
use crate::ruta::{AUTORES, REGIONES, REGION_COMPARTIDA};

pub const CONTRATO: u32 = 1;
pub const PROMOCIONES: [&str; 2] = ["ninguna", "promovida"];
pub const MAX_TITULO: usize = 200;
pub const MAX_TAGS: usize = 10;
pub const MAX_TAG: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cabecera {
    pub titulo: String,
    pub autor: String,
    pub region: String,
    pub creada: String,
    pub actualizada: String,
    pub origen: String,
    pub promocion: String,
    pub tags: Vec<String>,
    pub promovida_de: Option<String>,
    pub promovida_a: Option<String>,
    pub promovida_por: Option<String>,
    pub promovida_en: Option<String>,
    /// Claves que este contrato no conoce: se conservan al reescribir (el
    /// usuario puede haber añadido las suyas en Obsidian).
    pub extras: Vec<(String, String)>,
}

/// Resultado de mirar el principio de un texto.
#[derive(Debug)]
pub enum Cabeceras {
    /// No empieza por un bloque `---`.
    Ausente,
    /// Hay bloque pero no cumple el contrato.
    Invalida(String),
    Valida(Cabecera),
}

pub fn ahora() -> String {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .ok()
        .and_then(|d| d.format(&Rfc3339).ok())
        .unwrap_or_default()
}

fn fecha_valida(s: &str) -> bool {
    OffsetDateTime::parse(s, &Rfc3339).is_ok()
}

fn id_simple(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Origen que se admite al escribir: `trabajo:<id>`, `rutina:<slug>` o `manual`.
pub fn origen_valido(o: &str) -> bool {
    o == "manual"
        || o.strip_prefix("trabajo:").is_some_and(|id| id_simple(id, 64))
        || o.strip_prefix("rutina:").is_some_and(|slug| id_simple(slug, 64))
}

/// Origen que se tolera al leer: además, el `tarea:<n>` de notas antiguas.
pub fn origen_legible(o: &str) -> bool {
    origen_valido(o)
        || o.strip_prefix("tarea:")
            .is_some_and(|id| !id.is_empty() && id.len() <= 18 && id.bytes().all(|b| b.is_ascii_digit()))
}

pub fn titulo_valido(t: &str) -> Resultado<()> {
    let n = t.chars().count();
    if t.trim().is_empty() || n > MAX_TITULO || t.chars().any(char::is_control) || t != t.trim() {
        return Err(ErrorBoveda::Cabecera(format!(
            "el título debe tener entre 1 y {MAX_TITULO} caracteres, en una línea y sin espacios en los extremos"
        )));
    }
    Ok(())
}

pub fn tags_validos(tags: &[String]) -> Resultado<()> {
    if tags.len() > MAX_TAGS {
        return Err(ErrorBoveda::Cabecera(format!("como mucho {MAX_TAGS} etiquetas")));
    }
    for t in tags {
        let ok = !t.is_empty()
            && t.chars().count() <= MAX_TAG
            && t.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/');
        if !ok {
            return Err(ErrorBoveda::Cabecera(format!(
                "etiqueta '{t}' no válida (letras, cifras, '-', '_' o '/'; hasta {MAX_TAG} caracteres)"
            )));
        }
    }
    Ok(())
}

impl Cabecera {
    /// Comprobación estricta del contrato.
    pub fn validar(&self) -> Resultado<()> {
        let mal = |m: String| Err(ErrorBoveda::Cabecera(m));
        titulo_valido(&self.titulo)?;
        if !AUTORES.contains(&self.autor.as_str()) {
            return mal(format!("autor '{}' no válido ({})", self.autor, AUTORES.join(", ")));
        }
        if !REGIONES.contains(&self.region.as_str()) {
            return mal(format!("región '{}' no válida ({})", self.region, REGIONES.join(", ")));
        }
        // Partición por autor: en la región de un agente solo escribe ese agente.
        if self.region != REGION_COMPARTIDA && self.autor != self.region {
            return mal(format!(
                "la región '{}' es de su autor: la nota no puede firmarla '{}'",
                self.region, self.autor
            ));
        }
        for (campo, valor) in [("creada", &self.creada), ("actualizada", &self.actualizada)] {
            if !fecha_valida(valor) {
                return mal(format!("{campo} '{valor}' no es una fecha RFC 3339"));
            }
        }
        if !origen_legible(&self.origen) {
            return mal(format!(
                "origen '{}' no válido (trabajo:<id>, rutina:<slug> o manual)",
                self.origen
            ));
        }
        if !PROMOCIONES.contains(&self.promocion.as_str()) {
            return mal(format!("promocion '{}' no válida ({})", self.promocion, PROMOCIONES.join(", ")));
        }
        if self.region == REGION_COMPARTIDA && self.promocion != "promovida" && self.autor != "usuario" {
            return mal("una nota de agente en la región compartida solo llega por promoción".into());
        }
        tags_validos(&self.tags)?;
        for (clave, valor) in &self.extras {
            let ok = !clave.is_empty()
                && clave.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if !ok || valor.contains('\n') {
                return mal(format!("clave extra '{clave}' no válida"));
            }
        }
        Ok(())
    }

    /// El bloque `---` completo, terminado en salto de línea.
    pub fn render(&self) -> String {
        fn q(s: &str) -> String {
            serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
        }
        let mut s = String::from("---\n");
        s.push_str(&format!("contrato: {CONTRATO}\n"));
        s.push_str(&format!("titulo: {}\n", q(&self.titulo)));
        s.push_str(&format!("autor: {}\n", self.autor));
        s.push_str(&format!("region: {}\n", self.region));
        s.push_str(&format!("creada: {}\n", self.creada));
        s.push_str(&format!("actualizada: {}\n", self.actualizada));
        s.push_str(&format!("origen: {}\n", q(&self.origen)));
        s.push_str(&format!("promocion: {}\n", self.promocion));
        if !self.tags.is_empty() {
            s.push_str(&format!("tags: [{}]\n", self.tags.join(", ")));
        }
        if let Some(v) = &self.promovida_de {
            s.push_str(&format!("promovida_de: {}\n", q(v)));
        }
        if let Some(v) = &self.promovida_a {
            s.push_str(&format!("promovida_a: {}\n", q(v)));
        }
        if let Some(v) = &self.promovida_por {
            s.push_str(&format!("promovida_por: {v}\n"));
        }
        if let Some(v) = &self.promovida_en {
            s.push_str(&format!("promovida_en: {v}\n"));
        }
        for (k, v) in &self.extras {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s.push_str("---\n");
        s
    }

    /// Una nota completa: cabecera, línea en blanco y cuerpo.
    pub fn con_cuerpo(&self, cuerpo: &str) -> String {
        let mut s = self.render();
        s.push('\n');
        s.push_str(cuerpo);
        if !cuerpo.ends_with('\n') {
            s.push('\n');
        }
        s
    }
}

/// Separa el bloque de cabecera del cuerpo. `None` si el texto no empieza
/// por `---`. Devuelve (líneas del bloque, cuerpo).
fn separar(texto: &str) -> Option<(Vec<&str>, &str)> {
    let texto_sin_bom = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let mut lineas = texto_sin_bom.split_inclusive('\n');
    let primera = lineas.next()?;
    if primera.trim_end() != "---" {
        return None;
    }
    let mut bloque = Vec::new();
    let mut consumido = primera.len();
    for l in lineas {
        consumido += l.len();
        if l.trim_end() == "---" {
            let resto = &texto_sin_bom[consumido..];
            let resto = resto.strip_prefix("\r\n").or_else(|| resto.strip_prefix('\n')).unwrap_or(resto);
            return Some((bloque, resto));
        }
        bloque.push(l.trim_end_matches(['\n', '\r']));
    }
    None
}

fn desentrecomillar(v: &str) -> String {
    let v = v.trim();
    if v.starts_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(v) {
            return s;
        }
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].replace("''", "'");
    }
    v.to_string()
}

fn lista_en_linea(v: &str) -> Vec<String> {
    let v = v.trim();
    let dentro = v.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(v);
    dentro.split(',').map(desentrecomillar).filter(|s| !s.is_empty()).collect()
}

/// Claves y valores crudos del bloque, con soporte de la lista en bloque
/// (`tags:` seguido de líneas `- valor`) que escribe Obsidian.
fn pares(lineas: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in lineas {
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        if let Some(item) = l.trim_start().strip_prefix("- ").or_else(|| l.trim_start().strip_prefix("-\t")) {
            if (l.starts_with(' ') || l.starts_with('-')) && let Some((_, v)) = out.last_mut() {
                if !v.is_empty() && !v.starts_with('[') {
                    // valor escalar seguido de un guion: no es una lista
                    continue;
                }
                let item = desentrecomillar(item);
                *v = if v.is_empty() { format!("[{item}]") } else { format!("{}, {item}]", &v[..v.len() - 1]) };
            }
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            if !l.starts_with(' ') {
                out.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

/// Interpreta el principio de un texto contra el contrato. Devuelve también
/// el cuerpo (el texto sin el bloque, si lo había).
pub fn interpretar(texto: &str) -> (Cabeceras, &str) {
    let Some((lineas, cuerpo)) = separar(texto) else {
        return (Cabeceras::Ausente, texto);
    };
    let kv = pares(&lineas);
    let get = |k: &str| kv.iter().find(|(c, _)| c == k).map(|(_, v)| desentrecomillar(v));
    let requerido = |k: &str| get(k).ok_or_else(|| format!("falta '{k}'"));

    let construir = || -> Result<Cabecera, String> {
        let contrato = requerido("contrato")?;
        if contrato.parse::<u32>().ok() != Some(CONTRATO) {
            return Err(format!("contrato '{contrato}' no soportado (se esperaba {CONTRATO})"));
        }
        let conocidas = [
            "contrato", "titulo", "autor", "region", "creada", "actualizada", "origen", "promocion",
            "tags", "promovida_de", "promovida_a", "promovida_por", "promovida_en",
        ];
        Ok(Cabecera {
            titulo: requerido("titulo")?,
            autor: requerido("autor")?,
            region: requerido("region")?,
            creada: requerido("creada")?,
            actualizada: requerido("actualizada")?,
            origen: requerido("origen")?,
            promocion: requerido("promocion")?,
            tags: kv.iter().find(|(c, _)| c == "tags").map(|(_, v)| lista_en_linea(v)).unwrap_or_default(),
            promovida_de: get("promovida_de"),
            promovida_a: get("promovida_a"),
            promovida_por: get("promovida_por"),
            promovida_en: get("promovida_en"),
            extras: kv.iter().filter(|(k, _)| !conocidas.contains(&k.as_str())).cloned().collect(),
        })
    };
    match construir().and_then(|c| c.validar().map(|_| c).map_err(|e| e.to_string())) {
        Ok(c) => (Cabeceras::Valida(c), cuerpo),
        Err(e) => (Cabeceras::Invalida(e), cuerpo),
    }
}

/// Primer encabezado `# …` del cuerpo, para titular una nota ajena.
pub fn primer_encabezado(cuerpo: &str) -> Option<String> {
    cuerpo
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}
