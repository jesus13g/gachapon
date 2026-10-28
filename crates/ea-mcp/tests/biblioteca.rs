//! `ea_biblioteca` vista desde el servidor MCP, sobre HTTP de verdad, con un
//! HOME de `tempfile` (nunca el real): lista con filtros, ficha con uso, sin
//! secretos y solo lectura. El modelo local la lee con cotas más estrechas.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use ea_biblioteca::Raices;
use ea_boveda::Boveda;
use ea_mcp::EaTools;
use ea_store::{EventFilter, PropuestaFilter, Store};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams, ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;

type Client = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

const SKILL: &str = "claude-code/skill/usuario/propia";
const MCP: &str = "claude-code/mcp/usuario/con-secretos";
const SECRETOS: &[&str] = &["SECRETO_ENV", "SECRETO_HEADER"];

struct Entorno {
    url: String,
    db: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

fn escribe(ruta: &Path, texto: &str) {
    fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    fs::write(ruta, texto).unwrap();
}

async fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    escribe(&home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribe(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"con-secretos":{"type":"stdio","command":"npx","args":["-y","pkg"],"env":{"API_KEY":"SECRETO_ENV"}},
            "remoto":{"type":"http","url":"https://m.ejemplo.com/mcp","headers":{"Authorization":"Bearer SECRETO_HEADER"}}}}"#,
    );
    escribe(
        &home.join(".claude/projects/-p/s1.jsonl"),
        concat!(
            r#"{"type":"assistant","timestamp":"2026-10-01T10:00:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Skill","input":{"skill":"propia"}}]}}"#,
            "\n"
        ),
    );
    let estado = dir.path().join("estado");
    let raices = Raices::nuevo(home, None, estado.clone(), None);
    let db = dir.path().join("t.db");
    let store = Arc::new(Mutex::new(Store::open_at(&db).unwrap()));
    let rutinas = dir.path().join("rutinas");
    fs::create_dir_all(&rutinas).unwrap();
    let boveda = dir.path().join("boveda");
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let router = ea_mcp::router(
        move || {
            EaTools::con_entorno(store.clone(), rutinas.clone(), Boveda::en(boveda.clone()), estado.clone())
                .con_ajustes(ajustes.clone())
                .con_raices(raices.clone())
        },
        Default::default(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Entorno { url: format!("http://{addr}/mcp"), db, _dir: dir }
}

async fn conectar(url: &str, nombre: &str) -> Client {
    InitializeRequestParams::new(ClientCapabilities::default(), Implementation::new(nombre, "0.0.1"))
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_uri(url.to_string()),
            ClientLifecycleMode::Auto { preferred_versions: vec![ProtocolVersion::V_2025_11_25], legacy_version: None },
        )
        .await
        .unwrap()
}

async fn llamar(c: &Client, args: Value) -> Result<Value, String> {
    let r = c
        .call_tool(CallToolRequestParams::new("ea_biblioteca").with_arguments(args.as_object().cloned().unwrap_or_default()))
        .await
        .map_err(|e| e.to_string())?;
    let t = &r.content[0].as_text().unwrap().text;
    Ok(serde_json::from_str(t).unwrap_or(Value::String(t.clone())))
}

fn sin_secretos(v: &Value) {
    let t = v.to_string();
    for s in SECRETOS {
        assert!(!t.contains(s), "se coló {s}: {t}");
    }
}

#[tokio::test]
async fn lista_con_filtros_sin_secretos_y_con_usos() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, json!({})).await.unwrap();
    sin_secretos(&r);
    let ids: Vec<&str> = r["elementos"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&SKILL) && ids.contains(&MCP), "{ids:?}");
    assert_eq!(r["total"], r["mostrados"]);
    let skill = r["elementos"].as_array().unwrap().iter().find(|x| x["id"] == SKILL).unwrap();
    assert_eq!((skill["usos"].as_u64(), skill["activado"].as_str(), skill["categoria"].as_str()), (Some(1), Some("si"), Some("usuario")));

    let r = llamar(&c, json!({ "tipo": "mcp", "agente": "claude-code" })).await.unwrap();
    assert_eq!(r["total"], 2);
    assert!(r["elementos"].as_array().unwrap().iter().all(|x| x["tipo"] == "mcp"));
    let r = llamar(&c, json!({ "activado": false })).await.unwrap();
    assert_eq!(r["total"], 0);
    // El límite acota lo que se devuelve, no lo que se cuenta.
    let r = llamar(&c, json!({ "limite": 1 })).await.unwrap();
    assert_eq!(r["mostrados"], 1);
    assert!(r["total"].as_u64().unwrap() > 1);
    assert!(llamar(&c, json!({ "tipo": "nada" })).await.is_err());
}

#[tokio::test]
async fn la_ficha_trae_el_uso_por_actor_y_el_mcp_redactado() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, json!({ "id": SKILL })).await.unwrap();
    assert_eq!(r["elemento"]["id"], SKILL);
    assert_eq!(r["uso"]["total"], 1);
    assert_eq!(r["uso"]["por"][0]["actor"], "claude-code");
    let r = llamar(&c, json!({ "id": MCP })).await.unwrap();
    sin_secretos(&r);
    assert_eq!(r["elemento"]["def_mcp"]["env"], json!(["API_KEY"]));
    assert_eq!(r["uso"]["total"], 0);
    let err = llamar(&c, json!({ "id": "claude-code/skill/usuario/fantasma" })).await.unwrap_err();
    assert!(err.contains("no existe"), "{err}");
}

#[tokio::test]
async fn la_ven_todos_los_roles_y_el_modelo_local_con_cotas_mas_estrechas() {
    let e = entorno().await;
    // Planificador, un agente sin rol (Codex) y lo desatendido: leen.
    for cliente in ["claude-code", "codex-mcp-client", "ea-orchestrator"] {
        let c = conectar(&e.url, cliente).await;
        assert!(llamar(&c, json!({})).await.is_ok(), "{cliente}");
    }
    // El ejecutor también, pero con un tope de elementos y descripciones más cortas.
    let local = conectar(&e.url, "qwen-code-mcp-client").await;
    let r = llamar(&local, json!({ "limite": 1000 })).await.unwrap();
    sin_secretos(&r);
    assert!(r["mostrados"].as_u64().unwrap() <= 40);
    let planificador = conectar(&e.url, "claude-code").await;
    let r = llamar(&planificador, json!({ "id": SKILL })).await.unwrap();
    assert_eq!(r["elemento"]["id"], SKILL);
}

#[tokio::test]
async fn solo_mira_y_deja_rastro_en_el_diario() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    llamar(&c, json!({ "id": SKILL })).await.unwrap();
    let s = Store::open_at(&e.db).unwrap();
    assert!(s.list_propuestas(&PropuestaFilter::default()).unwrap().is_empty());
    let ev = s.list_events(&EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    assert!(ev.iter().any(|x| x.action == "mcp_biblioteca" && x.actor == "claude-code"));
    let tools = c.list_tools(Default::default()).await.unwrap().tools;
    let t = tools.iter().find(|t| t.name == "ea_biblioteca").expect("expuesta");
    assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(true));
}
