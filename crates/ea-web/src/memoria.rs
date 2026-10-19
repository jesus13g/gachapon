//! La memoria de Claude Code: `<raíz>/<id-proyecto>/memory/*.md`, un fichero
//! por memoria con frontmatter YAML y `MEMORY.md` como índice. Aquí solo hay
//! rutas y ficheros (nada de HTTP ni de base de datos). Leer es libre; la
//! escritura es sobre memorias que ya existen (reemplazar, renombrar o borrar),
//! siempre con control de versión (hash del contenido) para no pisar un cambio
//! hecho a mano en disco.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Tamaño máximo de una memoria al guardarla.
pub const MAX_BYTES: usize = 256 * 1024;
/// Cuántas líneas de una transcripción se miran buscando su `cwd`.
const LINEAS_CWD: usize = 50;
const INDICE: &str = "MEMORY.md";

#[derive(Debug)]
pub enum ErrorMemoria {
    NoEncontrado(String),
    Invalido(String),
    Conflicto(String),
    NoDisponible(String),
    Interno(String),
}

#[derive(Serialize)]
pub struct Proyecto {
    /// Nombre de la carpeta en `~/.claude/projects` (el cwd con todo lo no
    /// alfanumérico cambiado por `-`).
    pub id: String,
    /// El cwd real, si alguna transcripción lo cuenta (el id no se puede invertir).
    pub cwd: Option<String>,
    pub ficheros: Vec<FicheroMemoria>,
}

#[derive(Serialize)]
pub struct FicheroMemoria {
    pub nombre: String,
    pub titulo: Option<String>,
    pub descripcion: Option<String>,
    pub tipo: Option<String>,
    pub bytes: u64,
    pub modificada: String,
}

#[derive(Serialize)]
pub struct Memoria {
    pub proyecto: String,
    pub nombre: String,
    /// Ruta absoluta del fichero.
    pub ruta: String,
    pub contenido: String,
    /// Hash del contenido: lo que hay que devolver al guardar.
    pub version: String,
    pub modificada: String,
}

