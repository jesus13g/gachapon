//! Cliente MCP hacia `ea-mcp`: la única forma en que el orquestador
//! ejecuta un paso `accion` de una rutina. Se identifica ante `ea-mcp`
//! como `"ea-orchestrator"` para que el diario atribuya correctamente al
//! actor `"orquestador"` (ver `ea-mcp::tools::actor_for`).

use anyhow::{bail, Context};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::Value;

use crate::ejecutor::EjecutorHerramientas;

type RunningClient = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

pub struct McpClient {
    client: RunningClient,
}

impl McpClient {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let transport = StreamableHttpClientTransport::from_uri(url.to_string());
        let client_info = InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("ea-orchestrator", env!("CARGO_PKG_VERSION")),
        );
        let client = client_info
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Auto {
                    preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                    legacy_version: None,
                },
            )
            .await
            .context("no se pudo conectar con ea-mcp")?;
        Ok(Self { client })
    }
}

impl EjecutorHerramientas for McpClient {
    async fn llamar(&self, herramienta: &str, argumentos: Value) -> anyhow::Result<Value> {
        let args_map = argumentos.as_object().cloned().unwrap_or_default();
        let result = self
            .client
            .call_tool(CallToolRequestParams::new(herramienta.to_string()).with_arguments(args_map))
            .await
            .with_context(|| format!("llamada MCP a '{herramienta}' falló"))?;

        let Some(block) = result.content.first() else {
            bail!("'{herramienta}' no devolvió contenido");
        };
        let text = &block
            .as_text()
            .with_context(|| format!("'{herramienta}' no devolvió texto"))?
            .text;
        serde_json::from_str(text)
            .with_context(|| format!("'{herramienta}' no devolvió JSON válido: {text}"))
    }
}
