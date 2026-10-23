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
        if let Some(cambiado) = ea_boveda::enlaces::reescribir_por_nombre(&texto, viejo, nuevo_base) {
            escribir_atomico(&fp, cambiado.as_bytes())?;
            actualizadas.push(f.nombre);
        }
    }
    Ok((memoria(proyecto, nuevo, &destino)?, actualizadas))
}

/// Los tipos de memoria que reconoce Claude Code.
pub const TIPOS: [&str; 4] = ["user", "feedback", "project", "reference"];

/// Lo que se pide para crear una memoria nueva.
pub struct NuevaMemoria<'a> {
    /// kebab-case, con o sin `.md`.
    pub fichero: &'a str,
    /// El `name` del frontmatter y el texto del enlace en `MEMORY.md`.
    pub titulo: &'a str,
    /// El `description` del frontmatter y el gancho de la línea del índice.
    pub descripcion: &'a str,
    pub tipo: &'a str,
    pub cuerpo: &'a str,
}

/// El nombre de fichero de una memoria nueva: kebab-case (`a-z`, `0-9` y `-`
/// entre grupos), sin rutas, con `.md` puesto si faltaba y nunca el índice.
fn fichero_nuevo(fichero: &str) -> Result<String, ErrorMemoria> {
    let f = fichero.trim();
    let base = f.strip_suffix(".md").unwrap_or(f);
    let kebab = !base.is_empty()
        && base.split('-').all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
    if !kebab || base.len() > 100 {
        return Err(ErrorMemoria::Invalido(
            "nombre de memoria inválido: kebab-case (minúsculas, cifras y guiones, p. ej. `mi-memoria`)".into(),
        ));
    }
    Ok(format!("{base}.md"))
}

/// Un texto de una sola línea y no vacío.
fn una_linea<'a>(valor: &'a str, que: &str) -> Result<&'a str, ErrorMemoria> {
    let v = valor.trim();
    if v.is_empty() || v.contains(['\n', '\r']) || v.len() > 500 {
        return Err(ErrorMemoria::Invalido(format!("{que}: una línea, no vacía y de hasta 500 caracteres")));
    }
    Ok(v)
}

/// Un valor de YAML: tal cual si es seguro como texto plano; si no, entre comillas.
fn yaml(v: &str) -> String {
    let plano = v.starts_with(|c: char| c.is_alphanumeric()) && !v.contains([':', '#', '"', '\'', '\\']);
    if plano { v.to_string() } else { serde_json::to_string(v).unwrap_or_else(|_| format!("\"{v}\"")) }
}

/// El texto completo de una memoria: cabecera, línea en blanco y cuerpo.
fn texto_memoria(titulo: &str, descripcion: &str, tipo: &str, cuerpo: &str) -> String {
    let mut t = format!(
        "---\nname: {}\ndescription: {}\nmetadata:\n  type: {tipo}\n---\n\n",
        yaml(titulo),
        yaml(descripcion)
    );
    let cuerpo = cuerpo.trim();
    if !cuerpo.is_empty() {
        t.push_str(cuerpo);
        t.push('\n');
    }
    t
}

/// La línea de `MEMORY.md`: `- [Título](fichero.md) — gancho`. Los corchetes
/// del título se escapan para no romper el enlace.
fn linea_indice(titulo: &str, fichero: &str, gancho: &str) -> String {
    let t = titulo.replace('[', "\\[").replace(']', "\\]");
    format!("- [{t}]({fichero}) — {gancho}\n")
}

/// Crea una memoria nueva en un proyecto que ya existe y la anota en
/// `MEMORY.md`. Nunca pisa una memoria existente (`Conflicto`). Si el índice
/// no se puede actualizar, retira la memoria recién creada.
pub fn crear(raiz: &Path, proyecto: &str, n: &NuevaMemoria) -> Result<Memoria, ErrorMemoria> {
    let nombre = fichero_nuevo(n.fichero)?;
    nombre_valido(proyecto, &nombre)?;
    no_es_indice(&nombre)?;
    let titulo = una_linea(n.titulo, "título")?;
    let descripcion = una_linea(n.descripcion, "descripción")?;
    if !TIPOS.contains(&n.tipo) {
        return Err(ErrorMemoria::Invalido(format!("tipo inválido: {}", TIPOS.join(", "))));
    }
    let texto = texto_memoria(titulo, descripcion, n.tipo, n.cuerpo);
    if texto.len() > MAX_BYTES {
        return Err(ErrorMemoria::Invalido(format!("la memoria supera {} KiB", MAX_BYTES / 1024)));
    }
    let dir_proyecto = raiz.join(proyecto);
    if !es_dir_real(&dir_proyecto) {
        return Err(ErrorMemoria::NoEncontrado(format!("no existe el proyecto {proyecto}")));
    }
    let dir = dir_proyecto.join("memory");
    if std::fs::symlink_metadata(&dir).is_ok() {
        if !es_dir_real(&dir) {
            return Err(ErrorMemoria::Invalido("memory/ no es una carpeta".into()));
        }
    } else {
        std::fs::create_dir(&dir).map_err(|e| ErrorMemoria::Interno(format!("no se pudo crear memory/: {e}")))?;
    }
    let destino = dir.join(&nombre);
    // Temporal y `hard_link`: falla si el destino existe, así nunca se pisa nada.
    let tmp = dir.join(format!(".{nombre}.nueva-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(texto.as_bytes()).and_then(|_| f.sync_all()))
        .map_err(|e| ErrorMemoria::Interno(format!("no se pudo crear la memoria: {e}")))?;
    let enlazado = std::fs::hard_link(&tmp, &destino);
    let _ = std::fs::remove_file(&tmp);
    enlazado.map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => ErrorMemoria::Conflicto(format!("ya existe la memoria {nombre}")),
        _ => ErrorMemoria::Interno(format!("no se pudo crear la memoria: {e}")),
    })?;
    if let Err(e) = anotar_en_indice(&dir, &linea_indice(titulo, &nombre, descripcion)) {
        let _ = std::fs::remove_file(&destino);
        return Err(e);
    }
    memoria(proyecto, &nombre, &destino)
}

