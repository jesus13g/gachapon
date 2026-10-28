//! Las mutaciones supervisadas (§11.1): el agente nunca ejecuta. Con
//! `simular` obtiene la descripción de lo que pasaría; sin ella se encola una
//! propuesta con acción estructurada que solo el usuario puede aceptar
//! (`ea propuestas decidir`, panel web). No existe ninguna herramienta ni
//! token con el que un agente confirme.

use ea_acciones::{autorizar_mcp, buscar, observar, proponer, simular_con, Riesgo};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::{json, Value};

use super::EaTools;
use crate::args::{
    EncolarRutinaArgs, HerramientaArgs, LanzarSesionArgs, ProgramarRutinaArgs, PromoverNotaArgs, RegistrarRutinaArgs,
    ReiniciarServicioArgs, RutinaArgs, WorktreeArgs,
};

impl EaTools {
    /// Cuerpo común de las herramientas mutantes supervisadas.
    async fn mutante(
        &self,
        context: RequestContext<RoleServer>,
        nombre: &'static str,
        args: Value,
        simular: bool,
    ) -> Result<CallToolResult, McpError> {
        let mut args_log = args.clone();
        args_log["simular"] = json!(simular);
        let accion = format!("mcp_{}{}", nombre.trim_start_matches("ea_"), if simular { "_simulada" } else { "" });
        self.registrada(&context, &accion, None, args_log, |quien| async move {
            let actor = quien.actor;
            let h = buscar(nombre).ok_or_else(|| format!("herramienta desconocida: {nombre}"))?;
            autorizar_mcp(actor, h)?;
            // Lo asíncrono (preguntar a systemd) antes de tomar la base de datos.
            let hechos = observar(&*self.consulta, nombre, &args).await.map_err(|e| e.to_string())?;
            self.bloqueante(move |t| {
                let store = t.store.blocking_lock();
                if simular {
                    let sim = simular_con(&store, &*t.consulta, &hechos, nombre, &args).map_err(|e| e.to_string())?;
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
                let (p, nueva) = proponer(&store, &*t.consulta, &hechos, actor, None, nombre, &args).map_err(|e| e.to_string())?;
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
            })
            .await
        })
        .await
    }
}

#[tool_router(router = mutantes, vis = "pub(super)")]
impl EaTools {
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
        description = "[Riesgo: mutación reversible] Quita el worktree propio de un trabajo \
            terminado (cerrado o cancelado) con `git worktree remove`, sin --force: solo si \
            está limpio. La rama ea/<id> se conserva; fusionarla o borrarla es del usuario. \
            NO lo hace: con simular=true lo describe; sin él encola una propuesta que el \
            usuario debe aceptar (terminal o panel web).",
        annotations(
            title = "Limpiar worktree propio",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn ea_limpiar_worktree(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(args): Parameters<WorktreeArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.mutante(context, "ea_limpiar_worktree", json!({ "trabajo": args.trabajo }), args.simular.unwrap_or(false)).await
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
