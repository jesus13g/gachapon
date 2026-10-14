//! Argumentos de las herramientas MCP, con esquema JSON derivado vía
//! `schemars` para que el cliente (Claude Code) sepa qué forma aceptan.
//! Los enums de dominio se reciben como texto, para no tener que añadir
//! `schemars` como dependencia de las capas de dominio.

use schemars::JsonSchema;
use serde::Deserialize;
use time::OffsetDateTime;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListarTrabajosArgs {
    /// Filtra por estado: en_cola, ejecutando, en_revision, en_espera,
    /// cerrado o cancelado. Sin este argumento, trabajos en cualquier estado.
    pub estado: Option<String>,
    /// Filtra por modo: mision, obra o rutina.
    pub modo: Option<String>,
    /// Número máximo de trabajos a devolver, los más recientes primero.
    /// Por defecto 20.
    pub limite: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerTrabajoArgs {
    /// Identificador del trabajo (el que devuelve ea_listar_trabajos).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListarDiarioArgs {
    /// Limita el diario a los eventos asociados a un trabajo concreto.
    pub trabajo: Option<String>,
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
pub struct RutinaArgs {
    /// Nombre (slug) de la rutina, p. ej. "revision-diaria".
    pub slug: String,
    /// Si es true, solo describe lo que se haría. Si es false (por defecto),
    /// encola una propuesta que el usuario debe aceptar: nada se ejecuta aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReiniciarServicioArgs {
    /// Servicio de la lista cerrada: llama-server, ea-mcp,
    /// ea-orchestrator o ea-web.
    pub servicio: String,
    /// Si es true, solo describe lo que se haría. Si es false (por defecto),
    /// encola una propuesta que el usuario debe aceptar: nada se ejecuta aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuscarNotasArgs {
    /// Palabras a buscar en el título, las etiquetas y el texto de las notas
    /// (todas deben aparecer; sin acentos ni mayúsculas).
    pub consulta: String,
    /// Limita la búsqueda a una región: modelo-local, claude-code o compartida.
    pub region: Option<String>,
    /// Máximo de resultados; el servidor lo recorta a su tope por actor.
    pub limite: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LeerNotaArgs {
    /// Ruta de la nota tal como la devuelve la búsqueda, p. ej.
    /// "compartida/servicios/mcp.md".
    pub ruta: String,
    /// Carácter del cuerpo desde el que leer (0 por defecto). Una nota larga
    /// se devuelve por trozos: usa `siguiente_desde` de la respuesta anterior.
    pub desde: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscribirNotaArgs {
    /// Título de la nota (una línea, hasta 200 caracteres).
    pub titulo: String,
    /// Cuerpo de la nota en Markdown. No incluyas cabecera: la pone el sistema.
    pub contenido: String,
    /// Ruta dentro de TU región (p. ej. "ideas/rust.md"). Sin ella se deriva
    /// del título. No se puede elegir otra región.
    pub ruta: Option<String>,
    /// Etiquetas (letras, cifras, '-', '_' o '/').
    pub etiquetas: Option<Vec<String>>,
    /// Id del trabajo en cuyo marco se escribe la nota (queda en la cabecera
    /// como origen `trabajo:<id>`).
    pub trabajo: Option<String>,
    /// Reemplazar la nota si ya existe (por defecto falla).
    pub sobrescribir: Option<bool>,
    /// Si es true, valida y dice dónde caería la nota, sin escribir.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromoverNotaArgs {
    /// Nota de tu región que propones llevar a la región compartida.
    pub ruta: String,
    /// Ruta de destino dentro de "compartida" (por defecto, la misma que tiene
    /// en tu región).
    pub destino: Option<String>,
    /// Si es true, solo describe lo que se haría. Si es false (por defecto),
    /// encola una propuesta que el usuario debe aceptar: nada se copia aquí.
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
