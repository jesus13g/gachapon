//! Adaptadores del ejecutor (spec 017, RF-009): Qwen Code, Codex y OpenCode.
//!
//! Cada adaptador dice, con funciones puras que se prueban sin el binario,
//! cómo se llama su CLI (argumentos y entorno), qué directorios de estado
//! bajo `$HOME` necesita escribir dentro de bwrap y qué regla de
//! herramientas va en el prompt. Lanzarlo, cortarlo por tiempo o por
//! cancelación y extraer el resumen es común (`impl Ejecutor`).
//!
//! Todos van **sin herramientas web**, contra el endpoint local del perfil del
//! ejecutor y con aprobación automática solo porque corren dentro de bwrap.
//! Con `agente.sandbox = false` Codex usa su propio sandbox
//! (`workspace-write`); Qwen y OpenCode no tienen uno utilizable aquí.
//! Codex y OpenCode no admiten topes de turnos ni de llamadas: su único
//! tope es el `timeout` del sistema, igual al tiempo de la orden.

use std::path::{Path, PathBuf};
use std::process::Command;

use ea_bus::{ConfigOrquestador, PerfilEjecutor, TipoAgente, Topes};

use crate::ejecutor::{correr_cancelable, SalidaAgente};

/// Lo que el bucle del orquestador necesita de un ejecutor.
pub trait Ejecutor {
    /// `sistema` son las skills del trabajo; vacío si no hay. `cancelado`
    /// dice si el trabajo se ha cancelado: hay que consultarlo mientras
    /// corre y cortar en cuanto devuelva `true`.
    fn ejecutar(
        &self,
        prompt: &str,
        sistema: &str,
        worktree: &Path,
        topes: &Topes,
        config: &ConfigOrquestador,
        cancelado: &dyn Fn() -> bool,
    ) -> SalidaAgente;

    /// Regla sobre sus herramientas que va en las «Reglas» del prompt.
    fn regla_herramientas(&self) -> &str {
        ""
    }

    /// Código de salida con el que la CLI dice que agotó sus topes.
    fn codigo_topes(&self) -> Option<i32> {
        None
    }

    /// `Err` con el motivo si no se puede lanzar (p. ej. falta el binario).
    fn disponible(&self, _config: &ConfigOrquestador) -> Result<(), String> {
        Ok(())
    }
}

/// La parte propia de cada CLI.
pub trait AgenteCli {
    fn tipo(&self) -> TipoAgente;

    /// Argumentos del programa (sin él). `sandbox`: va dentro de bwrap.
    fn argumentos(&self, p: &PerfilEjecutor, prompt: &str, sistema: &str, wt: &Path, topes: &Topes, sandbox: bool) -> Vec<String>;

    /// Variables de entorno que se le añaden.
    fn entorno(&self, _p: &PerfilEjecutor) -> Vec<(String, String)> {
        vec![]
    }

    /// Directorios de estado, relativos a `$HOME`, escribibles dentro de bwrap.
    fn dirs_estado(&self) -> &'static [&'static str];

    /// Margen del `timeout` del sistema sobre el tiempo de la orden.
    fn margen_s(&self) -> u64 {
        0
    }

    fn regla_herramientas(&self) -> &'static str {
        ""
    }

    fn codigo_topes(&self) -> Option<i32> {
        None
    }
}

