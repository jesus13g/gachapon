//! Catálogo de herramientas MCP: consultas de solo lectura sobre el diario y
//! el bus de trabajos, la bóveda y las mutaciones supervisadas. Crear o
//! decidir trabajo se hace con la CLI y las skills, no desde aquí. Cada invocación queda registrada en el diario de
//! eventos (§11.3 de la especificación), independientemente de si tuvo
//! éxito o de su nivel de riesgo.

use std::sync::Arc;
use std::time::Instant;

use ea_bus::{Bus, Rol};
use ea_store::{Event, EventFilter, EventOutcome, NewEvent, Store};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde_json::json;
use tokio::sync::Mutex;

use ea_acciones::{autorizar_mcp, buscar, observar, proponer, simular_con, Riesgo, SoloConsulta};
use ea_boveda::{Boveda, Limites, NuevaNota};

use crate::args::{
    BuscarNotasArgs, EscribirNotaArgs, LeerNotaArgs, ListarDiarioArgs, ListarTrabajosArgs,
    BibliotecaArgs, EncolarRutinaArgs, HerramientaArgs, LanzarSesionArgs, ProgramarRutinaArgs, PromoverNotaArgs, RegistrarRutinaArgs, ReiniciarServicioArgs, RutinaArgs, VerEquipoArgs, VerTrabajoArgs,
};

/// Quién hace una llamada MCP: el actor que se escribe en el diario y del
/// que salen los permisos ([`autorizar_mcp`]), el agente del catálogo y el
/// nombre que declaró el cliente. Sale del `clientInfo.name` del
/// `initialize`, nunca de un argumento: el modelo no debe poder declarar que
/// actúa en nombre de otro (§7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Llamante {
    pub actor: &'static str,
    pub agente: Option<&'static str>,
    pub cliente: Option<String>,
}

/// Actor de un agente del catálogo que no ocupa ningún rol: solo lee.
pub const ACTOR_SIN_ROL: &str = "sin-rol";
/// Actor de un cliente que no es ningún agente del catálogo: solo lee.
pub const ACTOR_DESCONOCIDO: &str = "desconocido";

/// El actor de un cliente según el catálogo y los roles vigentes (spec 017,
/// RF-013). Un agente actúa con el nombre en disco del rol que ocupa
/// (`claude-code` el planificador, `modelo-local` el ejecutor), que es el
/// que da los permisos y la región de la bóveda; uno sin rol, o un cliente
/// desconocido, solo lee. Antes, un cliente desconocido actuaba como
/// `claude-code`.
pub fn actor_for(cliente: Option<&str>, ajustes: &ea_ajustes::Ajustes) -> Llamante {
    let cliente_s = cliente.map(str::to_string);
    if cliente == Some("ea-orchestrator") {
        return Llamante { actor: "orquestador", agente: None, cliente: cliente_s };
    }
    match cliente.and_then(ea_ajustes::por_cliente_mcp) {
        Some(f) => {
            let actor = ajustes.rol_de(f.id).map_or(ACTOR_SIN_ROL, |r| r.as_str());
            Llamante { actor, agente: Some(f.id), cliente: cliente_s }
        }
        None => Llamante { actor: ACTOR_DESCONOCIDO, agente: None, cliente: cliente_s },
    }
}

#[derive(Clone)]
pub struct EaTools {
    store: Arc<Mutex<Store>>,
    /// Lo único que este servidor sabe del sistema: mirar. No implementa
    /// `Sistema`, así que desde aquí no se puede mutar nada (solo proponer).
    consulta: Arc<SoloConsulta>,
    /// La bóveda. Aquí solo se escribe en la región del propio agente
    /// ([`Boveda::escribir_nota`]); promover a la compartida es del usuario y
    /// este servidor solo puede proponerlo.
    boveda: Boveda,
    /// De dónde se leen los roles vigentes en cada llamada.
    ajustes: ea_ajustes::Fuentes,
    /// Dónde mira la biblioteca; `None`: el entorno real en cada llamada.
    raices: Option<ea_biblioteca::Raices>,
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

/// Resumen de un trabajo del bus, o `None` si sus ficheros no se pueden leer.
fn trabajo_resumen(bus: &Bus, id: &str) -> Option<serde_json::Value> {
    let orden = bus.leer_orden(id).ok()?;
    let estado = bus.leer_estado(id).ok()?;
    Some(json!({
        "id": id,
        "modo": orden.modo.as_str(),
        "titulo": orden.titulo,
        "estado": estado.estado.as_str(),
        "fase_actual": estado.fase_actual,
        "fases": orden.fases.len(),
        "iteracion": estado.iteracion,
        "actualizado": estado.actualizado_en,
        "creado_por": orden.creada_por,
    }))
}

fn event_to_json(e: &Event) -> serde_json::Value {
    json!({
        "id": e.id,
        "cuando": fmt_ts(e.ts),
        "actor": e.actor,
        "accion": e.action,
        "trabajo": e.trabajo,
        "resultado": e.outcome.map(|o| o.as_str()),
        "duracion_ms": e.duration_ms,
        "detalle": e.detail,
    })
}

/// Trabajos por estado, colas y semáforos del bus. Si el bus no se puede
/// leer (aún no existe), todo vacío: un `ea_status` no debe fallar por eso.
fn resumen_bus() -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let mut por_estado = serde_json::Map::new();
    let mut colas = serde_json::Map::new();
    if let Ok(bus) = Bus::abrir() {
        for id in bus.listar_trabajos().unwrap_or_default() {
            if let Ok(e) = bus.leer_estado(&id) {
                let n = por_estado.entry(e.estado.as_str()).or_insert(json!(0));
                *n = json!(n.as_i64().unwrap_or(0) + 1);
            }
        }
        for a in Rol::TODOS {
            let ids: Vec<_> = bus.listar_cola(a).unwrap_or_default().into_iter().map(|e| e.trabajo).collect();
            colas.insert(a.as_str().into(), json!(ids));
        }
    }
    let semaforos: Vec<_> = ea_bus::listar_semaforos()
        .into_iter()
        .map(|(_, o)| json!({ "agente": o.agente, "trabajo": o.trabajo, "pid": o.pid, "desde": o.desde }))
        .collect();
    (json!(por_estado), json!(colas), json!(semaforos))
}

