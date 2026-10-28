//! Activar y desactivar elementos de la biblioteca (spec 018, RF-007 a RF-009).
//!
//! Dos pasos, como el resto de acciones del ecosistema:
//!
//! 1. [`plan_cambio`] (puro, solo lee): dado un [`Elemento`], dice QUÉ se haría
//!    ([`Cambio`]: mecanismo, objetivo, valor antes y después). Es lo que se
//!    simula y lo que se guarda en la propuesta.
//! 2. [`aplicar`] (el único que escribe): REVALIDA (recalcula el cambio con el
//!    estado de ahora y exige que sea idéntico al simulado, incluida la huella
//!    del fichero) y lo ejecuta con el mecanismo del elemento.
//!
//! # Mecanismos, por orden de preferencia
//!
//! - **Comando oficial** (argv sin shell, con tope de tiempo, vía [`Ordenes`]):
//!   `claude plugin enable|disable --scope user|project <id@mkt>`,
//!   `agy mcp|plugin enable|disable <n>` y
//!   `qwen extensions enable|disable --scope User <n>`. Sin el binario, error
//!   «binario no disponible».
//! - **Campo** (`enabled`) en la configuración del agente, con copia previa en
//!   `<estado>/biblioteca/copias/<marca>-<fichero>`, escritura atómica que
//!   conserva los permisos (y sigue un enlace simbólico hasta el fichero
//!   real) y sin tocar el formato del resto del fichero:
//!   `[mcp_servers.<n>].enabled` de `~/.codex/config.toml` (`toml_edit`) y
//!   `mcp.<n>.enabled` / `mcp.servers.<n>.enabled` de `opencode.json` (edición
//!   sobre el texto, ver [`json_texto`]). Un JSONC con comentarios se rechaza.
//!   Convención de ida y vuelta: **desactivar escribe `enabled = false`**
//!   (sustituyendo el valor si había uno, o añadiendo la clave al final de la
//!   tabla) y **activar quita la clave** (el valor por defecto de los dos
//!   agentes es «activado»). Así desactivar + activar deja el fichero
//!   byte a byte igual salvo si la clave era un `enabled = true` explícito
//!   (que se pierde; el efecto es el mismo).
//!   Codex 0.159.3 respeta `enabled = false` (`codex mcp list --json` lo
//!   informa como `"enabled": false`, comprobado con un `CODEX_HOME` temporal).
//! - **Mover** (skills, subagentes y comandos de fichero, NO los de proyecto,
//!   que están versionados en su repo y se editan allí): el fichero, el
//!   directorio o el ENLACE (con `renameat2` y `RENAME_NOREPLACE`, nunca se
//!   sigue el destino ni se pisa uno creado entre medias, y bajo un `flock`
//!   exclusivo sobre `desactivadas/.lock`) va a
//!   `<estado>/biblioteca/desactivadas/<agente>/<tipo>/<nombre>` y se apunta
//!   en `indice.json` de ese directorio con su origen; al activar vuelve. Si
//!   el sitio original está ocupado, se rechaza. `rename` exige el mismo
//!   sistema de ficheros (si no, error claro; no se copia nada).
//! - **Ajustes** (skills del ejecutor): `agente.skills.habilitadas` con
//!   `ea_ajustes::establecer`. Esa clave solo se edita desde una terminal real
//!   (afecta a lo que puede hacer el ejecutor, art. VIII): sin TTY el cambio
//!   se rechaza con el mensaje de ea-ajustes. Lista vacía = todas; si el
//!   resultado equivale a «todas», se quita la clave.
//!
//! `SoloSesion` y `No` se rechazan con «no se puede cambiar: <motivo>».
//! `~/.claude.json` nunca se escribe. Nada de esto instala ni desinstala.

mod json_texto;
mod ordenes;

use std::collections::BTreeMap;
use std::fs;
use ea_plataforma::fs::{escribir_atomico, opciones_privadas, ErrorRenombrar, Permisos};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ea_ajustes::{Ajustes, Canal, Fuentes};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use ordenes::{Ordenes, OrdenesReales, SalidaOrden, LIMITE};
#[cfg(any(test, feature = "dobles"))]
pub use ordenes::OrdenesDoble;

use crate::agentes::comun::quitar_comentarios;
use crate::error::ErrorBiblioteca;
use crate::modelo::{Activado, Ambito, Elemento, Id, Mecanismo, Tipo};
use crate::raices::Raices;
use crate::Biblioteca;

type Resultado<T> = Result<T, ErrorBiblioteca>;

/// La clave de ajustes de las skills del ejecutor.
pub const CLAVE_SKILLS: &str = "agente.skills.habilitadas";

/// Dónde se escribe y con qué permisos: las raíces, de dónde salen los
/// ajustes y por qué canal llega el cambio de ajustes.
#[derive(Debug, Clone)]
pub struct Contexto {
    pub raices: Raices,
    pub fuentes: Fuentes,
    /// Canal con que se escribe `agente.skills.habilitadas` (solo `Terminal`
    /// lo admite).
    pub canal: Canal,
}

