//! El diario de eventos, de solo lectura.

use ea_store::{Event, FiltroDiario};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::{json, Value};

use super::{cotas_listado, limite, EaTools};
use crate::args::ListarDiarioArgs;

pub(super) fn event_to_json(e: &Event) -> Value {
    json!({
        "id": e.id,
        "cuando": ea_store::rfc3339(e.ts),
        "actor": e.actor,
        "accion": e.action,
        "trabajo": e.trabajo,
        "resultado": e.outcome.map(|o| o.as_str()),
        "duracion_ms": e.duration_ms,
        "detalle": e.detail,
    })
}

#[tool_router(router = diario, vis = "pub(super)")]
impl EaTools {
    #[tool(
        description = "Lista eventos del diario, opcionalmente filtrados por \
            trabajo, actor o rango de fechas. Los más recientes primero, 20 por \
            defecto y como mucho 50 (10 y 20 para el agente local).",
        annotations(title = "Listar diario", read_only_hint = true)
    )]
    async fn ea_listar_diario(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarDiarioArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args_json = json!({
            "trabajo": args.trabajo, "actor": args.actor,
            "desde": args.desde, "hasta": args.hasta, "limite": args.limite,
        });
        let trabajo = args.trabajo.clone();
        self.registrada(&context, "mcp_listar_diario", trabajo.as_deref(), args_json, |quien| {
            let filtro = FiltroDiario {
                trabajo: args.trabajo,
                actor: args.actor,
                desde: args.desde,
                hasta: args.hasta,
                limite: Some(limite(args.limite, cotas_listado(quien.actor)) as i64),
                ..Default::default()
            };
            self.bloqueante(move |t| {
                let filter = filtro.a_filtro(None).map_err(|e| e.to_string())?;
                let events = t.store.blocking_lock().list_events(&filter).map_err(|e| e.to_string())?;
                Ok(json!({ "eventos": events.iter().map(event_to_json).collect::<Vec<_>>() }))
            })
        })
        .await
    }
}
