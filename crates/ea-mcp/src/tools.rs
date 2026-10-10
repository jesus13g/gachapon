//! Catálogo de herramientas MCP: cuatro verbos de consulta y uno de
//! creación de tarea, todos envolviendo la fachada `Store` de `ea-store`
//! sin añadirle lógica. Cada invocación queda registrada en el diario de
//! eventos (§11.3 de la especificación), independientemente de si tuvo
//! éxito o de su nivel de riesgo.

use std::sync::Arc;
use std::time::Instant;

use ea_store::{
    Event, EventFilter, EventOutcome, NewEvent, NewTask, Store, Task, TaskFilter, TaskOrigin,
    TaskStatus,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde_json::json;
use tokio::sync::Mutex;

use crate::args::{CrearTareaArgs, ListarDiarioArgs, ListarTareasArgs, VerTareaArgs};

/// Deriva el actor que este servidor escribe en el diario y en las tareas
/// que crea, a partir del nombre que el cliente MCP declaró al conectar
/// (`Implementation::name`, capturado en el handshake `initialize`). Nunca
/// es un argumento de ninguna herramienta: el modelo no debe poder declarar
/// que actúa en nombre de otro (§7.3). Por defecto cae en "claude-code" —
/// preserva el comportamiento anterior para cualquier cliente que no se
/// identifique explícitamente como el proxy del modelo local (§8.2: la
/// atribución del diario debe ser veraz, no la del primer consumidor que
/// existió).
fn actor_for(client_name: Option<&str>) -> &'static str {
    match client_name {
        Some("ea-proxy") => "modelo-local",
        Some("ea-orchestrator") => "orquestador",
        _ => "claude-code",
    }
}

#[derive(Clone)]
pub struct EaTools {
    store: Arc<Mutex<Store>>,
    // Leído por el código que genera `#[tool_handler]` sobre el impl de
    // `ServerHandler` más abajo; el análisis de dead-code no lo ve porque
    // ese acceso vive en un bloque distinto expandido por macro.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

type ToolBody = Result<serde_json::Value, String>;

fn fmt_ts(dt: time::OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

fn task_to_json(t: &Task) -> serde_json::Value {
    json!({
        "id": t.id,
        "creada": fmt_ts(t.created_at),
        "actualizada": fmt_ts(t.updated_at),
        "creada_por": t.created_by,
        "asignada_a": t.assigned_to,
        "origen": t.origin.as_str(),
        "tarea_padre": t.parent_task_id,
        "titulo": t.title,
        "solicita": t.request,
        "estado": t.status.as_str(),
        "resultado": t.result,
    })
}

fn event_to_json(e: &Event) -> serde_json::Value {
    json!({
        "id": e.id,
        "cuando": fmt_ts(e.ts),
        "actor": e.actor,
        "accion": e.action,
        "tarea": e.task_id,
        "resultado": e.outcome.map(|o| o.as_str()),
        "duracion_ms": e.duration_ms,
        "detalle": e.detail,
    })
}

fn respond(body: ToolBody) -> Result<CallToolResult, McpError> {
    match body {
        Ok(value) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        )])),
        Err(msg) => Err(McpError::internal_error(msg, None)),
    }
}

#[tool_router]
impl EaTools {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    /// Registra en el diario una invocación de herramienta: quién, qué
    /// verbo, con qué argumentos, con qué resultado y cuánto tardó.
    /// Independiente del nivel de riesgo de la herramienta (§11.3).
    async fn log_invocation(
        &self,
        actor: &str,
        action: &str,
        task_id: Option<i64>,
        started: Instant,
        args: serde_json::Value,
        body: &ToolBody,
    ) {
        let duration_ms = started.elapsed().as_millis() as i64;
        let detail = match body {
            Ok(_) => json!({ "args": args }),
            Err(msg) => json!({ "args": args, "error": msg }),
        };
        let outcome = if body.is_ok() {
            EventOutcome::Ok
        } else {
            EventOutcome::Error
        };
        let store = self.store.lock().await;
        let _ = store.append_event(NewEvent {
            actor: actor.to_string(),
            action: action.to_string(),
            task_id,
            outcome: Some(outcome),
            duration_ms: Some(duration_ms),
            detail: Some(detail.to_string()),
        });
    }

