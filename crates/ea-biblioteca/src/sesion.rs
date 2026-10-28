//! Sesiones con un subconjunto de herramientas (spec 018, F5).
//!
//! «Lanza <agente> con SOLO estas herramientas»: [`plan_sesion`] es una
//! función pura sobre el inventario (redactado, sin secretos) que decide el
//! `argv`, el entorno y el contenido de los ficheros efímeros; [`materializar`]
//! escribe esos ficheros (0600) en `$XDG_RUNTIME_DIR/ea-biblioteca/sesiones/<id>/`
//! y, SOLO ahí, relee la definición completa de los servidores MCP desde su
//! fuente original (`~/.claude.json`, `.mcp.json`, `settings.json`…, siempre
//! en solo lectura) para escribirla con sus `env` y cabeceras. Un secreto
//! nunca está en el plan, ni en el `argv`, ni en el entorno, ni en la
//! simulación: solo en un fichero 0600 de un directorio 0700 del runtime.
//!
//! # Qué hace cada agente (flags comprobados con las versiones del catálogo)
//!
//! - **Claude Code 2.1.301**: `claude --strict-mcp-config --mcp-config
//!   <mcp.json> --settings <ajustes.json> [--plugin-dir <complemento>]`.
//!   `--strict-mcp-config` ignora todo MCP que no venga de `--mcp-config`;
//!   `ajustes.json` lleva `enabledPlugins` con los plugins elegidos a `true`
//!   y el resto de los instalados a `false`; las skills, subagentes y
//!   comandos elegidos van en un plugin efímero (`--plugin-dir`) de enlaces
//!   simbólicos. Lo que Claude carga de forma nativa (skills de usuario…) no
//!   se excluye.
//! - **Qwen Code 0.25.1**: `qwen --mcp-config <mcp.json>
//!   --allowed-mcp-server-names <n…> -e <extensiones>`. Los MCP propios de
//!   Qwen solo se nombran; los de otro agente van en `mcp.json`. Sin MCP se
//!   pasa un nombre que no existe y sin extensiones `-e none` (NO verificado
//!   que `none` sea especial en 0.25.1).
//! - **Codex 0.159.3**: `codex -c mcp_servers.<n>.… `. Los MCP propios no
//!   elegidos se apagan con `enabled=false` (comprobado con `codex mcp list`)
//!   y los de otro agente se definen en línea. Como `-c` es argv, un MCP de
//!   otro agente con `env`, cabeceras, URL con ruta o argumentos redactados
//!   se RECHAZA: no hay forma de pasar el secreto sin argv ni entorno.
//! - **OpenCode 2.1.1**: `opencode --standalone` con `OPENCODE_CONFIG=<fichero>`.
//!   Comprobado con un HOME temporal: sin `--standalone` la configuración la
//!   decide el servicio en segundo plano; con él, `OPENCODE_CONFIG` (y
//!   `OPENCODE_CONFIG_CONTENT`) se FUSIONAN con la del usuario, así que los
//!   demás MCP de OpenCode se apagan con su definición completa y
//!   `enabled:false` (un `{"enabled":false}` suelto no basta). Se usa el
//!   fichero y no `OPENCODE_CONFIG_CONTENT` para que el secreto no esté en el
//!   entorno.
//! - **Antigravity**: no admite sesiones en la v1.
//!
//! # Directorio de cada ejecución
//!
//! El plan es determinista (su `id` sale de una huella del agente, los ids y
//! el proyecto, y es lo que se simula y revalida), pero cada [`materializar`]
//! crea su PROPIO directorio `sesiones/<id>-<nonce>/` (nunca borra uno
//! existente: dos lanzamientos del mismo plan no se pisan) y devuelve el plan
//! con las rutas de ese directorio. Las sesiones se limpian a los 7 días de
//! creadas (marca `.creada`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use ea_plataforma::fs::{crear_dir_nuevo_privado, crear_dir_privado, enlazar, huella, opciones_privadas, poner_permisos_unix, poner_privado};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ea_plataforma::escritorio::{argv_lanzador, OrdenTerminal};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::agentes::comun::{leer_texto, quitar_comentarios};
use crate::error::ErrorBiblioteca;
use crate::modelo::{Ambito, DefMcp, Elemento, Id, Tipo, AGENTE_ECOSISTEMA};
use crate::raices::Raices;
use crate::Inventario;

/// Los agentes que admiten sesión en la v1 (Antigravity no).
pub const AGENTES_CON_SESION: [&str; 4] = ["claude-code", "qwen-code", "codex", "opencode"];

/// Antigüedad (desde su creación) a partir de la cual se borra una sesión del
/// runtime: con margen, porque una sesión de Claude o Codex puede durar días.
const EDAD_MAXIMA: Duration = Duration::from_secs(7 * 24 * 3600);
/// Fichero con los segundos Unix de la creación de la sesión.
const MARCA_CREACION: &str = ".creada";
const MODO_FICHERO: u32 = 0o600;
/// Lo que ocupa el sitio de un valor secreto en un plan.
const OCULTO: &str = "<oculto>";
/// Nombre que no existe, para pedirle a Qwen «ningún MCP».
const MCP_NINGUNO: &str = "ea-ninguno";

