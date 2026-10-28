//! Sesiones con un subconjunto de herramientas (F5): plan puro, secretos
//! solo en ficheros 0600 y rechazos. Todo sobre un HOME y un runtime de
//! `tempfile`; no se ejecuta ningún agente.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ea_ajustes::{Ajustes, Fuentes};
use ea_biblioteca::*;
use ea_pruebas::escribe;

struct Mundo {
    _d: tempfile::TempDir,
    home: PathBuf,
    proyecto: PathBuf,
    runtime: PathBuf,
    inv: Inventario,
    r: Raices,
}

const SECRETOS: &[&str] = &[
    "SECRETO_ENV_CLAUDE",
    "SECRETO_ARG_TOKEN",
    "SECRETO_HEADER_CLAUDE",
    "SECRETO_RUTA_URL",
    "SECRETO_CONSULTA_URL",
    "SECRETO_USUARIO_URL",
    "SECRETO_ENV_CODEX",
    "SECRETO_HEADER_OPENCODE",
    "SECRETO_ENV_OPENCODE",
    "SECRETO_QWEN",
];

fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let proyecto = d.path().join("proyecto");
    let repo = d.path().join("repo");
    let runtime = d.path().join("rt");
    fs::create_dir_all(&proyecto).unwrap();
    fs::create_dir_all(&runtime).unwrap();

    escribe(repo.join("Cargo.toml"), "[workspace]\n");
    escribe(repo.join("claude/skills/mision/SKILL.md"), "---\ndescription: Lanza una misión\n---\n");
    escribe(repo.join("qwen/skills/depurar.md"), "---\ndescripcion: Arreglar\n---\n");

    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribe(home.join(".claude/agents/revisor.md"), "---\ndescription: Revisa\n---\n");
    escribe(home.join(".claude/commands/saludo.md"), "---\ndescription: Saluda\n---\n");
    let plugin_dir = home.join(".claude/plugins/cache/mkt/foo/1.0");
    escribe(plugin_dir.join("skills/dentro/SKILL.md"), "---\ndescription: Viene en el plugin\n---\n");
    escribe(
        home.join(".claude/plugins/installed_plugins.json"),
        &format!(
            r#"{{"version":2,"plugins":{{
              "foo@mkt":[{{"scope":"user","installPath":"{}","version":"1.0"}}],
              "apagado@mkt":[{{"scope":"user","installPath":"/no/existe","version":"2"}}]
            }}}}"#,
            plugin_dir.display().to_string().replace('\\', "/")
        ),
    );
    escribe(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"foo@mkt":true,"apagado@mkt":true}}"#);
    escribe(
        home.join(".claude.json"),
        &format!(
            r#"{{"mcpServers":{{
                "stdio-claude":{{"type":"stdio","command":"npx","args":["-y","@scope/server","--token","SECRETO_ARG_TOKEN"],"env":{{"API_KEY":"SECRETO_ENV_CLAUDE"}}}},
                "remoto-claude":{{"type":"http","url":"https://user:SECRETO_USUARIO_URL@mcp.ejemplo.com:8443/SECRETO_RUTA_URL/mcp?k=SECRETO_CONSULTA_URL","headers":{{"Authorization":"Bearer SECRETO_HEADER_CLAUDE"}}}}
              }},
              "projects":{{"{}":{{"mcpServers":{{"del-proyecto":{{"command":"servidor","args":["--ok"]}}}}}}}}
            }}"#,
            proyecto.display().to_string().replace('\\', "/")
        ),
    );
    escribe(proyecto.join(".mcp.json"), r#"{"mcpServers":{"aprobado":{"command":"c"}}}"#);
    escribe(
        home.join(".codex/config.toml"),
        r#"[mcp_servers.docs]
command = "docs-mcp"
args = ["--api-key=SECRETO_ENV_CODEX"]
[mcp_servers.docs.env]
TOKEN = "SECRETO_ENV_CODEX"
[mcp_servers.apagado]
command = "otro"
enabled = false
[mcp_servers.raro]
command = "raro"
"#,
    );
    escribe(
        home.join(".config/opencode/opencode.json"),
        r#"{"mcp":{"servers":{
            "remoto-oc":{"type":"remote","url":"https://oc.ejemplo.com/SECRETO_RUTA_URL","headers":{"Authorization":"SECRETO_HEADER_OPENCODE"}},
            "local-oc":{"type":"local","command":["bun","run","x"],"environment":{"K":"SECRETO_ENV_OPENCODE"}}}}}"#,
    );
    escribe(
        home.join(".config/opencode/opencode.jsonc"),
        "{\n // un comentario\n \"mcp\": { \"plano\": { \"type\": \"local\", \"command\": [\"x\"], }, },\n}",
    );
    escribe(
        home.join(".qwen/settings.json"),
        r#"{"mcpServers":{"qmcp":{"httpUrl":"https://q.ejemplo.com/SECRETO_RUTA_URL","headers":{"X":"SECRETO_QWEN"}}}}"#,
    );
    escribe(home.join(".qwen/extensions/ext1/qwen-extension.json"), r#"{"name":"ext1"}"#);
    escribe(home.join(".gemini/config/skills/ag/SKILL.md"), "---\ndescription: Skill de agy\n---\n");

    let ajustes = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
    let mut r = Raices::nuevo(home.clone(), Some(proyecto.clone()), home.join(".local/state/ea"), Some(runtime.clone()));
    r.repo = Some(repo);
    let inv = Biblioteca::cargar(&r, &ajustes);
    Mundo { home, proyecto, runtime, inv, r, _d: d }
}