impl<A: AgenteCli> Ejecutor for A {
    fn ejecutar(
        &self,
        prompt: &str,
        sistema: &str,
        worktree: &Path,
        topes: &Topes,
        config: &ConfigOrquestador,
        cancelado: &dyn Fn() -> bool,
    ) -> SalidaAgente {
        let p = &config.ejecutor;
        let mut args: Vec<String> = Vec::new();
        let mut cmd = if config.sandbox {
            let home = std::env::var("HOME").unwrap_or_default();
            let ejecucion = ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso).filter(|d| d.is_dir());
            let estado = preparar_dirs_estado(Path::new(&home), self.dirs_estado());
            args.extend(argumentos_bwrap(worktree, ejecucion.as_deref(), &estado));
            args.push(p.bin.clone());
            Command::new("bwrap")
        } else {
            Command::new(&p.bin)
        };
        args.extend(self.argumentos(p, prompt, sistema, worktree, topes, config.sandbox));
        cmd.args(&args).current_dir(worktree);
        for (k, v) in self.entorno(p) {
            cmd.env(k, v);
        }
        let c = correr_cancelable(cmd, topes.tiempo_s + self.margen_s(), cancelado);
        let resumen = ea_bus::resumen_salida_agente(self.tipo(), &c.salida);
        SalidaAgente { codigo: c.codigo, bruta: c.salida, resumen, cancelado: c.cancelado }
    }

    fn regla_herramientas(&self) -> &str {
        AgenteCli::regla_herramientas(self)
    }

    fn codigo_topes(&self) -> Option<i32> {
        AgenteCli::codigo_topes(self)
    }

    fn disponible(&self, config: &ConfigOrquestador) -> Result<(), String> {
        let bin = &config.ejecutor.bin;
        buscar_binario(bin, std::env::var_os("PATH").as_deref())
            .map(|_| ())
            .ok_or_else(|| format!("no se encuentra el binario {bin:?} (agentes.{}.bin)", config.ejecutor.id))
    }
}

/// El adaptador del agente que ocupa el rol de ejecutor. Se llama en cada
/// ciclo: un cambio de `roles.ejecutor` vale para el siguiente trabajo.
pub fn fabricar(p: &PerfilEjecutor) -> Box<dyn Ejecutor + Send + Sync> {
    match p.tipo {
        TipoAgente::QwenCode => Box::new(QwenCode),
        TipoAgente::Codex => Box::new(Codex),
        TipoAgente::OpenCode => Box::new(OpenCode),
    }
}

/// Las skills van delante del prompt en las CLI que no tienen dónde
/// añadirlas al system prompt sin tocar su configuración global.
fn con_sistema(sistema: &str, prompt: &str) -> String {
    if sistema.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n\n{prompt}", sistema.trim())
    }
}

/// Una cadena TOML (para los `-c clave=valor` de Codex).
fn toml(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// --- Qwen Code ---------------------------------------------------------------

/// Qwen Code headless (`--output-format json`).
pub struct QwenCode;

impl AgenteCli for QwenCode {
    fn tipo(&self) -> TipoAgente {
        TipoAgente::QwenCode
    }

    fn argumentos(&self, p: &PerfilEjecutor, prompt: &str, sistema: &str, _: &Path, topes: &Topes, _: bool) -> Vec<String> {
        // `--exclude-tools` es un array: va antes de las demás opciones para
        // que no se trague la orden, que va la última. Las de objetivos y
        // notebooks no le sirven y cada herramienta declarada cuesta tokens.
        let mut a: Vec<String> = [
            "--exclude-tools", "web_fetch", "web_search", "get_goal", "update_goal", "notebook_edit", "--bare", "--auth-type", "openai",
            "--openai-base-url", &p.modelo.base_url, "--openai-api-key", "local", "-m", &p.modelo.nombre,
            "--approval-mode", "yolo",
        ]
        .map(String::from)
        .into();
        if !sistema.is_empty() {
            a.push("--append-system-prompt".into());
            a.push(sistema.into());
        }
        a.extend([
            "--max-session-turns".into(),
            topes.turnos.to_string(),
            "--max-wall-time".into(),
            format!("{}s", topes.tiempo_s),
            "--max-tool-calls".into(),
            topes.llamadas_herramienta.to_string(),
            "--output-format".into(),
            "json".into(),
            prompt.into(),
        ]);
        a
    }

    fn dirs_estado(&self) -> &'static [&'static str] {
        &[".qwen"]
    }

    /// Qwen corta solo por sus topes; el `timeout` es la red de seguridad.
    fn margen_s(&self) -> u64 {
        120
    }

    fn regla_herramientas(&self) -> &'static str {
        "Para crear un fichero nuevo usa run_shell_command con `cat > ruta <<'EOF'`: no hay herramienta para escribir ficheros. `edit` solo cambia ficheros que ya has leído con read_file."
    }

    /// Qwen Code sale con 55 al agotar un tope.
    fn codigo_topes(&self) -> Option<i32> {
        Some(55)
    }
}

