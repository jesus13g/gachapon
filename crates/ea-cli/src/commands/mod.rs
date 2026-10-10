pub mod diario;
pub mod propuestas;
pub mod rutinas;
pub mod status;
pub mod tareas;
pub mod voz;

use anyhow::{Context, Result};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

/// Acepta tanto una fecha-hora completa RFC3339 como una fecha simple
/// AAAA-MM-DD (interpretada como medianoche UTC), para que los filtros
/// `--desde`/`--hasta` sean cómodos de escribir a mano.
pub fn parse_flexible_date(s: &str) -> Result<OffsetDateTime> {
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(dt);
    }
    let date_fmt = format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(s, &date_fmt)
        .with_context(|| format!("fecha inválida: '{s}' (usa RFC3339 o AAAA-MM-DD)"))?;
    Ok(date.midnight().assume_utc())
}

/// Formatea un timestamp para mostrarlo en tablas: legible, en UTC.
pub fn fmt_ts(dt: OffsetDateTime) -> String {
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    dt.format(&fmt).unwrap_or_else(|_| dt.to_string())
}