fn mal<T>(m: impl Into<String>) -> Result<T, ErrorBiblioteca> {
    Err(ErrorBiblioteca::Sesion(m.into()))
}

/// Un fichero que se escribirá en el directorio efímero de la sesión.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FicheroEfimero {
    pub ruta: PathBuf,
    /// En el plan, SIN secretos (los valores de `env` y cabeceras salen como
    /// `<oculto>`); [`materializar`] escribe la versión completa si
    /// `con_secretos`.
    pub contenido: String,
    /// Siempre 0600.
    pub modo: u32,
    /// El contenido real depende de la definición completa de un MCP.
    pub con_secretos: bool,
}

/// Un enlace simbólico del directorio de sesión (las skills, subagentes y
/// comandos elegidos de Claude Code).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnlaceEfimero {
    pub ruta: PathBuf,
    pub destino: PathBuf,
}

/// De dónde sacar la definición completa de un servidor MCP al materializar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FuenteMcp {
    pub id: String,
    pub agente: String,
    pub nombre: String,
    /// Texto del ámbito (`usuario`, `proyecto`, `ecosistema`…).
    pub ambito: String,
    /// El fichero de configuración de origen.
    pub ruta: PathBuf,
    /// Si en el fichero va activado (OpenCode también escribe los apagados).
    pub activo: bool,
    /// Solo el MCP del ecosistema (público: sin secretos).
    pub publica: Option<DefMcp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanSesion {
    /// Determinista: `s` + 12 hex de la huella de (agente, ids, proyecto).
    pub id: String,
    pub agente: String,
    /// Los ids elegidos, ordenados y sin repetir.
    pub herramientas: Vec<String>,
    /// El comando (sin `env`): el primer elemento es el binario del agente.
    pub argv: Vec<String>,
    /// Variables de entorno de la sesión. Nunca un secreto.
    pub entorno: Vec<(String, String)>,
    pub ficheros: Vec<FicheroEfimero>,
    pub enlaces: Vec<EnlaceEfimero>,
    /// `$XDG_RUNTIME_DIR/ea-biblioteca/sesiones/<id>/`.
    pub directorio: PathBuf,
    /// El proyecto en que se abre la sesión.
    pub cwd: Option<PathBuf>,
    pub mcp: Vec<FuenteMcp>,
    /// El HOME de las raíces con que se planeó (sin proyecto, la sesión es
    /// del HOME). No entra en la huella ni en lo que se muestra.
    #[serde(skip)]
    pub home: PathBuf,
}

impl PlanSesion {
    /// Lo que se abre en la terminal: el comando, el directorio del proyecto
    /// y las variables de la sesión. Cada sistema lo traduce a su lanzador
    /// (`ea_plataforma::escritorio::lanzar_en_terminal`).
    pub fn orden_terminal(&self) -> OrdenTerminal {
        OrdenTerminal { argv: self.argv.clone(), cwd: self.cwd.clone(), entorno: self.entorno.clone() }
    }

    /// El argv de Linux que se entrega a la terminal, sin shell: `env [-C
    /// <cwd>] K=V… <argv>`. Así el directorio y las variables no dependen de
    /// si el lanzador de terminal hereda el entorno. Es también la forma que
    /// se muestra y se apunta en el diario, en cualquier sistema.
    pub fn argv_terminal(&self) -> Vec<String> {
        argv_lanzador(&self.orden_terminal(), "xdg-terminal-exec").into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    /// Texto de una línea del comando, para mostrarlo.
    pub fn comando(&self) -> String {
        self.argv_terminal().join(" ")
    }

    /// La carpeta de la sesión: el proyecto o, si no hay, el HOME. Volver a
    /// planear con ella como directorio da el mismo plan, lo planee quien lo
    /// planee.
    pub fn carpeta(&self) -> &Path {
        self.cwd.as_deref().unwrap_or(&self.home)
    }
}

// ---------------------------------------------------------------- MCP

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tr {
    Stdio,
    Http,
    Sse,
}

/// Un servidor MCP normalizado, sea cual sea el agente de origen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mcp {
    tr: Tr,
    comando: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    env: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
}

fn url_ecosistema(url: &str) -> String {
    format!("{}/mcp", url.trim_end_matches('/'))
}

/// La definición redactada (la del inventario): los valores de `env` y
/// cabeceras no existen, solo sus nombres.
fn mcp_redactado(e: &Elemento) -> Result<Mcp, ErrorBiblioteca> {
    let Some(d) = &e.def_mcp else { return mal(format!("{}: no tiene definición de MCP", e.id)) };
    mcp_de_def(d, e.agente == AGENTE_ECOSISTEMA, &e.id)
}

fn mcp_de_def(d: &DefMcp, ecosistema: bool, id: &str) -> Result<Mcp, ErrorBiblioteca> {
    let tr = match d.transporte.as_str() {
        "stdio" => Tr::Stdio,
        "http" => Tr::Http,
        "sse" => Tr::Sse,
        _ => return mal(format!("{id}: se desconoce el transporte del MCP")),
    };
    let url = d.url.as_ref().map(|u| if ecosistema { url_ecosistema(u) } else { u.clone() });
    Ok(Mcp {
        tr,
        comando: d.comando.clone(),
        args: d.args.clone(),
        url,
        env: d.env.iter().map(|k| (k.clone(), OCULTO.to_string())).collect(),
        headers: d.headers.iter().map(|k| (k.clone(), OCULTO.to_string())).collect(),
    })
}

