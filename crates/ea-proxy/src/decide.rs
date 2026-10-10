//! El flujo de traducción de protocolo en sí: hasta 3 llamadas acotadas por
//! gramática contra `llama-server`, cada una con un schema mínimo y
//! aislado — el diseño que la validación empírica de esta fase mostró como
//! fiable (ver plan de la fase) — y una última llamada sin acotar que
//! redacta la respuesta en lenguaje natural. Cada paso queda en el diario
//! (`actor="modelo-local"`) para poder medir después el acierto del
//! modelo, igual que `ea-mcp` ya registra sus invocaciones.

use std::time::Instant;

use ea_store::{EventOutcome, NewEvent, Store};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::catalog::{self};
use crate::config::Profile;
use crate::llama_client::{ChatMessage, LlamaClient};
use crate::mcp_client::McpClient;

pub struct Decision {
    pub content: String,
    pub herramienta_usada: Option<String>,
    pub argumentos_usados: Option<Value>,
    /// Lo que devolvió la herramienta, tal cual — `content` es su redacción
    /// en lenguaje natural, y esto es lo que permite contrastarla.
    pub resultado_herramienta: Option<Value>,
}

/// Estilo de toda respuesta final: quien la recibe puede ser la voz, que
/// lee en alto lo que llegue — JSON y markdown incluidos.
const ESTILO_RESPUESTA: &str = "Responde en español, en una o dos frases cortas de texto \
     llano: sin JSON, sin markdown, sin listas y sin bloques de código.";

/// Tope del resultado de herramienta que se le pasa al modelo para
/// redactar: el contexto de `llama-server` es de 8192 tokens y un listado
/// del diario lo desborda con facilidad.
const MAX_RESULTADO_CHARS: usize = 6000;

/// Recorta `texto` a `max` caracteres sin partir un carácter multibyte.
pub fn recortar(texto: &str, max: usize) -> (&str, bool) {
    match texto.char_indices().nth(max) {
        Some((i, _)) => (&texto[..i], true),
        None => (texto, false),
    }
}

/// Mensajes del paso de redacción: la petición original más el resultado
/// de la herramienta, para que el modelo lo cuente en vez de devolverlo.
pub fn mensajes_redaccion(
    conversacion: &[ChatMessage],
    herramienta: &str,
    resultado: &Value,
) -> Vec<ChatMessage> {
    let crudo = resultado.to_string();
    let (recortado, truncado) = recortar(&crudo, MAX_RESULTADO_CHARS);
    let aviso = if truncado {
        "\n(El resultado está recortado: di que hay más de lo que cuentas.)"
    } else {
        ""
    };
    let mut mensajes = vec![ChatMessage::system(format!(
        "Para atender la última petición del usuario se consultó la herramienta \
         '{herramienta}', que devolvió este resultado:\n{recortado}{aviso}\n\n\
         Contesta a la petición usando solo esos datos, con sus cifras concretas y \
         sin inventar nada. Si el resultado está vacío, dilo. Si trae varios \
         elementos, di cuántos son y nombra como mucho tres dentro de la misma \
         frase, separados por comas, sin numerarlos y sin leer identificadores, \
         fechas ni mensajes de error completos. {ESTILO_RESPUESTA}"
    ))];
    mensajes.extend_from_slice(conversacion);
    mensajes
}

/// Instrucción del paso de extracción. Sin la fecha de hoy y sin la orden
/// explícita de dejar a null lo que no se pide, el modelo rellena todos los
/// filtros con valores plausibles (fechas de su entrenamiento, un actor
/// cualquiera) y la consulta vuelve vacía.
pub fn instruccion_extraccion(tool: &catalog::CatalogTool, hoy: &str) -> String {
    format!(
        "Extrae los argumentos para invocar '{}' a partir de la última petición del \
         usuario.\n\n{}\n\nHoy es {hoy}. Pon null en todo argumento que la petición \
         no mencione de forma expresa: no inventes filtros, nombres ni fechas. \
         \"Últimamente\" o \"reciente\" no son una fecha: déjalas a null.",
        tool.name, tool.args_help
    )
}

fn catalog_prompt() -> String {
    let mut s = String::from("Herramientas disponibles del ecosistema:\n");
    for t in catalog::CATALOGO {
        s.push_str(&format!("- {}: {}\n", t.name, t.description));
    }
    s
}

async fn log_event(store: &Mutex<Store>, action: &str, started: Instant, detail: Value) {
    let duration_ms = started.elapsed().as_millis() as i64;
    let store = store.lock().await;
    let _ = store.append_event(NewEvent {
        actor: "modelo-local".to_string(),
        action: action.to_string(),
        task_id: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: Some(duration_ms),
        detail: Some(detail.to_string()),
    });
}

