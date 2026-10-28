//! Pruebas del inventario sobre un HOME de `tempfile`: jamás se lee el real.

use std::fs;
use std::path::PathBuf;

use ea_ajustes::{Ajustes, Fuentes};
use ea_biblioteca::*;
use ea_pruebas::escribe;

struct Mundo {
    _d: tempfile::TempDir,
    home: PathBuf,
    proyecto: PathBuf,
    repo: PathBuf,
    ajustes: Ajustes,
}

const SECRETOS: &[&str] = &[
    "SECRETO_ENV_CLAUDE",
    "SECRETO_HEADER_CLAUDE",
    "SECRETO_RUTA_URL",
    "SECRETO_CONSULTA_URL",
    "SECRETO_USUARIO_URL",
    "SECRETO_ARG_TOKEN",
    "SECRETO_ENV_CODEX",
    "SECRETO_HEADER_CODEX",
    "SECRETO_ENV_OPENCODE",
    "SECRETO_HEADER_OPENCODE",
    "SECRETO_QWEN",
    "SECRETO_AGY",
    "SECRETO_BEARER",
];

/// Un HOME con al menos un elemento de cada tipo y agente, con secretos.
fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let proyecto = d.path().join("proyecto");
    let repo = d.path().join("repo");
    fs::create_dir_all(&proyecto).unwrap();

    // repo del ecosistema
    escribe(repo.join("Cargo.toml"), "[workspace]\n");
    escribe(repo.join("claude/skills/mision/SKILL.md"), "---\nname: mision\ndescription: \"Lanza una misión\"\n---\ncuerpo");
    escribe(repo.join("qwen/skills/depurar.md"), "---\ndescripcion: Arreglar un fallo\n---\n1. ...");

    // Claude Code
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    ea_plataforma::fs::enlazar(&repo.join("claude/skills/mision"), &home.join(".claude/skills/mision")).unwrap();
    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\nname: propia\ndescription: >\n  Hace cosas\n  en dos lineas\n---\n");
    escribe(home.join(".claude/skills/sin-skill-md/otro.txt"), "no es una skill");
    escribe(home.join(".claude/agents/revisor.md"), "---\nname: revisor\ndescription: Revisa\n---\n");
    escribe(home.join(".claude/commands/saludo.md"), "---\ndescription: Saluda\n---\n");
    escribe(proyecto.join(".claude/skills/local/SKILL.md"), "---\ndescription: De proyecto\n---\n");
    let plugin_dir = home.join(".claude/plugins/cache/mkt/foo/1.0");
    escribe(plugin_dir.join("skills/dentro/SKILL.md"), "---\ndescription: Viene en el plugin\n---\n");
    escribe(plugin_dir.join("agents/ag.md"), "---\ndescription: Subagente del plugin\n---\n");
    escribe(
        home.join(".claude/plugins/installed_plugins.json"),
        &format!(
            r#"{{"version":2,"plugins":{{
              "foo@mkt":[{{"scope":"user","installPath":"{}","version":"1.0"}}],
              "apagado@mkt":[{{"scope":"user","installPath":"/no/existe","version":"2"}}],
              "ajeno@mkt":[{{"scope":"project","projectPath":"/otro/proyecto","installPath":"/x","version":"1"}}]
            }}}}"#,
            plugin_dir.display().to_string().replace('\\', "/")
        ),
    );
    escribe(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"foo@mkt":true,"apagado@mkt":false}}"#);
    escribe(
        home.join(".claude.json"),
        &format!(
            r#"{{"numStartups":3,
              "mcpServers":{{
                "stdio-claude":{{"type":"stdio","command":"npx","args":["-y","@scope/server","--token","SECRETO_ARG_TOKEN"],"env":{{"API_KEY":"SECRETO_ENV_CLAUDE"}}}},
                "remoto-claude":{{"type":"http","url":"https://user:SECRETO_USUARIO_URL@mcp.ejemplo.com:8443/SECRETO_RUTA_URL/mcp?k=SECRETO_CONSULTA_URL","headers":{{"Authorization":"Bearer SECRETO_HEADER_CLAUDE"}}}},
                "ea-ecosistema":{{"type":"http","url":"http://127.0.0.1:8765/mcp"}}
              }},
              "projects":{{"{}":{{"mcpServers":{{"del-proyecto":{{"command":"servidor"}}}},"disabledMcpServers":["del-proyecto"],"disabledMcpjsonServers":["off"]}}}}
            }}"#,
            proyecto.display().to_string().replace('\\', "/")
        ),
    );
    escribe(
        proyecto.join(".mcp.json"),
        r#"{"mcpServers":{"off":{"command":"a"},"pendiente":{"command":"b"},"aprobado":{"command":"c"}}}"#,
    );
    escribe(proyecto.join(".claude/settings.local.json"), r#"{"enabledMcpjsonServers":["aprobado"]}"#);

    // Codex
    escribe(home.join(".codex/skills/cx/SKILL.md"), "---\ndescription: Skill de Codex\n---\n");
    escribe(home.join(".codex/skills/.system/SKILL.md"), "---\ndescription: oculta\n---\n");
    escribe(proyecto.join(".agents/skills/cxp/SKILL.md"), "---\ndescription: De proyecto\n---\n");
    escribe(
        home.join(".codex/config.toml"),
        r#"model = "x"
[mcp_servers.docs]
command = "docs-mcp"
args = ["--api-key=SECRETO_ENV_CODEX"]
[mcp_servers.docs.env]
TOKEN = "SECRETO_ENV_CODEX"
[mcp_servers.apagado]
url = "https://h.ejemplo.com/SECRETO_RUTA_URL"
enabled = false
[mcp_servers.apagado.http_headers]
X-Auth = "SECRETO_HEADER_CODEX"
"#,
    );

    // OpenCode
    escribe(
        home.join(".config/opencode/opencode.json"),
        r#"{"plugin":["opencode-foo@1.2","@scope/pkg"],
            "agent":{"build2":{"description":"Uno","disable":true}},
            "mcp":{"servers":{
               "remoto-oc":{"type":"remote","url":"https://oc.ejemplo.com/SECRETO_RUTA_URL","headers":{"Authorization":"SECRETO_HEADER_OPENCODE"}},
               "local-oc":{"type":"local","command":["bun","run","x"],"environment":{"K":"SECRETO_ENV_OPENCODE"},"enabled":false}}}}"#,
    );
    escribe(
        home.join(".config/opencode/opencode.jsonc"),
        "{\n // un comentario\n \"mcp\": { \"plano\": { \"type\": \"local\", \"command\": [\"x\"], }, },\n}",
    );
    escribe(home.join(".config/opencode/agent/oa.md"), "---\ndescription: Agente de fichero\n---\n");
    escribe(home.join(".config/opencode/commands/ea-mision.md"), "---\ndescription: Comando\n---\n");
    escribe(home.join(".config/opencode/skills/os/SKILL.md"), "---\ndescription: Skill\n---\n");

    // Qwen Code
    escribe(
        home.join(".qwen/settings.json"),
        r#"{"mcpServers":{"qmcp":{"httpUrl":"https://q.ejemplo.com/SECRETO_RUTA_URL","headers":{"X":"SECRETO_QWEN"}},"fuera":{"command":"z"}},"mcp":{"excluded":["fuera"]}}"#,
    );
    escribe(home.join(".qwen/extensions/ext1/qwen-extension.json"), r#"{"name":"ext1","description":"Una extension"}"#);
    escribe(home.join(".qwen/extensions/ext2/qwen-extension.json"), r#"{"name":"ext2"}"#);
    escribe(
        home.join(".qwen/extensions/extension-enablement.json"),
        &format!(r#"{{"ext2":{{"overrides":["{0}/*","!{1}/*"]}}}}"#, home.display().to_string().replace('\\', "/"), proyecto.display().to_string().replace('\\', "/")),
    );

    // Antigravity
    escribe(home.join(".gemini/config/skills/ag/SKILL.md"), "---\ndescription: Skill de agy\n---\n");
    escribe(home.join(".gemini/antigravity-cli/builtin/skills/guia/SKILL.md"), "---\ndescription: Integrada\n---\n");
    escribe(
        home.join(".gemini/config/mcp_config.json"),
        r#"{"mcpServers":{"agy-mcp":{"serverUrl":"https://agy.ejemplo.com/SECRETO_RUTA_URL?t=SECRETO_AGY","disabled":true},"ea-ecosistema":{"serverUrl":"http://127.0.0.1:8765/mcp"}}}"#,
    );

    // Ajustes con las skills del ejecutor en el HOME de prueba
    let f = Fuentes::aislada(d.path());
    let mut ajustes = Ajustes::por_defecto(&f);
    let skills = d.path().join("qwen-skills");
    escribe(skills.join("rust.md"), "---\ndescripcion: Skill del ejecutor\n---\n");
    escribe(skills.join("tests.md"), "---\ndescripcion: Otra\n---\n");
    ajustes.agente.skills.dir = skills.to_string_lossy().into_owned();
    ajustes.agente.skills.habilitadas = vec!["rust".into()];

    Mundo { home, proyecto, repo, ajustes, _d: d }
}

fn raices(m: &Mundo) -> Raices {
    Raices::nuevo(m.home.clone(), Some(m.proyecto.clone()), m.home.join(".local/state/ea"), None)
}

fn get<'a>(inv: &'a Inventario, id: &str) -> &'a Elemento {
    inv.buscar(id).unwrap_or_else(|| {
        let ids: Vec<_> = inv.elementos.iter().map(|e| e.id.as_str()).collect();
        panic!("falta {id}; hay {ids:#?}")
    })
}