fn texto_de<'a>(v: &'a Value, claves: &[&str]) -> Option<&'a str> {
    claves.iter().find_map(|k| v.get(*k).and_then(Value::as_str)).filter(|s| !s.is_empty())
}

fn mapa_de(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, x)| Some((k.clone(), x.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

/// La definición COMPLETA, a partir del JSON original de cualquier agente.
fn mcp_de_json(v: &Value, id: &str, entorno: &dyn Fn(&str) -> Option<String>) -> Result<Mcp, ErrorBiblioteca> {
    let (comando, args) = match v.get("command") {
        Some(Value::String(c)) => (
            Some(c.clone()),
            v.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        ),
        Some(Value::Array(l)) => {
            let mut t: Vec<String> = l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
            let c = (!t.is_empty()).then(|| t.remove(0));
            (c, t)
        }
        _ => (None, Vec::new()),
    };
    let url = texto_de(v, &["url", "serverUrl", "httpUrl", "server_url"]).map(str::to_string);
    let tipo = texto_de(v, &["type", "transport"]).map(str::to_ascii_lowercase);
    let tr = match tipo.as_deref() {
        Some("stdio" | "local") => Tr::Stdio,
        Some("sse") => Tr::Sse,
        Some("http" | "remote" | "streamable-http" | "streamablehttp" | "streamable_http") => Tr::Http,
        _ if url.is_some() => Tr::Http,
        _ if comando.is_some() => Tr::Stdio,
        _ => return mal(format!("{id}: se desconoce el transporte del MCP")),
    };
    let mut headers = mapa_de(v.get("headers").or_else(|| v.get("http_headers")));
    // Codex: `env_http_headers` es cabecera -> NOMBRE de variable de entorno, y
    // `bearer_token_env_var` el nombre de la variable del token. Los valores
    // se leen del entorno del proceso solo aquí (va a un fichero 0600).
    let variable = |nombre: &str| {
        entorno(nombre).ok_or_else(|| {
            ErrorBiblioteca::Sesion(format!(
                "{id}: usa la variable de entorno {nombre} y no está definida en el entorno de ea; \
                 defínela antes de abrir la sesión (su valor no se muestra)"
            ))
        })
    };
    for (cabecera, nombre) in mapa_de(v.get("env_http_headers")) {
        headers.insert(cabecera, variable(&nombre)?);
    }
    if let Some(nombre) = texto_de(v, &["bearer_token_env_var"]) {
        headers.insert("Authorization".into(), format!("Bearer {}", variable(nombre)?));
    }
    Ok(Mcp { tr, comando, args, url, env: mapa_de(v.get("env").or_else(|| v.get("environment"))), headers })
}

fn parece_servidor(v: &Value) -> bool {
    ["type", "command", "url", "enabled"].iter().any(|k| v.get(*k).is_some())
}

/// Relee la definición completa desde la fuente original (solo lectura).
fn leer_completo(f: &FuenteMcp, r: &Raices, entorno: &dyn Fn(&str) -> Option<String>) -> Result<Mcp, ErrorBiblioteca> {
    if f.agente == AGENTE_ECOSISTEMA {
        let Some(d) = &f.publica else { return mal(format!("{}: falta su definición", f.id)) };
        return mcp_de_def(d, true, &f.id);
    }
    // Solo se lee dentro del HOME o del proyecto: la ruta viene de un plan. Se
    // compara la ruta CANÓNICA (un enlace dentro del HOME no puede sacar la
    // lectura de él).
    let canonica = |p: &Path| ea_plataforma::rutas::canonica(p).unwrap_or_else(|_| p.to_path_buf());
    let real = ea_plataforma::rutas::canonica(&f.ruta);
    let dentro = real.as_ref().is_ok_and(|c| c.starts_with(canonica(&r.home)) || r.proyecto.as_ref().is_some_and(|p| c.starts_with(canonica(p))));
    if !dentro || f.ruta.components().any(|c| c.as_os_str() == "..") {
        return mal(format!("{}: la fuente {} está fuera del HOME y del proyecto", f.id, f.ruta.display()));
    }
    let Some(texto) = leer_texto(&f.ruta) else { return mal(format!("{}: no se puede leer {}", f.id, f.ruta.display())) };
    let fichero = f.ruta.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let no_esta = || ErrorBiblioteca::Sesion(format!("{}: ya no está «{}» en {}", f.id, f.nombre, f.ruta.display()));
    let json = |t: &str| -> Result<Value, ErrorBiblioteca> {
        serde_json::from_str(t).map_err(|e| ErrorBiblioteca::Sesion(format!("{}: no se interpreta ({e})", f.ruta.display())))
    };
    let def: Value = match f.agente.as_str() {
        "claude-code" if fichero == ".claude.json" => {
            let j = json(&texto)?;
            if f.ambito == "proyecto" {
                let p = r.proyecto.as_ref().ok_or_else(no_esta)?;
                j.get("projects").and_then(|x| crate::agentes::comun::proyecto_en(x, p)).and_then(|x| x.get("mcpServers")).and_then(|x| x.get(&f.nombre)).cloned().ok_or_else(no_esta)?
            } else {
                j.get("mcpServers").and_then(|x| x.get(&f.nombre)).cloned().ok_or_else(no_esta)?
            }
        }
        "claude-code" | "qwen-code" | "antigravity" => json(&texto)?.get("mcpServers").and_then(|x| x.get(&f.nombre)).cloned().ok_or_else(no_esta)?,
        "codex" => {
            let t: toml::Table = texto.parse().map_err(|_| ErrorBiblioteca::Sesion(format!("{}: no es TOML válido", f.ruta.display())))?;
            let d = t.get("mcp_servers").and_then(|x| x.get(&f.nombre)).ok_or_else(no_esta)?;
            serde_json::to_value(d).map_err(|_| no_esta())?
        }
        "opencode" => {
            let (limpio, _) = quitar_comentarios(&texto);
            let j = json(&limpio)?;
            let mcp = j.get("mcp").and_then(Value::as_object).ok_or_else(no_esta)?;
            let contenedor = match mcp.get("servers") {
                Some(s) if s.is_object() && !parece_servidor(s) => s.as_object().expect("objeto"),
                _ => mcp,
            };
            contenedor.get(&f.nombre).cloned().ok_or_else(no_esta)?
        }
        otro => return mal(format!("{}: agente de origen sin lector ({otro})", f.id)),
    };
    mcp_de_json(&def, &f.id, entorno)
}

// ------------------------------------------- Formato de cada agente

fn mapa_json(m: &BTreeMap<String, String>) -> Value {
    Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<Map<_, _>>())
}

fn entrada_claude(m: &Mcp) -> Value {
    let mut o = Map::new();
    match m.tr {
        Tr::Stdio => {
            o.insert("type".into(), json!("stdio"));
            o.insert("command".into(), json!(m.comando));
            o.insert("args".into(), json!(m.args));
            if !m.env.is_empty() {
                o.insert("env".into(), mapa_json(&m.env));
            }
        }
        Tr::Http | Tr::Sse => {
            o.insert("type".into(), json!(if m.tr == Tr::Http { "http" } else { "sse" }));
            o.insert("url".into(), json!(m.url));
            if !m.headers.is_empty() {
                o.insert("headers".into(), mapa_json(&m.headers));
            }
        }
    }
    Value::Object(o)
}

fn entrada_qwen(m: &Mcp) -> Value {
    let mut o = Map::new();
    match m.tr {
        Tr::Stdio => {
            o.insert("command".into(), json!(m.comando));
            o.insert("args".into(), json!(m.args));
            if !m.env.is_empty() {
                o.insert("env".into(), mapa_json(&m.env));
            }
        }
        Tr::Http | Tr::Sse => {
            o.insert(if m.tr == Tr::Http { "httpUrl" } else { "url" }.into(), json!(m.url));
            if !m.headers.is_empty() {
                o.insert("headers".into(), mapa_json(&m.headers));
            }
        }
    }
    Value::Object(o)
}

fn entrada_opencode(m: &Mcp, activo: bool) -> Value {
    let mut o = Map::new();
    match m.tr {
        Tr::Stdio => {
            o.insert("type".into(), json!("local"));
            let mut c = vec![m.comando.clone().unwrap_or_default()];
            c.extend(m.args.iter().cloned());
            o.insert("command".into(), json!(c));
            if !m.env.is_empty() {
                o.insert("environment".into(), mapa_json(&m.env));
            }
        }
        Tr::Http | Tr::Sse => {
            o.insert("type".into(), json!("remote"));
            o.insert("url".into(), json!(m.url));
            if !m.headers.is_empty() {
                o.insert("headers".into(), mapa_json(&m.headers));
            }
        }
    }
    o.insert("enabled".into(), json!(activo));
    Value::Object(o)
}

/// El JSON de MCP de un agente a partir de sus entradas `(nombre, mcp, activo)`.
fn texto_mcp(agente: &str, entradas: &[(String, Mcp, bool)]) -> String {
    let v = match agente {
        "claude-code" | "qwen-code" => {
            let f = if agente == "claude-code" { entrada_claude } else { entrada_qwen };
            let m: Map<String, Value> = entradas.iter().filter(|e| e.2).map(|(n, m, _)| (n.clone(), f(m))).collect();
            json!({ "mcpServers": m })
        }
        _ => {
            let m: Map<String, Value> = entradas.iter().map(|(n, m, a)| (n.clone(), entrada_opencode(m, *a))).collect();
            json!({ "mcp": m })
        }
    };
    serde_json::to_string_pretty(&v).expect("JSON serializable") + "\n"
}

// -------------------------------------------------------------- Plan

/// Si el elemento tiene sentido en una sesión de `agente`, y si no, por qué.
/// Es la regla del plan y la que usa el panel para no ofrecer lo que el plan
/// va a rechazar.
pub fn admite(agente: &str, e: &Elemento) -> Result<(), ErrorBiblioteca> {
    let propio = e.agente == agente || e.agente == AGENTE_ECOSISTEMA;
    if !propio && e.tipo != Tipo::Mcp {
        return mal(format!("{}: es de {} y no tiene sentido en una sesión de {agente}", e.id, e.agente));
    }
    let tipos: &[Tipo] = match agente {
        "claude-code" => &[Tipo::Mcp, Tipo::Plugin, Tipo::Skill, Tipo::Subagente, Tipo::Comando],
        "qwen-code" => &[Tipo::Mcp, Tipo::Extension],
        _ => &[Tipo::Mcp],
    };
    if !tipos.contains(&e.tipo) {
        return mal(format!("{}: una sesión de {agente} no admite elementos de tipo {}", e.id, e.tipo));
    }
    if e.tipo != Tipo::Mcp
        && let Ambito::Plugin(p) = &e.ambito
    {
        return mal(format!("{}: viene con el plugin {p}; elige el plugin", e.id));
    }
    Ok(())
}

fn fuente(e: &Elemento, activo: bool) -> FuenteMcp {
    FuenteMcp {
        id: e.id.clone(),
        agente: e.agente.clone(),
        nombre: e.nombre.clone(),
        ambito: e.ambito.texto(),
        ruta: e.ruta.clone(),
        activo,
        publica: (e.agente == AGENTE_ECOSISTEMA).then(|| e.def_mcp.clone()).flatten(),
    }
}

fn sin_repetidos<'a>(nombres: impl IntoIterator<Item = &'a str>, que: &str) -> Result<(), ErrorBiblioteca> {
    let mut vistos = BTreeSet::new();
    for n in nombres {
        if !vistos.insert(n) {
            return mal(format!("dos {que} se llaman «{n}»: elige solo uno"));
        }
    }
    Ok(())
}