pub async fn decidir(
    llama: &LlamaClient,
    mcp: &McpClient,
    store: &Mutex<Store>,
    conversacion: &[ChatMessage],
    profile: &Profile,
) -> anyhow::Result<Decision> {
    // Paso 1: clasificar, aislado del resto — mezclar "elige herramienta o
    // responde" en un solo schema sesga al modelo hacia elegir siempre una
    // herramienta (validado empíricamente).
    let started = Instant::now();
    let mut mensajes = vec![ChatMessage::system(format!(
        "Decides si para responder a la última petición del usuario hace falta \
         consultar alguna herramienta del ecosistema de tareas/eventos, o si puedes \
         responder sin consultar nada.\n\n{}",
        catalog_prompt()
    ))];
    mensajes.extend_from_slice(conversacion);
    let clasificacion = llama
        .constrained(&mensajes, "clasificacion", catalog::schema_clasificar())
        .await?;
    let necesita_herramienta = clasificacion["necesita_herramienta"]
        .as_bool()
        .unwrap_or(false);
    log_event(
        store,
        "proxy_clasificacion",
        started,
        json!({ "necesita_herramienta": necesita_herramienta }),
    )
    .await;

    if !necesita_herramienta {
        let started = Instant::now();
        let mut mensajes = vec![ChatMessage::system(ESTILO_RESPUESTA)];
        mensajes.extend_from_slice(conversacion);
        let content = llama
            .free(&mensajes, profile.temperature, profile.max_tokens)
            .await?;
        log_event(
            store,
            "proxy_respuesta_directa",
            started,
            json!({ "longitud": content.len() }),
        )
        .await;
        return Ok(Decision {
            content,
            herramienta_usada: None,
            argumentos_usados: None,
            resultado_herramienta: None,
        });
    }

    // Paso 2: elegir herramienta, sin opción null — ya se sabe que hace
    // falta una.
    let started = Instant::now();
    let mut mensajes = vec![ChatMessage::system(format!(
        "Elige qué herramienta hay que invocar para atender la última petición del \
         usuario.\n\n{}",
        catalog_prompt()
    ))];
    mensajes.extend_from_slice(conversacion);
    let eleccion = llama
        .constrained(
            &mensajes,
            "eleccion_herramienta",
            catalog::schema_elegir_herramienta(),
        )
        .await?;
    let herramienta = eleccion["herramienta"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("el modelo no eligió herramienta: {eleccion}"))?
        .to_string();
    log_event(
        store,
        "proxy_decision_herramienta",
        started,
        json!({ "herramienta": herramienta }),
    )
    .await;

    let tool = catalog::find(&herramienta)
        .ok_or_else(|| anyhow::anyhow!("herramienta fuera de catálogo: {herramienta}"))?;

    // Paso 3: extraer argumentos, solo si la herramienta los tiene.
    let argumentos: Value = if (tool.args_schema)()["properties"]
        .as_object()
        .is_some_and(|p| p.is_empty())
    {
        json!({})
    } else {
        let started = Instant::now();
        let mut mensajes = vec![ChatMessage::system(instruccion_extraccion(
            tool,
            &time::OffsetDateTime::now_utc().date().to_string(),
        ))];
        mensajes.extend_from_slice(conversacion);
        let extraidos = llama
            .constrained(&mensajes, "argumentos", (tool.args_schema)())
            .await?;
        log_event(
            store,
            "proxy_extraccion_argumentos",
            started,
            json!({ "herramienta": herramienta, "argumentos": extraidos }),
        )
        .await;
        extraidos
    };

    let args_map: Map<String, Value> = argumentos
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .collect();

    let resultado = mcp.call_tool(&herramienta, args_map).await?;

    // Paso 4: redactar. El resultado crudo es JSON; devolverlo tal cual
    // hace que la voz lo lea en alto.
    let started = Instant::now();
    let content = llama
        .free(
            &mensajes_redaccion(conversacion, &herramienta, &resultado),
            profile.temperature,
            profile.max_tokens,
        )
        .await?;
    log_event(
        store,
        "proxy_redaccion",
        started,
        json!({ "herramienta": herramienta, "longitud": content.len() }),
    )
    .await;

    Ok(Decision {
        content,
        herramienta_usada: Some(herramienta),
        argumentos_usados: Some(argumentos),
        resultado_herramienta: Some(resultado),
    })
}

/// Punto de decisión de una rutina (§9.2): elige una rama de entre un
/// conjunto cerrado declarado por el paso, acotado por gramática igual que
/// el resto de pasos de este módulo — nunca por instrucción en el prompt
/// (§9.2: "la elección de rama se fuerza mediante restricción de la forma
/// de salida"). El único cliente de esta función es el ejecutor de
/// rutinas (`ea-orchestrator`); este proxy sigue siendo el único punto que
/// habla con `llama-server`.
pub async fn elegir_rama(
    llama: &LlamaClient,
    store: &Mutex<Store>,
    contexto: &str,
    opciones: &[String],
) -> anyhow::Result<String> {
    let started = Instant::now();
    let schema = json!({
        "type": "object",
        "properties": {
            "rama": { "type": "string", "enum": opciones },
        },
        "required": ["rama"],
        "additionalProperties": false,
    });
    // Igual que el resto de pasos de este módulo, un system message
    // aislado con la instrucción, no mezclado en el contexto — la rutina
    // solo aporta el contexto y las etiquetas de rama, que por sí solas no
    // le dicen al modelo que esto es una elección forzada entre ellas.
    let mensajes = vec![
        ChatMessage::system(format!(
            "Elige exactamente una de estas ramas según el contexto que sigue: {}.",
            opciones.join(", ")
        )),
        ChatMessage::user(contexto),
    ];
    let eleccion = llama.constrained(&mensajes, "eleccion_rama", schema).await?;
    let rama = eleccion["rama"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("el modelo no eligió rama: {eleccion}"))?
        .to_string();
    log_event(
        store,
        "proxy_eleccion_rama",
        started,
        json!({ "opciones": opciones, "rama": rama }),
    )
    .await;
    Ok(rama)
}