// --- Codex ---------------------------------------------------------------------

/// Herramientas de Codex que salen a la red o al escritorio, o que instalan
/// cosas: fuera, además de `web_search`. Las de objetivos y de ver imágenes
/// tampoco le sirven y cuestan tokens en cada turno, como en Qwen. Con estas,
/// Codex 0.159 solo declara `exec_command`, `write_stdin` y
/// `request_user_input` (comprobado capturando la petición, T-007).
pub const CODEX_SIN: &[&str] = &[
    "goals",
    "view_image",
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "in_app_browser",
    "image_generation",
    "plugins",
    "remote_plugin",
    "multi_agent",
    "tool_suggest",
    "skill_mcp_dependency_install",
];

/// `codex exec --json` con un proveedor `ea` que apunta al endpoint local.
pub struct Codex;

impl AgenteCli for Codex {
    fn tipo(&self) -> TipoAgente {
        TipoAgente::Codex
    }

    fn argumentos(&self, p: &PerfilEjecutor, prompt: &str, sistema: &str, wt: &Path, _: &Topes, sandbox: bool) -> Vec<String> {
        let mut a: Vec<String> = ["exec", "--json", "--ephemeral", "--skip-git-repo-check", "--ignore-user-config", "--ignore-rules", "-C"]
            .map(String::from)
            .into();
        a.push(wt.to_string_lossy().into());
        for c in [
            "model_provider=\"ea\"".to_string(),
            "model_providers.ea.name=\"ea\"".to_string(),
            format!("model_providers.ea.base_url={}", toml(&p.modelo.base_url)),
            "model_providers.ea.wire_api=\"responses\"".to_string(),
            "web_search=\"disabled\"".to_string(),
        ] {
            a.push("-c".into());
            a.push(c);
        }
        for f in CODEX_SIN {
            a.push("--disable".into());
            a.push((*f).into());
        }
        a.push("-m".into());
        a.push(p.modelo.nombre.clone());
        if sandbox {
            // Dentro de bwrap su sandbox (landlock) sobra y puede fallar.
            a.push("--dangerously-bypass-approvals-and-sandbox".into());
        } else {
            a.extend(["--sandbox".into(), "workspace-write".into()]);
        }
        a.push(con_sistema(sistema, prompt));
        a
    }

    /// `CODEX_HOME` por defecto (registros y autenticación).
    fn dirs_estado(&self) -> &'static [&'static str] {
        &[".codex"]
    }
}

// --- OpenCode ------------------------------------------------------------------

/// Herramientas de OpenCode que se deniegan: las de la web, el intérprete de
/// JavaScript (`execute`) y las preguntas al usuario (no hay nadie delante).
pub const OPENCODE_DENEGADAS: &[&str] = &["webfetch", "websearch", "codesearch", "execute", "question"];

/// `opencode run --format json` con una configuración en línea
/// (`OPENCODE_CONFIG_CONTENT`): ni la del usuario ni la del proyecto.
pub struct OpenCode;

impl OpenCode {
    /// La configuración que se le pasa: el proveedor `ea` con el modelo del
    /// perfil y los permisos.
    pub fn configuracion(p: &PerfilEjecutor) -> serde_json::Value {
        let permisos: serde_json::Map<String, serde_json::Value> =
            OPENCODE_DENEGADAS.iter().map(|h| ((*h).to_string(), "deny".into())).collect();
        serde_json::json!({
            "autoupdate": false,
            "share": "disabled",
            "model": format!("ea/{}", p.modelo.nombre),
            "provider": {
                "ea": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "ea",
                    "options": { "baseURL": p.modelo.base_url, "apiKey": "local" },
                    "models": { p.modelo.nombre.clone(): { "name": p.modelo.nombre } }
                }
            },
            "permission": permisos,
        })
    }
}

