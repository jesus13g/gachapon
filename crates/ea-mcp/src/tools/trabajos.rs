//! Los trabajos del bus, de solo lectura: crearlos o decidirlos es de la CLI
//! y las skills.

use ea_bus::Bus;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::{json, Value};

use super::diario::event_to_json;
use super::{cotas_listado, limite, EaTools};
use crate::args::{ListarTrabajosArgs, VerTrabajoArgs};

/// Resumen de un trabajo del bus, o `None` si sus ficheros no se pueden leer.
fn trabajo_resumen(bus: &Bus, id: &str) -> Option<Value> {
    let r = bus.resumen_trabajo(id)?;
    Some(json!({
        "id": r.id,
        "modo": r.modo.as_str(),
        "titulo": r.titulo,
        "estado": r.estado.as_str(),
        "fase_actual": r.fase_actual,
        "fases": r.fases_total,
        "iteracion": r.iteracion,
        "actualizado": r.actualizado_en,
        "creado_por": r.creada_por,
    }))
}

#[tool_router(router = trabajos, vis = "pub(super)")]
impl EaTools {
    #[tool(
        description = "Lista trabajos del bus (misiones, obras y rutinas), \
            opcionalmente filtrados por estado o modo. Los más recientes primero, \
            20 por defecto y como mucho 50 (10 y 20 para el agente local). \
            Solo lectura: crear o decidir trabajo se hace con la CLI (`ea trabajo`) \
            y las skills /mision, /obra, /rutina y /revisar.",
        annotations(title = "Listar trabajos", read_only_hint = true)
    )]
    async fn ea_listar_trabajos(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarTrabajosArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args_json = json!({ "estado": args.estado, "modo": args.modo, "limite": args.limite });
        self.registrada(&context, "mcp_listar_trabajos", None, args_json, |quien| {
            self.bloqueante(move |t| {
                let bus = Bus::abrir_en(&t.estado_dir).map_err(|e| e.to_string())?;
                let mut ids = bus.listar_trabajos().map_err(|e| e.to_string())?;
                ids.reverse();
                let limite = limite(args.limite, cotas_listado(quien.actor));
                let trabajos: Vec<_> = ids
                    .iter()
                    .filter_map(|id| trabajo_resumen(&bus, id))
                    .filter(|t| args.estado.as_deref().is_none_or(|e| t["estado"] == e))
                    .filter(|t| args.modo.as_deref().is_none_or(|m| t["modo"] == m))
                    .take(limite)
                    .collect();
                Ok(json!({ "trabajos": trabajos }))
            })
        })
        .await
    }

    #[tool(
        description = "Muestra el detalle de un trabajo del bus: orden (objetivo, \
            criterio, fases), estado actual e informe si ya lo hay. Solo lectura.",
        annotations(title = "Ver trabajo", read_only_hint = true)
    )]
    async fn ea_ver_trabajo(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<VerTrabajoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = args.id;
        self.registrada(&context, "mcp_ver_trabajo", Some(&id), json!({ "id": id }), |_| {
            let id = id.clone();
            self.bloqueante(move |t| {
                let bus = Bus::abrir_en(&t.estado_dir).map_err(|e| e.to_string())?;
                let orden = bus.leer_orden(&id).map_err(|e| e.to_string())?;
                let estado = bus.leer_estado(&id).map_err(|e| e.to_string())?;
                let informe = bus.leer_informe(&id).map_err(|e| e.to_string())?;
                let mut eventos = t.store.blocking_lock().list_events_by_trabajo(&id).map_err(|e| e.to_string())?;
                eventos.reverse();
                Ok(json!({
                    "orden": orden,
                    "estado": estado,
                    "informe": informe,
                    "eventos": eventos.iter().map(event_to_json).collect::<Vec<_>>(),
                }))
            })
        })
        .await
    }
}
