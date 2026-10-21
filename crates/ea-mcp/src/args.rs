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
pub struct HerramientaArgs {
    /// Id del elemento de la biblioteca: `agente/tipo/ámbito/nombre`, p. ej.
    /// "codex/mcp/usuario/docs" o "claude-code/plugin/usuario/foo@mkt".
    pub elemento: String,
    /// Si es true, solo describe lo que se haría. Si es false (por defecto),
    /// encola una propuesta que el usuario debe aceptar: nada se ejecuta aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EncolarRutinaArgs {
    /// Nombre (slug) de la rutina, p. ej. "revision-diaria".
    pub slug: String,
    /// Entradas que sustituyen a los valores por defecto, como
    /// {"clave": "valor"}. Solo las que la rutina declara; como mucho 32,
    /// de 1024 bytes cada una y sin saltos de línea ni otros caracteres de
    /// control. Se sustituyen tal cual en los pasos y en el criterio (un
    /// comando de shell): la simulación muestra el criterio resultante.
    pub entradas: Option<std::collections::BTreeMap<String, String>>,
    /// Si es true, solo describe lo que se haría. Si es false (por defecto),
    /// encola una propuesta que el usuario debe aceptar: nada se ejecuta aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegistrarRutinaArgs {
    /// La definición completa de la rutina en TOML ([rutina] con slug,
    /// version, worktree absoluto y criterio_exito; [entradas]; [[pasos]]),
    /// como la que se pasaría a `ea rutina registrar`. Para corregir una
    /// rutina, la versión debe ser mayor que la registrada. Como mucho 64 KiB.
    pub toml: String,
    /// Si es true, solo describe lo que se haría (diff frente a la versión
    /// registrada y criterio resultante). Si es false (por defecto), encola
    /// una propuesta que el usuario debe aceptar: nada se registra aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LanzarSesionArgs {
    /// Agente que se lanza: "claude-code", "qwen-code", "codex" u "opencode".
    pub agente: String,
    /// Ids de la biblioteca de herramientas (`agente/tipo/ámbito/nombre`,
    /// p. ej. "claude-code/mcp/usuario/mi-servidor"): solo estos estarán
    /// disponibles en la sesión.
    pub herramientas: Vec<String>,
    /// Carpeta del proyecto (ruta absoluta; opcional).
    pub directorio: Option<String>,
    /// Si es true, solo describe la sesión (comando y ficheros, sin secretos).
    /// Si es false (por defecto), encola una propuesta que el usuario debe
    /// aceptar: nada se abre aquí.
    pub simular: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProgramarRutinaArgs {
    /// Nombre (slug) de una rutina registrada, p. ej. "revision-diaria".
    pub slug: String,
    /// Expresión `OnCalendar` de systemd, p. ej. "*-*-* 08:00:00" (cada día a
    /// las 8), "Mon *-*-* 09:00" (los lunes) o "daily". Una sola línea: sin
    /// saltos de línea ni otros caracteres de control, sin `%` ni `\`; como
    /// mucho 200 caracteres. La valida `systemd-analyze calendar`.
    pub calendario: String,
    /// Si es true, solo describe lo que se haría (con las próximas 3
    /// ejecuciones). Si es false (por defecto), encola una propuesta que el
    /// usuario debe aceptar: nada se programa aquí.
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