impl Contexto {
    pub fn nuevo(raices: Raices, fuentes: Fuentes, canal: Canal) -> Self {
        Self { raices, fuentes, canal }
    }

    /// El entorno real con el canal `Web` (el más restrictivo): para planear.
    /// Quien APLICA pasa el canal real de la confirmación con
    /// [`Contexto::sistema_con_canal`]; el canal nunca se deduce del TTY del
    /// proceso (un `ea-web` lanzado desde una terminal no es una terminal).
    pub fn sistema() -> Self {
        Self::con_raices(Raices::desde_entorno())
    }

    /// El entorno real con el canal por el que el usuario confirmó.
    pub fn sistema_con_canal(canal: Canal) -> Self {
        Self { raices: Raices::desde_entorno(), fuentes: Fuentes::sistema(), canal }
    }

    /// Con las raíces dadas y el canal más restrictivo (`Web`): las funciones
    /// `plan_cambio` y `aplicar` no pueden cambiar `agente.skills.habilitadas`.
    pub fn con_raices(raices: Raices) -> Self {
        Self { raices, fuentes: Fuentes::sistema(), canal: Canal::Web }
    }

    fn ajustes(&self) -> Ajustes {
        ea_ajustes::cargar_de(&self.fuentes).ajustes
    }
}

/// Sobre qué actúa el cambio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Objetivo {
    /// Un campo de un fichero de configuración: `campo` es la ruta de claves
    /// hasta `enabled`; `huella` es la del fichero entero al simular.
    Fichero { ruta: PathBuf, campo: Vec<String>, huella: String },
    /// Un comando oficial (argv sin shell) y su directorio de trabajo.
    Orden { argv: Vec<String>, cwd: Option<PathBuf> },
    /// Mover un fichero, directorio o enlace.
    Mover { desde: PathBuf, hacia: PathBuf },
    /// Una clave de `ajustes.toml`.
    Ajuste { clave: String },
}

/// Lo que se haría: se simula, se guarda en la propuesta y se revalida.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cambio {
    pub id: String,
    pub activar: bool,
    pub mecanismo: Mecanismo,
    pub objetivo: Objetivo,
    /// El valor de ahora: `"si"`/`"no"` (orden), el valor del campo (`null`: sin
    /// la clave), `{"en": ruta}` (mover) o la lista de ajustes.
    pub antes: Value,
    pub despues: Value,
}

impl Cambio {
    /// Descripción en español para la simulación.
    pub fn describir(&self) -> String {
        let accion = if self.activar { "Activaría" } else { "Desactivaría" };
        let que = match &self.objetivo {
            Objetivo::Orden { argv, cwd } => {
                let en = cwd.as_ref().map(|c| format!(" (en {})", c.display())).unwrap_or_default();
                format!("ejecutando `{}`{en}", argv.join(" "))
            }
            Objetivo::Fichero { ruta, campo, .. } => {
                format!("cambiando `{}` en {} (con copia previa en el estado del ecosistema)", campo.join("."), ruta.display())
            }
            Objetivo::Mover { desde, hacia } => format!("moviendo {} a {}", desde.display(), hacia.display()),
            Objetivo::Ajuste { clave } => format!("cambiando `{clave}` de ajustes.toml"),
        };
        format!("{accion} {} {que}: antes {}, después {}.", self.id, self.antes, self.despues)
    }

    /// Si hay que lanzar un binario, cuál.
    pub fn binario(&self) -> Option<&str> {
        match &self.objetivo {
            Objetivo::Orden { argv, .. } => argv.first().map(String::as_str),
            _ => None,
        }
    }
}

/// Lo que ocurrió al aplicar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hecho {
    pub id: String,
    pub activar: bool,
    pub mecanismo: Mecanismo,
    pub objetivo: Objetivo,
    pub antes: Value,
    pub despues: Value,
    /// La copia previa del fichero, si el mecanismo es un campo.
    pub copia: Option<PathBuf>,
    pub texto: String,
}

// ---------------------------------------------------------------------------
// Planificar
// ---------------------------------------------------------------------------

/// Qué cambio produciría activar o desactivar `el`. Solo lee.
pub fn plan_cambio(el: &Elemento, activar: bool, r: &Raices) -> Resultado<Cambio> {
    plan_cambio_con(el, activar, &Contexto::con_raices(r.clone()))
}