#[test]
fn hay_un_elemento_de_cada_tipo_y_agente() {
    let m = mundo();
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    for agente in ["claude-code", "qwen-code", "codex", "opencode", "antigravity", "ecosistema"] {
        assert!(inv.elementos.iter().any(|e| e.agente == agente), "sin elementos de {agente}");
    }
    for tipo in Tipo::TODOS {
        assert!(inv.elementos.iter().any(|e| e.tipo == tipo), "sin elementos de tipo {tipo}");
    }
    // sin avisos inesperados (el JSONC con comentarios no es un aviso)
    assert!(inv.avisos.is_empty(), "{:?}", inv.avisos);
    // ids únicos y todos analizables, ida y vuelta
    let mut vistos = std::collections::HashSet::new();
    for e in &inv.elementos {
        assert!(vistos.insert(&e.id), "repetido {}", e.id);
        let id = Id::analizar(&e.id).unwrap();
        assert_eq!(id.to_string(), e.id);
        assert_eq!((id.agente.as_str(), id.tipo, &id.ambito, id.nombre.as_str()), (e.agente.as_str(), e.tipo, &e.ambito, e.nombre.as_str()));
    }
}

#[test]
fn claude_code() {
    let m = mundo();
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    let e = get(&inv, "claude-code/skill/ecosistema/mision");
    assert_eq!((e.mecanismo, e.activado, e.categoria.as_str()), (Mecanismo::Mover, Activado::Si, "ecosistema"));
    assert_eq!(e.descripcion, "Lanza una misión");
    assert_eq!(e.version_formato, "2.1.301");
    let e = get(&inv, "claude-code/skill/usuario/propia");
    assert_eq!(e.descripcion, "Hace cosas en dos lineas");
    assert!(inv.buscar("claude-code/skill/usuario/sin-skill-md").is_none());
    get(&inv, "claude-code/skill/proyecto/local");
    get(&inv, "claude-code/subagente/usuario/revisor");
    get(&inv, "claude-code/comando/usuario/saludo");

    let p = get(&inv, "claude-code/plugin/usuario/foo@mkt");
    assert_eq!((p.activado, p.mecanismo, p.categoria.as_str()), (Activado::Si, Mecanismo::ComandoOficial, "usuario"));
    assert_eq!(get(&inv, "claude-code/plugin/usuario/apagado@mkt").activado, Activado::No);
    assert!(inv.buscar("claude-code/plugin/proyecto/ajeno@mkt").is_none(), "plugin de otro proyecto");
    let d = get(&inv, "claude-code/skill/plugin:foo@mkt/dentro");
    assert_eq!((d.activado, d.mecanismo, d.categoria.as_str()), (Activado::Si, Mecanismo::No, "plugin:foo@mkt"));
    get(&inv, "claude-code/subagente/plugin:foo@mkt/ag");

    let s = get(&inv, "claude-code/mcp/usuario/stdio-claude");
    assert_eq!(s.mecanismo, Mecanismo::SoloSesion);
    let def = s.def_mcp.as_ref().unwrap();
    assert_eq!((def.transporte.as_str(), def.comando.as_deref()), ("stdio", Some("npx")));
    assert_eq!(def.env, ["API_KEY"]);
    assert_eq!(def.args, ["-y", "@scope/server", "--token", "***"]);
    let r = get(&inv, "claude-code/mcp/usuario/remoto-claude").def_mcp.as_ref().unwrap();
    assert_eq!((r.transporte.as_str(), r.url.as_deref(), r.headers.as_slice()), ("http", Some("https://mcp.ejemplo.com:8443"), ["Authorization".to_string()].as_slice()));
    assert_eq!(get(&inv, "claude-code/mcp/proyecto/del-proyecto").activado, Activado::No);
    assert_eq!(get(&inv, "claude-code/mcp/proyecto/off").activado, Activado::No);
    assert_eq!(get(&inv, "claude-code/mcp/proyecto/pendiente").activado, Activado::Desconocido);
    assert_eq!(get(&inv, "claude-code/mcp/proyecto/aprobado").activado, Activado::Si);
    // el servidor local del ecosistema cuenta como del ecosistema
    assert_eq!(get(&inv, "claude-code/mcp/usuario/ea-ecosistema").categoria, "ecosistema");
    assert_eq!(get(&inv, "antigravity/mcp/usuario/ea-ecosistema").categoria, "ecosistema");
}

