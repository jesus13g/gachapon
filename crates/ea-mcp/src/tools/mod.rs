//! Catálogo de herramientas MCP: consultas de solo lectura sobre el diario y
//! el bus de trabajos, la bóveda y las mutaciones supervisadas. Crear o
//! decidir trabajo se hace con la CLI y las skills, no desde aquí. Cada invocación queda registrada en el diario de
//! eventos (§11.3 de la especificación), independientemente de si tuvo
//! éxito o de su nivel de riesgo.
//!
//! Cada área está en su módulo (consultas, trabajos, diario, bóveda,
//! biblioteca y mutantes) y todas pasan por la misma plantilla
//! ([`EaTools::registrada`]). Lo que lee el disco o SQLite corre fuera del
//! hilo del runtime ([`EaTools::bloqueante`]).

use std::sync::Arc;
use std::time::Instant;

use ea_store::{EventOutcome, NewEvent, Store};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, tool_handler};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use ea_acciones::{autorizar_mcp, buscar, SoloConsulta};
use ea_boveda::Boveda;

use crate::credencial::Credencial;

mod biblioteca;
mod boveda;
mod consultas;
mod diario;
mod mutantes;
mod trabajos;

/// Quién hace una llamada MCP: el actor que se escribe en el diario y del
/// que salen los permisos ([`autorizar_mcp`]), el agente del catálogo y el
/// nombre que declaró el cliente. Sale de la credencial de la petición y del
/// `clientInfo.name` del `initialize` ([`actor_for`]), nunca de un argumento:
/// el modelo no debe poder declarar que actúa en nombre de otro (§7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Llamante {
    pub actor: &'static str,
    pub agente: Option<&'static str>,
    pub cliente: Option<String>,
}

/// Los actores de un agente del catálogo que no ocupa ningún rol y de un
/// cliente que no es ningún agente: solo leen.
pub use ea_ajustes::{ACTOR_DESCONOCIDO, ACTOR_SIN_ROL};

/// El actor de un cliente según el catálogo, los roles vigentes (spec 017,
/// RF-013) y si la petición trae la credencial del planificador (U-02). Un
/// agente actúa con el nombre en disco del rol que ocupa (`claude-code` el
/// planificador, `modelo-local` el ejecutor), que es el que da los permisos y
/// la región de la bóveda; uno sin rol, o un cliente desconocido, solo lee.
///
/// El nombre lo declara el propio cliente, así que solo vale con
/// `credencial` (la cabecera `X-EA-Credencial` correcta): sin ella, o con una
/// que no coincide, el actor es `desconocido`. La única excepción es el
/// agente que ocupa el ejecutor: es el rol que menos puede de los que actúan
/// y el que corre en el sandbox, donde la credencial no se puede leer, así
/// que hacerse pasar por él no da nada que un proceso sin sandbox no tenga ya.
pub fn actor_for(cliente: Option<&str>, credencial: bool, ajustes: &ea_ajustes::Ajustes) -> Llamante {
    let cliente_s = cliente.map(str::to_string);
    let desconocido = Llamante { actor: ACTOR_DESCONOCIDO, agente: None, cliente: cliente_s.clone() };
    if cliente == Some("ea-orchestrator") {
        if !credencial {
            return desconocido;
        }
        return Llamante { actor: ea_ajustes::ACTOR_ORQUESTADOR, agente: None, cliente: cliente_s };
    }
    let Some(f) = cliente.and_then(ea_ajustes::por_cliente_mcp) else { return desconocido };
    let rol = ajustes.rol_de(f.id.as_str());
    if !credencial && rol != Some(ea_ajustes::Rol::Ejecutor) {
        return desconocido;
    }
    let actor = rol.map_or(ACTOR_SIN_ROL, |r| r.as_str());
    Llamante { actor, agente: Some(f.id.as_str()), cliente: cliente_s }
}

/// ¿Trae la petición la credencial del planificador? Se compara en tiempo
/// constante; sin credencial en el servidor (sin directorio de ejecución)
/// ninguna petición la trae.
fn trae_credencial(context: &RequestContext<RoleServer>, credencial: Option<&Credencial>) -> bool {
    let Some(c) = credencial else { return false };
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|p| p.headers.get(ea_ajustes::CABECERA_CREDENCIAL_MCP))
        .is_some_and(|v| c.coincide(v.as_bytes()))
}

