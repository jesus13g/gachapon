//! Sesiones con un subconjunto de herramientas (F5): plan puro, secretos
//! solo en ficheros 0600 y rechazos. Todo sobre un HOME y un runtime de
//! `tempfile`; no se ejecuta ningún agente.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ea_ajustes::{Ajustes, Fuentes};
use ea_biblioteca::*;

struct Mundo {
    _d: tempfile::TempDir,
    home: PathBuf,
    proyecto: PathBuf,
    runtime: PathBuf,
    inv: Inventario,
    r: Raices,
}

fn escribe(ruta: impl AsRef<Path>, texto: &str) {
    let ruta = ruta.as_ref();
    fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    fs::write(ruta, texto).unwrap();
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
            plugin_dir.display()
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
            proyecto.display()
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
    for s in SECRETOS {
        assert!(!todo.contains(s), "el plan lleva {s}: {todo}");
    }
}

fn modo(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
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
    let dir = m.runtime.join("ea-biblioteca/sesiones").join(&plan.id);
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
    let enlaces: Vec<String> = plan.enlaces.iter().map(|e| e.ruta.strip_prefix(&dir).unwrap().display().to_string()).collect();
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
    materializar(&plan, &m.r).unwrap();

    assert_eq!(modo(&plan.directorio), 0o700);
    assert_eq!(modo(&plan.directorio.parent().unwrap().to_path_buf()), 0o700);
    for f in &plan.ficheros {
        assert_eq!(modo(&f.ruta), 0o600, "{}", f.ruta.display());
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

    // Materializar otra vez reemplaza la sesión sin quejarse.
    materializar(&plan, &m.r).unwrap();
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

    materializar(&plan, &m.r).unwrap();
    assert_eq!(modo(&fichero), 0o600);
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
    materializar(&plan, &m.r).unwrap();
    let mcp: serde_json::Value = serde_json::from_str(&texto(&plan.directorio.join("mcp.json"))).unwrap();
    assert_eq!(mcp["mcpServers"]["stdio-claude"]["env"]["API_KEY"], "SECRETO_ENV_CLAUDE");
    assert!(mcp["mcpServers"].get("qmcp").is_none(), "los propios no se reescriben");

    // Solo una extensión: ningún MCP y las extensiones elegidas.
    let solo = plan_sesion("qwen-code", &ids(&m, &["qwen-code/extension/usuario/ext1"]), &m.inv, &m.r).unwrap();
    assert_eq!(solo.argv, ["qwen", "--allowed-mcp-server-names", "ea-ninguno", "-e", "ext1"]);
    assert!(solo.ficheros.is_empty());
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

fn envejecer(ruta: &Path, horas: u64) {
    let f = fs::File::open(ruta).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(horas * 3600)).unwrap();
}

#[test]
fn las_sesiones_de_mas_de_24_horas_se_limpian() {
    let m = mundo();
    let base = m.runtime.join("ea-biblioteca/sesiones");
    for n in ["vieja", "reciente"] {
        escribe(base.join(n).join("mcp.json"), "{}");
    }
    envejecer(&base.join("vieja"), 48);
    envejecer(&base.join("reciente"), 2);
    assert_eq!(limpiar_sesiones(&m.runtime, Duration::from_secs(24 * 3600), None), 1);
    assert!(!base.join("vieja").exists() && base.join("reciente").exists());

    // Materializar también limpia, y no toca la sesión que escribe.
    escribe(base.join("otra-vieja").join("x"), "x");
    envejecer(&base.join("otra-vieja"), 30);
    let plan = plan_sesion("claude-code", &ids(&m, &["claude-code/mcp/usuario/stdio-claude"]), &m.inv, &m.r).unwrap();
    materializar(&plan, &m.r).unwrap();
    assert!(!base.join("otra-vieja").exists());
    assert!(base.join("reciente").exists() && plan.directorio.exists());
}