/// La clave de un `-c` de Codex: pelada si puede, entre comillas si no.
fn clave_codex(n: &str) -> String {
    if !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        n.to_string()
    } else {
        serde_json::to_string(n).expect("texto")
    }
}

/// El plan de una sesión de `agente` con solo `ids`. Puro: no toca el disco.
/// Rechaza lo que no esté en el inventario, lo que no tenga sentido para ese
/// agente y a Antigravity.
pub fn plan_sesion(agente: &str, ids: &[Id], inv: &Inventario, raices: &Raices) -> Result<PlanSesion, ErrorBiblioteca> {
    if agente == "antigravity" {
        return mal("Antigravity no admite sesiones en la v1");
    }
    if !AGENTES_CON_SESION.contains(&agente) {
        return mal(format!("«{agente}» no es un agente con sesiones ({})", AGENTES_CON_SESION.join(", ")));
    }
    let ids: BTreeSet<String> = ids.iter().map(Id::to_string).collect();
    if ids.is_empty() {
        return mal("hay que elegir al menos una herramienta");
    }
    let Some(runtime) = &raices.runtime else {
        return mal("no hay directorio de ejecución (en Linux, $XDG_RUNTIME_DIR): no hay dónde escribir los ficheros efímeros de la sesión");
    };
    let mut elegidos: Vec<&Elemento> = Vec::new();
    for id in &ids {
        let e = inv.buscar(id).ok_or_else(|| ErrorBiblioteca::NoExiste(id.clone()))?;
        admite(agente, e)?;
        elegidos.push(e);
    }
    // Un nombre o una ruta que empieza por «-» se leería como una opción del
    // agente al ir en su argv.
    for e in &elegidos {
        if e.nombre.starts_with('-') || e.ruta.to_string_lossy().starts_with('-') {
            return mal(format!("{}: su nombre o ruta empieza por «-» y se leería como una opción del agente", e.id));
        }
    }
    let herramientas: Vec<String> = ids.into_iter().collect();
    let cwd = raices.proyecto.clone();
    let id = format!(
        "s{}",
        &huella(format!("{agente}\0{}\0{}", herramientas.join("\0"), cwd.as_ref().map(|c| c.display().to_string()).unwrap_or_default()).as_bytes())[..12]
    );
    let directorio = runtime.join("ea-biblioteca").join("sesiones").join(&id);
    let fichero = |nombre: &str, contenido: String, con_secretos: bool| FicheroEfimero {
        ruta: directorio.join(nombre),
        contenido,
        modo: MODO_FICHERO,
        con_secretos,
    };
    let mut plan = PlanSesion {
        id,
        agente: agente.to_string(),
        herramientas,
        argv: Vec::new(),
        entorno: Vec::new(),
        ficheros: Vec::new(),
        enlaces: Vec::new(),
        directorio: directorio.clone(),
        cwd,
        mcp: Vec::new(),
        home: raices.home.clone(),
    };
    let mcps: Vec<&Elemento> = elegidos.iter().copied().filter(|e| e.tipo == Tipo::Mcp).collect();
    sin_repetidos(mcps.iter().map(|e| e.nombre.as_str()), "servidores MCP")?;

    match agente {
        "claude-code" => {
            let mut entradas = Vec::new();
            for e in &mcps {
                entradas.push((e.nombre.clone(), mcp_redactado(e)?, true));
                plan.mcp.push(fuente(e, true));
            }
            let activos: BTreeSet<&str> = elegidos.iter().filter(|e| e.tipo == Tipo::Plugin).map(|e| e.nombre.as_str()).collect();
            let mut plugins: BTreeMap<String, bool> = inv
                .elementos
                .iter()
                .filter(|e| e.agente == agente && e.tipo == Tipo::Plugin)
                .map(|e| (e.nombre.clone(), activos.contains(e.nombre.as_str())))
                .collect();
            for p in &activos {
                plugins.insert((*p).to_string(), true);
            }
            let ajustes = serde_json::to_string_pretty(&json!({ "enabledPlugins": plugins })).expect("JSON") + "\n";

            let complemento = directorio.join("complemento");
            let mut hay_complemento = false;
            for (tipo, carpeta, sufijo) in [(Tipo::Skill, "skills", ""), (Tipo::Subagente, "agents", ".md"), (Tipo::Comando, "commands", ".md")] {
                let del_tipo: Vec<&&Elemento> = elegidos.iter().filter(|e| e.tipo == tipo).collect();
                sin_repetidos(del_tipo.iter().map(|e| e.nombre.as_str()), &format!("elementos de tipo {tipo}"))?;
                for e in del_tipo {
                    let es_md = e.ruta.extension().is_some_and(|x| x == "md");
                    if (tipo == Tipo::Skill) == es_md {
                        return mal(format!("{}: no tiene el formato de {tipo} de Claude Code", e.id));
                    }
                    hay_complemento = true;
                    plan.enlaces.push(EnlaceEfimero { ruta: complemento.join(carpeta).join(format!("{}{sufijo}", e.nombre)), destino: e.ruta.clone() });
                }
            }
            plan.ficheros.push(fichero("mcp.json", texto_mcp(agente, &entradas), true));
            plan.ficheros.push(fichero("ajustes.json", ajustes, false));
            plan.argv = ["claude", "--strict-mcp-config", "--mcp-config"].map(String::from).to_vec();
            plan.argv.push(directorio.join("mcp.json").display().to_string());
            plan.argv.push("--settings".into());
            plan.argv.push(directorio.join("ajustes.json").display().to_string());
            if hay_complemento {
                plan.ficheros.push(FicheroEfimero {
                    ruta: complemento.join(".claude-plugin").join("plugin.json"),
                    contenido: serde_json::to_string_pretty(&json!({
                        "name": "ea-sesion", "version": "0.0.0", "description": "Herramientas elegidas para esta sesión (ecosistema-asistente)"
                    }))
                    .expect("JSON")
                        + "\n",
                    modo: MODO_FICHERO,
                    con_secretos: false,
                });
                plan.argv.push("--plugin-dir".into());
                plan.argv.push(complemento.display().to_string());
            }
        }
        "qwen-code" => {
            let mut entradas = Vec::new();
            for e in &mcps {
                if e.agente != agente {
                    entradas.push((e.nombre.clone(), mcp_redactado(e)?, true));
                    plan.mcp.push(fuente(e, true));
                }
            }
            plan.argv.push("qwen".into());
            if !entradas.is_empty() {
                plan.ficheros.push(fichero("mcp.json", texto_mcp(agente, &entradas), true));
                plan.argv.push("--mcp-config".into());
                plan.argv.push(directorio.join("mcp.json").display().to_string());
            }
            plan.argv.push("--allowed-mcp-server-names".into());
            if mcps.is_empty() {
                plan.argv.push(MCP_NINGUNO.into());
            } else {
                plan.argv.extend(mcps.iter().map(|e| e.nombre.clone()));
            }
            plan.argv.push("-e".into());
            let exts: Vec<String> = elegidos.iter().filter(|e| e.tipo == Tipo::Extension).map(|e| e.nombre.clone()).collect();
            if exts.is_empty() {
                plan.argv.push("none".into());
            } else {
                plan.argv.extend(exts);
            }
        }
        "codex" => {
            plan.argv.push("codex".into());
            let elegidos_ids: BTreeSet<&str> = mcps.iter().map(|e| e.id.as_str()).collect();
            let c = |plan: &mut PlanSesion, v: String| {
                plan.argv.push("-c".into());
                plan.argv.push(v);
            };
            // Los MCP de Codex que no se han elegido, apagados.
            for e in inv.elementos.iter().filter(|e| e.agente == agente && e.tipo == Tipo::Mcp && !elegidos_ids.contains(e.id.as_str())) {
                if mcps.iter().all(|m| m.nombre != e.nombre) {
                    c(&mut plan, format!("mcp_servers.{}.enabled=false", clave_codex(&e.nombre)));
                }
            }
            for e in &mcps {
                let k = clave_codex(&e.nombre);
                if e.agente == agente {
                    c(&mut plan, format!("mcp_servers.{k}.enabled=true"));
                    continue;
                }
                let m = mcp_redactado(e)?;
                let rechazo = || {
                    mal(format!(
                        "{}: Codex recibe los MCP por argumentos y este lleva (o puede llevar) secretos, argumentos o una URL redactados; \
                         usa uno de los MCP propios de Codex",
                        e.id
                    ))
                };
                // Si el MCP no es del ecosistema se relee entero de su fuente: solo
                // pasa si es idéntico a lo redactado (nada se ocultó ni se
                // alteró) y no lleva `env` ni cabeceras.
                let m = if e.agente == AGENTE_ECOSISTEMA {
                    m
                } else {
                    let completo = leer_completo(&fuente(e, true), raices, &|_| Some(String::new()))?;
                    if completo.tr != m.tr || completo.comando != m.comando || completo.args != m.args || completo.url != m.url {
                        return rechazo();
                    }
                    completo
                };
                if !m.env.is_empty() || !m.headers.is_empty() {
                    return rechazo();
                }
                match m.tr {
                    Tr::Stdio => {
                        c(&mut plan, format!("mcp_servers.{k}.command={}", serde_json::to_string(&m.comando.unwrap_or_default()).expect("texto")));
                        if !m.args.is_empty() {
                            c(&mut plan, format!("mcp_servers.{k}.args={}", serde_json::to_string(&m.args).expect("lista")));
                        }
                    }
                    Tr::Http => c(&mut plan, format!("mcp_servers.{k}.url={}", serde_json::to_string(&m.url.unwrap_or_default()).expect("texto"))),
                    Tr::Sse => return mal(format!("{}: Codex no admite servidores SSE", e.id)),
                }
                c(&mut plan, format!("mcp_servers.{k}.enabled=true"));
            }
        }
        _ => {
            // OpenCode: con --standalone, OPENCODE_CONFIG se fusiona con la config del usuario.
            let elegidos_nombres: BTreeSet<&str> = mcps.iter().map(|e| e.nombre.as_str()).collect();
            let mut entradas = Vec::new();
            for e in &mcps {
                entradas.push((e.nombre.clone(), mcp_redactado(e)?, true));
                plan.mcp.push(fuente(e, true));
            }
            for e in inv.elementos.iter().filter(|e| e.agente == agente && e.tipo == Tipo::Mcp && !elegidos_nombres.contains(e.nombre.as_str())) {
                if entradas.iter().all(|x| x.0 != e.nombre)
                    && let Ok(m) = mcp_redactado(e)
                {
                    entradas.push((e.nombre.clone(), m, false));
                    plan.mcp.push(fuente(e, false));
                }
            }
            plan.ficheros.push(fichero("opencode.json", texto_mcp(agente, &entradas), true));
            plan.argv = vec!["opencode".into(), "--standalone".into()];
            plan.entorno.push(("OPENCODE_CONFIG".into(), directorio.join("opencode.json").display().to_string()));
        }
    }
    Ok(plan)
}