pub fn plan_cambio_con(el: &Elemento, activar: bool, ctx: &Contexto) -> Resultado<Cambio> {
    let no = |m: &str| Err(ErrorBiblioteca::NoSePuede(m.to_string()));
    match el.mecanismo {
        Mecanismo::SoloSesion => {
            return no("está definido en ~/.claude.json o .mcp.json, que la biblioteca no edita: solo se puede acotar al abrir una sesión");
        }
        Mecanismo::No => {
            if let Ambito::Plugin(p) = &el.ambito {
                return no(&format!("viene con el plugin {p}: se cambia el plugin"));
            }
            return no(el.nota.as_deref().unwrap_or("el agente no ofrece un interruptor para esto"));
        }
        _ => {}
    }
    // Un elemento de proyecto no se mueve (versionado; ids que colisionan entre
    // proyectos). Se puede RECUPERAR uno que una versión anterior ya guardó.
    if el.mecanismo == Mecanismo::Mover && el.ambito == Ambito::Proyecto && !activar {
        return no(crate::agentes::comun::NOTA_PROYECTO);
    }
    match (el.activado, activar) {
        (Activado::Si, true) => return no("ya está activado"),
        (Activado::No, false) => return no("ya está desactivado"),
        _ => {}
    }
    match el.mecanismo {
        Mecanismo::ComandoOficial => plan_orden(el, activar, ctx),
        Mecanismo::Campo => plan_campo(el, activar),
        Mecanismo::Mover => plan_mover(el, activar, &ctx.raices),
        Mecanismo::Ajustes => plan_ajustes(el, activar, ctx),
        Mecanismo::SoloSesion | Mecanismo::No => unreachable!("ya se rechazaron"),
    }
}

fn estado_texto(activado: bool) -> Value {
    json!(if activado { "si" } else { "no" })
}

fn plan_orden(el: &Elemento, activar: bool, ctx: &Contexto) -> Resultado<Cambio> {
    let verbo = if activar { "enable" } else { "disable" };
    let n = el.nombre.as_str();
    if n.starts_with('-') || n.is_empty() {
        return Err(ErrorBiblioteca::NoSePuede(format!("el nombre {n:?} podría leerse como una opción")));
    }
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (argv, cwd) = match (el.agente.as_str(), el.tipo) {
        ("claude-code", Tipo::Plugin) => {
            let (ambito, cwd) = match &el.ambito {
                Ambito::Usuario => ("user", Some(ctx.raices.home.clone())),
                Ambito::Proyecto => ("project", ctx.raices.proyecto.clone()),
                otro => return Err(ErrorBiblioteca::NoSePuede(format!("ámbito {otro} sin comando oficial"))),
            };
            (a(&["claude", "plugin", verbo, "--scope", ambito, n]), cwd)
        }
        ("qwen-code", Tipo::Extension) => (a(&["qwen", "extensions", verbo, "--scope", "User", n]), Some(ctx.raices.home.clone())),
        ("antigravity", Tipo::Mcp) => (a(&["agy", "mcp", verbo, n]), None),
        ("antigravity", Tipo::Plugin) => (a(&["agy", "plugin", verbo, n]), None),
        (ag, t) => return Err(ErrorBiblioteca::NoSePuede(format!("{ag} no tiene comando oficial para un elemento de tipo {t}"))),
    };
    Ok(Cambio {
        id: el.id.clone(),
        activar,
        mecanismo: Mecanismo::ComandoOficial,
        objetivo: Objetivo::Orden { argv, cwd },
        antes: estado_texto(!activar),
        despues: estado_texto(activar),
    })
}

// -- campo -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Formato {
    Toml,
    Json,
}

/// El valor de `enabled` en un fichero de configuración, y la ruta de claves
/// del campo.
fn leer_campo(el: &Elemento, texto: &str) -> Resultado<(Formato, Vec<String>, Option<bool>)> {
    let no = |m: String| ErrorBiblioteca::NoSePuede(m);
    match (el.agente.as_str(), el.tipo) {
        ("codex", Tipo::Mcp) => {
            let doc: toml_edit::DocumentMut = texto.parse().map_err(|e: toml_edit::TomlError| no(format!("config.toml no es TOML válido: {}", e.message())))?;
            let t = doc
                .get("mcp_servers")
                .and_then(|i| i.as_table_like())
                .and_then(|t| t.get(&el.nombre))
                .and_then(|i| i.as_table_like())
                .ok_or_else(|| no(format!("[mcp_servers.{}] ya no está en {}", el.nombre, el.ruta.display())))?;
            let actual = match t.get("enabled") {
                None => None,
                Some(i) => Some(i.as_bool().ok_or_else(|| no("`enabled` no es un booleano".into()))?),
            };
            Ok((Formato::Toml, vec!["mcp_servers".into(), el.nombre.clone(), "enabled".into()], actual))
        }
        ("opencode", Tipo::Mcp) => {
            let (limpio, comentarios) = quitar_comentarios(texto);
            if comentarios {
                return Err(no("JSONC con comentarios: no se edita".into()));
            }
            let j: Value = serde_json::from_str(&limpio).map_err(|e| no(format!("el JSON no se puede interpretar: {e}")))?;
            // La misma regla que el lector: `mcp.servers` es un contenedor si
            // es un objeto que no parece un servidor.
            let mcp = j.get("mcp").and_then(Value::as_object).ok_or_else(|| no("no hay `mcp` en el fichero".into()))?;
            let parece_servidor = |v: &Value| ["type", "command", "url", "enabled"].iter().any(|k| v.get(*k).is_some());
            let con_servers = matches!(mcp.get("servers"), Some(s) if s.is_object() && !parece_servidor(s));
            let (mut ruta, def) = if con_servers {
                (vec!["mcp".to_string(), "servers".to_string(), el.nombre.clone()], mcp["servers"].get(&el.nombre))
            } else {
                (vec!["mcp".to_string(), el.nombre.clone()], mcp.get(&el.nombre))
            };
            let def = def.filter(|d| d.is_object()).ok_or_else(|| no(format!("el servidor {} ya no está en {}", el.nombre, el.ruta.display())))?;
            let actual = match def.get("enabled") {
                None => None,
                Some(v) => Some(v.as_bool().ok_or_else(|| no("`enabled` no es un booleano".into()))?),
            };
            ruta.push("enabled".into());
            Ok((Formato::Json, ruta, actual))
        }
        (ag, t) => Err(no(format!("{ag} no tiene un campo `enabled` para un elemento de tipo {t}"))),
    }
}