#[test]
fn codex_opencode_qwen_antigravity_y_ecosistema() {
    let m = mundo();
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);

    get(&inv, "codex/skill/usuario/cx");
    get(&inv, "codex/skill/proyecto/cxp");
    assert!(inv.buscar("codex/skill/usuario/.system").is_none());
    let docs = get(&inv, "codex/mcp/usuario/docs");
    assert_eq!((docs.activado, docs.mecanismo), (Activado::Si, Mecanismo::Campo));
    assert_eq!(get(&inv, "codex/mcp/usuario/apagado").activado, Activado::No);

    let r = get(&inv, "opencode/mcp/usuario/remoto-oc");
    assert_eq!((r.activado, r.mecanismo), (Activado::Si, Mecanismo::Campo));
    assert_eq!(r.def_mcp.as_ref().unwrap().url.as_deref(), Some("https://oc.ejemplo.com"));
    let l = get(&inv, "opencode/mcp/usuario/local-oc");
    assert_eq!(l.activado, Activado::No);
    assert_eq!(l.def_mcp.as_ref().unwrap().comando.as_deref(), Some("bun"));
    // JSONC con comentarios: se lee, pero no se podrá editar
    let j = get(&inv, "opencode/mcp/usuario/plano");
    assert_eq!(j.mecanismo, Mecanismo::No);
    assert!(j.nota.as_deref().unwrap().contains("JSONC"));
    get(&inv, "opencode/plugin/usuario/opencode-foo@1.2");
    get(&inv, "opencode/plugin/usuario/@scope/pkg");
    assert_eq!(get(&inv, "opencode/subagente/usuario/build2").activado, Activado::No);
    assert_eq!(get(&inv, "opencode/subagente/usuario/oa").mecanismo, Mecanismo::Mover);
    get(&inv, "opencode/comando/usuario/ea-mision");
    get(&inv, "opencode/skill/usuario/os");

    assert_eq!(get(&inv, "qwen-code/mcp/usuario/qmcp").def_mcp.as_ref().unwrap().url.as_deref(), Some("https://q.ejemplo.com"));
    assert_eq!(get(&inv, "qwen-code/mcp/usuario/fuera").activado, Activado::No);
    let e1 = get(&inv, "qwen-code/extension/usuario/ext1");
    assert_eq!((e1.activado, e1.mecanismo, e1.descripcion.as_str()), (Activado::Si, Mecanismo::ComandoOficial, "Una extension"));
    assert_eq!(get(&inv, "qwen-code/extension/usuario/ext2").activado, Activado::No);
    let rust = get(&inv, "qwen-code/skill/ecosistema/rust");
    assert_eq!((rust.activado, rust.mecanismo), (Activado::Si, Mecanismo::Ajustes));
    assert_eq!(get(&inv, "qwen-code/skill/ecosistema/tests").activado, Activado::No);

    get(&inv, "antigravity/skill/usuario/ag");
    let b = get(&inv, "antigravity/skill/integrada/guia");
    assert_eq!((b.mecanismo, b.categoria.as_str()), (Mecanismo::No, "integrada"));
    let a = get(&inv, "antigravity/mcp/usuario/agy-mcp");
    assert_eq!((a.activado, a.mecanismo), (Activado::No, Mecanismo::ComandoOficial));

    let mcp = get(&inv, "ecosistema/mcp/ecosistema/ea-ecosistema");
    assert_eq!(mcp.def_mcp.as_ref().unwrap().url.as_deref(), Some("http://127.0.0.1:8765"));
    get(&inv, "ecosistema/skill/ecosistema/mision");
    get(&inv, "ecosistema/skill/ecosistema/depurar");
}

