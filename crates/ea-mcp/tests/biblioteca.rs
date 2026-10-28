//! `ea_biblioteca` vista desde el servidor MCP, sobre HTTP de verdad, con un
//! HOME de `tempfile` (nunca el real): lista con filtros, ficha con uso, sin
//! secretos y solo lectura. El modelo local la lee con cotas más estrechas.

use std::fs;
use std::sync::Arc;

use ea_biblioteca::Raices;
use ea_boveda::Boveda;
use ea_mcp::EaTools;
use ea_store::{EventFilter, PropuestaFilter, Store};
use ea_pruebas::biblioteca::{MCP, SECRETOS, SKILL};
use ea_pruebas::mcp::{conectar, llamar};
use ea_pruebas::sin_secretos;
use serde_json::json;
use tokio::sync::Mutex;

struct Entorno {
    url: String,
    db: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    ea_pruebas::biblioteca::home(&home);
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
    Entorno { url: ea_pruebas::mcp::servidor(router).await, db, _dir: dir }
}

#[tokio::test]
async fn lista_con_filtros_sin_secretos_y_con_usos() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_biblioteca", json!({})).await.unwrap();
    sin_secretos(&r.to_string(), SECRETOS);
    let ids: Vec<&str> = r["elementos"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&SKILL) && ids.contains(&MCP), "{ids:?}");
    assert_eq!(r["total"], r["mostrados"]);
    let skill = r["elementos"].as_array().unwrap().iter().find(|x| x["id"] == SKILL).unwrap();
    assert_eq!((skill["usos"].as_u64(), skill["activado"].as_str(), skill["categoria"].as_str()), (Some(1), Some("si"), Some("usuario")));

    let r = llamar(&c, "ea_biblioteca", json!({ "tipo": "mcp", "agente": "claude-code" })).await.unwrap();
    assert_eq!(r["total"], 2);
    assert!(r["elementos"].as_array().unwrap().iter().all(|x| x["tipo"] == "mcp"));
    let r = llamar(&c, "ea_biblioteca", json!({ "activado": false })).await.unwrap();
    assert_eq!(r["total"], 0);
    // El límite acota lo que se devuelve, no lo que se cuenta.
    let r = llamar(&c, "ea_biblioteca", json!({ "limite": 1 })).await.unwrap();
    assert_eq!(r["mostrados"], 1);
    assert!(r["total"].as_u64().unwrap() > 1);
    assert!(llamar(&c, "ea_biblioteca", json!({ "tipo": "nada" })).await.is_err());
}

#[tokio::test]
async fn la_ficha_trae_el_uso_por_actor_y_el_mcp_redactado() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_biblioteca", json!({ "id": SKILL })).await.unwrap();
    assert_eq!(r["elemento"]["id"], SKILL);
    assert_eq!(r["uso"]["total"], 1);
    assert_eq!(r["uso"]["por"][0]["actor"], "claude-code");
    let r = llamar(&c, "ea_biblioteca", json!({ "id": MCP })).await.unwrap();
    sin_secretos(&r.to_string(), SECRETOS);
    assert_eq!(r["elemento"]["def_mcp"]["env"], json!(["API_KEY"]));
    assert_eq!(r["uso"]["total"], 0);
    let err = llamar(&c, "ea_biblioteca", json!({ "id": "claude-code/skill/usuario/fantasma" })).await.unwrap_err();
    assert!(err.contains("no existe"), "{err}");
}

#[tokio::test]
async fn la_ven_todos_los_roles_y_el_modelo_local_con_cotas_mas_estrechas() {
    let e = entorno().await;
    // Planificador, un agente sin rol (Codex) y lo desatendido: leen.
    for cliente in ["claude-code", "codex-mcp-client", "ea-orchestrator"] {
        let c = conectar(&e.url, cliente).await;
        assert!(llamar(&c, "ea_biblioteca", json!({})).await.is_ok(), "{cliente}");
    }
    // El ejecutor también, pero con un tope de elementos y descripciones más cortas.
    let local = conectar(&e.url, "qwen-code-mcp-client").await;
    let r = llamar(&local, "ea_biblioteca", json!({ "limite": 1000 })).await.unwrap();
    sin_secretos(&r.to_string(), SECRETOS);
    assert!(r["mostrados"].as_u64().unwrap() <= 40);
    let planificador = conectar(&e.url, "claude-code").await;
    let r = llamar(&planificador, "ea_biblioteca", json!({ "id": SKILL })).await.unwrap();
    assert_eq!(r["elemento"]["id"], SKILL);
}

#[tokio::test]
async fn solo_mira_y_deja_rastro_en_el_diario() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    llamar(&c, "ea_biblioteca", json!({ "id": SKILL })).await.unwrap();
    let s = Store::open_at(&e.db).unwrap();
    assert!(s.list_propuestas(&PropuestaFilter::default()).unwrap().is_empty());
    let ev = s.list_events(&EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    assert!(ev.iter().any(|x| x.action == "mcp_biblioteca" && x.actor == "claude-code"));
    let tools = c.list_tools(Default::default()).await.unwrap().tools;
    let t = tools.iter().find(|t| t.name == "ea_biblioteca").expect("expuesta");
    assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(true));
}
