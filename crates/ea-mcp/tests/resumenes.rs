//! El texto exacto de `ea_status` y `ea_listar_trabajos` sobre un bus conocido.

use std::sync::Arc;

use ea_bus::{Bus, Evento, Fase, Orden};
use ea_mcp::EaTools;
use ea_store::Store;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams, ProtocolVersion};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::json;
use tokio::sync::Mutex;

/// Cuatro trabajos que aparecen en un orden distinto del de los estados: una
/// misión cerrada, otra en cola, una obra en curso y una misión bloqueada en
/// revisión.
fn sembrar(bus: &Bus, wt: &str) -> [String; 4] {
    let ids = ["20260101T000001-00000001", "20260101T000002-00000002", "20260101T000003-00000003", "20260101T000004-00000004"]
        .map(String::from);
    let fase = |n: u32| Fase { numero: n, titulo: format!("fase {n}"), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
    for (i, id) in ids.iter().enumerate() {
        let base = if i == 2 {
            Orden::obra("una obra", wt, "o", vec![fase(1), fase(2)], None, "prueba")
        } else {
            Orden::mision(format!("misión {}", i + 1), wt, "o", "true", "prueba")
        };
        bus.crear_trabajo(Orden { id: id.clone(), creada_en: "2026-01-01T00:00:00Z".into(), ..base }).unwrap();
    }
    for id in [&ids[0], &ids[2], &ids[3]] {
        bus.retirar_de_colas(id).unwrap();
        bus.aplicar(id, Evento::Tomar).unwrap();
    }
    let informe = |v: serde_json::Value| serde_json::from_value(v).unwrap();
    bus.aplicar(&ids[0], Evento::Entregar(informe(json!({ "resultado": "terminado", "duracion_s": 12 })))).unwrap();
    bus.aplicar(&ids[0], Evento::Cerrar).unwrap();
    bus.aplicar(&ids[3], Evento::Entregar(informe(json!({ "resultado": "bloqueado", "motivo": "no pasa", "duracion_s": 30 })))).unwrap();
    ids
}

async fn llamar(url: &str, herramienta: &str) -> String {
    let cliente = InitializeRequestParams::new(ClientCapabilities::default(), Implementation::new("ea-mcp-test-client", "0.0.1"))
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_uri(url.to_string()),
            ClientLifecycleMode::Auto { preferred_versions: vec![ProtocolVersion::V_2025_11_25], legacy_version: None },
        )
        .await
        .expect("conectar al servidor MCP de prueba");
    let r = cliente.call_tool(CallToolRequestParams::new(herramienta.to_string())).await.expect(herramienta);
    let texto = r.content[0].as_text().expect("contenido de texto").text.clone();
    cliente.cancel().await.ok();
    texto
}

#[tokio::test]
async fn ea_status_y_ea_listar_trabajos_dicen_lo_mismo_que_siempre() {
    let dir = tempfile::tempdir().unwrap();
    let estado = dir.path().join("estado");
    let bus = Bus::abrir_en(&estado).unwrap();
    let ids = sembrar(&bus, &dir.path().to_string_lossy());

    let store = Arc::new(Mutex::new(Store::open_at(&dir.path().join("ea.db")).unwrap()));
    let rutinas = dir.path().join("rutinas");
    let boveda = ea_boveda::Boveda::en(dir.path().join("boveda"));
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let router = ea_mcp::router(
        move || EaTools::con_entorno(store.clone(), rutinas.clone(), boveda.clone(), estado.clone()).con_ajustes(ajustes.clone()),
        Default::default(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    assert_eq!(
        llamar(&url, "ea_status").await,
        r#"{
  "orquestador": {
    "vivo": false,
    "ultimo_latido": null,
    "estado": "sin señal (ningún heartbeat registrado todavía)"
  },
  "trabajos_por_estado": {
    "cerrado": 1,
    "en_cola": 1,
    "ejecutando": 1,
    "en_revision": 1
  },
  "colas": {
    "claude-code": [
      "20260101T000004-00000004"
    ],
    "modelo-local": [
      "20260101T000002-00000002"
    ]
  },
  "semaforos": [],
  "equipos": [],
  "presencias": [],
  "ultimos_eventos": []
}"#
    );

    // Lo único que cambia de una ejecución a otra: cuándo se tocó cada estado.
    let mut listado = llamar(&url, "ea_listar_trabajos").await;
    for (n, id) in ids.iter().enumerate() {
        listado = listado.replace(&bus.leer_estado(id).unwrap().actualizado_en, &format!("<actualizado {}>", n + 1));
    }
    assert_eq!(
        listado,
        r#"{
  "trabajos": [
    {
      "id": "20260101T000004-00000004",
      "modo": "mision",
      "titulo": "misión 4",
      "estado": "en_revision",
      "fase_actual": null,
      "fases": 0,
      "iteracion": 0,
      "actualizado": "<actualizado 4>",
      "creado_por": "prueba"
    },
    {
      "id": "20260101T000003-00000003",
      "modo": "obra",
      "titulo": "una obra",
      "estado": "ejecutando",
      "fase_actual": 1,
      "fases": 2,
      "iteracion": 0,
      "actualizado": "<actualizado 3>",
      "creado_por": "prueba"
    },
    {
      "id": "20260101T000002-00000002",
      "modo": "mision",
      "titulo": "misión 2",
      "estado": "en_cola",
      "fase_actual": null,
      "fases": 0,
      "iteracion": 0,
      "actualizado": "<actualizado 2>",
      "creado_por": "prueba"
    },
    {
      "id": "20260101T000001-00000001",
      "modo": "mision",
      "titulo": "misión 1",
      "estado": "cerrado",
      "fase_actual": null,
      "fases": 0,
      "iteracion": 0,
      "actualizado": "<actualizado 1>",
      "creado_por": "prueba"
    }
  ]
}"#
    );
}