    #[tool(
        description = "Resumen del estado del ecosistema: cuántas tareas hay en \
            cada estado y los últimos eventos del diario. Primer vistazo antes \
            de profundizar con las otras herramientas.",
        annotations(title = "Estado del ecosistema", read_only_hint = true)
    )]
    async fn ea_status(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let actor = actor_for(context.client_info().as_ref().map(|i| i.name.as_str()));
        let started = Instant::now();

        let body: ToolBody = async {
            const ESTADOS: [TaskStatus; 5] = [
                TaskStatus::Pendiente,
                TaskStatus::EnProgreso,
                TaskStatus::Completada,
                TaskStatus::Fallida,
                TaskStatus::Cancelada,
            ];
            let store = self.store.lock().await;
            let mut counts = serde_json::Map::new();
            for status in ESTADOS {
                let n = store
                    .list_tasks(&TaskFilter {
                        status: Some(status),
                        ..Default::default()
                    })
                    .map_err(|e| e.to_string())?
                    .len();
                counts.insert(status.as_str().to_string(), json!(n));
            }
            let events = store
                .list_events(&EventFilter {
                    limit: Some(5),
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?;
            // Mismo criterio que `ea status` y `ea-web`: último latido del
            // diario, con umbral del doble del intervalo configurado.
            let umbral_s = (ea_orchestrator::config::load().intervalo_heartbeat_s * 2) as i64;
            let orquestador = store.estado_orquestador(umbral_s).map_err(|e| e.to_string())?;
            Ok(json!({
                "orquestador": {
                    "vivo": orquestador.vivo,
                    "ultimo_latido": orquestador.ultimo_latido.map(fmt_ts),
                    "estado": match (orquestador.vivo, orquestador.ultimo_latido) {
                        (true, _) => "vivo",
                        (false, Some(_)) => "sin señal reciente",
                        (false, None) => "sin señal (ningún heartbeat registrado todavía)",
                    },
                },
                "tareas_por_estado": counts,
                "ultimos_eventos": events.iter().map(event_to_json).collect::<Vec<_>>(),
            }))
        }
        .await;

        self.log_invocation(actor, "mcp_status", None, started, json!({}), &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Lista tareas, opcionalmente filtradas por estado o por a \
            quién están asignadas. Las más recientes primero.",
        annotations(title = "Listar tareas", read_only_hint = true)
    )]
    async fn ea_listar_tareas(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarTareasArgs>,
    ) -> Result<CallToolResult, McpError> {
        let actor = actor_for(context.client_info().as_ref().map(|i| i.name.as_str()));
        let started = Instant::now();
        let args_json = json!({
            "estado": args.estado, "asignado_a": args.asignado_a, "limite": args.limite,
        });

        let body: ToolBody = async {
            let status = match &args.estado {
                Some(s) => Some(s.parse::<TaskStatus>().map_err(|e| e.to_string())?),
                None => None,
            };
            let filter = TaskFilter {
                status,
                assigned_to: args.asignado_a.clone(),
                limit: Some(args.limite.unwrap_or(20)),
            };
            let store = self.store.lock().await;
            let tasks = store.list_tasks(&filter).map_err(|e| e.to_string())?;
            Ok(json!({ "tareas": tasks.iter().map(task_to_json).collect::<Vec<_>>() }))
        }
        .await;

        self.log_invocation(actor, "mcp_listar_tareas", None, started, args_json, &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Muestra el detalle de una tarea junto con su historial \
            completo de eventos.",
        annotations(title = "Ver tarea", read_only_hint = true)
    )]
    async fn ea_ver_tarea(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<VerTareaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let actor = actor_for(context.client_info().as_ref().map(|i| i.name.as_str()));
        let started = Instant::now();
        let args_json = json!({ "id": args.id });

        let body: ToolBody = async {
            let store = self.store.lock().await;
            let task = store.get_task(args.id).map_err(|e| e.to_string())?;
            let mut events = store
                .list_events_by_task(args.id)
                .map_err(|e| e.to_string())?;
            events.reverse();
            Ok(json!({
                "tarea": task_to_json(&task),
                "eventos": events.iter().map(event_to_json).collect::<Vec<_>>(),
            }))
        }
        .await;

        self.log_invocation(actor, "mcp_ver_tarea", Some(args.id), started, args_json, &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Lista eventos del diario, opcionalmente filtrados por \
            tarea, actor o rango de fechas. Los más recientes primero.",
        annotations(title = "Listar diario", read_only_hint = true)
    )]
    async fn ea_listar_diario(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarDiarioArgs>,
    ) -> Result<CallToolResult, McpError> {
        let actor = actor_for(context.client_info().as_ref().map(|i| i.name.as_str()));
        let started = Instant::now();
        let args_json = json!({
            "tarea": args.tarea, "actor": args.actor,
            "desde": args.desde, "hasta": args.hasta, "limite": args.limite,
        });

        let body: ToolBody = async {
            let since = match &args.desde {
                Some(s) => Some(crate::args::parse_flexible_date(s)?),
                None => None,
            };
            let until = match &args.hasta {
                Some(s) => Some(crate::args::parse_flexible_date(s)?),
                None => None,
            };
            let filter = EventFilter {
                task_id: args.tarea,
                actor: args.actor.clone(),
                since,
                until,
                exclude_action: None,
                limit: Some(args.limite.unwrap_or(20)),
            };
            let store = self.store.lock().await;
            let events = store.list_events(&filter).map_err(|e| e.to_string())?;
            Ok(json!({ "eventos": events.iter().map(event_to_json).collect::<Vec<_>>() }))
        }
        .await;

        self.log_invocation(actor, "mcp_listar_diario", args.tarea, started, args_json, &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Crea una tarea dirigida a un actor del ecosistema (p. ej. \
            'usuario', 'modelo-local', o esta misma sesión para \
            autoseguimiento). El origen queda fijado como 'agente' y quien la \
            crea como el actor que invoca esta herramienta. Usa simular=true \
            para ver qué tarea se crearía sin escribir nada.",
        annotations(
            title = "Crear tarea",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_crear_tarea(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<CrearTareaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let actor = actor_for(context.client_info().as_ref().map(|i| i.name.as_str()));
        let started = Instant::now();
        let simular = args.simular.unwrap_or(false);
        let args_json = json!({
            "titulo": args.titulo, "solicita": args.solicita,
            "asignado_a": args.asignado_a, "tarea_padre": args.tarea_padre,
            "simular": simular,
        });

        if simular {
            let body: ToolBody = Ok(json!({
                "simulado": true,
                "tarea_que_se_crearia": {
                    "creada_por": actor,
                    "asignada_a": args.asignado_a,
                    "origen": TaskOrigin::Agente.as_str(),
                    "tarea_padre": args.tarea_padre,
                    "titulo": args.titulo,
                    "solicita": args.solicita,
                }
            }));
            self.log_invocation(
                actor,
                "mcp_crear_tarea_simulada",
                None,
                started,
                args_json,
                &body,
            )
            .await;
            return respond(body);
        }

        let body: ToolBody = async {
            let mut store = self.store.lock().await;
            store
                .create_task(NewTask {
                    created_by: actor.to_string(),
                    assigned_to: args.asignado_a.clone(),
                    origin: TaskOrigin::Agente,
                    parent_task_id: args.tarea_padre,
                    title: args.titulo.clone(),
                    request: args.solicita.clone(),
                })
                .map(|task| json!({ "tarea": task_to_json(&task) }))
                .map_err(|e| e.to_string())
        }
        .await;

        // Si tuvo éxito, `create_task` ya insertó el evento `tarea_creada`
        // (actor=claude-code) en la misma transacción: duplicarlo aquí
        // violaría "una sola fuente de verdad" para el mismo hecho. Si
        // falló, no hay evento (la transacción no se comitió), así que se
        // registra explícitamente para no perder la traza (§11.3).
        if body.is_err() {
            self.log_invocation(actor, "mcp_crear_tarea", None, started, args_json, &body)
                .await;
        }
        respond(body)
    }
}

#[tool_handler]
impl ServerHandler for EaTools {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Herramientas del ecosistema de asistente personal. ea_status da \
                 una vista general; ea_listar_tareas y ea_ver_tarea inspeccionan \
                 tareas; ea_listar_diario consulta el historial de eventos; \
                 ea_crear_tarea (con simular=true para previsualizar) delega \
                 trabajo a otro actor del ecosistema."
                    .to_string(),
            )
    }
}
