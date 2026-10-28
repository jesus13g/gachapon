//! Lo común a todas las áreas: el panel montado sobre un estado temporal
//! (bus, base, bóveda, ajustes, HOME de la biblioteca y timers falsos; nunca
//! el estado real) y las peticiones como las haría la página.

pub use std::sync::Arc;

pub use axum::Router;
pub use axum::body::Body;
pub use axum::http::{Method, Request, StatusCode};
pub use ea_pruebas::{escribe, sin_secretos};
pub use ea_store::{EventOutcome, NewEvent, Store};
pub use ea_web::{AppState, WebConfig};
pub use http_body_util::BodyExt;
pub use serde_json::{Value, json};
pub use tempfile::TempDir;
pub use tower::ServiceExt;

pub const RUTINA_EJEMPLO: &str = "descripcion = \"Revisión diaria de ejemplo\"
";

pub struct Fixture {
    pub state: Arc<AppState>,
    pub app: Router,
    pub _dir: TempDir,
    /// Las terminales que la sección Biblioteca habría abierto.
    pub terminales: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
}

pub fn texto(descripcion: &str) -> ea_store::NuevaPropuesta {
    ea_store::NuevaPropuesta { proponente: "claude-code".into(), descripcion: descripcion.into(), accion: None }
}

/// Con el repo común de las pruebas de worktree (U-05: el de una rutina es la
/// raíz de un repo con commits).
pub static RUTINA_BUS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    concat!(
        "[rutina]\nslug = \"revision-diaria\"\nversion = 1\nworktree = \"",
        tmp!(),
        "\"\ncriterio_exito = \"test -f {salida}\"\n[entradas]\nsalida = \"informe.md\"\n[[pasos]]\ntexto = \"revisa y escribe {salida}\"\n"
    )
    .replace(tmp!(), ea_pruebas::repo_comun())
});

/// El bus de la prueba: el mismo directorio temporal que lee el panel
/// (`WebConfig::estado_dir`), nunca el real.
pub fn bus(f: &Fixture) -> ea_bus::Bus {
    ea_bus::Bus::abrir_en(&f.state.config.estado_dir).unwrap()
}

pub fn montar() -> Fixture {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let rutinas_dir = dir.path().join("rutinas");
    std::fs::create_dir_all(&rutinas_dir).unwrap();
    std::fs::write(rutinas_dir.join("revision-diaria.toml"), RUTINA_EJEMPLO).unwrap();

    let store = Store::open_at(&dir.path().join("test.db")).expect("abrir store");
    ea_pruebas::guardar_rutinas(&store, &[("revision-diaria", 1, &RUTINA_BUS)]);
    // El `ea` que abriría la terminal: un fichero, como el de junto a ea-web.
    escribe(dir.path().join("bin/ea"), "");
    let terminal = Arc::new(ea_acciones::TerminalSimulada::default());
    let terminales = terminal.lanzadas.clone();
    let config = WebConfig {
        puerto: 0,
        umbral_heartbeat_s: 60,
        tema_path: dir.path().join("no-existe.toml"),
        rutinas_dir,
        boveda: ea_boveda::Boveda::en(dir.path().join("boveda")),
        systemctl: false,
        sesiones_dir: Some(dir.path().join("sesiones")),
        subagentes_dir: Some(dir.path().join("subagentes")),
        proyectos_claude_dir: Some(dir.path().join("proyectos")),
        estado_dir: dir.path().join("estado"),
        ajustes: ea_ajustes::Fuentes::aislada(dir.path()),
        temporizadores: temporizadores_falsos(dir.path()),
        ea: Some(dir.path().join("bin/ea")),
        terminal,
        biblioteca_raices: Some(raices_biblioteca(dir.path())),
    };
    let state = Arc::new(AppState::new(store, config).expect("estado"));
    Fixture { app: ea_web::http::router(state.clone()), state, _dir: dir, terminales }
}

/// Un HOME de prueba con un elemento de varios tipos y agentes, y secretos en
/// sus MCP; el proyecto, el estado y el runtime, también temporales.
pub fn raices_biblioteca(dir: &std::path::Path) -> ea_biblioteca::Raices {
    let home = dir.join("home");
    let proyecto = dir.join("proyecto");
    std::fs::create_dir_all(&proyecto).unwrap();
    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribe(
        home.join(".claude.json"),
        r#"{"mcpServers":{
            "stdio-claude":{"type":"stdio","command":"npx","args":["-y","@scope/server","--token","SECRETO_ARG_TOKEN"],"env":{"API_KEY":"SECRETO_ENV_CLAUDE"}},
            "remoto-claude":{"type":"http","url":"https://mcp.ejemplo.com:8443/SECRETO_RUTA_URL/mcp?k=SECRETO_CONSULTA_URL","headers":{"Authorization":"Bearer SECRETO_HEADER_CLAUDE"}}},
          "skillUsage":{"propia":{"usageCount":4,"lastUsedAt":1791408135818}}}"#,
    );
    escribe(
        home.join(".codex/config.toml"),
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--api-key=SECRETO_ENV_CODEX\"]\n[mcp_servers.docs.env]\nTOKEN = \"SECRETO_ENV_CODEX\"\n[mcp_servers.apagado]\ncommand = \"otro\"\nenabled = false\n",
    );
    escribe(
        home.join(".config/opencode/opencode.json"),
        r#"{"mcp":{"servers":{"remoto-oc":{"type":"remote","url":"https://oc.ejemplo.com/SECRETO_RUTA_URL","headers":{"Authorization":"SECRETO_HEADER_OPENCODE"}}}}}"#,
    );
    let mut r = ea_biblioteca::Raices::nuevo(home, Some(proyecto), dir.join("estado-biblioteca"), Some(dir.join("runtime")));
    r.repo = None;
    r
}