fn ids(m: &Mundo, textos: &[&str]) -> Vec<Id> {
    textos
        .iter()
        .map(|t| {
            assert!(m.inv.buscar(t).is_some(), "falta {t}; hay {:#?}", m.inv.elementos.iter().map(|e| &e.id).collect::<Vec<_>>());
            t.parse().unwrap()
        })
        .collect()
}

fn sin_secretos(plan: &PlanSesion) {
    let todo = serde_json::to_string(plan).unwrap() + &plan.comando() + &format!("{:?}", plan.entorno);
    ea_pruebas::sin_secretos(&todo, SECRETOS);
}

/// Comprueba los bits de permiso (solo en Unix; en Windows no hay modos).
fn assert_modo(p: &Path, esperado: u32) {
    if let Some(m) = ea_plataforma::fs::permisos_unix(&fs::metadata(p).unwrap()) {
        assert_eq!(m & 0o777, esperado, "{}", p.display());
    }
}

fn texto(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn plan_de_claude_code_con_mcp_plugins_skills_y_subagentes() {
    let m = mundo();
    let sel = ids(
        &m,
        &[
            "claude-code/mcp/usuario/stdio-claude",
            "claude-code/mcp/usuario/remoto-claude",
            "claude-code/mcp/proyecto/del-proyecto",
            "claude-code/plugin/usuario/foo@mkt",
            "claude-code/skill/usuario/propia",
            "claude-code/subagente/usuario/revisor",
            "claude-code/comando/usuario/saludo",
            "ecosistema/skill/ecosistema/mision",
        ],
    );
    let plan = plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap();
    let dir = m.runtime.join("ea-biblioteca").join("sesiones").join(&plan.id);
    assert_eq!(plan.directorio, dir);
    let d = |n: &str| dir.join(n).display().to_string();
    assert_eq!(
        plan.argv,
        ["claude", "--strict-mcp-config", "--mcp-config", &d("mcp.json"), "--settings", &d("ajustes.json"), "--plugin-dir", &d("complemento")]
    );
    assert!(plan.entorno.is_empty());
    assert_eq!(plan.argv_terminal(), ["env", "-C", &m.proyecto.display().to_string()].into_iter().map(String::from).chain(plan.argv.clone()).collect::<Vec<_>>());
    assert!(plan.ficheros.iter().all(|f| f.modo == 0o600));
    sin_secretos(&plan);

    let ajustes: serde_json::Value = serde_json::from_str(&plan.ficheros.iter().find(|f| f.ruta.ends_with("ajustes.json")).unwrap().contenido).unwrap();
    assert_eq!(ajustes["enabledPlugins"], serde_json::json!({ "foo@mkt": true, "apagado@mkt": false }));
    let enlaces: Vec<String> = plan.enlaces.iter().map(|e| e.ruta.strip_prefix(&dir).unwrap().display().to_string().replace('\\', "/")).collect();
    assert_eq!(enlaces, ["complemento/skills/propia", "complemento/skills/mision", "complemento/agents/revisor.md", "complemento/commands/saludo.md"]);

    // El plan es determinista (la simulación y la ejecución coinciden).
    assert_eq!(plan, plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap());
}

#[test]
fn materializar_claude_escribe_los_secretos_solo_en_ficheros_0600() {
    let m = mundo();
    let sel = ids(&m, &["claude-code/mcp/usuario/stdio-claude", "claude-code/mcp/usuario/remoto-claude", "claude-code/mcp/proyecto/del-proyecto", "claude-code/skill/usuario/propia"]);
    let plan = plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap();
    sin_secretos(&plan);
    let real = materializar(&plan, &m.r).unwrap();
    assert!(real.directorio.starts_with(m.runtime.join("ea-biblioteca/sesiones")) && real.directorio != plan.directorio);
    assert!(real.directorio.file_name().unwrap().to_str().unwrap().starts_with(&format!("{}-", plan.id)));
    assert!(real.argv.iter().all(|a| !a.contains(&plan.directorio.display().to_string()) || a.contains(&real.directorio.display().to_string())));
    let plan = real;

    assert_modo(&plan.directorio, 0o700);
    assert_modo(plan.directorio.parent().unwrap(), 0o700);
    for f in &plan.ficheros {
        assert_modo(&f.ruta, 0o600);
    }
    let mcp: serde_json::Value = serde_json::from_str(&texto(&plan.directorio.join("mcp.json"))).unwrap();
    assert_eq!(mcp["mcpServers"]["stdio-claude"]["env"]["API_KEY"], "SECRETO_ENV_CLAUDE");
    assert_eq!(mcp["mcpServers"]["stdio-claude"]["args"][3], "SECRETO_ARG_TOKEN");
    assert_eq!(mcp["mcpServers"]["remoto-claude"]["headers"]["Authorization"], "Bearer SECRETO_HEADER_CLAUDE");
    assert_eq!(mcp["mcpServers"]["remoto-claude"]["url"], "https://user:SECRETO_USUARIO_URL@mcp.ejemplo.com:8443/SECRETO_RUTA_URL/mcp?k=SECRETO_CONSULTA_URL");
    assert_eq!(mcp["mcpServers"]["remoto-claude"]["type"], "http");
    assert_eq!(mcp["mcpServers"]["del-proyecto"]["command"], "servidor");
    let enlace = plan.directorio.join("complemento/skills/propia");
    assert_eq!(fs::read_link(&enlace).unwrap(), m.home.join(".claude/skills/propia"));
    assert!(plan.directorio.join("complemento/.claude-plugin/plugin.json").is_file());

    // Relanzar el mismo plan NO borra la sesión viva: crea otro directorio.
    let otra = materializar(&plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap(), &m.r).unwrap();
    assert_ne!(otra.directorio, plan.directorio);
    assert!(plan.directorio.join("mcp.json").is_file() && otra.directorio.join("mcp.json").is_file());
}

#[test]
fn materializar_falla_sin_escribir_nada_si_la_fuente_cambio() {
    let m = mundo();
    let sel = ids(&m, &["claude-code/mcp/usuario/stdio-claude"]);
    let plan = plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap();
    escribe(m.home.join(".claude.json"), r#"{"mcpServers":{}}"#);
    let e = materializar(&plan, &m.r).unwrap_err();
    assert!(e.to_string().contains("ya no está"), "{e}");
    assert!(!plan.directorio.exists());
}

#[test]
fn plan_de_codex_apaga_los_demas_mcp_propios_y_define_el_del_ecosistema() {
    let m = mundo();
    let sel = ids(&m, &["codex/mcp/usuario/docs", "ecosistema/mcp/ecosistema/ea-ecosistema"]);
    let plan = plan_sesion("codex", &sel, &m.inv, &m.r).unwrap();
    assert_eq!(
        plan.argv,
        [
            "codex",
            "-c", "mcp_servers.apagado.enabled=false",
            "-c", "mcp_servers.raro.enabled=false",
            "-c", "mcp_servers.docs.enabled=true",
            "-c", "mcp_servers.ea-ecosistema.url=\"http://127.0.0.1:8765/mcp\"",
            "-c", "mcp_servers.ea-ecosistema.enabled=true",
        ]
    );
    assert!(plan.ficheros.is_empty() && plan.entorno.is_empty());
    sin_secretos(&plan);
    // Codex lee sus secretos de su propia config: no hay nada que materializar.
    materializar(&plan, &m.r).unwrap();
}

#[test]
fn codex_rechaza_un_mcp_ajeno_que_llevaria_secretos_en_argv() {
    let m = mundo();
    for id in ["claude-code/mcp/usuario/stdio-claude", "claude-code/mcp/usuario/remoto-claude", "opencode/mcp/usuario/remoto-oc"] {
        let e = plan_sesion("codex", &ids(&m, &[id]), &m.inv, &m.r).unwrap_err();
        assert!(e.to_string().contains("secretos"), "{id}: {e}");
    }
    // Uno de stdio sin secretos sí pasa.
    let plan = plan_sesion("codex", &ids(&m, &["claude-code/mcp/proyecto/del-proyecto"]), &m.inv, &m.r).unwrap();
    assert!(plan.argv.contains(&"mcp_servers.del-proyecto.command=\"servidor\"".to_string()), "{:?}", plan.argv);
    assert!(plan.argv.contains(&"mcp_servers.del-proyecto.args=[\"--ok\"]".to_string()));
}

#[test]
fn plan_de_opencode_usa_standalone_y_un_fichero_con_los_demas_apagados() {
    let m = mundo();
    let sel = ids(&m, &["opencode/mcp/usuario/remoto-oc", "claude-code/mcp/usuario/stdio-claude"]);
    let plan = plan_sesion("opencode", &sel, &m.inv, &m.r).unwrap();
    let fichero = plan.directorio.join("opencode.json");
    assert_eq!(plan.argv, ["opencode", "--standalone"]);
    assert_eq!(plan.entorno, [("OPENCODE_CONFIG".to_string(), fichero.display().to_string())]);
    sin_secretos(&plan);
    let en_plan: serde_json::Value = serde_json::from_str(&plan.ficheros[0].contenido).unwrap();
    assert_eq!(en_plan["mcp"]["remoto-oc"]["enabled"], true);
    assert_eq!(en_plan["mcp"]["stdio-claude"]["enabled"], true);
    assert_eq!(en_plan["mcp"]["local-oc"]["enabled"], false);
    assert_eq!(en_plan["mcp"]["plano"]["enabled"], false);

    let plan = materializar(&plan, &m.r).unwrap();
    let fichero = plan.directorio.join("opencode.json");
    assert_eq!(plan.entorno, [("OPENCODE_CONFIG".to_string(), fichero.display().to_string())]);
    assert_modo(&fichero, 0o600);
    let real: serde_json::Value = serde_json::from_str(&texto(&fichero)).unwrap();
    assert_eq!(real["mcp"]["remoto-oc"]["headers"]["Authorization"], "SECRETO_HEADER_OPENCODE");
    assert_eq!(real["mcp"]["remoto-oc"]["url"], "https://oc.ejemplo.com/SECRETO_RUTA_URL");
    assert_eq!(real["mcp"]["stdio-claude"]["environment"]["API_KEY"], "SECRETO_ENV_CLAUDE");
    // Apagado pero con su definición completa (si no, OpenCode no lo apaga).
    assert_eq!(real["mcp"]["local-oc"]["enabled"], false);
    assert_eq!(real["mcp"]["local-oc"]["command"], serde_json::json!(["bun", "run", "x"]));
    assert_eq!(real["mcp"]["local-oc"]["environment"]["K"], "SECRETO_ENV_OPENCODE");
    assert_eq!(real["mcp"]["plano"]["command"], serde_json::json!(["x"]));
}

#[test]
fn plan_de_qwen_nombra_los_propios_y_escribe_los_ajenos() {
    let m = mundo();
    let sel = ids(&m, &["qwen-code/mcp/usuario/qmcp", "claude-code/mcp/usuario/stdio-claude", "qwen-code/extension/usuario/ext1"]);
    let plan = plan_sesion("qwen-code", &sel, &m.inv, &m.r).unwrap();
    let d = plan.directorio.join("mcp.json").display().to_string();
    assert_eq!(plan.argv, ["qwen", "--mcp-config", &d, "--allowed-mcp-server-names", "stdio-claude", "qmcp", "-e", "ext1"]);
    sin_secretos(&plan);
    let plan = materializar(&plan, &m.r).unwrap();
    assert!(plan.argv.contains(&plan.directorio.join("mcp.json").display().to_string()));
    let mcp: serde_json::Value = serde_json::from_str(&texto(&plan.directorio.join("mcp.json"))).unwrap();
    assert_eq!(mcp["mcpServers"]["stdio-claude"]["env"]["API_KEY"], "SECRETO_ENV_CLAUDE");
    assert!(mcp["mcpServers"].get("qmcp").is_none(), "los propios no se reescriben");

    // Solo una extensión: ningún MCP y las extensiones elegidas.
    let solo = plan_sesion("qwen-code", &ids(&m, &["qwen-code/extension/usuario/ext1"]), &m.inv, &m.r).unwrap();
    assert_eq!(solo.argv, ["qwen", "--allowed-mcp-server-names", "ea-ninguno", "-e", "ext1"]);
    assert!(solo.ficheros.is_empty());
}

#[test]
fn una_fuente_de_un_agente_sin_lector_no_se_lee() {
    let m = mundo();
    let mut plan = plan_sesion("claude-code", &ids(&m, &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &m.r).unwrap();
    plan.mcp[0].agente = "gemini".into();
    let e = materializar(&plan, &m.r).unwrap_err();
    assert_eq!(e, ErrorBiblioteca::Sesion("claude-code/mcp/usuario/stdio-claude: agente de origen sin lector (gemini)".into()));
}

#[test]
fn rechaza_lo_que_no_tiene_sentido() {
    let m = mundo();
    let un = |a: &str, s: &[&str]| plan_sesion(a, &ids(m_ref(&m), s), &m.inv, &m.r).unwrap_err().to_string();
    fn m_ref(m: &Mundo) -> &Mundo {
        m
    }
    // Antigravity no admite sesiones.
    assert!(un("antigravity", &["claude-code/mcp/usuario/stdio-claude"]).contains("no admite sesiones en la v1"));
    assert!(un("gemini", &["claude-code/mcp/usuario/stdio-claude"]).contains("no es un agente con sesiones"));
    // El texto exacto (también lo ven la CLI, el panel y el MCP).
    let err = |a: &str| plan_sesion(a, &ids(m_ref(&m), &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &m.r).unwrap_err();
    assert_eq!(err("antigravity"), ErrorBiblioteca::Sesion("Antigravity no admite sesiones en la v1".into()));
    let no_es = |a: &str| ErrorBiblioteca::Sesion(format!("«{a}» no es un agente con sesiones (claude-code, qwen-code, codex, opencode)"));
    for a in ["gemini", "Codex", " codex", "ecosistema", ""] {
        assert_eq!(err(a), no_es(a), "{a:?}");
    }
    assert_eq!(AGENTES_CON_SESION, ["claude-code", "qwen-code", "codex", "opencode"]);
    // Para un agente que no es del catálogo, solo los MCP tienen sentido.
    let elemento = |id: &str| m.inv.buscar(id).unwrap();
    assert_eq!(sesion::admite("gemini", elemento("claude-code/mcp/usuario/stdio-claude")), Ok(()));
    assert_eq!(
        sesion::admite("gemini", elemento("ecosistema/skill/ecosistema/mision")),
        Err(ErrorBiblioteca::Sesion("ecosistema/skill/ecosistema/mision: una sesión de gemini no admite elementos de tipo skill".into()))
    );
    // Un plugin o una skill de Claude no sirve en Codex, OpenCode ni Qwen.
    assert!(un("codex", &["claude-code/plugin/usuario/foo@mkt"]).contains("no tiene sentido"));
    assert!(un("opencode", &["claude-code/skill/usuario/propia"]).contains("no tiene sentido"));
    // Del propio agente pero de un tipo que esa sesión no admite.
    assert!(un("codex", &["codex/mcp/usuario/docs", "ecosistema/skill/ecosistema/mision"]).contains("no admite elementos de tipo skill"));
    assert!(un("qwen-code", &["ecosistema/skill/ecosistema/depurar"]).contains("no admite elementos de tipo skill"));
    // Lo que trae un plugin se elige con el plugin.
    assert!(un("claude-code", &["claude-code/skill/plugin:foo@mkt/dentro"]).contains("elige el plugin"));
    // Una skill de Qwen del ecosistema (un .md) no es una skill de Claude.
    assert!(un("claude-code", &["ecosistema/skill/ecosistema/depurar"]).contains("formato"));
    // Fuera del inventario.
    let e = plan_sesion("claude-code", &["claude-code/mcp/usuario/fantasma".parse().unwrap()], &m.inv, &m.r).unwrap_err();
    assert_eq!(e, ErrorBiblioteca::NoExiste("claude-code/mcp/usuario/fantasma".into()));
    // Sin ids.
    assert!(plan_sesion("claude-code", &[], &m.inv, &m.r).is_err());
    // Dos MCP con el mismo nombre.
    escribe(m.proyecto.join(".mcp.json"), r#"{"mcpServers":{"stdio-claude":{"command":"c"}}}"#);
    let inv = Biblioteca::cargar(&m.r, &Ajustes::por_defecto(&Fuentes::aislada(&m.home)));
    let e = plan_sesion("claude-code", &["claude-code/mcp/usuario/stdio-claude".parse().unwrap(), "claude-code/mcp/proyecto/stdio-claude".parse().unwrap()], &inv, &m.r).unwrap_err();
    assert!(e.to_string().contains("se llaman"), "{e}");
}

#[test]
fn sin_runtime_no_hay_sesion() {
    let m = mundo();
    let mut r = m.r.clone();
    r.runtime = None;
    let e = plan_sesion("claude-code", &ids(&m, &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &r).unwrap_err();
    assert!(e.to_string().contains("XDG_RUNTIME_DIR"), "{e}");
}

#[test]
fn materializar_no_sale_del_runtime() {
    let m = mundo();
    let sel = ids(&m, &["claude-code/mcp/usuario/stdio-claude"]);
    let mut plan = plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap();
    plan.directorio = m.home.clone();
    assert!(materializar(&plan, &m.r).is_err());
    assert!(m.home.join(".claude.json").exists(), "no se borró nada");
    let mut plan = plan_sesion("claude-code", &sel, &m.inv, &m.r).unwrap();
    plan.ficheros[0].ruta = m.home.join("fuera.json");
    assert!(materializar(&plan, &m.r).is_err());
    assert!(!m.home.join("fuera.json").exists());
}

/// Una sesión creada hace `horas` (la marca `.creada` manda sobre el mtime).
fn sesion_vieja(base: &Path, nombre: &str, horas: u64) {
    let ahora = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    escribe(base.join(nombre).join("mcp.json"), "{}");
    escribe(base.join(nombre).join(".creada"), &format!("{}\n", ahora - horas * 3600));
}

/// Abre un directorio para cambiarle la fecha (en Windows hace falta pedirlo).
fn abrir_dir(ruta: &Path) -> fs::File {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new().write(true).custom_flags(0x0200_0000).open(ruta).unwrap() // FILE_FLAG_BACKUP_SEMANTICS
    }
    #[cfg(not(windows))]
    {
        fs::File::open(ruta).unwrap()
    }
}

#[test]
fn las_sesiones_se_limpian_por_antiguedad_de_creacion_con_margen_de_7_dias() {
    let m = mundo();
    let base = m.runtime.join("ea-biblioteca/sesiones");
    sesion_vieja(&base, "vieja", 24 * 8);
    sesion_vieja(&base, "de-ayer", 30);
    sesion_vieja(&base, "reciente", 2);
    // Un directorio con el mtime de hace años pero creado ahora no es vieja.
    escribe(base.join("mtime-antiguo/x"), "x");
    let f = abrir_dir(&base.join("mtime-antiguo"));
    f.set_modified(SystemTime::now() - Duration::from_secs(40 * 24 * 3600)).unwrap();
    escribe(base.join("mtime-antiguo/.creada"), &format!("{}\n", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()));
    f.set_modified(SystemTime::now() - Duration::from_secs(40 * 24 * 3600)).unwrap();
    assert_eq!(limpiar_sesiones(&m.runtime, Duration::from_secs(7 * 24 * 3600), None), 1);
    assert!(!base.join("vieja").exists());
    assert!(base.join("de-ayer").exists() && base.join("reciente").exists() && base.join("mtime-antiguo").exists());

    // Materializar también limpia, y no toca la sesión que escribe.
    sesion_vieja(&base, "otra-vieja", 24 * 9);
    let plan = plan_sesion("claude-code", &ids(&m, &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &m.r).unwrap();
    let real = materializar(&plan, &m.r).unwrap();
    assert!(!base.join("otra-vieja").exists());
    assert!(base.join("reciente").exists() && real.directorio.exists());
}

#[test]
fn nombres_que_empiezan_por_guion_se_rechazan() {
    let m = mundo();
    escribe(m.home.join(".qwen/extensions/-raro/qwen-extension.json"), r#"{"name":"-raro"}"#);
    let inv = Biblioteca::cargar(&m.r, &Ajustes::por_defecto(&Fuentes::aislada(&m.home)));
    assert!(inv.buscar("qwen-code/extension/usuario/-raro").is_some(), "el inventario debe verla");
    let e = plan_sesion("qwen-code", &["qwen-code/extension/usuario/-raro".parse().unwrap()], &inv, &m.r).unwrap_err();
    assert!(e.to_string().contains("«-»"), "{e}");
}

#[test]
fn codex_rechaza_un_mcp_ajeno_si_lo_redactado_no_es_lo_original() {
    let m = mundo();
    // Una URL con ruta y argumentos con espacios: redactar los recorta o normaliza.
    escribe(
        m.proyecto.join(".mcp.json"),
        r#"{"mcpServers":{"remoto-ruta":{"type":"http","url":"https://host/mcp"},"remoto":{"command":"npx","args":["mcp-remote","https://host/mcp"]},"espacios":{"command":"x","args":["a  b"]}}}"#,
    );
    let inv = Biblioteca::cargar(&m.r, &Ajustes::por_defecto(&Fuentes::aislada(&m.home)));
    for id in ["claude-code/mcp/proyecto/remoto-ruta", "claude-code/mcp/proyecto/remoto", "claude-code/mcp/proyecto/espacios"] {
        assert!(inv.buscar(id).is_some(), "{id}");
        let e = plan_sesion("codex", &[id.parse::<Id>().unwrap()], &inv, &m.r);
        assert!(e.is_err(), "{id} debe rechazarse: {e:?}");
    }
}

#[test]
fn materializar_no_lee_fuentes_que_por_un_enlace_salen_del_home() {
    let m = mundo();
    let plan0 = plan_sesion("claude-code", &ids(&m, &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &m.r).unwrap();
    let fuera = m._d.path().join("fuera.json");
    fs::write(&fuera, r#"{"mcpServers":{"stdio-claude":{"command":"malo"}}}"#).unwrap();
    let enlace = m.home.join("enlace.json");
    ea_plataforma::fs::enlazar(&fuera, &enlace).unwrap();
    let mut plan = plan0.clone();
    plan.mcp[0].ruta = enlace;
    let e = materializar(&plan, &m.r).unwrap_err();
    assert!(e.to_string().contains("fuera del HOME"), "{e}");
}

#[test]
fn codex_resuelve_env_http_headers_y_bearer_desde_el_entorno_solo_al_materializar() {
    let m = mundo();
    escribe(
        m.home.join(".codex/config.toml"),
        r#"[mcp_servers.web]
url = "https://web.ejemplo.com/mcp"
bearer_token_env_var = "EA_PRUEBA_TOKEN"
[mcp_servers.web.env_http_headers]
"X-Api" = "EA_PRUEBA_CLAVE"
"#,
    );
    let inv = Biblioteca::cargar(&m.r, &Ajustes::por_defecto(&Fuentes::aislada(&m.home)));
    let plan = plan_sesion("claude-code", &["codex/mcp/usuario/web".parse().unwrap()], &inv, &m.r).unwrap();
    sin_secretos(&plan);
    assert!(!serde_json::to_string(&plan).unwrap().contains("VALOR_"));
    let entorno = |k: &str| match k {
        "EA_PRUEBA_TOKEN" => Some("VALOR_TOKEN".to_string()),
        "EA_PRUEBA_CLAVE" => Some("VALOR_CLAVE".to_string()),
        _ => None,
    };
    let real = materializar_con_entorno(&plan, &m.r, &entorno).unwrap();
    let mcp: serde_json::Value = serde_json::from_str(&texto(&real.directorio.join("mcp.json"))).unwrap();
    assert_eq!(mcp["mcpServers"]["web"]["headers"]["X-Api"], "VALOR_CLAVE");
    assert_eq!(mcp["mcpServers"]["web"]["headers"]["Authorization"], "Bearer VALOR_TOKEN");
    // Falta una variable: se rechaza con un mensaje claro y no se crea nada nuevo.
    let antes = fs::read_dir(m.runtime.join("ea-biblioteca/sesiones")).unwrap().count();
    let e = materializar_con_entorno(&plan, &m.r, &|k| (k == "EA_PRUEBA_TOKEN").then(|| "x".to_string())).unwrap_err();
    assert!(e.to_string().contains("EA_PRUEBA_CLAVE") && e.to_string().contains("no está definida"), "{e}");
    assert!(!e.to_string().contains("VALOR_"));
    assert_eq!(fs::read_dir(m.runtime.join("ea-biblioteca/sesiones")).unwrap().count(), antes);
}