// ------------------------------------------------------- Materializar

fn io(ruta: &Path, e: std::io::Error) -> ErrorBiblioteca {
    ErrorBiblioteca::Sesion(format!("{}: {e}", ruta.display()))
}

fn crear_directorios(ruta: &Path) -> Result<(), ErrorBiblioteca> {
    // Todo directorio que se cree (también los intermedios) nace 0700.
    crear_dir_privado(ruta).map_err(|e| io(ruta, e))
}

/// Segundos Unix de la creación de una sesión: la marca `.creada` y, si no
/// está, la fecha de modificación del directorio.
fn creada_en(dir: &Path) -> Option<u64> {
    if let Some(t) = std::fs::read_to_string(dir.join(MARCA_CREACION)).ok().and_then(|t| t.trim().parse().ok()) {
        return Some(t);
    }
    std::fs::metadata(dir).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs())
}

/// Borra las sesiones de `<runtime>/ea-biblioteca/sesiones` creadas hace más
/// de `edad` (salvo `conservar`, el nombre de un directorio). Devuelve cuántas.
pub fn limpiar_sesiones(runtime: &Path, edad: Duration, conservar: Option<&str>) -> usize {
    let base = runtime.join("ea-biblioteca").join("sesiones");
    let ahora = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut n = 0;
    for entrada in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        let ruta = entrada.path();
        if conservar.is_some_and(|c| entrada.file_name() == c) || !ruta.is_dir() {
            continue;
        }
        let vieja = creada_en(&ruta).is_some_and(|c| ahora.saturating_sub(c) > edad.as_secs());
        if vieja && std::fs::remove_dir_all(&ruta).is_ok() {
            n += 1;
        }
    }
    n
}

