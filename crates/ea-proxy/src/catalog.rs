//! Catálogo reducido de herramientas proyectado hacia el modelo local:
//! lecturas más una única herramienta mutante, `ea_cambiar_estado_tarea`, que
//! en ea-mcp solo propone (el usuario confirma); descripciones cortas e imperativas, argumentos fuertemente
//! tipados (§7.1 de la especificación) — declarado a mano, no derivado en
//! caliente del catálogo flexible que `ea-mcp` expone a Claude Code (ver
//! razonamiento en el plan de esta fase). Cada entrada aquí corresponde a
//! una herramienta MCP real del mismo nombre; `mcp_client` las invoca tal
//! cual.

use serde_json::{Value, json};

pub struct CatalogTool {
    pub name: &'static str,
    /// Descripción corta e imperativa (§7.1: "cortas, imperativas" para el
    /// modelo local, frente a las ricas de Claude Code).
    pub description: &'static str,
    /// JSON Schema estricto de los argumentos: todo campo va en `required`
    /// (modo estricto de generación acotada por forma); un campo opcional
    /// se expresa como `type: [T, "null"]`, nunca omitiéndolo.
    pub args_schema: fn() -> Value,
    /// Explicación breve del significado de cada argumento, para el paso
    /// de extracción (el schema por sí solo no explica semántica).
    pub args_help: &'static str,
}

fn schema_ea_status() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": false,
    })
}

fn schema_ea_listar_tareas() -> Value {
    json!({
        "type": "object",
        "properties": {
            "estado": {
                "type": ["string", "null"],
                "enum": ["pendiente", "en_progreso", "completada", "fallida", "cancelada", null],
            },
            "asignado_a": { "type": ["string", "null"] },
            "limite": { "type": ["integer", "null"] },
        },
        "required": ["estado", "asignado_a", "limite"],
        "additionalProperties": false,
    })
}

fn schema_ea_ver_tarea() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer" },
        },
        "required": ["id"],
        "additionalProperties": false,
    })
}

fn schema_ea_listar_diario() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tarea": { "type": ["integer", "null"] },
            "actor": { "type": ["string", "null"] },
            "desde": { "type": ["string", "null"] },
            "hasta": { "type": ["string", "null"] },
            "limite": { "type": ["integer", "null"] },
        },
        "required": ["tarea", "actor", "desde", "hasta", "limite"],
        "additionalProperties": false,
    })
}

fn schema_ea_cambiar_estado_tarea() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer" },
            "estado": {
                "type": "string",
                "enum": ["pendiente", "en_progreso", "completada", "fallida", "cancelada"],
            },
            "resultado": { "type": ["string", "null"] },
        },
        "required": ["id", "estado", "resultado"],
        "additionalProperties": false,
    })
}

pub const CATALOGO: &[CatalogTool] = &[
    CatalogTool {
        name: "ea_status",
        description: "Resumen del ecosistema: tareas por estado y últimos eventos.",
        args_schema: schema_ea_status,
        args_help: "Sin argumentos.",
    },
    CatalogTool {
        name: "ea_listar_tareas",
        description: "Lista tareas, filtrables por estado y por a quién están asignadas.",
        args_schema: schema_ea_listar_tareas,
        args_help: "estado: uno de pendiente/en_progreso/completada/fallida/cancelada, o null \
            para cualquier estado. asignado_a: nombre del actor, o null. limite: máximo de \
            resultados, o null para el valor por defecto.",
    },
    CatalogTool {
        name: "ea_ver_tarea",
        description: "Detalle de una tarea concreta y su historial de eventos.",
        args_schema: schema_ea_ver_tarea,
        args_help: "id: identificador numérico de la tarea.",
    },
    CatalogTool {
        name: "ea_listar_diario",
        description: "Lista eventos del diario, filtrables por tarea, actor o fechas.",
        args_schema: schema_ea_listar_diario,
        args_help: "tarea: id numérico o null. actor: nombre del actor o null. desde/hasta: \
            fecha AAAA-MM-DD o null. limite: máximo de resultados o null.",
    },
    CatalogTool {
        name: "ea_cambiar_estado_tarea",
        description: "Pide cambiar el estado de una tarea. Solo propone: el usuario lo confirma.",
        args_schema: schema_ea_cambiar_estado_tarea,
        args_help: "id: identificador numérico de la tarea. estado: el estado nuevo \
            (pendiente/en_progreso/completada/fallida/cancelada). resultado: texto breve \
            del resultado, o null.",
    },
];

pub fn find(name: &str) -> Option<&'static CatalogTool> {
    CATALOGO.iter().find(|t| t.name == name)
}

pub fn tool_names() -> Vec<&'static str> {
    CATALOGO.iter().map(|t| t.name).collect()
}

/// Schema del paso 1: clasificación binaria aislada. Validado como el único
/// diseño fiable para decidir si hace falta una herramienta — un schema que
/// mezcla "elige herramienta o responde" en un solo campo sesga al modelo
/// hacia elegir una herramienta igualmente.
pub fn schema_clasificar() -> Value {
    json!({
        "type": "object",
        "properties": {
            "necesita_herramienta": { "type": "boolean" },
        },
        "required": ["necesita_herramienta"],
        "additionalProperties": false,
    })
}

/// Schema del paso 3: elegir herramienta entre el catálogo reducido. Sin
/// opción `null` — el paso 1 ya decidió que hace falta una.
pub fn schema_elegir_herramienta() -> Value {
    json!({
        "type": "object",
        "properties": {
            "herramienta": { "type": "string", "enum": tool_names() },
        },
        "required": ["herramienta"],
        "additionalProperties": false,
    })
}