impl AgenteCli for OpenCode {
    fn tipo(&self) -> TipoAgente {
        TipoAgente::OpenCode
    }

    fn argumentos(&self, p: &PerfilEjecutor, prompt: &str, sistema: &str, _: &Path, _: &Topes, _: bool) -> Vec<String> {
        let mut a: Vec<String> = ["run", "--format", "json", "--auto", "--standalone", "-m"].map(String::from).into();
        a.push(format!("ea/{}", p.modelo.nombre));
        a.push(con_sistema(sistema, prompt));
        a
    }

    fn entorno(&self, p: &PerfilEjecutor) -> Vec<(String, String)> {
        let mut e = vec![("OPENCODE_CONFIG_CONTENT".to_string(), Self::configuracion(p).to_string())];
        for v in ["OPENCODE_DISABLE_AUTOUPDATE", "OPENCODE_DISABLE_PROJECT_CONFIG", "OPENCODE_DISABLE_MODELS_FETCH", "OPENCODE_DISABLE_CLAUDE_CODE"] {
            e.push((v.to_string(), "1".to_string()));
        }
        e
    }

    fn dirs_estado(&self) -> &'static [&'static str] {
        &[".local/share/opencode", ".local/state/opencode", ".cache/opencode"]
    }
}

// --- comunes -------------------------------------------------------------------

/// Crea (si faltan) los directorios de estado del agente bajo `home` y
/// devuelve los que existen: bwrap no puede montar uno que no existe.
fn preparar_dirs_estado(home: &Path, dirs: &[&str]) -> Vec<PathBuf> {
    if home.as_os_str().is_empty() {
        return vec![];
    }
    dirs.iter()
        .map(|d| home.join(d))
        .filter(|d| std::fs::create_dir_all(d).is_ok() && d.is_dir())
        .collect()
}

/// Argumentos de `bwrap` (sin el programa): `/` en solo lectura, `/tmp`
/// propio, el worktree y los directorios de estado del agente escribibles.
/// El directorio de ejecución de la sesión (`$XDG_RUNTIME_DIR`), si existe,
/// queda tapado por un tmpfs vacío: ahí está la clave de sesión del panel
/// web (con ella el agente, que tiene la red abierta, podría escribir por
/// `127.0.0.1:8092`), y también los sockets de D-Bus y de `systemd --user`,
/// que no necesita. Se tapa entero y no solo `ea-web/`: así no hace falta
/// que exista ese subdirectorio (bwrap no puede crear el punto de montaje en
/// un `/` de solo lectura) y no queda ningún otro socket de la sesión a su
/// alcance.
pub fn argumentos_bwrap(worktree: &Path, ejecucion: Option<&Path>, estado: &[PathBuf]) -> Vec<String> {
    let wt = worktree.to_string_lossy().to_string();
    let mut args: Vec<String> =
        ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"].map(String::from).into();
    if let Some(d) = ejecucion {
        args.push("--tmpfs".into());
        args.push(d.to_string_lossy().into());
    }
    // Los bind escribibles van después del tmpfs: si alguno cayera dentro del
    // directorio de ejecución, se montaría encima y seguiría visible.
    for d in estado {
        let d = d.to_string_lossy().to_string();
        args.extend(["--bind".into(), d.clone(), d]);
    }
    for a in ["--bind", &wt, &wt, "--chdir", &wt, "--unshare-pid", "--die-with-parent"] {
        args.push(a.into());
    }
    args
}