/// Añade una línea al final de `MEMORY.md` (que se crea si falta).
fn anotar_en_indice(dir: &Path, linea: &str) -> Result<(), ErrorMemoria> {
    let p = dir.join(INDICE);
    let mut texto = match std::fs::symlink_metadata(&p) {
        Ok(m) if m.is_file() => {
            std::fs::read_to_string(&p).map_err(|e| ErrorMemoria::Interno(format!("no se pudo leer {INDICE}: {e}")))?
        }
        Ok(_) => return Err(ErrorMemoria::Invalido(format!("{INDICE} no es un fichero normal"))),
        Err(_) => String::new(),
    };
    if !texto.is_empty() && !texto.ends_with('\n') {
        texto.push('\n');
    }
    texto.push_str(linea);
    escribir_atomico(&p, texto.as_bytes())
}

#[cfg(test)]
mod pruebas_crear {
    use super::*;

    fn nueva(fichero: &str) -> NuevaMemoria<'_> {
        NuevaMemoria { fichero, titulo: "Mi memoria", descripcion: "Qué recuerda", tipo: "feedback", cuerpo: "Cuerpo." }
    }

    #[test]
    fn los_nombres_de_fichero_son_kebab_case() {
        assert_eq!(fichero_nuevo("mi-memoria").unwrap(), "mi-memoria.md");
        assert_eq!(fichero_nuevo("mi-memoria.md").unwrap(), "mi-memoria.md");
        assert_eq!(fichero_nuevo("v2-nota").unwrap(), "v2-nota.md");
        for malo in ["", ".md", "Mi-Memoria", "mi_memoria", "a/b", "../x", "-a", "a-", "a--b", ".oculta", "MEMORY", "con espacio", "x.txt"] {
            assert!(fichero_nuevo(malo).is_err(), "{malo}");
        }
    }

    #[test]
    fn crear_escribe_cabecera_y_linea_de_indice_y_no_pisa() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("p")).unwrap();
        let m = crear(d.path(), "p", &nueva("mi-memoria")).unwrap();
        assert_eq!(m.nombre, "mi-memoria.md");
        assert_eq!(
            m.contenido,
            "---\nname: Mi memoria\ndescription: Qué recuerda\nmetadata:\n  type: feedback\n---\n\nCuerpo.\n"
        );
        assert_eq!(frontmatter(&m.contenido).tipo.as_deref(), Some("feedback"));
        let indice = std::fs::read_to_string(d.path().join("p/memory/MEMORY.md")).unwrap();
        assert_eq!(indice, "- [Mi memoria](mi-memoria.md) — Qué recuerda\n");
        // Un segundo intento no toca nada.
        let otra = NuevaMemoria { cuerpo: "OTRO", ..nueva("mi-memoria") };
        assert!(matches!(crear(d.path(), "p", &otra), Err(ErrorMemoria::Conflicto(_))));
        assert!(std::fs::read_to_string(d.path().join("p/memory/mi-memoria.md")).unwrap().contains("Cuerpo."));
        assert_eq!(std::fs::read_to_string(d.path().join("p/memory/MEMORY.md")).unwrap(), indice);
        // Un índice sin salto final se respeta.
        std::fs::write(d.path().join("p/memory/MEMORY.md"), "- viejo").unwrap();
        crear(d.path(), "p", &nueva("otra")).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("p/memory/MEMORY.md")).unwrap(),
            "- viejo\n- [Mi memoria](otra.md) — Qué recuerda\n"
        );
    }

    #[test]
    fn crear_valida_los_campos_y_el_proyecto() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("p")).unwrap();
        assert!(matches!(crear(d.path(), "no-existe", &nueva("x")), Err(ErrorMemoria::NoEncontrado(_))));
        assert!(matches!(crear(d.path(), "..", &nueva("x")), Err(ErrorMemoria::Invalido(_))));
        for n in [
            NuevaMemoria { tipo: "otro", ..nueva("x") },
            NuevaMemoria { titulo: "  ", ..nueva("x") },
            NuevaMemoria { descripcion: "a\nb", ..nueva("x") },
            NuevaMemoria { fichero: "MEMORY.md", ..nueva("x") },
        ] {
            assert!(matches!(crear(d.path(), "p", &n), Err(ErrorMemoria::Invalido(_))));
        }
        assert!(!d.path().join("p/memory").exists());
    }

    #[test]
    fn los_valores_delicados_se_entrecomillan() {
        assert_eq!(yaml("Hola mundo"), "Hola mundo");
        assert_eq!(yaml("a: b"), "\"a: b\"");
        assert_eq!(yaml("- x"), "\"- x\"");
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("p")).unwrap();
        let n = NuevaMemoria { descripcion: "Usa: dos puntos", ..nueva("x") };
        let m = crear(d.path(), "p", &n).unwrap();
        assert_eq!(frontmatter(&m.contenido).descripcion.as_deref(), Some("Usa: dos puntos"));
    }
}
