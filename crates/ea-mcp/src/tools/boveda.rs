//! La bóveda de notas: buscar y leer en todas las regiones y escribir solo en
//! la propia. Promover a la compartida es del usuario (está en `mutantes`).

use std::time::Instant;

use ea_boveda::{Limites, NuevaNota};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::json;

use super::{respond, EaTools};
use crate::args::{BuscarNotasArgs, EscribirNotaArgs, LeerNotaArgs};

#[tool_router(router = boveda, vis = "pub(super)")]
impl EaTools {
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
        let args_json = json!({ "consulta": args.consulta, "region": args.region, "limite": args.limite });
        self.registrada(&context, "mcp_buscar_notas", None, args_json, |quien| {
            self.bloqueante(move |t| {
                let lim = Limites::para_actor(quien.actor);
                let pedido = args.limite.filter(|n| *n > 0).map(|n| n as usize);
                let mut store = t.store.blocking_lock();
                let r = t
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
            })
        })
        .await
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
        let args_json = json!({ "ruta": args.ruta, "desde": args.desde });
        self.registrada(&context, "mcp_leer_nota", None, args_json, |quien| {
            self.bloqueante(move |t| {
                let lim = Limites::para_actor(quien.actor);
                let desde = args.desde.filter(|n| *n > 0).unwrap_or(0) as usize;
                let n = t.boveda.leer(&args.ruta, desde, &lim).map_err(|e| e.to_string())?;
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
            })
        })
        .await
    }

    // No usa la plantilla: si escribe bien, el evento es el del escritor
    // (`boveda_escritura`); solo se anotan la simulación, el fallo y la
    // denegación.
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
        let quien = self.llamante(&context).await;
        let actor = quien.actor;
        let started = Instant::now();
        let simular = args.simular.unwrap_or(false);
        let args_json = json!({
            "titulo": args.titulo, "bytes": args.contenido.len(), "ruta": args.ruta,
            "etiquetas": args.etiquetas, "trabajo": args.trabajo, "sobrescribir": args.sobrescribir,
            "simular": simular,
        });
        self.autorizar(&quien, "ea_escribir_nota", "mcp_escribir_nota", started, &args_json).await?;
        let trabajo = args.trabajo.clone();
        let body = self
            .bloqueante(move |t| {
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
                    return t
                        .boveda
                        .simular_escritura(actor, &nota, &lim)
                        .map(|(ruta, existe)| {
                            json!({
                                "simulado": true, "ruta": ruta, "existe": existe,
                                "aviso": "No se ha escrito nada. Repite sin simular para escribirla.",
                            })
                        })
                        .map_err(|e| e.to_string());
                }
                let mut store = t.store.blocking_lock();
                t.boveda
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
            })
            .await;
        // Si tuvo éxito, el escritor ya dejó su evento
        // `boveda_escritura` (actor, ruta, bytes, huella); duplicarlo aquí
        // contradiría "una sola fuente de verdad". Si falló, se anota.
        if simular {
            self.log_invocation(&quien, "mcp_escribir_nota_simulada", trabajo.as_deref(), started, args_json, &body).await;
        } else if body.is_err() {
            self.log_invocation(&quien, "mcp_escribir_nota", trabajo.as_deref(), started, args_json, &body).await;
        }
        respond(body)
    }
}