fn plan_campo(el: &Elemento, activar: bool) -> Resultado<Cambio> {
    let texto = fs::read_to_string(&el.ruta).map_err(|e| ErrorBiblioteca::Io(format!("{}: {e}", el.ruta.display())))?;
    let (_, campo, actual) = leer_campo(el, &texto)?;
    let (antes, despues) = if activar {
        if actual != Some(false) {
            return Err(ErrorBiblioteca::NoSePuede("ya está activado".into()));
        }
        (json!(false), Value::Null)
    } else {
        if actual == Some(false) {
            return Err(ErrorBiblioteca::NoSePuede("ya está desactivado".into()));
        }
        (actual.map_or(Value::Null, Value::Bool), json!(false))
    };
    Ok(Cambio {
        id: el.id.clone(),
        activar,
        mecanismo: Mecanismo::Campo,
        objetivo: Objetivo::Fichero { ruta: el.ruta.clone(), campo, huella: ea_ajustes::huella(texto.as_bytes()) },
        antes,
        despues,
    })
}

/// El texto del fichero con el campo cambiado (y comprobado releyéndolo).
fn editar_campo(el: &Elemento, formato: Formato, texto: &str, campo: &[String], poner_falso: bool) -> Resultado<String> {
    let el_nombre = el.nombre.as_str();
    let nuevo = match formato {
        Formato::Toml => {
            let mut doc: toml_edit::DocumentMut = texto.parse().map_err(|e: toml_edit::TomlError| ErrorBiblioteca::Io(format!("TOML no válido: {}", e.message())))?;
            let t = doc
                .as_table_mut()
                .get_mut(&campo[0])
                .and_then(|i| i.as_table_like_mut())
                .and_then(|t| t.get_mut(&campo[1]))
                .and_then(|i| i.as_table_like_mut())
                .ok_or_else(|| ErrorBiblioteca::Io(format!("falta [mcp_servers.{el_nombre}]")))?;
            if poner_falso {
                let mut v = toml_edit::value(false);
                // Se sustituye en el sitio: se conserva el comentario de la línea.
                if let Some(viejo) = t.get_mut("enabled") {
                    if let (Some(v), Some(vv)) = (viejo.as_value(), v.as_value_mut()) {
                        *vv.decor_mut() = v.decor().clone();
                    }
                    *viejo = v;
                } else {
                    t.insert("enabled", v);
                }
            } else {
                t.remove("enabled");
            }
            let mut t = doc.to_string();
            // toml_edit añade el salto de línea final que faltara: se conserva el estado original.
            if !texto.ends_with('\n') && t.ends_with('\n') {
                t.pop();
            }
            t
        }
        Formato::Json => {
            let ruta: Vec<&str> = campo[..campo.len() - 1].iter().map(String::as_str).collect();
            if poner_falso {
                json_texto::poner(texto, &ruta, "enabled", "false")?
            } else {
                json_texto::quitar(texto, &ruta, "enabled")?
            }
        }
    };
    // Comprobación: lo escrito se lee como se quería y el resto no cambió de sentido.
    let (_, _, leido) = leer_campo(el, &nuevo).map_err(|e| ErrorBiblioteca::Io(format!("el resultado no se pudo releer: {e}")))?;
    let esperado = if poner_falso { Some(false) } else { None };
    if leido != esperado {
        return Err(ErrorBiblioteca::Io("el resultado de la edición no tiene el valor esperado; no se escribe nada".into()));
    }
    Ok(nuevo)
}