/// FNV-1a de 64 bits en hexadecimal: basta para detectar un cambio en disco.
pub fn version(contenido: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in contenido {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn rfc3339(t: SystemTime) -> String {
    OffsetDateTime::from(t).format(&Rfc3339).unwrap_or_default()
}

/// Un componente de ruta que llega de fuera: sin separadores ni `\0`, sin
/// punto inicial (ni `.` ni `..` ni ocultos) y de longitud razonable.
fn componente_valido(s: &str) -> bool {
    !s.is_empty() && s.len() <= 255 && !s.contains(['/', '\\', '\0']) && !s.starts_with('.')
}

/// Valida el par proyecto/fichero antes de formar ninguna ruta.
pub fn nombre_valido(proyecto: &str, fichero: &str) -> Result<(), ErrorMemoria> {
    if !componente_valido(proyecto) {
        return Err(ErrorMemoria::Invalido("nombre de proyecto inválido".into()));
    }
    if !componente_valido(fichero) || !fichero.ends_with(".md") {
        return Err(ErrorMemoria::Invalido("nombre de memoria inválido (un fichero .md)".into()));
    }
    Ok(())
}

fn es_dir_real(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
}

fn es_fichero_real(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// `<raíz>/<proyecto>/memory/<fichero>`, validada: nada de symlinks por el camino.
fn ruta(raiz: &Path, proyecto: &str, fichero: &str) -> Result<PathBuf, ErrorMemoria> {
    nombre_valido(proyecto, fichero)?;
    let dir = raiz.join(proyecto).join("memory");
    let p = dir.join(fichero);
    if !es_dir_real(&raiz.join(proyecto)) || !es_dir_real(&dir) || !es_fichero_real(&p) {
        return Err(ErrorMemoria::NoEncontrado(format!("no existe la memoria {proyecto}/{fichero}")));
    }
    Ok(p)
}

/// Los proyectos con al menos una memoria, ordenados por id.
pub fn listar(raiz: &Path) -> Vec<Proyecto> {
    let Ok(entradas) = std::fs::read_dir(raiz) else { return Vec::new() };
    let mut v: Vec<Proyecto> = entradas
        .flatten()
        .filter(|e| componente_valido(&e.file_name().to_string_lossy()) && es_dir_real(&e.path()))
        .filter_map(|e| {
            let dir = e.path();
            let ficheros = ficheros(&dir.join("memory"));
            (!ficheros.is_empty()).then(|| Proyecto {
                id: e.file_name().to_string_lossy().into_owned(),
                cwd: cwd_de(&dir),
                ficheros,
            })
        })
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

/// Los `.md` regulares de una carpeta `memory/`: el índice primero, luego por nombre.
fn ficheros(dir: &Path) -> Vec<FicheroMemoria> {
    if !es_dir_real(dir) {
        return Vec::new();
    }
    let Ok(entradas) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<FicheroMemoria> = entradas
        .flatten()
        .filter_map(|e| {
            let nombre = e.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            if !meta.is_file() || !nombre.ends_with(".md") || nombre.starts_with('.') {
                return None;
            }
            let texto = std::fs::read_to_string(e.path()).unwrap_or_default();
            let f = frontmatter(&texto);
            Some(FicheroMemoria {
                nombre,
                titulo: f.nombre,
                descripcion: f.descripcion,
                tipo: f.tipo,
                bytes: meta.len(),
                modificada: meta.modified().map(rfc3339).unwrap_or_default(),
            })
        })
        .collect();
    v.sort_by(|a, b| (a.nombre != INDICE, &a.nombre).cmp(&(b.nombre != INDICE, &b.nombre)));
    v
}

#[derive(Default)]
struct Cabecera {
    nombre: Option<String>,
    descripcion: Option<String>,
    tipo: Option<String>,
}

/// El frontmatter, sin crate YAML: solo `name`, `description` y `type`
/// (este último a cualquier sangría, p. ej. bajo `metadata:`). Tolerante: sin
/// bloque `---` inicial y final devuelve todo vacío.
fn frontmatter(texto: &str) -> Cabecera {
    let mut c = Cabecera::default();
    let mut lineas = texto.lines();
    if lineas.next().map(str::trim_end) != Some("---") {
        return c;
    }
    for l in lineas {
        if l.trim_end() == "---" {
            break;
        }
        let sin_sangria = l.trim_start();
        let Some((clave, valor)) = sin_sangria.split_once(':') else { continue };
        let valor = valor.trim().trim_matches(['"', '\'']).trim();
        if valor.is_empty() {
            continue;
        }
        let destino = match (clave, l.len() == sin_sangria.len()) {
            ("name", true) => &mut c.nombre,
            ("description", true) => &mut c.descripcion,
            ("type", _) if c.tipo.is_none() => &mut c.tipo,
            _ => continue,
        };
        *destino = Some(valor.to_string());
    }
    c
}

/// El cwd del proyecto: el primer campo `"cwd"` de las primeras líneas de la
/// transcripción más reciente.
fn cwd_de(dir: &Path) -> Option<String> {
    let reciente = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)?
        .1;
    BufReader::new(std::fs::File::open(reciente).ok()?)
        .lines()
        .take(LINEAS_CWD)
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(&l).ok())
        .find_map(|v| v.get("cwd")?.as_str().filter(|c| !c.is_empty()).map(str::to_string))
}

fn memoria(proyecto: &str, nombre: &str, p: &Path) -> Result<Memoria, ErrorMemoria> {
    let bytes = std::fs::read(p).map_err(|e| ErrorMemoria::Interno(format!("no se pudo leer la memoria: {e}")))?;
    let modificada = std::fs::metadata(p).and_then(|m| m.modified()).map(rfc3339).unwrap_or_default();
    Ok(Memoria {
        proyecto: proyecto.to_string(),
        nombre: nombre.to_string(),
        ruta: p.display().to_string(),
        version: version(&bytes),
        contenido: String::from_utf8(bytes).map_err(|_| ErrorMemoria::Invalido("la memoria no es UTF-8".into()))?,
        modificada,
    })
}

pub fn leer(raiz: &Path, proyecto: &str, fichero: &str) -> Result<Memoria, ErrorMemoria> {
    memoria(proyecto, fichero, &ruta(raiz, proyecto, fichero)?)
}

/// Falla con `Conflicto` si en disco ya no está la versión con la que el
/// usuario empezó; si sigue, devuelve el contenido actual.
fn comprobar_version(p: &Path, version_base: &str) -> Result<Vec<u8>, ErrorMemoria> {
    let actual = std::fs::read(p).map_err(|e| ErrorMemoria::Interno(format!("no se pudo leer la memoria: {e}")))?;
    if version(&actual) != version_base {
        return Err(ErrorMemoria::Conflicto(
            "la memoria cambió en disco desde que la abriste; recárgala".into(),
        ));
    }
    Ok(actual)
}

/// Temporal en el mismo directorio y rename: nunca queda a medias.
fn escribir_atomico(p: &Path, contenido: &[u8]) -> Result<(), ErrorMemoria> {
    let nombre = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = p.with_file_name(format!(".{nombre}.tmp-{}", std::process::id()));
    let escrito = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(contenido).and_then(|_| f.sync_all()))
        .and_then(|_| std::fs::rename(&tmp, p));
    if let Err(e) = escrito {
        let _ = std::fs::remove_file(&tmp);
        return Err(ErrorMemoria::Interno(format!("no se pudo guardar la memoria: {e}")));
    }
    Ok(())
}

/// Reemplaza una memoria que ya existe. Falla con `Conflicto` si en disco ya
/// no tiene la versión con la que el usuario empezó a editar.
pub fn guardar(
    raiz: &Path,
    proyecto: &str,
    fichero: &str,
    contenido: &str,
    version_base: &str,
) -> Result<Memoria, ErrorMemoria> {
    let p = ruta(raiz, proyecto, fichero)?;
    if contenido.len() > MAX_BYTES {
        return Err(ErrorMemoria::Invalido(format!("la memoria supera {} KiB", MAX_BYTES / 1024)));
    }
    comprobar_version(&p, version_base)?;
    escribir_atomico(&p, contenido.as_bytes())?;
    memoria(proyecto, fichero, &p)
}

fn no_es_indice(fichero: &str) -> Result<(), ErrorMemoria> {
    if fichero == INDICE {
        return Err(ErrorMemoria::Invalido(format!("{INDICE} es el índice de Claude Code: no se borra ni se renombra")));
    }
    Ok(())
}

/// Borra una memoria (no el índice) si sigue en la versión que vio el usuario.
/// Devuelve el contenido borrado, para dejarlo en el diario.
pub fn eliminar(raiz: &Path, proyecto: &str, fichero: &str, version_base: &str) -> Result<String, ErrorMemoria> {
    let p = ruta(raiz, proyecto, fichero)?;
    no_es_indice(fichero)?;
    let actual = comprobar_version(&p, version_base)?;
    std::fs::remove_file(&p).map_err(|e| ErrorMemoria::Interno(format!("no se pudo borrar la memoria: {e}")))?;
    Ok(String::from_utf8_lossy(&actual).into_owned())
}

/// Renombra una memoria sin pisar ninguna otra y reescribe los enlaces que
/// apuntaban a ella en las demás memorias del proyecto. Devuelve la memoria
/// con su nombre nuevo y los ficheros cuyos enlaces cambiaron.
pub fn renombrar(
    raiz: &Path,
    proyecto: &str,
    fichero: &str,
    nuevo: &str,
    version_base: &str,
) -> Result<(Memoria, Vec<String>), ErrorMemoria> {
    let p = ruta(raiz, proyecto, fichero)?;
    nombre_valido(proyecto, nuevo)?;
    no_es_indice(fichero)?;
    no_es_indice(nuevo)?;
    if nuevo == fichero {
        return Err(ErrorMemoria::Invalido("el nombre nuevo es el mismo".into()));
    }
    comprobar_version(&p, version_base)?;
    let destino = p.with_file_name(nuevo);
    // `hard_link` falla si el destino existe: así nunca se pisa otra memoria.
    std::fs::hard_link(&p, &destino).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => ErrorMemoria::Conflicto(format!("ya existe la memoria {nuevo}")),
        _ => ErrorMemoria::Interno(format!("no se pudo renombrar la memoria: {e}")),
    })?;
    if let Err(e) = std::fs::remove_file(&p) {
        let _ = std::fs::remove_file(&destino);
        return Err(ErrorMemoria::Interno(format!("no se pudo renombrar la memoria: {e}")));
    }
    let dir = destino.parent().unwrap_or(raiz);
    let (viejo, nuevo_base) = (fichero.trim_end_matches(".md"), nuevo.trim_end_matches(".md"));
    let mut actualizadas = Vec::new();
    for f in ficheros(dir) {
        let fp = dir.join(&f.nombre);
        let Ok(texto) = std::fs::read_to_string(&fp) else { continue };
        if let Some(cambiado) = reescribir_enlaces(&texto, viejo, nuevo_base) {
            escribir_atomico(&fp, cambiado.as_bytes())?;
            actualizadas.push(f.nombre);
        }
    }
    Ok((memoria(proyecto, nuevo, &destino)?, actualizadas))
}