/// Las herramientas de una sesión MCP. Se copia (barato: rutas y punteros
/// compartidos) a un hilo aparte para todo lo que bloquea.
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
    /// La raíz del bus que se lee (la misma que la de `consulta`).
    estado_dir: std::path::PathBuf,
    /// De dónde se leen los roles vigentes en cada llamada.
    ajustes: ea_ajustes::Fuentes,
    /// Dónde mira la biblioteca; `None`: el entorno real en cada llamada.
    raices: Option<ea_biblioteca::Raices>,
    /// La credencial del planificador ([`crate::credencial`]); `None`: nadie
    /// la trae, y solo el ejecutor actúa.
    credencial: Option<Arc<Credencial>>,
}

type ToolBody = Result<Value, String>;

fn respond(body: ToolBody) -> Result<CallToolResult, McpError> {
    match body {
        Ok(value) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        )])),
        Err(msg) => Err(McpError::internal_error(msg, None)),
    }
}

impl EaTools {
    /// Con las rutas de los ajustes de ahora (una carga): rutinas, bóveda y
    /// bus (`EA_BOVEDA_DIR` y `EA_ESTADO_DIR` mandan, a través de los ajustes).
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        let a = ea_ajustes::cargar();
        Self::con_entorno(store, a.orquestador.rutinas_dir, Boveda::en(a.rutas.boveda), a.rutas.estado)
    }

    pub fn con_entorno(
        store: Arc<Mutex<Store>>,
        rutinas_dir: std::path::PathBuf,
        boveda: Boveda,
        estado_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            store,
            consulta: Arc::new(SoloConsulta::usuario(rutinas_dir, boveda.clone(), estado_dir.clone())),
            boveda,
            estado_dir,
            ajustes: ea_ajustes::Fuentes::sistema(),
            raices: None,
            credencial: None,
        }
    }

    /// La credencial con la que el planificador demuestra quién es.
    pub fn con_credencial(mut self, credencial: impl Into<Arc<Credencial>>) -> Self {
        self.credencial = Some(credencial.into());
        self
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

    /// Todas las herramientas: las de cada área. Lo usa `#[tool_handler]`.
    fn tool_router() -> ToolRouter<Self> {
        Self::consultas() + Self::trabajos() + Self::diario() + Self::boveda() + Self::biblioteca() + Self::mutantes()
    }

    /// Corre `f` en un hilo aparte con una copia de las herramientas: leer el
    /// bus, la bóveda, los ajustes o SQLite bloquea, y no debe hacerlo en el
    /// hilo del runtime. La base de datos se toma ahí (`blocking_lock`) y solo
    /// cuando hace falta.
    async fn bloqueante<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Self) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let t = self.clone();
        tokio::task::spawn_blocking(move || f(&t)).await.map_err(|e| e.to_string())?
    }

    /// Quién llama, con los roles de ahora y la credencial de esta petición:
    /// cambiar de planificador no exige reconectar.
    async fn llamante(&self, context: &RequestContext<RoleServer>) -> Llamante {
        let cliente = context.client_info().as_ref().map(|i| i.name.clone());
        let credencial = trae_credencial(context, self.credencial.as_deref());
        self.bloqueante(move |t| Ok(actor_for(cliente.as_deref(), credencial, &ea_ajustes::cargar_de(&t.ajustes).ajustes)))
            .await
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// La plantilla de cada herramienta: quién llama, el cuerpo, el evento
    /// del diario (§11.3, también si falla) con los argumentos, el actor y lo
    /// que tardó, y la respuesta.
    async fn registrada<F, Fut>(
        &self,
        context: &RequestContext<RoleServer>,
        accion: &str,
        trabajo: Option<&str>,
        args: Value,
        cuerpo: F,
    ) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(Llamante) -> Fut,
        Fut: Future<Output = ToolBody>,
    {
        let quien = self.llamante(context).await;
        let started = Instant::now();
        let body = cuerpo(quien.clone()).await;
        self.log_invocation(&quien, accion, trabajo, started, args, &body).await;
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
        args: Value,
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
        let outcome = if body.is_ok() {
            EventOutcome::Ok
        } else {
            EventOutcome::Error
        };
        let evento = NewEvent {
            actor: quien.actor.to_string(),
            action: action.to_string(),
            trabajo: trabajo.map(str::to_string),
            outcome: Some(outcome),
            duration_ms: Some(duration_ms),
            detail: Some(detail.to_string()),
        };
        let _ = self
            .bloqueante(move |t| {
                let _ = t.store.blocking_lock().append_event(evento);
                Ok(())
            })
            .await;
    }

    /// Autoriza una herramienta del catálogo por el actor y, si falla, deja la
    /// denegación en el diario (§11.3). `Ok(())` si puede seguir.
    async fn autorizar(
        &self,
        quien: &Llamante,
        nombre: &str,
        accion: &str,
        started: Instant,
        args: &Value,
    ) -> Result<(), McpError> {
        let h = buscar(nombre).expect("en el catálogo");
        if let Err(msg) = autorizar_mcp(quien.actor, h) {
            let body: ToolBody = Err(msg.clone());
            self.log_invocation(quien, &format!("{accion}_denegada"), None, started, args.clone(), &body).await;
            return Err(McpError::internal_error(msg, None));
        }
        Ok(())
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
                 ea_desactivar_herramienta, ea_lanzar_sesion, ea_limpiar_worktree) nunca se \
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
        let actor = |c: Option<&str>, a: &ea_ajustes::Ajustes| actor_for(c, true, a);
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

    /// Sin la credencial, el nombre declarado no da ningún rol salvo el del
    /// ejecutor (U-02): ni el planificador, ni el orquestador, ni sin-rol.
    #[test]
    fn sin_credencial_solo_el_ejecutor_actua() {
        let mut a = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(std::path::Path::new("/nada")));
        let actor = |c: &str, a: &ea_ajustes::Ajustes| actor_for(Some(c), false, a);
        let desconocido = |c: &str| Llamante { actor: ACTOR_DESCONOCIDO, agente: None, cliente: Some(c.into()) };
        assert_eq!(actor("claude-code", &a), desconocido("claude-code"));
        assert_eq!(actor("ea-orchestrator", &a), desconocido("ea-orchestrator"));
        assert_eq!(actor("codex-mcp-client", &a), desconocido("codex-mcp-client"));
        assert_eq!(actor("lo-que-sea", &a), desconocido("lo-que-sea"));
        assert_eq!(actor_for(None, false, &a).actor, ACTOR_DESCONOCIDO);
        assert_eq!(actor("qwen-code-mcp-client", &a), Llamante { actor: "modelo-local", agente: Some("qwen-code"), cliente: Some("qwen-code-mcp-client".into()) });
        // El ejecutor es el agente que ocupe el rol ahora, no un nombre fijo.
        (a.roles.planificador, a.roles.ejecutor) = ("qwen-code".into(), "codex".into());
        assert_eq!(actor("qwen-code-mcp-client", &a), desconocido("qwen-code-mcp-client"));
        assert_eq!(actor("codex-mcp-client", &a).actor, "modelo-local");
    }

    /// Ningún cliente MCP, se presente como se presente, ocupe el rol que
    /// ocupe y traiga o no la credencial, actúa como `usuario` (el único que
    /// edita la bóveda a mano y acepta propuestas).
    #[test]
    fn ningun_cliente_mcp_actua_como_usuario() {
        let mut a = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(std::path::Path::new("/nada")));
        let clientes = [
            None, Some("usuario"), Some("ea-cli"), Some("ea-web"), Some("ea-orchestrator"), Some("claude-code"), Some("qwen-code-mcp-client"),
            Some("codex-mcp-client"), Some("opencode"), Some("antigravity-client"), Some("gemini-cli-mcp-client"), Some("lo-que-sea"), Some(""),
        ];
        for (planificador, ejecutor) in [("claude-code", "qwen-code"), ("opencode", "codex"), ("antigravity", "opencode")] {
            (a.roles.planificador, a.roles.ejecutor) = (planificador.into(), ejecutor.into());
            for c in clientes {
                for credencial in [true, false] {
                    assert_ne!(actor_for(c, credencial, &a).actor, ea_ajustes::ACTOR_USUARIO, "{c:?} con {planificador}/{ejecutor}");
                }
            }
        }
    }
}