/// Los equipos despiertos y las presencias anotadas (vivas o caducadas), para
/// `ea_status`. Vacío si el bus no se puede leer.
fn resumen_equipos() -> (serde_json::Value, serde_json::Value) {
    let Ok(bus) = Bus::abrir() else { return (json!([]), json!([])) };
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

fn respond(body: ToolBody) -> Result<CallToolResult, McpError> {
    match body {
        Ok(value) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        )])),
        Err(msg) => Err(McpError::internal_error(msg, None)),
    }
}

/// Cotas de `ea_biblioteca`: (límite por defecto, máximo, largo de la
/// descripción). El modelo local, con 8192 tokens de contexto, recibe las más
/// estrechas, como en la bóveda.
const COTAS_BIBLIOTECA: (usize, usize, usize) = (50, 200, 200);
const COTAS_BIBLIOTECA_LOCAL: (usize, usize, usize) = (15, 40, 80);

/// El cuerpo de `ea_biblioteca`: la ficha de un id o el listado filtrado y
/// acotado. Todo sale del inventario, que ya viene redactado.
fn biblioteca_json(
    args: &BibliotecaArgs,
    raices: &ea_biblioteca::Raices,
    ajustes: &ea_ajustes::Ajustes,
    (por_defecto, maximo, largo): (usize, usize, usize),
) -> ToolBody {
    use ea_biblioteca::uso::{calcular_uso, FuentesUso};
    use ea_biblioteca::{Activado, Biblioteca, Filtro, Tipo};

    let inv = Biblioteca::cargar(raices, ajustes);
    let informe = calcular_uso(raices, &inv, &FuentesUso::desde_raices(raices));
    let uso_vacio = json!({ "total": 0, "ultimo": null, "por": [] });
    if let Some(id) = args.id.as_deref().map(str::trim).filter(|i| !i.is_empty()) {
        let e = inv.buscar(id).ok_or_else(|| format!("no existe el elemento {id:?} (lista sin id para ver los que hay)"))?;
        let uso = informe.buscar(id).map_or(uso_vacio, |u| json!(u));
        return Ok(json!({ "elemento": e, "uso": uso }));
    }
    let tipo = match args.tipo.as_deref() {
        Some(t) => Some(Tipo::desde(t).ok_or_else(|| format!("tipo desconocido {t:?} (uno de: {})", Tipo::TODOS.map(|t| t.as_str()).join(", ")))?),
        None => None,
    };
    let filtro = Filtro {
        agente: args.agente.clone(),
        tipo,
        categoria: args.categoria.clone(),
        activado: args.activado.map(Activado::desde_bool),
    };
    let limite = args.limite.filter(|n| *n > 0).map_or(por_defecto, |n| (n as usize).min(maximo));
    let todos = inv.filtrar(&filtro);
    let elementos: Vec<_> = todos
        .iter()
        .take(limite)
        .map(|e| {
            let descripcion: String = e.descripcion.chars().take(largo).collect();
            json!({
                "id": e.id, "agente": e.agente, "tipo": e.tipo, "ambito": e.ambito, "nombre": e.nombre,
                "categoria": e.categoria, "activado": e.activado, "mecanismo": e.mecanismo,
                "descripcion": descripcion,
                "usos": informe.buscar(&e.id).map_or(0, |u| u.total),
            })
        })
        .collect();
    Ok(json!({
        "total": todos.len(),
        "mostrados": elementos.len(),
        "elementos": elementos,
        "avisos": inv.avisos.iter().take(20).collect::<Vec<_>>(),
    }))
}

