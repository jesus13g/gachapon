//! La biblioteca de herramientas de los agentes (spec 018), de solo lectura:
//! activar, desactivar y abrir sesiones son mutantes y solo se proponen.

use ea_acciones::{autorizar_mcp, buscar};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde_json::json;

use super::{EaTools, ToolBody};
use crate::args::BibliotecaArgs;

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
    let limite = super::limite(args.limite, (por_defecto, maximo));
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

#[tool_router(router = biblioteca, vis = "pub(super)")]
impl EaTools {
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
        let args_json = json!({
            "id": args.id, "agente": args.agente, "tipo": args.tipo, "categoria": args.categoria,
            "activado": args.activado, "limite": args.limite,
        });
        self.registrada(&context, "mcp_biblioteca", None, args_json, |quien| {
            self.bloqueante(move |t| {
                let h = buscar("ea_biblioteca").ok_or("herramienta desconocida: ea_biblioteca")?;
                autorizar_mcp(quien.actor, h)?;
                let raices = t.raices.clone().unwrap_or_else(ea_biblioteca::Raices::desde_entorno);
                let ajustes = ea_ajustes::cargar_de(&t.ajustes).ajustes;
                let cotas = if quien.actor == ea_ajustes::Rol::Ejecutor.as_str() { COTAS_BIBLIOTECA_LOCAL } else { COTAS_BIBLIOTECA };
                biblioteca_json(&args, &raices, &ajustes, cotas)
            })
        })
        .await
    }
}