/// La ruta del binario: tal cual si lleva `/`, si no, en el `PATH`.
fn buscar_binario(bin: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let ejecutable = |p: &Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if bin.is_empty() {
        return None;
    }
    if bin.contains('/') {
        let p = PathBuf::from(bin);
        return ejecutable(&p).then_some(p);
    }
    std::env::split_paths(path?).map(|d| d.join(bin)).find(|p| ejecutable(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_ajustes::PerfilModelo;

    fn perfil(tipo: TipoAgente) -> PerfilEjecutor {
        PerfilEjecutor {
            id: tipo.id().into(),
            tipo,
            bin: tipo.id().into(),
            modelo: PerfilModelo { nombre: "qwen3-coder-30b".into(), base_url: "http://127.0.0.1:8080/v1".into(), remoto: false },
        }
    }

    fn topes() -> Topes {
        Topes { turnos: 7, tiempo_s: 300, llamadas_herramienta: 9, iteraciones: 2 }
    }

    fn args(a: &dyn AgenteCli, sistema: &str, sandbox: bool) -> Vec<String> {
        a.argumentos(&perfil(a.tipo()), "# tarea", sistema, Path::new("/w/repo"), &topes(), sandbox)
    }

    fn par(v: &[String], a: &str, b: &str) -> bool {
        v.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    /// Ninguna palabra de las herramientas web aparece como algo activado.
    fn sin_web(v: &[String]) {
        for w in v {
            assert!(!w.contains("web_search=\"live\"") && !w.contains("web_search=\"cached\""), "{w}");
        }
    }

    #[test]
    fn qwen_va_contra_el_endpoint_local_sin_web_y_con_sus_topes() {
        let a = args(&QwenCode, "## skill", true);
        assert!(par(&a, "--exclude-tools", "web_fetch") && a.contains(&"web_search".into()));
        assert!(par(&a, "--openai-base-url", "http://127.0.0.1:8080/v1") && par(&a, "-m", "qwen3-coder-30b"));
        assert!(par(&a, "--append-system-prompt", "## skill"));
        assert!(par(&a, "--max-session-turns", "7") && par(&a, "--max-wall-time", "300s") && par(&a, "--max-tool-calls", "9"));
        assert_eq!(a.last().unwrap(), "# tarea", "la orden va la última");
        assert!(!args(&QwenCode, "", true).contains(&"--append-system-prompt".into()));
        assert_eq!((AgenteCli::codigo_topes(&QwenCode), QwenCode.margen_s()), (Some(55), 120));
        assert!(AgenteCli::regla_herramientas(&QwenCode).contains("run_shell_command"));
    }

    #[test]
    fn codex_va_contra_el_endpoint_local_sin_web() {
        let a = args(&Codex, "## skill", true);
        assert_eq!(&a[..2], ["exec", "--json"]);
        assert!(a.contains(&"--ignore-user-config".into()), "sin la config del usuario (MCP, otros proveedores…)");
        assert!(par(&a, "-c", "model_provider=\"ea\""));
        assert!(par(&a, "-c", "model_providers.ea.base_url=\"http://127.0.0.1:8080/v1\""));
        assert!(par(&a, "-c", "web_search=\"disabled\""));
        for f in ["browser_use", "apps", "computer_use", "plugins"] {
            assert!(par(&a, "--disable", f), "{f}");
        }
        assert!(par(&a, "-m", "qwen3-coder-30b") && par(&a, "-C", "/w/repo"));
        assert!(a.contains(&"--dangerously-bypass-approvals-and-sandbox".into()), "dentro de bwrap");
        assert!(a.last().unwrap().starts_with("## skill") && a.last().unwrap().ends_with("# tarea"));
        sin_web(&a);
        // Fuera de bwrap, su propio sandbox; nunca sin ninguno.
        let fuera = args(&Codex, "", false);
        assert!(par(&fuera, "--sandbox", "workspace-write"));
        assert!(!fuera.contains(&"--dangerously-bypass-approvals-and-sandbox".into()));
        assert_eq!(fuera.last().unwrap(), "# tarea");
        assert_eq!((AgenteCli::codigo_topes(&Codex), Codex.margen_s()), (None, 0), "solo el timeout");
        // Una URL con comillas no rompe el TOML del `-c`.
        assert_eq!(toml(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn opencode_va_contra_el_endpoint_local_sin_web() {
        let a = args(&OpenCode, "## skill", true);
        assert_eq!(&a[..3], ["run", "--format", "json"]);
        assert!(par(&a, "-m", "ea/qwen3-coder-30b") && a.contains(&"--standalone".into()));
        assert!(a.last().unwrap().starts_with("## skill"));
        let e: std::collections::HashMap<_, _> = OpenCode.entorno(&perfil(TipoAgente::OpenCode)).into_iter().collect();
        assert_eq!(e["OPENCODE_DISABLE_PROJECT_CONFIG"], "1", "la config del worktree no cuenta");
        let c: serde_json::Value = serde_json::from_str(&e["OPENCODE_CONFIG_CONTENT"]).unwrap();
        assert_eq!(c["provider"]["ea"]["options"]["baseURL"], "http://127.0.0.1:8080/v1");
        assert!(c["provider"]["ea"]["models"]["qwen3-coder-30b"].is_object());
        for h in ["webfetch", "websearch", "execute", "question"] {
            assert_eq!(c["permission"][h], "deny", "{h}");
        }
        sin_web(&a);
    }

    #[test]
    fn fabricar_da_el_adaptador_del_tipo() {
        for t in [TipoAgente::QwenCode, TipoAgente::Codex, TipoAgente::OpenCode] {
            let e = fabricar(&perfil(t));
            assert_eq!(e.codigo_topes().is_some(), t == TipoAgente::QwenCode);
        }
    }

    #[test]
    fn bwrap_oculta_el_directorio_de_ejecucion_y_monta_el_estado_del_agente() {
        let ejecucion = tempfile::tempdir().unwrap();
        let ej = ejecucion.path().to_string_lossy().to_string();
        let estado = [PathBuf::from("/home/u/.codex")];
        let args = argumentos_bwrap(Path::new("/w/repo"), Some(ejecucion.path()), &estado);
        let pos = |par: [&str; 2]| args.windows(2).position(|w| w[0] == par[0] && w[1] == par[1]);
        let tmpfs = pos(["--tmpfs", &ej]).expect("tmpfs sobre el directorio de ejecución");
        // Encima del / de solo lectura y antes de los bind escribibles.
        assert!(pos(["--ro-bind", "/"]).unwrap() < tmpfs);
        assert!(tmpfs < pos(["--bind", "/w/repo"]).unwrap());
        assert!(tmpfs < pos(["--bind", "/home/u/.codex"]).unwrap());
        assert!(pos(["--bind", "/home/u/.qwen"]).is_none(), "solo el estado de este agente");
        assert!(args.ends_with(&["--unshare-pid".into(), "--die-with-parent".into()]));
        // Sin directorio de ejecución no hay nada que tapar.
        let sin = argumentos_bwrap(Path::new("/w/repo"), None, &[]);
        assert_eq!(sin.iter().filter(|a| *a == "--tmpfs").count(), 1, "solo el de /tmp");
    }

    #[test]
    fn los_dirs_de_estado_se_crean_bajo_home() {
        let h = tempfile::tempdir().unwrap();
        let d = preparar_dirs_estado(h.path(), OpenCode.dirs_estado());
        assert_eq!(d.len(), 3);
        assert!(h.path().join(".local/share/opencode").is_dir());
        assert!(preparar_dirs_estado(Path::new(""), &[".qwen"]).is_empty(), "sin HOME no se monta nada");
    }

    #[test]
    fn el_binario_se_busca_en_el_path() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let b = d.path().join("codex");
        std::fs::write(&b, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.path().join("noejec"), "").unwrap();
        let path = std::ffi::OsString::from(format!("/no/existe:{}", d.path().display()));
        assert_eq!(buscar_binario("codex", Some(&path)), Some(b.clone()));
        assert_eq!(buscar_binario("noejec", Some(&path)), None);
        assert_eq!(buscar_binario("opencode", Some(&path)), None);
        assert_eq!(buscar_binario(&b.to_string_lossy(), None), Some(b));
        assert_eq!(buscar_binario("", Some(&path)), None);
    }
}
