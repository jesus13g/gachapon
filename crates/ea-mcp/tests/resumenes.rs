//! El texto exacto de `ea_status` y `ea_listar_trabajos` sobre un bus conocido.

use std::sync::Arc;

use ea_bus::Bus;
use ea_mcp::EaTools;
use ea_store::Store;
use rmcp::model::CallToolRequestParams;
use tokio::sync::Mutex;

async fn llamar(url: &str, herramienta: &str) -> String {
    let cliente = ea_pruebas::mcp::conectar(url, "ea-mcp-test-client").await;
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
    let ids = ea_pruebas::sembrar_trabajos(&bus, &dir.path().to_string_lossy());

    let store = Arc::new(Mutex::new(Store::open_at(&dir.path().join("ea.db")).unwrap()));
    let rutinas = dir.path().join("rutinas");
    let boveda = ea_boveda::Boveda::en(dir.path().join("boveda"));
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let router = ea_mcp::router(
        move || EaTools::con_entorno(store.clone(), rutinas.clone(), boveda.clone(), estado.clone()).con_ajustes(ajustes.clone()).con_credencial(credencial()),
        Default::default(),
    );
    let url = ea_pruebas::mcp::servidor(router).await;

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

/// La credencial de las pruebas, la que manda `conectar`.
fn credencial() -> ea_mcp::Credencial {
    ea_mcp::Credencial::de_texto(ea_pruebas::mcp::CREDENCIAL).unwrap()
}