fn aplicar_campo(c: &Cambio, ruta: &Path, campo: &[String], ctx: &Contexto, el: &Elemento) -> Resultado<(Option<PathBuf>, String)> {
    let texto = fs::read_to_string(ruta)?;
    let (formato, _, _) = leer_campo(el, &texto)?;
    let nuevo = editar_campo(el, formato, &texto, campo, !c.activar)?;
    let copia = copia_previa(&ctx.raices, ruta)?;
    escribir_atomico_conservando(ruta, nuevo.as_bytes())?;
    Ok((Some(copia), format!("`{}` {} en {}", campo.join("."), if c.activar { "quitado" } else { "puesto a false" }, ruta.display())))
}

/// `<estado>/biblioteca/copias/<marca>-<fichero>` con el contenido de `ruta`.
fn copia_previa(r: &Raices, ruta: &Path) -> Resultado<PathBuf> {
    let dir = r.estado.join("biblioteca/copias");
    fs::create_dir_all(&dir)?;
    let nombre = ruta.file_name().and_then(|n| n.to_str()).unwrap_or("fichero");
    let destino = dir.join(format!("{}-{nombre}", marca_tiempo()));
    let origen = ea_plataforma::rutas::canonica(ruta)?;
    let mut f = opciones_privadas().write(true).create_new(true).open(&destino)?;
    let mut src = fs::File::open(origen)?;
    std::io::copy(&mut src, &mut f)?;
    f.sync_all()?;
    Ok(destino)
}

/// `AAAAMMDDThhmmssZ-<nanos>` en UTC (único dentro del proceso).
fn marca_tiempo() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let (dias, resto) = ((d.as_secs() / 86_400) as i64, d.as_secs() % 86_400);
    // Fecha civil desde días (algoritmo de H. Hinnant).
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let anio = yoe + era * 400 + i64::from(mes <= 2);
    format!(
        "{anio:04}{mes:02}{dia:02}T{:02}{:02}{:02}Z-{:06}{:03}",
        resto / 3600,
        resto % 3600 / 60,
        resto % 60,
        d.subsec_micros(),
        N.fetch_add(1, Ordering::Relaxed) % 1000
    )
}

/// Escritura atómica (la de `ea-plataforma`) con los permisos del original
/// (en Unix, los bits de modo; en Windows, el atributo de solo lectura); si
/// `ruta` es un enlace simbólico se escribe el fichero real.
fn escribir_atomico_conservando(ruta: &Path, bytes: &[u8]) -> Resultado<()> {
    let destino = ea_plataforma::rutas::canonica(ruta)?;
    escribir_atomico(&destino, bytes, Permisos::ComoOriginal)?;
    Ok(())
}

// -- mover -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EntradaIndice {
    origen: PathBuf,
    almacen: PathBuf,
    desactivada: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Indice {
    #[serde(default)]
    elementos: BTreeMap<String, EntradaIndice>,
}

fn base_desactivadas(r: &Raices) -> PathBuf {
    r.estado.join("biblioteca/desactivadas")
}

fn ruta_indice(r: &Raices) -> PathBuf {
    base_desactivadas(r).join("indice.json")
}