#[test]
fn ningun_secreto_sale_en_el_json_del_inventario() {
    let m = mundo();
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    let json = serde_json::to_string(&inv).unwrap();
    for s in SECRETOS {
        assert!(!json.contains(s), "se filtra {s}");
    }
    assert!(!json.contains("SECRETO"), "se filtra algo");
    // los campos del contrato están
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let e = &v["elementos"][0];
    for campo in ["id", "agente", "tipo", "ambito", "nombre", "descripcion", "ruta", "activado", "mecanismo", "categoria", "version_formato"] {
        assert!(e.get(campo).is_some(), "falta {campo}");
    }
}

#[test]
fn sin_directorios_el_inventario_esta_vacio_sin_avisos() {
    let d = tempfile::tempdir().unwrap();
    let f = Fuentes::aislada(d.path());
    let a = Ajustes::por_defecto(&f);
    let r = Raices::nuevo(d.path().join("no-existe"), Some(d.path().join("tampoco")), d.path().join("estado"), None);
    let inv = Biblioteca::cargar(&r, &a);
    // solo queda el servidor del ecosistema, que no depende del disco
    assert_eq!(inv.elementos.len(), 1, "{:?}", inv.elementos);
    assert_eq!(inv.elementos[0].id, "ecosistema/mcp/ecosistema/ea-ecosistema");
    assert!(inv.avisos.is_empty(), "{:?}", inv.avisos);
}