/// Como el panel abierto con `ea-web-open`: con la clave de sesión del
/// estado de prueba (que nunca se escribe en el `$XDG_RUNTIME_DIR` real).
pub async fn enviar(f: &Fixture, metodo: Method, ruta: &str, host: &str, cuerpo: Option<Value>, json_ct: bool) -> (StatusCode, String) {
    let clave = f.state.clave.texto().to_string();
    enviar_con_clave(f, metodo, ruta, host, cuerpo, json_ct, Some(&clave)).await
}

pub async fn enviar_con_clave(
    f: &Fixture,
    metodo: Method,
    ruta: &str,
    host: &str,
    cuerpo: Option<Value>,
    json_ct: bool,
    clave: Option<&str>,
) -> (StatusCode, String) {
    let mut req = Request::builder().method(metodo).uri(ruta).header("host", host);
    if let Some(c) = clave {
        req = req.header("x-ea-clave", c);
    }
    if json_ct {
        req = req.header("content-type", "application/json");
    }
    let body = cuerpo.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty);
    let resp = f.app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn get(f: &Fixture, ruta: &str) -> (StatusCode, Value) {
    let (s, t) = enviar(f, Method::GET, ruta, "localhost:8092", None, false).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

pub async fn post(f: &Fixture, ruta: &str, cuerpo: Value) -> (StatusCode, Value) {
    let (s, t) = enviar(f, Method::POST, ruta, "localhost:8092", Some(cuerpo), true).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

pub fn orden(modo: ea_bus::Modo, titulo: &str, worktree: &str) -> ea_bus::Orden {
    ea_bus::Orden { modo, ..ea_bus::Orden::mision(titulo, worktree, "hacer algo", "true", "test") }
}

/// `systemctl` y `systemd-analyze` falsos para los timers de las rutinas
/// (`ea_pruebas::systemd_falso`): nunca el `~/.config/systemd/user` real ni el
/// `systemctl` de verdad. Las unidades van a `<dir>/unidades`; las órdenes, a
/// `<dir>/ordenes.log`. Sin `sh` (Windows), órdenes que no existen: las
/// pruebas que dependen de ellas se compilan solo en Unix.
pub fn temporizadores_falsos(dir: &std::path::Path) -> ea_acciones::Temporizadores {
    let (systemctl, analyze) = if cfg!(unix) {
        let (systemctl, analyze, _) = ea_pruebas::systemd_falso(dir);
        (systemctl, analyze)
    } else {
        let no = || vec!["orden-inexistente-para-las-pruebas".to_string()];
        (no(), no())
    };
    ea_acciones::Temporizadores::systemd(dir.join("unidades"), systemctl, analyze)
}

/// Lo que la terminal simulada de la prueba habría lanzado.
pub fn lanzadas(f: &Fixture) -> Vec<Vec<String>> {
    f.terminales.lock().unwrap().clone()
}

pub fn nota(f: &Fixture, autor: &str, titulo: &str, cuerpo: &str) -> String {
    let mut s = Store::open_at(&f.state.config.rutinas_dir.parent().unwrap().join("test.db")).unwrap();
    f.state
        .boveda
        .escribir_nota(
            &mut s,
            autor,
            &ea_boveda::NuevaNota { titulo, cuerpo, ruta: None, tags: &[], origen: "manual", sobrescribir: false },
            &ea_boveda::Limites::AMPLIOS,
        )
        .unwrap()
        .ruta
}

pub async fn proponer_accion(f: &Fixture, herramienta: &str, args: Value) -> i64 {
    let sis = ea_acciones::SoloConsulta::usuario(f.state.config.rutinas_dir.clone(), f.state.boveda.clone(), f.state.config.estado_dir.clone());
    let hechos = ea_acciones::observar(&sis, herramienta, &args).await.unwrap();
    let store = f.state.store.lock().await;
    ea_acciones::proponer(&store, &sis, &hechos, "claude-code", None, herramienta, &args).unwrap().0.id
}

pub fn acciones_del_diario(d: &Value) -> Vec<String> {
    d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect()
}

/// Un repo git de verdad dentro del directorio temporal de la prueba, con
/// un commit inicial (el ejecutor arranca con `rev-parse HEAD`).
pub fn repo_git(f: &Fixture) -> std::path::PathBuf {
    let wt = repo_git_vacio(f, "repo");
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(&wt)
        .args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"])
        .args(["commit", "-q", "--allow-empty", "-m", "inicial"])
        .status()
        .unwrap()
        .success();
    assert!(ok, "git commit");
    wt
}

/// Un repo recién iniciado, sin ningún commit.
pub fn repo_git_vacio(f: &Fixture, nombre: &str) -> std::path::PathBuf {
    let wt = f._dir.path().join(nombre);
    std::fs::create_dir_all(&wt).unwrap();
    let ok = std::process::Command::new("git").arg("init").arg("-q").arg(&wt).status().unwrap().success();
    assert!(ok, "git init");
    ea_plataforma::rutas::canonica(&wt).unwrap()
}

pub fn mision(wt: &str) -> Value {
    json!({
        "titulo": "Añadir un saludo",
        "worktree": wt,
        "objetivo": "Crea hola.txt con un saludo.",
        "criterio": "test -f hola.txt",
        "topes": { "iteraciones": 3, "tiempo_s": 999_999 },
    })
}