/// Un nonce corto (hex) distinto en cada llamada, sin dependencias.
fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    huella(format!("{nanos}\0{}\0{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)).as_bytes())[..10].to_string()
}

/// El mismo plan con las rutas bajo otro directorio.
fn reubicar(plan: &PlanSesion, nuevo: &Path) -> PlanSesion {
    let (viejo_s, nuevo_s) = (plan.directorio.display().to_string(), nuevo.display().to_string());
    let texto = |t: &str| t.replace(&viejo_s, &nuevo_s);
    let ruta = |p: &Path| p.strip_prefix(&plan.directorio).map_or_else(|_| p.to_path_buf(), |r| nuevo.join(r));
    PlanSesion {
        argv: plan.argv.iter().map(|a| texto(a)).collect(),
        entorno: plan.entorno.iter().map(|(k, v)| (k.clone(), texto(v))).collect(),
        ficheros: plan.ficheros.iter().map(|f| FicheroEfimero { ruta: ruta(&f.ruta), ..f.clone() }).collect(),
        enlaces: plan.enlaces.iter().map(|l| EnlaceEfimero { ruta: ruta(&l.ruta), destino: l.destino.clone() }).collect(),
        directorio: nuevo.to_path_buf(),
        ..plan.clone()
    }
}

/// Escribe la sesión: un directorio NUEVO (0700) `<id>-<nonce>`, los ficheros
/// (0600) con las definiciones completas de los MCP releídas de su fuente, y
/// los enlaces. Devuelve el plan con las rutas reales. Antes limpia las
/// sesiones de más de 7 días; nunca borra un directorio existente.
pub fn materializar(plan: &PlanSesion, raices: &Raices) -> Result<PlanSesion, ErrorBiblioteca> {
    materializar_con_entorno(plan, raices, &|k| std::env::var(k).ok())
}

/// Como [`materializar`], con el entorno desde el que se resuelven las
/// variables que un MCP de Codex nombra (`env_http_headers`,
/// `bearer_token_env_var`).
pub fn materializar_con_entorno(plan: &PlanSesion, raices: &Raices, entorno: &dyn Fn(&str) -> Option<String>) -> Result<PlanSesion, ErrorBiblioteca> {
    let Some(runtime) = &raices.runtime else { return mal("no hay directorio de ejecución (en Linux, $XDG_RUNTIME_DIR)") };
    let base = runtime.join("ea-biblioteca").join("sesiones");
    let esperado = base.join(&plan.id);
    let id_ok = plan.id.len() == 13 && plan.id.starts_with('s') && plan.id[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if !id_ok || plan.directorio != esperado {
        return mal(format!("el directorio {} no es el de una sesión de este runtime", plan.directorio.display()));
    }
    if plan.ficheros.iter().any(|f| !f.ruta.starts_with(&plan.directorio) || f.ruta.components().any(|c| c.as_os_str() == ".."))
        || plan.enlaces.iter().any(|f| !f.ruta.starts_with(&plan.directorio) || f.ruta.components().any(|c| c.as_os_str() == ".."))
    {
        return mal("un fichero del plan queda fuera del directorio de la sesión");
    }
    // Primero las definiciones: si falta una fuente, no se escribe nada.
    let mut completas = Vec::new();
    for f in &plan.mcp {
        completas.push((f.nombre.clone(), leer_completo(f, raices, entorno)?, f.activo));
    }
    for l in &plan.enlaces {
        if !l.destino.exists() {
            return mal(format!("{} ya no existe", l.destino.display()));
        }
    }
    limpiar_sesiones(runtime, EDAD_MAXIMA, None);
    crear_directorios(&base)?;
    // Un directorio nuevo: si ya existe (otro lanzamiento del mismo plan, aún
    // vivo), se prueba con otro nonce; jamás se borra.
    let mut real = None;
    for _ in 0..16 {
        let dir = base.join(format!("{}-{}", plan.id, nonce()));
        match crear_dir_nuevo_privado(&dir) {
            Ok(()) => {
                poner_privado(&dir).map_err(|e| io(&dir, e))?;
                real = Some(dir);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(&dir, e)),
        }
    }
    let Some(dir) = real else { return mal("no se pudo crear un directorio de sesión nuevo") };
    let hecho = escribir_sesion(&reubicar(plan, &dir), &completas);
    match hecho {
        Ok(p) => Ok(p),
        Err(e) => {
            // El directorio es nuestro (acabamos de crearlo): se retira.
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

fn escribir_sesion(plan: &PlanSesion, completas: &[(String, Mcp, bool)]) -> Result<PlanSesion, ErrorBiblioteca> {
    let marca = plan.directorio.join(MARCA_CREACION);
    let ahora = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut h = opciones_privadas().write(true).create_new(true).open(&marca).map_err(|e| io(&marca, e))?;
    h.write_all(format!("{ahora}\n").as_bytes()).map_err(|e| io(&marca, e))?;
    for f in &plan.ficheros {
        if let Some(padre) = f.ruta.parent() {
            crear_directorios(padre)?;
        }
        let contenido = if f.con_secretos { texto_mcp(&plan.agente, completas) } else { f.contenido.clone() };
        let mut h = opciones_privadas().write(true).create_new(true).open(&f.ruta).map_err(|e| io(&f.ruta, e))?;
        if f.modo != MODO_FICHERO {
            poner_permisos_unix(&f.ruta, f.modo).map_err(|e| io(&f.ruta, e))?;
        }
        h.write_all(contenido.as_bytes()).map_err(|e| io(&f.ruta, e))?;
    }
    for l in &plan.enlaces {
        if let Some(padre) = l.ruta.parent() {
            crear_directorios(padre)?;
        }
        enlazar(&l.destino, &l.ruta).map_err(|e| io(&l.ruta, e))?;
    }
    Ok(plan.clone())
}