#[test]
fn un_claude_json_corrupto_da_lista_vacia_y_aviso() {
    let m = mundo();
    escribe(m.home.join(".claude.json"), "{\"mcpServers\": {\"a\": {\"comm");
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    assert!(inv.elementos.iter().all(|e| !(e.agente == "claude-code" && e.tipo == Tipo::Mcp && e.ambito == Ambito::Usuario)));
    assert!(inv.avisos.iter().any(|a| a.contains(".claude.json")), "{:?}", inv.avisos);
    // lo demás sigue ahí
    get(&inv, "claude-code/plugin/usuario/foo@mkt");
    get(&inv, "claude-code/mcp/proyecto/aprobado");
}

#[test]
fn ficheros_corruptos_de_otros_agentes_solo_avisan() {
    let m = mundo();
    escribe(m.home.join(".codex/config.toml"), "esto = = no es toml");
    escribe(m.home.join(".qwen/settings.json"), "{");
    escribe(m.home.join(".gemini/config/mcp_config.json"), "");
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    assert!(inv.avisos.iter().any(|a| a.contains("config.toml")));
    assert!(inv.avisos.iter().any(|a| a.contains("settings.json")));
    assert!(inv.buscar("codex/mcp/usuario/docs").is_none());
}

#[test]
fn categorias_propias_y_filtros() {
    let mut m = mundo();
    m.ajustes.biblioteca.categorias.insert("claude-code/skill/usuario/propia".into(), "Escritura".into());
    m.ajustes.biblioteca.categorias.insert("claude-code/plugin/usuario/foo@mkt".into(), "Escritura".into());
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    assert_eq!(get(&inv, "claude-code/skill/usuario/propia").categoria, "Escritura");
    let f = Filtro { categoria: Some("Escritura".into()), ..Default::default() };
    assert_eq!(inv.filtrar(&f).len(), 2);
    let f = Filtro { agente: Some("codex".into()), tipo: Some(Tipo::Mcp), activado: Some(Activado::Si), ..Default::default() };
    let ids: Vec<_> = inv.filtrar(&f).iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["codex/mcp/usuario/docs"]);
    assert!(inv.categorias().contains(&"Escritura".to_string()));
    assert!(inv.buscar("no/existe").is_none());
}