/// Cambia los enlaces a la memoria `viejo` (sin `.md`) por `nuevo`: los
/// `[[viejo]]` (con o sin `.md`, alias `|` o ancla `#`) y los enlaces markdown
/// `](viejo.md)`, `](./viejo.md)` o con ancla. Conserva la forma de cada
/// enlace. `None` si no había ninguno.
pub fn reescribir_enlaces(texto: &str, viejo: &str, nuevo: &str) -> Option<String> {
    let mut out = String::with_capacity(texto.len());
    let mut resto = texto;
    let mut cambios = false;
    while !resto.is_empty() {
        if let Some(tras) = resto.strip_prefix("[[") {
            if let Some(fin) = tras.find("]]").filter(|&f| !tras[..f].contains('\n')) {
                let dentro = &tras[..fin];
                let corte = dentro.find(['|', '#']).unwrap_or(dentro.len());
                let (destino, cola) = dentro.split_at(corte);
                let (base, ext) = match destino.trim().strip_suffix(".md") {
                    Some(b) => (b, ".md"),
                    None => (destino.trim(), ""),
                };
                if base == viejo {
                    out.push_str(&format!("[[{nuevo}{ext}{cola}]]"));
                    cambios = true;
                } else {
                    out.push_str("[[");
                    out.push_str(dentro);
                    out.push_str("]]");
                }
                resto = &tras[fin + 2..];
                continue;
            }
        }
        if let Some(tras) = resto.strip_prefix("](") {
            let (prefijo, sin) = match tras.strip_prefix("./") {
                Some(s) => ("./", s),
                None => ("", tras),
            };
            let objetivo = format!("{viejo}.md");
            if let Some(cola) = sin.strip_prefix(objetivo.as_str()).filter(|c| c.starts_with([')', '#'])) {
                out.push_str(&format!("]({prefijo}{nuevo}.md"));
                cambios = true;
                resto = cola;
                continue;
            }
        }
        let c = resto.chars().next().unwrap_or_default();
        out.push(c);
        resto = &resto[c.len_utf8()..];
    }
    cambios.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::reescribir_enlaces;

    #[test]
    fn reescribe_todas_las_formas_de_enlace() {
        let t = "[[refactor]] [[refactor.md]] [[refactor|el refactor]] [[refactor#Fases]] \
                 [Ver](refactor.md) [Ver](./refactor.md) [Ver](refactor.md#fases)";
        assert_eq!(
            reescribir_enlaces(t, "refactor", "rehecho").unwrap(),
            "[[rehecho]] [[rehecho.md]] [[rehecho|el refactor]] [[rehecho#Fases]] \
                 [Ver](rehecho.md) [Ver](./rehecho.md) [Ver](rehecho.md#fases)"
        );
    }

    #[test]
    fn no_toca_nombres_parecidos_ni_texto_suelto() {
        let t = "[[refactorizar]] [[otro|refactor]] [x](refactorizar.md) [x](otro/refactor.md) refactor.md ñandú";
        assert_eq!(reescribir_enlaces(t, "refactor", "rehecho"), None);
    }

    #[test]
    fn un_corchete_sin_cerrar_no_rompe_nada() {
        let t = "[[refactor\n]] y luego [[refactor]]";
        assert_eq!(reescribir_enlaces(t, "refactor", "r").unwrap(), "[[refactor\n]] y luego [[r]]");
    }
}
