//! Cliente MCP hacia `ea-mcp`, la única forma en que este proxy ejecuta una
//! herramienta ya decidida. Se identifica ante `ea-mcp` como `"ea-proxy"`
//! para que el diario atribuya correctamente al actor `modelo-local`
//! (ver `ea-mcp::tools::actor_for`).

use anyhow::{Context, bail};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::Value;

type RunningClient = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

pub struct McpClient {
    client: RunningClient,
}

impl McpClient {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let transport = StreamableHttpClientTransport::from_uri(url.to_string());
        let client_info = InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new("ea-proxy", env!("CARGO_PKG_VERSION")),
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

    pub async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Map<String, Value>,
    ) -> anyhow::Result<Value> {
        let result = self
            .client
            .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(args))
            .await
            .with_context(|| format!("llamada MCP a '{name}' falló"))?;

        let Some(block) = result.content.first() else {
            bail!("'{name}' no devolvió contenido");
        };
        let text = &block
            .as_text()
            .with_context(|| format!("'{name}' no devolvió texto"))?
            .text;
        serde_json::from_str(text)
            .with_context(|| format!("'{name}' no devolvió JSON válido: {text}"))
    }
}