fn leer_indice(r: &Raices) -> Resultado<Indice> {
    match fs::read_to_string(ruta_indice(r)) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| ErrorBiblioteca::Io(format!("indice.json no se puede interpretar: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Indice::default()),
        Err(e) => Err(e.into()),
    }
}

fn escribir_indice(r: &Raices, i: &Indice) -> Resultado<()> {
    let mut texto = serde_json::to_string_pretty(i).map_err(|e| ErrorBiblioteca::Io(e.to_string()))?;
    texto.push('\n');
    ea_ajustes::escribir_atomico(&ruta_indice(r), texto.as_bytes())?;
    Ok(())
}

/// ¿Existe algo en `ruta`, aunque sea un enlace roto? (no se sigue el enlace)
fn existe(ruta: &Path) -> bool {
    fs::symlink_metadata(ruta).is_ok()
}

fn sin_subidas(p: &Path) -> bool {
    p.is_absolute() && !p.components().any(|c| matches!(c, Component::ParentDir))
}

/// Una entrada del índice es de fiar si el almacén está bajo el nuestro y el
/// origen es una ruta absoluta limpia dentro del HOME o del proyecto.
fn entrada_valida(e: &EntradaIndice, r: &Raices) -> bool {
    sin_subidas(&e.almacen)
        && e.almacen.starts_with(base_desactivadas(r))
        && sin_subidas(&e.origen)
        && (e.origen.starts_with(&r.home) || r.proyecto.as_ref().is_some_and(|p| e.origen.starts_with(p)))
}

/// Los elementos que la biblioteca ha desactivado moviéndolos: ya no están en
/// su sitio, así que el inventario no los ve. Se devuelven con
/// `activado: no`, `mecanismo: mover` y la ruta de ORIGEN, para poder
/// activarlos con [`plan_cambio`].
pub fn desactivadas(r: &Raices) -> Vec<Elemento> {
    let Ok(indice) = leer_indice(r) else { return Vec::new() };
    indice
        .elementos
        .iter()
        .filter(|(_, e)| entrada_valida(e, r) && existe(&e.almacen))
        .filter_map(|(id, e)| {
            let id = Id::analizar(id).ok()?;
            let mut el = Elemento::nuevo(&id.agente, id.tipo, id.ambito, &id.nombre, e.origen.clone()).ok()?;
            el.activado = Activado::No;
            el.mecanismo = Mecanismo::Mover;
            Some(el)
        })
        .collect()
}

fn plan_mover(el: &Elemento, activar: bool, r: &Raices) -> Resultado<Cambio> {
    let indice = leer_indice(r)?;
    let (desde, hacia) = if activar {
        let e = indice.elementos.get(&el.id).ok_or_else(|| ErrorBiblioteca::NoSePuede("no lo desactivó la biblioteca: no hay dónde recuperarlo".into()))?;
        if !entrada_valida(e, r) {
            return Err(ErrorBiblioteca::NoSePuede(format!("el índice de desactivadas apunta fuera de sitio ({}); no se toca", e.origen.display())));
        }
        if !existe(&e.almacen) {
            return Err(ErrorBiblioteca::NoSePuede(format!("falta {} en el almacén", e.almacen.display())));
        }
        if existe(&e.origen) {
            return Err(ErrorBiblioteca::Ocupado(format!("{} ya existe; no se pisa", e.origen.display())));
        }
        (e.almacen.clone(), e.origen.clone())
    } else {
        if !existe(&el.ruta) {
            return Err(ErrorBiblioteca::NoSePuede(format!("{} ya no existe", el.ruta.display())));
        }
        let fichero = el.ruta.file_name().ok_or_else(|| ErrorBiblioteca::NoSePuede("ruta sin nombre".into()))?;
        let hacia = base_desactivadas(r).join(&el.agente).join(el.tipo.as_str()).join(fichero);
        if existe(&hacia) || indice.elementos.contains_key(&el.id) {
            return Err(ErrorBiblioteca::Ocupado(format!("{} ya está ocupado en el almacén de desactivadas", hacia.display())));
        }
        (el.ruta.clone(), hacia)
    };
    Ok(Cambio {
        id: el.id.clone(),
        activar,
        mecanismo: Mecanismo::Mover,
        antes: json!({ "en": desde }),
        despues: json!({ "en": hacia }),
        objetivo: Objetivo::Mover { desde, hacia },
    })
}

/// Mueve `a` a `b` sin pisar nada, atómico frente a un destino creado entre
/// la comprobación y el movimiento (lo resuelve `ea-plataforma` por sistema).
fn renombrar_sin_pisar(a: &Path, b: &Path) -> Result<(), ErrorRenombrar> {
    ea_plataforma::fs::renombrar_sin_pisar(a, b)
}

/// `renombrar_sin_pisar` con errores en español.
fn mover_sin_pisar(a: &Path, b: &Path) -> Resultado<()> {
    if let Some(p) = b.parent() {
        fs::create_dir_all(p)?;
    }
    renombrar_sin_pisar(a, b).map_err(|e| match e {
        ErrorRenombrar::OtroDispositivo => {
            ErrorBiblioteca::Io(format!("{} y {} están en sistemas de ficheros distintos: no se puede mover sin copiar", a.display(), b.display()))
        }
        ErrorRenombrar::Existe => ErrorBiblioteca::Ocupado(format!("{} ya existe; no se pisa", b.display())),
        ErrorRenombrar::NoSoportado(m) => ErrorBiblioteca::Io(format!("no se puede mover {} a {} sin riesgo de pisar algo: {m}", a.display(), b.display())),
        ErrorRenombrar::Io(e) => e.into(),
    })
}

/// Cerrojo exclusivo de `<estado>/biblioteca/desactivadas/.lock`: serializa
/// leer el índice, mover y escribirlo. Se suelta al cerrar el fichero.
fn bloquear_almacen(r: &Raices) -> Resultado<fs::File> {
    let base = base_desactivadas(r);
    fs::create_dir_all(&base)?;
    let ruta = base.join(".lock");
    let f = opciones_privadas().read(true).write(true).create(true).truncate(false).open(&ruta)?;
    f.lock().map_err(|e| ErrorBiblioteca::Io(format!("{}: no se pudo tomar el cerrojo: {e}", ruta.display())))?;
    Ok(f)
}

fn aplicar_mover(c: &Cambio, desde: &Path, hacia: &Path, r: &Raices) -> Resultado<String> {
    // Todo el ciclo bajo el cerrojo: dos aplicaciones a la vez no se pierden el índice.
    let _cerrojo = bloquear_almacen(r)?;
    let mut indice = leer_indice(r)?;
    if c.activar != indice.elementos.contains_key(&c.id) {
        return Err(ErrorBiblioteca::Revalidacion(
            if c.activar { "ya no está en el índice de desactivadas".to_string() } else { "ya está en el índice de desactivadas".to_string() },
        ));
    }
    mover_sin_pisar(desde, hacia)?;
    if c.activar {
        indice.elementos.remove(&c.id);
    } else {
        indice.elementos.insert(c.id.clone(), EntradaIndice { origen: desde.to_path_buf(), almacen: hacia.to_path_buf(), desactivada: marca_tiempo() });
    }
    if let Err(e) = escribir_indice(r, &indice) {
        // Sin índice no hay vuelta: se deshace el movimiento.
        let _ = renombrar_sin_pisar(hacia, desde);
        return Err(e);
    }
    Ok(format!("{} movido a {}", desde.display(), hacia.display()))
}

// -- ajustes ---------------------------------------------------------------

fn lista_de(v: &[String]) -> Value {
    Value::from(v.to_vec())
}

fn plan_ajustes(el: &Elemento, activar: bool, ctx: &Contexto) -> Resultado<Cambio> {
    if el.agente != "qwen-code" || el.tipo != Tipo::Skill {
        return Err(ErrorBiblioteca::NoSePuede("solo las skills del ejecutor se cambian por ajustes".into()));
    }
    let dir = el.ruta.parent().ok_or_else(|| ErrorBiblioteca::NoSePuede("ruta sin directorio".into()))?;
    let mut todas: Vec<String> = fs::read_dir(dir)
        .map_err(|e| ErrorBiblioteca::Io(format!("{}: {e}", dir.display())))?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".md")).map(str::to_string))
        .filter(|n| !n.starts_with('.'))
        .collect();
    todas.sort();
    if !todas.contains(&el.nombre) {
        return Err(ErrorBiblioteca::NoSePuede(format!("{} ya no está en {}", el.nombre, dir.display())));
    }
    let actual = ctx.ajustes().agente.skills.habilitadas;
    let efectivas: Vec<String> = if actual.is_empty() { todas.clone() } else { actual.clone() };
    let mut nuevas = efectivas.clone();
    if activar {
        if efectivas.contains(&el.nombre) {
            return Err(ErrorBiblioteca::NoSePuede("ya está activada".into()));
        }
        nuevas.push(el.nombre.clone());
    } else {
        if !efectivas.contains(&el.nombre) {
            return Err(ErrorBiblioteca::NoSePuede("ya está desactivada".into()));
        }
        nuevas.retain(|n| n != &el.nombre);
        if nuevas.is_empty() {
            return Err(ErrorBiblioteca::NoSePuede(
                "es la única skill habilitada y una lista vacía las habilitaría todas; habilita otra antes".into(),
            ));
        }
    }
    nuevas.sort();
    nuevas.dedup();
    // «Todas» se escribe como lista vacía (clave quitada).
    let despues = if todas.iter().all(|t| nuevas.contains(t)) { Vec::new() } else { nuevas };
    Ok(Cambio {
        id: el.id.clone(),
        activar,
        mecanismo: Mecanismo::Ajustes,
        objetivo: Objetivo::Ajuste { clave: CLAVE_SKILLS.into() },
        antes: lista_de(&actual),
        despues: lista_de(&despues),
    })
}

