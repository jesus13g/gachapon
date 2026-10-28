//! El servidor MCP de prueba, sobre HTTP de verdad, y su cliente.

use rmcp::model::{CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams, ProtocolVersion};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::Value;

pub type Cliente = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

/// Sirve `router` (el de `ea_mcp::router`) en un puerto libre de 127.0.0.1
/// y devuelve la URL del MCP.
pub async fn servidor(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    url
}

/// La credencial del planificador de las pruebas: la que el servidor de
/// prueba recibe con `EaTools::con_credencial` y la que manda [`conectar`].
pub const CREDENCIAL: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

/// Un cliente que se presenta como `nombre` y trae [`CREDENCIAL`] (como el
/// planificador fuera del sandbox): el actor que ve el servidor.
pub async fn conectar(url: &str, nombre: &str) -> Cliente {
    conectar_con(url, nombre, Some(CREDENCIAL)).await
}

/// Como [`conectar`], con la credencial que se diga en `X-EA-Credencial`
/// (`None`: sin la cabecera, como el agente local dentro del sandbox).
pub async fn conectar_con(url: &str, nombre: &str, credencial: Option<&str>) -> Cliente {
    let mut cabeceras = std::collections::HashMap::new();
    if let Some(c) = credencial {
        cabeceras.insert(axum::http::HeaderName::from_static("x-ea-credencial"), axum::http::HeaderValue::from_str(c).unwrap());
    }
    let config = StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(cabeceras);
    InitializeRequestParams::new(ClientCapabilities::default(), Implementation::new(nombre, "0.0.1"))
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(config),
            ClientLifecycleMode::Auto { preferred_versions: vec![ProtocolVersion::V_2025_11_25], legacy_version: None },
        )
        .await
        .unwrap()
}

/// Llama a `herramienta`: `Ok` con el JSON que devuelve (o su texto, si no
/// es JSON), o `Err(mensaje)` si se rechazó.
pub async fn llamar(c: &Cliente, herramienta: &str, args: Value) -> Result<Value, String> {
    let r = c
        .call_tool(CallToolRequestParams::new(herramienta.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default()))
        .await
        .map_err(|e| e.to_string())?;
    let t = &r.content[0].as_text().unwrap().text;
    Ok(serde_json::from_str(t).unwrap_or(Value::String(t.clone())))
}
