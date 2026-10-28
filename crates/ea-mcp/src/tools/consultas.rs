//! El vistazo general (`ea_status`) y los equipos de agentes (spec 019), de
//! solo lectura.

use ea_bus::{Bus, Rol};
use ea_store::EventFilter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::{json, Value};

use super::diario::event_to_json;
use super::EaTools;
use crate::args::VerEquipoArgs;

/// Trabajos por estado, colas y semáforos del bus. Si el bus no se puede
/// leer (aún no existe), todo vacío: un `ea_status` no debe fallar por eso.
fn resumen_bus(estado: &std::path::Path) -> (Value, Value, Value) {
    let mut por_estado = serde_json::Map::new();
    let mut colas = serde_json::Map::new();
    if let Ok(bus) = Bus::abrir_en(estado) {
        for (estado, n) in bus.conteo_por_estado() {
            por_estado.insert(estado.as_str().into(), json!(n));
        }
        for a in Rol::TODOS {
            let ids: Vec<_> = bus.listar_cola(a).unwrap_or_default().into_iter().map(|e| e.trabajo).collect();
            colas.insert(a.as_str().into(), json!(ids));
        }
    }
    let semaforos: Vec<_> = ea_bus::listar_semaforos_en(estado)
        .into_iter()
        .map(|(_, o)| json!({ "agente": o.agente, "trabajo": o.trabajo, "pid": o.pid, "desde": o.desde }))
        .collect();
    (json!(por_estado), json!(colas), json!(semaforos))
}

/// Los equipos despiertos y las presencias anotadas (vivas o caducadas), para
/// `ea_status`. Vacío si el bus no se puede leer.
fn resumen_equipos(estado: &std::path::Path) -> (Value, Value) {
    let Ok(bus) = Bus::abrir_en(estado) else { return (json!([]), json!([])) };
    let equipos: Vec<_> = bus
        .resumen_equipos()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.estado == ea_bus::EstadoEquipo::Despierto)
        .map(|e| json!({ "id": e.id, "titulo": e.titulo, "trabajos": e.trabajos }))
        .collect();
    let presencias: Vec<_> = bus
        .presencias()
        .into_iter()
        .map(|(p, viva)| {
            json!({ "agente": p.agente, "rol": p.rol.as_str(), "equipo": p.equipo, "trabajo": p.trabajo, "viva": viva, "latido": p.latido })
        })
        .collect();
    (json!(equipos), json!(presencias))
}

/// Cotas de `ea_listar_equipos` y `ea_ver_equipo`: el modelo local tiene
/// poco contexto.
const MAX_EQUIPOS: usize = 20;
const TRABAJOS_POR_DEFECTO: usize = 10;
const MAX_TRABAJOS: usize = 20;

#[tool_router(router = consultas, vis = "pub(super)")]
impl EaTools {
    #[tool(
        description = "Resumen del estado del ecosistema: orquestador, trabajos \
            del bus por estado, colas, semáforos de worktree, equipos despiertos con la presencia de los agentes y los últimos eventos del diario. Primer vistazo antes \
            de profundizar con las otras herramientas.",
        annotations(title = "Estado del ecosistema", read_only_hint = true)
    )]
    async fn ea_status(&self, context: RequestContext<RoleServer>) -> Result<CallToolResult, McpError> {
        self.registrada(&context, "mcp_status", None, json!({}), |_| {
            self.bloqueante(|t| {
                // El bus antes de tomar la base de datos.
                let (por_estado, colas, semaforos) = resumen_bus(&t.estado_dir);
                let (equipos, presencias) = resumen_equipos(&t.estado_dir);
                // Mismo criterio que `ea status` y `ea-web`: último latido del
                // diario, con umbral del doble del intervalo configurado.
                let umbral_s = ea_ajustes::cargar_de(&t.ajustes).ajustes.umbral_latido_s();
                let store = t.store.blocking_lock();
                let events = store.list_events(&EventFilter { limit: Some(5), ..Default::default() }).map_err(|e| e.to_string())?;
                let orquestador = store.estado_orquestador(umbral_s).map_err(|e| e.to_string())?;
                Ok(json!({
                    "orquestador": {
                        "vivo": orquestador.vivo,
                        "ultimo_latido": orquestador.ultimo_latido.map(ea_store::rfc3339),
                        "estado": orquestador.descripcion(),
                    },
                    "trabajos_por_estado": por_estado,
                    "colas": colas,
                    "semaforos": semaforos,
                    "equipos": equipos,
                    "presencias": presencias,
                    "ultimos_eventos": events.iter().map(event_to_json).collect::<Vec<_>>(),
                }))
            })
        })
        .await
    }

    #[tool(
        description = "Lista los equipos de agentes (spec 019) con su estado, sus \
            miembros (agente, rol y si están despiertos) y cuántos trabajos tienen; \
            los más recientes primero. Solo lectura: despertar o dormir un equipo \
            es del usuario (`ea equipo`).",
        annotations(title = "Listar equipos", read_only_hint = true)
    )]
    async fn ea_listar_equipos(&self, context: RequestContext<RoleServer>) -> Result<CallToolResult, McpError> {
        self.registrada(&context, "mcp_listar_equipos", None, json!({}), |_| {
            self.bloqueante(|t| {
                let bus = Bus::abrir_en(&t.estado_dir).map_err(|e| e.to_string())?;
                let mut equipos = bus.resumen_equipos().map_err(|e| e.to_string())?;
                equipos.reverse();
                let total = equipos.len();
                equipos.truncate(MAX_EQUIPOS);
                Ok(json!({ "equipos": equipos, "total": total }))
            })
        })
        .await
    }

    #[tool(
        description = "Un equipo de agentes: objetivo, miembros con su presencia \
            (despierto, en qué trabajo) y las entradas de su cola de órdenes o de \
            revisión, y los últimos trabajos con su estado y su último informe en \
            una línea. Solo lectura.",
        annotations(title = "Ver equipo", read_only_hint = true)
    )]
    async fn ea_ver_equipo(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<VerEquipoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args_json = json!({ "id": args.id, "trabajos": args.trabajos });
        self.registrada(&context, "mcp_ver_equipo", None, args_json, |_| {
            self.bloqueante(move |t| {
                let bus = Bus::abrir_en(&t.estado_dir).map_err(|e| e.to_string())?;
                let n = args.trabajos.filter(|n| *n > 0).map_or(TRABAJOS_POR_DEFECTO, |n| (n as usize).min(MAX_TRABAJOS));
                let v = bus.vista_equipo(&args.id, n).map_err(|e| e.to_string())?;
                serde_json::to_value(v).map_err(|e| e.to_string())
            })
        })
        .await
    }
}