fn aplicar_ajustes(c: &Cambio, ctx: &Contexto) -> Resultado<String> {
    let valor = match &c.despues {
        Value::Array(l) if l.is_empty() => None,
        v => Some(v.clone()),
    };
    ea_ajustes::establecer(&ctx.fuentes, ctx.canal, None, &[(CLAVE_SKILLS.to_string(), valor)])
        .map_err(|e| ErrorBiblioteca::Ajustes(e.to_string()))?;
    Ok(format!("{CLAVE_SKILLS}: {} -> {}", c.antes, c.despues))
}

// ---------------------------------------------------------------------------
// Planificar por id, revalidar y aplicar
// ---------------------------------------------------------------------------

/// Busca `id` en el inventario (y entre las desactivadas por mover) y planifica.
pub fn planear(id: &str, activar: bool, ctx: &Contexto) -> Resultado<Cambio> {
    Id::analizar(id)?;
    let ajustes = ctx.ajustes();
    let inv = Biblioteca::cargar(&ctx.raices, &ajustes);
    // Un elemento guardado por «mover» puede tener otro con su id en su sitio
    // original: al activar manda el guardado (para rechazar el destino ocupado);
    // al desactivar, el que está a la vista.
    let guardado = || desactivadas(&ctx.raices).into_iter().find(|e| e.id == id);
    let el = if activar { guardado().or_else(|| inv.buscar(id).cloned()) } else { inv.buscar(id).cloned().or_else(guardado) }
        .ok_or_else(|| ErrorBiblioteca::NoExiste(id.to_string()))?;
    plan_cambio_con(&el, activar, ctx)
}