#[test]
fn el_repo_como_proyecto_se_reconoce_y_no_duplica_el_home() {
    let m = mundo();
    let r = Raices::nuevo(m.home.clone(), Some(m.repo.clone()), m.home.join("estado"), None);
    assert_eq!(r.repo.as_deref(), Some(m.repo.as_path()));
    let inv = Biblioteca::cargar(&r, &m.ajustes);
    get(&inv, "ecosistema/skill/ecosistema/depurar");
    // proyecto == home: se descarta para no leer dos veces lo mismo
    let r = Raices::nuevo(m.home.clone(), Some(m.home.clone()), m.home.join("estado"), None);
    assert!(r.proyecto.is_none());
    let inv = Biblioteca::cargar(&r, &m.ajustes);
    assert!(inv.avisos.iter().all(|a| !a.contains("id repetido")), "{:?}", inv.avisos);
}

#[test]
fn ids_ida_y_vuelta_y_validacion() {
    for t in [
        "claude-code/plugin/usuario/foo@mkt",
        "claude-code/skill/plugin:foo@mkt/dentro",
        "opencode/plugin/usuario/@scope/pkg",
        "ecosistema/mcp/ecosistema/ea-ecosistema",
        "antigravity/skill/integrada/guia",
    ] {
        let id = Id::analizar(t).unwrap();
        assert_eq!(id.to_string(), t);
        assert_eq!(t.parse::<Id>().unwrap(), id);
    }
    for mal in [
        "",
        "claude-code/skill/usuario",
        "claude-code/skill/usuario/",
        "claude-code/skill/usuario/../etc",
        "claude-code/skill/usuario/a//b",
        "claude-code/skill/usuario/a\\b",
        "claude-code/skill/usuario/\u{7}",
        "claude-code/skill/plugin:/x",
        "claude-code/cosa/usuario/x",
        "claude-code/skill/raro/x",
        "desconocido/skill/usuario/x",
    ] {
        assert!(Id::analizar(mal).is_err(), "{mal:?} debería rechazarse");
    }
    // serde de Elemento
    let e = Elemento::nuevo("codex", Tipo::Skill, Ambito::Plugin("a@b".into()), "x", PathBuf::from("/r")).unwrap();
    let j = serde_json::to_string(&e).unwrap();
    assert!(j.contains("\"ambito\":\"plugin:a@b\"") && j.contains("\"activado\":\"desconocido\"") && j.contains("\"mecanismo\":\"no\""));
    assert_eq!(serde_json::from_str::<Elemento>(&j).unwrap(), e);
}

#[test]
fn lo_que_viene_de_ficheros_ajenos_no_lleva_secuencias_de_escape() {
    let m = mundo();
    // Frontmatter, nota (JSONC), aviso (nombre raro de directorio) y argumentos de un MCP.
    escribe(m.home.join(".claude/skills/rara/SKILL.md"), "---\ndescription: \"hola\x1b]0;pwn\x07 \x1b[2Jmundo\"\n---\n");
    // Windows no admite caracteres de control en los nombres de fichero.
    if cfg!(unix) {
        escribe(m.home.join(".claude/skills/mal\x1bnombre/SKILL.md"), "---\ndescription: x\n---\n");
    }
    escribe(m.home.join(".config/opencode/opencode.jsonc"), "{\n // comentario\n \"mcp\": { \"j\": { \"type\": \"local\", \"command\": [\"x\x1b[31m\", \"a\x07b\"] } }\n}\n");
    let inv = Biblioteca::cargar(&raices(&m), &m.ajustes);
    let rara = get(&inv, "claude-code/skill/usuario/rara");
    assert_eq!(rara.descripcion, "hola]0;pwn [2Jmundo");
    let json = serde_json::to_string(&inv).unwrap();
    for c in ['\u{1b}', '\u{7}'] {
        assert!(!json.contains(&format!("\\u{:04x}", c as u32)), "quedó un carácter de control: {json}");
    }
    if cfg!(unix) {
        assert!(inv.avisos.iter().any(|a| a.contains("mal")), "{:?}", inv.avisos);
    }
    assert!(inv.avisos.iter().all(|a| !a.contains('\x1b')));
}
