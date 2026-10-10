//! Argumentos de las herramientas MCP, con esquema JSON derivado vía
//! `schemars` para que el cliente (Claude Code) sepa qué forma aceptan.
//! Los enums de dominio (`TaskStatus`, `TaskOrigin`) se reciben como texto
//! y se parsean con su `FromStr` ya existente en `ea-store`, para no tener
//! que añadir `schemars` como dependencia de esa capa.

use schemars::JsonSchema;
use serde::Deserialize;
use time::OffsetDateTime;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListarTareasArgs {
    /// Filtra por estado: pendiente, en_progreso, completada, fallida o
    /// cancelada. Sin este argumento, devuelve tareas en cualquier estado.
    pub estado: Option<String>,
    /// Filtra por el actor al que está asignada la tarea (p. ej.
    /// "claude-code", "modelo-local", "usuario").
    pub asignado_a: Option<String>,
    /// Número máximo de tareas a devolver, las más recientes primero.
    /// Por defecto 20.
    pub limite: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerTareaArgs {
    /// Identificador numérico de la tarea.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListarDiarioArgs {
    /// Limita el diario a los eventos de una tarea concreta.
    pub tarea: Option<i64>,
    /// Limita el diario a los eventos de un actor concreto.
    pub actor: Option<String>,
    /// Fecha/hora desde la que listar (RFC3339 o AAAA-MM-DD, UTC).
    pub desde: Option<String>,
    /// Fecha/hora hasta la que listar (RFC3339 o AAAA-MM-DD, UTC).
    pub hasta: Option<String>,
    /// Número máximo de eventos a devolver, los más recientes primero.
    /// Por defecto 20.
    pub limite: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CrearTareaArgs {
    /// Título breve de la tarea.
    pub titulo: String,
    /// Descripción de qué se solicita.
    pub solicita: String,
    /// Actor al que se delega la tarea (p. ej. "usuario", "modelo-local",
    /// o "claude-code" para autoseguimiento).
    pub asignado_a: String,
    /// Id de una tarea existente de la que esta es subtarea, si aplica.
    pub tarea_padre: Option<i64>,
    /// Si es true, no escribe nada: devuelve la tarea que se crearía.
    pub simular: Option<bool>,
}

/// Acepta tanto RFC3339 completo como una fecha simple AAAA-MM-DD
/// (interpretada como medianoche UTC) — mismo comportamiento que
/// `ea-cli`'s `parse_flexible_date`, duplicado aquí porque es una función
/// de formato de entrada de CLI/MCP, no lógica de `ea-store`.
pub fn parse_flexible_date(s: &str) -> Result<OffsetDateTime, String> {
    use time::format_description::well_known::Rfc3339;
    use time::macros::format_description;

    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(dt);
    }
    let date_fmt = format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(s, &date_fmt)
        .map_err(|_| format!("fecha inválida: '{s}' (usa RFC3339 o AAAA-MM-DD)"))?;
    Ok(date.midnight().assume_utc())
}
