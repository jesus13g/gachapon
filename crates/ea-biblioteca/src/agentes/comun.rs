//! Piezas comunes de los lectores: ficheros tolerantes, frontmatter, listado
//! de directorios de skills y de ficheros `.md`, y la definición redactada de
//! un servidor MCP. Todo es solo lectura; lo que falta es vacío, no error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::modelo::{Activado, Ambito, DefMcp, Elemento, Mecanismo, Tipo};
use crate::redactar::{nombres_de, redactar_args, redactar_texto, redactar_url};

pub fn leer_texto(ruta: &Path) -> Option<String> {
    std::fs::read_to_string(ruta).ok()
}

/// JSON de un fichero. `None` si no existe; si no se puede interpretar (p. ej.
/// a medio escribir), aviso y `None`. Con `reintento`, vuelve a leerlo una vez
/// tras una pausa breve (lo reescribe el propio agente mientras corre).
pub fn leer_json(ruta: &Path, reintento: bool, avisos: &mut Vec<String>) -> Option<Value> {
    let texto = leer_texto(ruta)?;
    match serde_json::from_str(&texto) {
        Ok(v) => Some(v),
        Err(_) if reintento => {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let texto = leer_texto(ruta)?;
            match serde_json::from_str(&texto) {
                Ok(v) => Some(v),
                Err(e) => {
                    avisos.push(format!("{} no se pudo interpretar ({e}); se ignora", ruta.display()));
                    None
                }
            }
        }
        Err(e) => {
            avisos.push(format!("{} no se pudo interpretar ({e}); se ignora", ruta.display()));
            None
        }
    }
}

/// JSON con comentarios (JSONC): devuelve el valor y si llevaba comentarios.
pub fn leer_jsonc(ruta: &Path, avisos: &mut Vec<String>) -> Option<(Value, bool)> {
    let texto = leer_texto(ruta)?;
    let (limpio, con_comentarios) = quitar_comentarios(&texto);
    match serde_json::from_str(&limpio) {
        Ok(v) => Some((v, con_comentarios)),
        Err(e) => {
            avisos.push(format!("{} no se pudo interpretar ({e}); se ignora", ruta.display()));
            None
        }
    }
}

/// Quita `//`, `/* */` y comas finales respetando las cadenas. Devuelve el
/// texto y si había algún comentario.
pub fn quitar_comentarios(texto: &str) -> (String, bool) {
    let c: Vec<char> = texto.chars().collect();
    let mut sal = String::with_capacity(texto.len());
    let mut hubo = false;
    let mut i = 0;
    let mut en_cadena = false;
    while i < c.len() {
        let ch = c[i];
        if en_cadena {
            sal.push(ch);
            if ch == '\\' && i + 1 < c.len() {
                sal.push(c[i + 1]);
                i += 1;
            } else if ch == '"' {
                en_cadena = false;
            }
        } else if ch == '"' {
            en_cadena = true;
            sal.push(ch);
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            hubo = true;
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            hubo = true;
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            sal.push(ch);
        }
        i += 1;
    }
    // comas finales antes de `}` o `]`
    let s: Vec<char> = sal.chars().collect();
    let mut fin = String::with_capacity(s.len());
    let mut cad = false;
    for (j, &ch) in s.iter().enumerate() {
        if cad {
            fin.push(ch);
            if ch == '\\' {
                continue;
            }
            if ch == '"' && (j == 0 || s[j - 1] != '\\') {
                cad = false;
            }
            continue;
        }
        if ch == '"' {
            cad = true;
        }
        if ch == ',' {
            let sig = s[j + 1..].iter().find(|x| !x.is_whitespace());
            if matches!(sig, Some('}') | Some(']')) {
                continue;
            }
        }
        fin.push(ch);
    }
    (fin, hubo)
}

/// Los pares `clave: valor` del frontmatter (`---` … `---`) de un Markdown.
/// Admite valores entre comillas y bloques `>`/`|` (se unen en una línea).
pub fn frontmatter(texto: &str) -> BTreeMap<String, String> {
    let mut salida = BTreeMap::new();
    let mut lineas = texto.lines();
    if lineas.next().map(str::trim) != Some("---") {
        return salida;
    }
    let mut actual: Option<String> = None;
    for l in lineas {
        if l.trim() == "---" {
            break;
        }
        if l.starts_with([' ', '\t']) {
            if let Some(k) = &actual {
                let e = salida.entry(k.clone()).or_insert_with(String::new);
                if !e.is_empty() {
                    e.push(' ');
                }
                e.push_str(l.trim());
            }
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim();
            let v = if matches!(v, ">" | "|" | ">-" | "|-") {
                String::new()
            } else {
                v.trim_matches(|c| c == '"' || c == '\'').to_string()
            };
            salida.insert(k.clone(), v);
            actual = Some(k);
        }
    }
    salida
}

/// Descripción corta (una línea, hasta 300 caracteres).
pub fn recortar(texto: &str) -> String {
    let uno: String = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    if uno.chars().count() > 300 { uno.chars().take(299).collect::<String>() + "…" } else { uno }
}

/// Crea un elemento; si el nombre no vale como id, deja un aviso.
pub fn crear(avisos: &mut Vec<String>, agente: &str, tipo: Tipo, ambito: Ambito, nombre: &str, ruta: PathBuf, version: &str) -> Option<Elemento> {
    match Elemento::nuevo(agente, tipo, ambito, nombre, ruta.clone()) {
        Ok(mut e) => {
            e.version_formato = version.into();
            Some(e)
        }
        Err(err) => {
            avisos.push(format!("{}: se omite ({err})", ruta.display()));
            None
        }
    }
}