#[tool_router]
impl EaTools {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self::con_rutinas_dir(store, ea_ajustes::cargar().orquestador.rutinas_dir)
    }

    /// Con la bóveda configurada (`EA_BOVEDA_DIR`, `boveda.toml`).
    pub fn con_rutinas_dir(store: Arc<Mutex<Store>>, rutinas_dir: std::path::PathBuf) -> Self {
        Self::con_entorno(store, rutinas_dir, Boveda::configurada())
    }

    pub fn con_entorno(store: Arc<Mutex<Store>>, rutinas_dir: std::path::PathBuf, boveda: Boveda) -> Self {
        Self {
            store,
            consulta: Arc::new(SoloConsulta { rutinas_dir, boveda: boveda.clone() }),
            boveda,
            ajustes: ea_ajustes::Fuentes::sistema(),
            raices: None,
            tool_router: Self::tool_router(),
        }
    }

    /// La biblioteca mira estas raíces en vez del entorno real (pruebas).
    pub fn con_raices(mut self, raices: ea_biblioteca::Raices) -> Self {
        self.raices = Some(raices);
        self
    }

    /// Lee los roles de estos ajustes en vez de los del sistema (pruebas).
    pub fn con_ajustes(mut self, fuentes: ea_ajustes::Fuentes) -> Self {
        self.ajustes = fuentes;
        self
    }

    /// Quién llama, con los roles de ahora: cambiar de planificador no exige
    /// reconectar.
    fn llamante(&self, context: &RequestContext<RoleServer>) -> Llamante {
        let ajustes = ea_ajustes::cargar_de(&self.ajustes).ajustes;
        actor_for(context.client_info().as_ref().map(|i| i.name.as_str()), &ajustes)
    }

    /// Cuerpo común de las herramientas mutantes supervisadas (§11.1): el
    /// agente nunca ejecuta. Con `simular` obtiene la descripción de lo que
    /// pasaría; sin ella se encola una propuesta con acción estructurada que
    /// solo el usuario puede aceptar (`ea propuestas decidir`, panel web).
    /// No existe ninguna herramienta ni token con el que un agente confirme.
    async fn mutante(
        &self,
        context: RequestContext<RoleServer>,
        nombre: &'static str,
        args: serde_json::Value,
        simular: bool,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let actor = quien.actor;
        let started = Instant::now();
        let mut args_log = args.clone();
        args_log["simular"] = json!(simular);

        let body: ToolBody = async {
            let h = buscar(nombre).ok_or_else(|| format!("herramienta desconocida: {nombre}"))?;
            autorizar_mcp(actor, h)?;
            // Lo asíncrono (preguntar a systemd) antes de tomar la base de datos.
            let hechos = observar(&*self.consulta, nombre, &args).await.map_err(|e| e.to_string())?;
            let store = self.store.lock().await;
            if simular {
                let sim = simular_con(&store, &*self.consulta, &hechos, nombre, &args)
                    .map_err(|e| e.to_string())?;
                return Ok(json!({
                    "simulado": true,
                    "herramienta": nombre,
                    "riesgo": sim.riesgo.as_str(),
                    "argumentos": sim.argumentos,
                    "simulacion": sim.texto,
                    "aviso": "No se ha hecho nada. Para pedirlo, repite sin simular: se encolará una \
                              propuesta que el usuario decide.",
                }));
            }
            let (p, nueva) = proponer(&store, &*self.consulta, &hechos, actor, None, nombre, &args)
                .map_err(|e| e.to_string())?;
            let a = p.accion.as_ref().ok_or("propuesta sin acción")?;
            let donde = if a.riesgo == Riesgo::Irreversible.as_str() {
                format!("Es irreversible: solo se confirma desde una terminal (`ea propuestas decidir {} --aceptar`).", p.id)
            } else {
                format!("El usuario la acepta con `ea propuestas decidir {} --aceptar` o en el panel web (◇).", p.id)
            };
            Ok(json!({
                "ejecutado": false,
                "propuesta": { "id": p.id, "estado": p.estado.as_str(), "nueva": nueva },
                "herramienta": nombre,
                "riesgo": a.riesgo,
                "argumentos": a.argumentos,
                "simulacion": a.simulacion,
                "aviso": format!("NO se ha ejecutado nada: queda pendiente de la confirmación del usuario. {donde}"),
            }))
        }
        .await;

        let accion = format!(
            "mcp_{}{}",
            nombre.trim_start_matches("ea_"),
            if simular { "_simulada" } else { "" }
        );
        self.log_invocation(&quien, &accion, None, started, args_log, &body).await;
        respond(body)
    }

    /// Registra en el diario una invocación de herramienta: quién, qué
    /// verbo, con qué argumentos, con qué resultado y cuánto tardó.
    /// Independiente del nivel de riesgo de la herramienta (§11.3).
    async fn log_invocation(
        &self,
        quien: &Llamante,
        action: &str,
        trabajo: Option<&str>,
        started: Instant,
        args: serde_json::Value,
        body: &ToolBody,
    ) {
        let duration_ms = started.elapsed().as_millis() as i64;
        let mut detail = match body {
            Ok(_) => json!({ "args": args }),
            Err(msg) => json!({ "args": args, "error": msg }),
        };
        // El agente real detrás del rol; de un cliente desconocido, el nombre
        // que declaró (así se sabe cuál añadir al catálogo).
        match (quien.agente, &quien.cliente) {
            (Some(a), _) => detail["agente"] = json!(a),
            (None, Some(c)) if quien.actor == ACTOR_DESCONOCIDO => detail["cliente"] = json!(c),
            _ => {}
        }
        let actor = quien.actor;
        let outcome = if body.is_ok() {
            EventOutcome::Ok
        } else {
            EventOutcome::Error
        };
        let store = self.store.lock().await;
        let _ = store.append_event(NewEvent {
            actor: actor.to_string(),
            action: action.to_string(),
            trabajo: trabajo.map(str::to_string),
            outcome: Some(outcome),
            duration_ms: Some(duration_ms),
            detail: Some(detail.to_string()),
        });
    }

    #[tool(
        description = "Resumen del estado del ecosistema: orquestador, trabajos \
            del bus por estado, colas, semáforos de worktree, equipos despiertos con la presencia de los agentes y los últimos eventos del diario. Primer vistazo antes \
            de profundizar con las otras herramientas.",
        annotations(title = "Estado del ecosistema", read_only_hint = true)
    )]
    async fn ea_status(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let started = Instant::now();

        let body: ToolBody = async {
            let store = self.store.lock().await;
            let (por_estado, colas, semaforos) = resumen_bus();
            let (equipos, presencias) = resumen_equipos();
            let events = store
                .list_events(&EventFilter {
                    limit: Some(5),
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?;
            // Mismo criterio que `ea status` y `ea-web`: último latido del
            // diario, con umbral del doble del intervalo configurado.
            let umbral_s = ea_ajustes::cargar().umbral_latido_s();
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
                "trabajos_por_estado": por_estado,
                "colas": colas,
                "semaforos": semaforos,
                "equipos": equipos,
                "presencias": presencias,
                "ultimos_eventos": events.iter().map(event_to_json).collect::<Vec<_>>(),
            }))
        }
        .await;

        self.log_invocation(&quien, "mcp_status", None, started, json!({}), &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Lista trabajos del bus (misiones, obras y rutinas), \
            opcionalmente filtrados por estado o modo. Los más recientes primero. \
            Solo lectura: crear o decidir trabajo se hace con la CLI (`ea trabajo`) \
            y las skills /mision, /obra, /rutina y /revisar.",
        annotations(title = "Listar trabajos", read_only_hint = true)
    )]
    async fn ea_listar_trabajos(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarTrabajosArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let started = Instant::now();
        let args_json = json!({ "estado": args.estado, "modo": args.modo, "limite": args.limite });

        let body: ToolBody = async {
            let bus = Bus::abrir().map_err(|e| e.to_string())?;
            let mut ids = bus.listar_trabajos().map_err(|e| e.to_string())?;
            ids.reverse();
            let limite = args.limite.filter(|n| *n > 0).unwrap_or(20) as usize;
            let trabajos: Vec<_> = ids
                .iter()
                .filter_map(|id| trabajo_resumen(&bus, id))
                .filter(|t| args.estado.as_deref().is_none_or(|e| t["estado"] == e))
                .filter(|t| args.modo.as_deref().is_none_or(|m| t["modo"] == m))
                .take(limite)
                .collect();
            Ok(json!({ "trabajos": trabajos }))
        }
        .await;

        self.log_invocation(&quien, "mcp_listar_trabajos", None, started, args_json, &body)
            .await;
        respond(body)
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
        let quien = self.llamante(&context);
        let started = Instant::now();
        let args_json = json!({ "id": args.id });

        let body: ToolBody = async {
            let bus = Bus::abrir().map_err(|e| e.to_string())?;
            let orden = bus.leer_orden(&args.id).map_err(|e| e.to_string())?;
            let estado = bus.leer_estado(&args.id).map_err(|e| e.to_string())?;
            let informe = bus.leer_informe(&args.id).map_err(|e| e.to_string())?;
            let store = self.store.lock().await;
            let mut eventos = store.list_events_by_trabajo(&args.id).map_err(|e| e.to_string())?;
            eventos.reverse();
            Ok(json!({
                "orden": orden,
                "estado": estado,
                "informe": informe,
                "eventos": eventos.iter().map(event_to_json).collect::<Vec<_>>(),
            }))
        }
        .await;

        self.log_invocation(&quien, "mcp_ver_trabajo", Some(&args.id), started, args_json, &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Lista los equipos de agentes (spec 019) con su estado, sus \
            miembros (agente, rol y si están despiertos) y cuántos trabajos tienen; \
            los más recientes primero. Solo lectura: despertar o dormir un equipo \
            es del usuario (`ea equipo`).",
        annotations(title = "Listar equipos", read_only_hint = true)
    )]
    async fn ea_listar_equipos(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let started = Instant::now();

        let body: ToolBody = async {
            let bus = Bus::abrir().map_err(|e| e.to_string())?;
            let mut equipos = bus.resumen_equipos().map_err(|e| e.to_string())?;
            equipos.reverse();
            let total = equipos.len();
            equipos.truncate(MAX_EQUIPOS);
            Ok(json!({ "equipos": equipos, "total": total }))
        }
        .await;

        self.log_invocation(&quien, "mcp_listar_equipos", None, started, json!({}), &body).await;
        respond(body)
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
        let quien = self.llamante(&context);
        let started = Instant::now();
        let args_json = json!({ "id": args.id, "trabajos": args.trabajos });

        let body: ToolBody = async {
            let bus = Bus::abrir().map_err(|e| e.to_string())?;
            let n = args.trabajos.filter(|n| *n > 0).map_or(TRABAJOS_POR_DEFECTO, |n| (n as usize).min(MAX_TRABAJOS));
            let v = bus.vista_equipo(&args.id, n).map_err(|e| e.to_string())?;
            serde_json::to_value(v).map_err(|e| e.to_string())
        }
        .await;

        self.log_invocation(&quien, "mcp_ver_equipo", None, started, args_json, &body).await;
        respond(body)
    }

    #[tool(
        description = "Lista eventos del diario, opcionalmente filtrados por \
            trabajo, actor o rango de fechas. Los más recientes primero.",
        annotations(title = "Listar diario", read_only_hint = true)
    )]
    async fn ea_listar_diario(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListarDiarioArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let started = Instant::now();
        let args_json = json!({
            "trabajo": args.trabajo, "actor": args.actor,
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
                trabajo: args.trabajo.clone(),
                actor: args.actor.clone(),
                since,
                until,
                limit: Some(args.limite.unwrap_or(20)),
                ..Default::default()
            };
            let store = self.store.lock().await;
            let events = store.list_events(&filter).map_err(|e| e.to_string())?;
            Ok(json!({ "eventos": events.iter().map(event_to_json).collect::<Vec<_>>() }))
        }
        .await;

        self.log_invocation(&quien, "mcp_listar_diario", args.trabajo.as_deref(), started, args_json, &body)
            .await;
        respond(body)
    }

    #[tool(
        description = "Inventario de herramientas de los agentes (skills, plugins, \
            subagentes, servidores MCP, comandos y extensiones de Claude Code, Qwen \
            Code, Codex, OpenCode y Antigravity, más los del ecosistema) con su \
            agente, ámbito, categoría, si están activados y cuánto se usan. Sin \
            'id', lista con filtros (agente, tipo, categoria, activado, limite); \
            con 'id' devuelve la ficha de ese elemento (definición MCP sin secretos) \
            y su uso por actor. Solo lectura: activar, desactivar y abrir sesiones \
            acotadas se pide con ea_activar_herramienta, ea_desactivar_herramienta y \
            ea_lanzar_sesion, que solo proponen.",
        annotations(title = "Biblioteca de herramientas", read_only_hint = true)
    )]
    async fn ea_biblioteca(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<BibliotecaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let started = Instant::now();
        let args_json = json!({
            "id": args.id, "agente": args.agente, "tipo": args.tipo, "categoria": args.categoria,
            "activado": args.activado, "limite": args.limite,
        });
        let body: ToolBody = async {
            let h = buscar("ea_biblioteca").ok_or("herramienta desconocida: ea_biblioteca")?;
            autorizar_mcp(quien.actor, h)?;
            let raices = self.raices.clone().unwrap_or_else(ea_biblioteca::Raices::desde_entorno);
            let ajustes = ea_ajustes::cargar_de(&self.ajustes).ajustes;
            // Leer el disco y la base del diario no es asunto del ejecutor async.
            let cotas = if quien.actor == "modelo-local" { COTAS_BIBLIOTECA_LOCAL } else { COTAS_BIBLIOTECA };
            tokio::task::spawn_blocking(move || biblioteca_json(&args, &raices, &ajustes, cotas))
                .await
                .map_err(|e| e.to_string())?
        }
        .await;
        self.log_invocation(&quien, "mcp_biblioteca", None, started, args_json, &body).await;
        respond(body)
    }

    /// Autoriza una herramienta del catálogo por el actor y, si falla, deja la
    /// denegación en el diario (§11.3). `Ok(())` si puede seguir.
    async fn autorizar(
        &self,
        quien: &Llamante,
        nombre: &str,
        accion: &str,
        started: Instant,
        args: &serde_json::Value,
    ) -> Result<(), McpError> {
        let h = buscar(nombre).expect("en el catálogo");
        if let Err(msg) = autorizar_mcp(quien.actor, h) {
            let body: ToolBody = Err(msg.clone());
            self.log_invocation(quien, &format!("{accion}_denegada"), None, started, args.clone(), &body).await;
            return Err(McpError::internal_error(msg, None));
        }
        Ok(())
    }

    #[tool(
        description = "Busca notas en la bóveda por palabras (todas deben aparecer) \
            en las regiones modelo-local, claude-code y compartida. Devuelve pocos \
            resultados, con un fragmento; lee la nota entera con ea_leer_nota. Lo de \
            la región compartida es contenido que el usuario promovió.",
        annotations(title = "Buscar notas", read_only_hint = true)
    )]
    async fn ea_buscar_notas(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<BuscarNotasArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let actor = quien.actor;
        let started = Instant::now();
        let args_json = json!({ "consulta": args.consulta, "region": args.region, "limite": args.limite });
        let body: ToolBody = async {
            let lim = Limites::para_actor(actor);
            let pedido = args.limite.filter(|n| *n > 0).map(|n| n as usize);
            let mut store = self.store.lock().await;
            let r = self
                .boveda
                .buscar(&mut store, &args.consulta, args.region.as_deref(), pedido, &lim)
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "total": r.len(),
                "maximo_por_busqueda": lim.max_resultados,
                "resultados": r.iter().map(|x| json!({
                    "ruta": x.ruta, "titulo": x.titulo, "region": x.region, "autor": x.autor,
                    "actualizada": x.actualizada, "fragmento": x.fragmento,
                })).collect::<Vec<_>>(),
            }))
        }
        .await;
        self.log_invocation(&quien, "mcp_buscar_notas", None, started, args_json, &body).await;
        respond(body)
    }

    #[tool(
        description = "Lee una nota de la bóveda por su ruta (la que devuelve \
            ea_buscar_notas). Las notas largas se devuelven por trozos: si \
            'siguiente_desde' no es null, repite con desde=ese valor.",
        annotations(title = "Leer nota", read_only_hint = true)
    )]
    async fn ea_leer_nota(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<LeerNotaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let actor = quien.actor;
        let started = Instant::now();
        let args_json = json!({ "ruta": args.ruta, "desde": args.desde });
        let body: ToolBody = async {
            let lim = Limites::para_actor(actor);
            let desde = args.desde.filter(|n| *n > 0).unwrap_or(0) as usize;
            let n = self.boveda.leer(&args.ruta, desde, &lim).map_err(|e| e.to_string())?;
            Ok(json!({
                "ruta": n.ruta,
                "region": n.region,
                "titulo": n.cabecera.as_ref().map(|c| c.titulo.clone()),
                "autor": n.cabecera.as_ref().map(|c| c.autor.clone()),
                "origen": n.cabecera.as_ref().map(|c| c.origen.clone()),
                "promocion": n.cabecera.as_ref().map(|c| c.promocion.clone()),
                "actualizada": n.cabecera.as_ref().map(|c| c.actualizada.clone()),
                "tags": n.cabecera.as_ref().map(|c| c.tags.clone()),
                "aviso_cabecera": n.aviso_cabecera,
                "desde": n.desde,
                "total_caracteres": n.total_caracteres,
                "truncado": n.truncado,
                "siguiente_desde": n.siguiente_desde,
                "contenido": n.contenido,
            }))
        }
        .await;
        self.log_invocation(&quien, "mcp_leer_nota", None, started, args_json, &body).await;
        respond(body)
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Escribe una nota en TU región de la \
            bóveda (libertad total ahí; no se puede escribir en otra región ni en la \
            compartida). La cabecera la pone el sistema. Con simular=true solo valida y \
            dice dónde caería. Para llevarla a la región compartida usa ea_promover_nota.",
        annotations(
            title = "Escribir nota",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_escribir_nota(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<EscribirNotaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let quien = self.llamante(&context);
        let actor = quien.actor;
        let started = Instant::now();
        let simular = args.simular.unwrap_or(false);
        let args_json = json!({
            "titulo": args.titulo, "bytes": args.contenido.len(), "ruta": args.ruta,
            "etiquetas": args.etiquetas, "trabajo": args.trabajo, "sobrescribir": args.sobrescribir,
            "simular": simular,
        });
        self.autorizar(&quien, "ea_escribir_nota", "mcp_escribir_nota", started, &args_json).await?;
        let origen = args.trabajo.as_ref().map(|t| format!("trabajo:{t}")).unwrap_or_else(|| "manual".into());
        let etiquetas = args.etiquetas.clone().unwrap_or_default();
        let nota = NuevaNota {
            titulo: &args.titulo,
            cuerpo: &args.contenido,
            ruta: args.ruta.as_deref(),
            tags: &etiquetas,
            origen: &origen,
            sobrescribir: args.sobrescribir.unwrap_or(false),
        };
        let lim = Limites::para_actor(actor);
        if simular {
            let body: ToolBody = self
                .boveda
                .simular_escritura(actor, &nota, &lim)
                .map(|(ruta, existe)| {
                    json!({
                        "simulado": true, "ruta": ruta, "existe": existe,
                        "aviso": "No se ha escrito nada. Repite sin simular para escribirla.",
                    })
                })
                .map_err(|e| e.to_string());
            self.log_invocation(&quien, "mcp_escribir_nota_simulada", args.trabajo.as_deref(), started, args_json, &body).await;
            return respond(body);
        }
        let body: ToolBody = {
            let mut store = self.store.lock().await;
            self.boveda
                .escribir_nota(&mut store, actor, &nota, &lim)
                .map(|n| {
                    json!({
                        "escrita": true, "ruta": n.ruta, "region": n.region, "bytes": n.bytes,
                        "creada": n.creada,
                        "aviso": "Está en tu región. Llevarla a la región compartida lo decide el \
                                  usuario: propónlo con ea_promover_nota.",
                    })
                })
                .map_err(|e| e.to_string())
        };
        // Si tuvo éxito, el escritor ya dejó su evento
        // `boveda_escritura` (actor, ruta, bytes, huella); duplicarlo aquí
        // contradiría "una sola fuente de verdad". Si falló, se anota.
        if body.is_err() {
            self.log_invocation(&quien, "mcp_escribir_nota", args.trabajo.as_deref(), started, args_json, &body).await;
        }
        respond(body)
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Propone copiar una nota de TU región \
            a la región compartida de la bóveda. NO la copia: con simular=true lo describe; \
            sin él encola una propuesta que solo el usuario puede aceptar (la promoción \
            la decide el usuario, nunca un agente).",
        annotations(
            title = "Proponer promoción de nota",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_promover_nota(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<PromoverNotaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut a = json!({ "ruta": args.ruta });
        if let Some(d) = args.destino {
            a["destino"] = json!(d);
        }
        self.mutante(context, "ea_promover_nota", a, args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Lanza una rutina registrada: crea un \
            trabajo en el bus que ejecuta el agente local en el worktree de la rutina (puede \
            escribir en él). Acepta `entradas` opcionales que sustituyen a las de por defecto. \
            NO lo ejecuta: con simular=true lo describe, con el criterio resultante; sin él \
            encola una propuesta que el usuario debe aceptar.",
        annotations(
            title = "Encolar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_encolar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<EncolarRutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut a = json!({ "slug": args.slug });
        if let Some(e) = args.entradas.filter(|e| !e.is_empty()) {
            a["entradas"] = json!(e);
        }
        self.mutante(context, "ea_encolar_rutina", a, args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Registra una rutina nueva o una versión \
            nueva de una registrada (p. ej. la corrección tras un paquete de corrección), con \
            su definición TOML completa: la versión debe ser mayor que la registrada. No lanza \
            la rutina. NO lo registra: con simular=true lo describe, con el diff frente a la \
            versión registrada y el criterio resultante; sin él encola una propuesta que el \
            usuario debe aceptar (terminal o panel web).",
        annotations(
            title = "Registrar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_registrar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<RegistrarRutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_registrar_rutina", json!({ "toml": args.toml }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Programa una rutina registrada con un \
            timer de systemd --user (ea-rutina@<slug>.timer con OnCalendar=<calendario>): a \
            cada hora el timer solo crea el trabajo de la rutina (ea rutina lanzar), que \
            ejecuta el agente local. Si ya tenía horario, lo sustituye. NO lo programa: con \
            simular=true valida el calendario con systemd-analyze y describe lo que haría, \
            con las próximas 3 ejecuciones; sin él encola una propuesta que el usuario debe \
            aceptar (terminal o panel web). Se deshace con ea_desprogramar_rutina.",
        annotations(
            title = "Programar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_programar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ProgramarRutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        let a = json!({ "slug": args.slug, "calendario": args.calendario });
        self.mutante(context, "ea_programar_rutina", a, args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Abre en una terminal del usuario un agente \
            (claude-code, qwen-code, codex u opencode) con SOLO las herramientas elegidas de la \
            biblioteca (ids `agente/tipo/ámbito/nombre`). Los secretos de los MCP van a ficheros \
            0600 efímeros, nunca a argumentos. NO lo abre: con simular=true describe el comando y \
            los ficheros (sin secretos); sin él encola una propuesta que el usuario debe aceptar \
            (terminal o panel web). No está disponible para el agente local.",
        annotations(
            title = "Lanzar sesión con herramientas",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_lanzar_sesion(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<LanzarSesionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut a = json!({ "agente": args.agente, "herramientas": args.herramientas });
        if let Some(d) = args.directorio {
            a["directorio"] = json!(d);
        }
        self.mutante(context, "ea_lanzar_sesion", a, args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Quita el timer de systemd de una rutina \
            (systemctl --user disable --now ea-rutina@<slug>.timer y borra su fichero). La \
            rutina sigue registrada. NO lo hace: con simular=true lo describe; sin él encola \
            una propuesta que el usuario debe aceptar (terminal o panel web).",
        annotations(
            title = "Desprogramar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn ea_desprogramar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<RutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_desprogramar_rutina", json!({ "slug": args.slug }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Reinicia un servicio del ecosistema \
            (lista cerrada: llama-server, ea-mcp, ea-orchestrator, ea-web). \
            NO lo ejecuta: con simular=true lo describe; sin él encola una propuesta \
            que el usuario debe aceptar.",
        annotations(
            title = "Reiniciar servicio",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_reiniciar_servicio(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<ReiniciarServicioArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_reiniciar_servicio", json!({ "servicio": args.servicio }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Retira una rutina: mueve su TOML a la \
            papelera de rutinas_dir y desprograma su timer, si lo tiene. Deja de listarse y de \
            poder lanzarse; sus versiones siguen en SQLite. Se deshace con ea_restaurar_rutina. \
            NO lo hace: con simular=true lo describe; sin él encola una propuesta que el \
            usuario debe aceptar (terminal o panel web).",
        annotations(
            title = "Retirar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_retirar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<RutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_retirar_rutina", json!({ "slug": args.slug }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Restaura una rutina retirada: devuelve su \
            TOML de la papelera a rutinas_dir, y vuelve a listarse y a poder lanzarse. No \
            vuelve a programar su timer (eso es ea_programar_rutina). NO lo hace: con \
            simular=true lo describe; sin él encola una propuesta que el usuario debe aceptar \
            (terminal o panel web).",
        annotations(
            title = "Restaurar rutina",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_restaurar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<RutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_restaurar_rutina", json!({ "slug": args.slug }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Activa un elemento de la biblioteca de \
            herramientas (skill, plugin, subagente, MCP, comando o extensión de un agente) \
            por su id `agente/tipo/ámbito/nombre`. NO lo activa: con simular=true describe \
            qué fichero o comando se usaría y el valor antes y después; sin él encola una \
            propuesta que el usuario debe aceptar (terminal o panel web). Se deshace con \
            ea_desactivar_herramienta.",
        annotations(
            title = "Activar herramienta",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_activar_herramienta(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<HerramientaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_activar_herramienta", json!({ "elemento": args.elemento }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación reversible] Desactiva un elemento de la biblioteca de \
            herramientas (skill, plugin, subagente, MCP, comando o extensión de un agente) \
            por su id `agente/tipo/ámbito/nombre`. NO lo desactiva: con simular=true describe \
            qué fichero o comando se usaría y el valor antes y después; sin él encola una \
            propuesta que el usuario debe aceptar (terminal o panel web). Se deshace con \
            ea_activar_herramienta.",
        annotations(
            title = "Desactivar herramienta",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_desactivar_herramienta(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<HerramientaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_desactivar_herramienta", json!({ "elemento": args.elemento }), args.simular.unwrap_or(false)).await
    }

    #[tool(
        description = "[Riesgo: mutación IRREVERSIBLE] Borra para siempre la copia retirada de \
            una rutina (la de la papelera; antes hay que retirarla con ea_retirar_rutina). Sus \
            versiones siguen en SQLite. NO lo ejecuta: con simular=true lo describe; sin él \
            encola una propuesta que el usuario solo puede confirmar desde una terminal.",
        annotations(
            title = "Eliminar rutina",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false
        )
    )]
    async fn ea_eliminar_rutina(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<RutinaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_eliminar_rutina", json!({ "slug": args.slug }), args.simular.unwrap_or(false)).await
    }
}

#[tool_handler]
impl ServerHandler for EaTools {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Herramientas del ecosistema de asistente personal. ea_status da \
                 una vista general (orquestador, trabajos, colas y semáforos); \
                 ea_listar_equipos y ea_ver_equipo muestran los equipos de agentes \
                 y su presencia (solo lectura: despertarlos o dormirlos es del usuario); \
                 ea_listar_trabajos y ea_ver_trabajo inspeccionan los trabajos del \
                 bus (solo lectura: crear o decidir trabajo se hace con la CLI y las \
                 skills); ea_listar_diario consulta el historial de eventos; \
                 ea_biblioteca lista las herramientas de los agentes (skills, plugins, \
                 subagentes, MCP) con su estado y su uso. Las \
                 herramientas que cambian el sistema (ea_encolar_rutina, \
                 ea_registrar_rutina, ea_programar_rutina, ea_desprogramar_rutina, \
                 ea_retirar_rutina, ea_restaurar_rutina, ea_reiniciar_servicio, \
                 ea_eliminar_rutina, ea_promover_nota, ea_activar_herramienta, \
                 ea_desactivar_herramienta, ea_lanzar_sesion) nunca se \
                 ejecutan desde aquí: con simular=true describen lo que harían; sin él \
                 encolan una propuesta que solo el usuario puede aceptar. Las \
                 irreversibles solo se confirman desde una terminal. La bóveda de \
                 notas se consulta con ea_buscar_notas y ea_leer_nota; ea_escribir_nota \
                 escribe en la región propia del agente; la región compartida solo \
                 recibe notas que el usuario promueve."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_actor_sale_del_catalogo_y_del_rol() {
        let mut a = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(std::path::Path::new("/nada")));
        let actor = |c: Option<&str>, a: &ea_ajustes::Ajustes| actor_for(c, a);
        assert_eq!(actor(Some("claude-code"), &a).actor, "claude-code");
        assert_eq!(actor(Some("qwen-code-mcp-client"), &a), Llamante { actor: "modelo-local", agente: Some("qwen-code"), cliente: Some("qwen-code-mcp-client".into()) });
        assert_eq!(actor(Some("ea-orchestrator"), &a).actor, "orquestador");
        assert_eq!(actor(Some("codex-mcp-client"), &a).actor, ACTOR_SIN_ROL);
        assert_eq!(actor(Some("antigravity-client"), &a).actor, ACTOR_SIN_ROL);
        assert_eq!(actor(Some("lo-que-sea"), &a).actor, ACTOR_DESCONOCIDO);
        assert_eq!(actor(None, &a).actor, ACTOR_DESCONOCIDO, "antes era claude-code");
        a.roles.planificador = "opencode".into();
        assert_eq!(actor(Some("opencode"), &a), Llamante { actor: "claude-code", agente: Some("opencode"), cliente: Some("opencode".into()) });
        assert_eq!(actor(Some("claude-code"), &a).actor, ACTOR_SIN_ROL);
        a.roles.planificador = "antigravity".into();
        assert_eq!(actor(Some("antigravity-client"), &a), Llamante { actor: "claude-code", agente: Some("antigravity"), cliente: Some("antigravity-client".into()) });
    }
}