/// Comprueba que los binarios que necesita el cambio existen.
pub fn comprobar_disponible(c: &Cambio, ordenes: &dyn Ordenes) -> Resultado<()> {
    match c.binario() {
        Some(b) if !ordenes.disponible(b) => Err(ErrorBiblioteca::BinarioNoDisponible(b.to_string())),
        _ => Ok(()),
    }
}

/// Aplica el cambio con el entorno real (HOME, estado y ajustes del sistema).
pub fn aplicar(c: &Cambio, r: &Raices, ordenes: &dyn Ordenes) -> Resultado<Hecho> {
    aplicar_con(c, &Contexto::con_raices(r.clone()), ordenes)
}

/// Revalida y aplica. Falla sin tocar nada si el estado ya no es el simulado.
pub fn aplicar_con(c: &Cambio, ctx: &Contexto, ordenes: &dyn Ordenes) -> Resultado<Hecho> {
    // Revalidar: el cambio recalculado con el estado de ahora debe ser idéntico.
    let ahora = planear(&c.id, c.activar, ctx).map_err(|e| ErrorBiblioteca::Revalidacion(e.to_string()))?;
    if &ahora != c {
        let que = if ahora.antes != c.antes { format!("el valor era {} y ahora es {}", c.antes, ahora.antes) } else { "el fichero cambió".to_string() };
        return Err(ErrorBiblioteca::Revalidacion(que));
    }
    comprobar_disponible(c, ordenes)?;
    let (copia, texto) = match &c.objetivo {
        Objetivo::Orden { argv, cwd } => {
            let s = ordenes.ejecutar(argv, cwd.as_deref())?;
            if !s.exito() {
                let mut m = if s.stderr.trim().is_empty() { s.stdout.trim().to_string() } else { s.stderr.trim().to_string() };
                m.truncate(m.char_indices().nth(400).map_or(m.len(), |(i, _)| i));
                return Err(ErrorBiblioteca::Orden(format!("`{}` terminó con {:?}: {m}", argv.join(" "), s.codigo)));
            }
            (None, format!("`{}` ejecutado", argv.join(" ")))
        }
        Objetivo::Fichero { ruta, campo, .. } => {
            // El elemento solo hace falta para saber el agente y el nombre del servidor.
            let id = Id::analizar(&c.id)?;
            let el = Elemento::nuevo(&id.agente, id.tipo, id.ambito, &id.nombre, ruta.clone())?;
            aplicar_campo(c, ruta, campo, ctx, &el)?
        }
        Objetivo::Mover { desde, hacia } => (None, aplicar_mover(c, desde, hacia, &ctx.raices)?),
        Objetivo::Ajuste { .. } => (None, aplicar_ajustes(c, ctx)?),
    };
    Ok(Hecho {
        id: c.id.clone(),
        activar: c.activar,
        mecanismo: c.mecanismo,
        objetivo: c.objetivo.clone(),
        antes: c.antes.clone(),
        despues: c.despues.clone(),
        copia,
        texto,
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn un_destino_que_aparece_entre_la_comprobacion_y_el_movimiento_no_se_pisa() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let r = Raices::nuevo(home.clone(), None, d.path().join("estado"), None);
        let desde = home.join(".claude/skills/propia");
        fs::create_dir_all(&desde).unwrap();
        fs::write(desde.join("SKILL.md"), "x").unwrap();
        let hacia = base_desactivadas(&r).join("claude-code/skill/propia");
        let c = Cambio {
            id: "claude-code/skill/usuario/propia".into(),
            activar: false,
            mecanismo: Mecanismo::Mover,
            antes: json!({ "en": desde }),
            despues: json!({ "en": hacia }),
            objetivo: Objetivo::Mover { desde: desde.clone(), hacia: hacia.clone() },
        };
        // Alguien crea el destino después de planear y de revalidar.
        fs::create_dir_all(&hacia).unwrap();
        fs::write(hacia.join("ajeno.txt"), "no me pises").unwrap();
        let e = aplicar_mover(&c, &desde, &hacia, &r).unwrap_err();
        assert!(matches!(e, ErrorBiblioteca::Ocupado(_)), "{e}");
        assert_eq!(fs::read_to_string(hacia.join("ajeno.txt")).unwrap(), "no me pises");
        assert!(desde.join("SKILL.md").exists(), "el origen sigue en su sitio");
        assert!(leer_indice(&r).unwrap().elementos.is_empty(), "no quedó entrada en el índice");

        // Un fichero suelto tampoco se pisa.
        let a = d.path().join("a");
        let b = d.path().join("b");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        assert!(matches!(renombrar_sin_pisar(&a, &b).unwrap_err(), ErrorRenombrar::Existe));
        assert_eq!(fs::read_to_string(&b).unwrap(), "b");
        fs::remove_file(&b).unwrap();
        renombrar_sin_pisar(&a, &b).unwrap();
        assert_eq!(fs::read_to_string(&b).unwrap(), "a");
    }
}