/// Entradas de un directorio, ordenadas, sin las ocultas.
fn entradas(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.path())))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    v.sort();
    v
}

/// ¿Esta ruta (quizá un enlace) apunta dentro de `<repo>/claude/skills`?
fn apunta_al_repo(ruta: &Path, repo: Option<&Path>) -> bool {
    let Some(repo) = repo else { return false };
    let (Ok(destino), Ok(base)) = (std::fs::canonicalize(ruta), std::fs::canonicalize(repo.join("claude/skills"))) else {
        return false;
    };
    destino.starts_with(base)
}

/// El mecanismo de cambio del elemento. Lo que vive en el PROYECTO está
/// versionado en su repo (y su id no distingue un proyecto de otro): no se
/// mueve a un almacén, se edita en el repo.
fn poner_mecanismo(e: &mut Elemento, mecanismo: Mecanismo) {
    if mecanismo == Mecanismo::Mover && e.ambito == Ambito::Proyecto {
        e.mecanismo = Mecanismo::No;
        e.nota = Some(NOTA_PROYECTO.into());
    } else {
        e.mecanismo = mecanismo;
    }
}

/// Por qué un elemento de proyecto no se activa ni se desactiva desde aquí.
pub const NOTA_PROYECTO: &str = "está en el proyecto (versionado): se edita en el repo";

/// Skills de directorio `<dir>/<n>/SKILL.md` (frontmatter `name`/`description`).
/// Un enlace al `claude/skills` del repo pasa a ámbito `ecosistema`. El
/// nombre del elemento es el del directorio (el que se mueve al desactivar).
pub fn skills_en(
    dir: &Path,
    agente: &str,
    ambito: Ambito,
    repo: Option<&Path>,
    mecanismo: Mecanismo,
    version: &str,
    avisos: &mut Vec<String>,
) -> Vec<Elemento> {
    let mut sal = Vec::new();
    for (nombre, ruta) in entradas(dir) {
        let md = ruta.join("SKILL.md");
        let Some(texto) = leer_texto(&md) else { continue };
        let ambito = if apunta_al_repo(&ruta, repo) { Ambito::Ecosistema } else { ambito.clone() };
        let Some(mut e) = crear(avisos, agente, Tipo::Skill, ambito, &nombre, ruta, version) else { continue };
        e.descripcion = recortar(frontmatter(&texto).get("description").map_or("", String::as_str));
        e.activado = Activado::Si;
        poner_mecanismo(&mut e, mecanismo);
        sal.push(e);
    }
    sal
}

/// Ficheros `<dir>/*.md` (subagentes, comandos, skills sueltas).
pub fn md_en(dir: &Path, agente: &str, tipo: Tipo, ambito: Ambito, mecanismo: Mecanismo, version: &str, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let mut sal = Vec::new();
    for (fichero, ruta) in entradas(dir) {
        let Some(nombre) = fichero.strip_suffix(".md") else { continue };
        if !ruta.is_file() {
            continue;
        }
        let Some(mut e) = crear(avisos, agente, tipo, ambito.clone(), nombre, ruta.clone(), version) else { continue };
        let fm = leer_texto(&ruta).map(|t| frontmatter(&t)).unwrap_or_default();
        e.descripcion = recortar(fm.get("description").or_else(|| fm.get("descripcion")).map_or("", String::as_str));
        e.activado = Activado::Si;
        poner_mecanismo(&mut e, mecanismo);
        sal.push(e);
    }
    sal
}

fn texto_de<'a>(v: &'a Value, claves: &[&str]) -> Option<&'a str> {
    claves.iter().find_map(|k| v.get(*k).and_then(Value::as_str)).filter(|s| !s.is_empty())
}

/// La definición redactada de un servidor MCP a partir de su JSON (vale para
/// Claude Code, Qwen, Antigravity, OpenCode y, convertido, Codex).
pub fn def_mcp(v: &Value) -> DefMcp {
    let (comando, args) = match v.get("command") {
        Some(Value::String(c)) => (
            Some(redactar_texto(c)),
            v.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        ),
        // OpenCode: `command` es la lista entera.
        Some(Value::Array(l)) => {
            let mut t: Vec<String> = l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
            let c = (!t.is_empty()).then(|| t.remove(0));
            (c.map(|c| redactar_texto(&c)), t)
        }
        _ => (None, Vec::new()),
    };
    let url = texto_de(v, &["url", "serverUrl", "httpUrl", "server_url"]).map(redactar_url);
    let tipo = texto_de(v, &["type", "transport"]).map(str::to_ascii_lowercase);
    let transporte = match tipo.as_deref() {
        Some("stdio" | "local") => "stdio",
        Some("sse") => "sse",
        Some("http" | "remote" | "streamable-http" | "streamablehttp" | "streamable_http") => "http",
        _ if url.is_some() => "http",
        _ if comando.is_some() => "stdio",
        _ => "desconocido",
    };
    let mut env = nombres_de(v.get("env").or_else(|| v.get("environment")));
    let mut headers = nombres_de(v.get("headers").or_else(|| v.get("http_headers")));
    headers.extend(nombres_de(v.get("env_http_headers")));
    headers.sort();
    env.dedup();
    DefMcp { transporte: transporte.into(), comando, args: redactar_args(&args), url, env, headers }
}

/// Un MCP con su definición ya redactada.
pub fn mcp(
    avisos: &mut Vec<String>,
    agente: &str,
    ambito: Ambito,
    nombre: &str,
    config: &Path,
    def: &Value,
    version: &str,
) -> Option<Elemento> {
    let mut e = crear(avisos, agente, Tipo::Mcp, ambito, nombre, config.to_path_buf(), version)?;
    e.def_mcp = Some(def_mcp(def));
    Some(e)
}
