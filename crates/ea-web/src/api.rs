//! Rutas HTTP: la página estática y la API JSON. Todo se apoya en la
//! fachada `Store` y en `ea_orchestrator::rutina`; aquí solo se traduce a
//! JSON y se validan las entradas.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ea_orchestrator::rutina::{self, Paso, Rutina, RutinaError};
use ea_store::{
    EstadoPropuesta, Event, EventFilter, EventOutcome, NewEvent, NewTask, Propuesta,
    PropuestaFilter, RoutineRun, StoreError, Task, TaskFilter, TaskOrigin, TaskStatus,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::{estado, tema, AppState};

const ACTOR: &str = "usuario";
const ESTADOS: [TaskStatus; 5] = [
    TaskStatus::Pendiente,
    TaskStatus::EnProgreso,
    TaskStatus::Completada,
    TaskStatus::Fallida,
    TaskStatus::Cancelada,
];

type Estado = State<Arc<AppState>>;
type ApiResult<T> = Result<T, ApiError>;

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }

    fn bad(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        let status = match e {
            StoreError::TaskNotFound(_)
            | StoreError::EventNotFound(_)
            | StoreError::RoutineRunNotFound(_)
            | StoreError::PropuestaNotFound(_) => StatusCode::NOT_FOUND,
            StoreError::InvalidStatus(_)
            | StoreError::InvalidOrigin(_)
            | StoreError::InvalidOutcome(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, e.to_string())
    }
}

impl From<RutinaError> for ApiError {
    fn from(e: RutinaError) -> Self {
        let status = match &e {
            RutinaError::Io(_, io) if io.kind() == std::io::ErrorKind::NotFound => {
                StatusCode::NOT_FOUND
            }
            RutinaError::Io(..) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        Self(status, e.to_string())
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(app_css))
        .route("/app.js", get(app_js))
        .route("/tema.css", get(tema_css))
        .route("/api/estado", get(estado_general))
        .route("/api/tareas", get(tareas_listar).post(tareas_crear))
        .route("/api/tareas/{id}", get(tarea_ver))
        .route("/api/tareas/{id}/estado", post(tarea_estado))
        .route("/api/diario", get(diario))
        .route("/api/rutinas", get(rutinas_listar))
        .route("/api/rutinas/{slug}", get(rutina_ver))
        .route("/api/rutinas/{slug}/encolar", post(rutina_encolar))
        .route("/api/propuestas", get(propuestas_listar))
        .route("/api/propuestas/{id}/decidir", post(propuesta_decidir))
        .route("/api/voz/hablar", post(voz_hablar))
        .route("/api/servicios/{nombre}/reiniciar", post(servicio_reiniciar))
        .layer(middleware::from_fn(guardia))
        .with_state(state)
}

/// Defensa de una API local frente a páginas ajenas abiertas en el mismo
/// navegador: solo se atiende `Host: localhost|127.0.0.1` (un ataque de
/// reenlace DNS llega con otro host) y toda escritura debe declarar
/// `application/json`, lo que obliga a una petición previa (preflight) que
/// este servidor nunca autoriza.
async fn guardia(req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            let nombre = match h.rsplit_once(':') {
                Some((n, puerto)) if puerto.bytes().all(|b| b.is_ascii_digit()) => n,
                _ => h,
            };
            nombre.eq_ignore_ascii_case("localhost") || nombre == "127.0.0.1"
        });
    if !host_ok {
        return ApiError::new(StatusCode::FORBIDDEN, "host no permitido").into_response();
    }

    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        let es_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if !es_json {
            return ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "se espera application/json")
                .into_response();
        }
    }
    next.run(req).await
}

// ---------------------------------------------------------------- estático

fn estatico(tipo: &'static str, cuerpo: String) -> Response {
    (
        [(header::CONTENT_TYPE, tipo), (header::CACHE_CONTROL, "no-store")],
        cuerpo,
    )
        .into_response()
}

async fn index() -> Response {
    estatico("text/html; charset=utf-8", include_str!("../static/index.html").into())
}

async fn app_css() -> Response {
    estatico("text/css; charset=utf-8", include_str!("../static/app.css").into())
}

async fn app_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../static/app.js").into())
}

async fn tema_css(State(s): Estado) -> Response {
    estatico("text/css; charset=utf-8", tema::css(&s.config.tema_path))
}

// ----------------------------------------------------------------- helpers

fn ts(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_default()
}

fn parse_fecha(s: &str) -> ApiResult<OffsetDateTime> {
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(dt);
    }
    time::Date::parse(s, &format_description!("[year]-[month]-[day]"))
        .map(|d| d.midnight().assume_utc())
        .map_err(|_| ApiError::bad(format!("fecha inválida: '{s}' (usa RFC3339 o AAAA-MM-DD)")))
}

fn limite(pedido: Option<i64>, defecto: i64, maximo: i64) -> i64 {
    pedido.unwrap_or(defecto).clamp(1, maximo)
}

/// Los slugs acaban en una ruta de fichero (`<dir>/<slug>.toml`) y en el
/// nombre de una unidad de systemd: nada que no sea `[A-Za-z0-9_-]`.
fn slug_valido(slug: &str) -> ApiResult<()> {
    let ok = !slug.is_empty()
        && slug.len() <= 64
        && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok { Ok(()) } else { Err(ApiError::bad("slug de rutina inválido")) }
}

fn texto_no_vacio(valor: &str, campo: &str, max: usize) -> ApiResult<String> {
    let v = valor.trim();
    if v.is_empty() {
        return Err(ApiError::bad(format!("'{campo}' no puede estar vacío")));
    }
    if v.chars().count() > max {
        return Err(ApiError::bad(format!("'{campo}' supera {max} caracteres")));
    }
    Ok(v.to_string())
}

fn tarea_json(t: &Task) -> Value {
    json!({
        "id": t.id,
        "creada": ts(t.created_at),
        "actualizada": ts(t.updated_at),
        "creado_por": t.created_by,
        "asignado_a": t.assigned_to,
        "origen": t.origin.as_str(),
        "padre": t.parent_task_id,
        "titulo": t.title,
        "peticion": t.request,
        "estado": t.status.as_str(),
        "resultado": t.result,
    })
}

fn evento_json(e: &Event) -> Value {
    json!({
        "id": e.id,
        "ts": ts(e.ts),
        "actor": e.actor,
        "accion": e.action,
        "tarea": e.task_id,
        "resultado": e.outcome.map(|o| o.as_str()),
        "duracion_ms": e.duration_ms,
        "detalle": e.detail,
    })
}

fn run_json(r: &RoutineRun) -> Value {
    json!({
        "id": r.id,
        "tarea": r.task_id,
        "inicio": ts(r.started_at),
        "fin": r.finished_at.map(ts),
        "resultado": r.outcome.map(|o| o.as_str()),
        "pasos": r.pasos_ejecutados,
        "llamadas_modelo": r.llamadas_modelo,
        "detalle": r.detalle,
    })
}

fn propuesta_json(p: &Propuesta, rutina: Option<String>) -> Value {
    json!({
        "id": p.id,
        "ejecucion": p.routine_run_id,
        "rutina": rutina,
        "creada": ts(p.creado_en),
        "descripcion": p.descripcion,
        "estado": p.estado.as_str(),
        "decidida": p.decidido_en.map(ts),
    })
}

// ------------------------------------------------------------------ estado

/// Último heartbeat de una lista de eventos. Ignora los fechados en el
/// futuro: el diario es append-only y un arranque con el reloj mal puesto
/// (esta máquina alterna con Windows) deja filas que, ordenadas por `ts`,
/// taparían a los latidos reales y darían por vivo a un orquestador caído.
fn ultimo_latido(eventos: &[Event], ahora: OffsetDateTime) -> Option<OffsetDateTime> {
    const MARGEN: time::Duration = time::Duration::seconds(5);
    eventos
        .iter()
        .filter(|e| e.action == "heartbeat" && e.ts <= ahora + MARGEN)
        .map(|e| e.ts)
        .max()
}

async fn estado_general(State(s): Estado) -> ApiResult<Json<Value>> {
    let servicios = estado::servicios(s.config.systemctl).await;

    let store = s.store.lock().await;
    let tareas = store.list_tasks(&TaskFilter::default())?;
    let por_estado: serde_json::Map<String, Value> = ESTADOS
        .iter()
        .map(|e| {
            let n = tareas.iter().filter(|t| t.status == *e).count();
            (e.as_str().to_string(), json!(n))
        })
        .collect();

    let ahora = OffsetDateTime::now_utc();
    let eventos_orquestador = store.list_events(&EventFilter {
        actor: Some("orquestador".into()),
        limit: Some(50),
        ..Default::default()
    })?;
    let ultimo_latido = ultimo_latido(&eventos_orquestador, ahora);
    let vivo = ultimo_latido
        .is_some_and(|t| (ahora - t).whole_seconds() <= s.config.umbral_heartbeat_s);

    let mut agentes = Vec::new();
    for nombre in ["claude-code", "modelo-local"] {
        let en_curso = tareas
            .iter()
            .find(|t| t.assigned_to == nombre && t.status == TaskStatus::EnProgreso);
        let ultimo = store
            .list_events(&EventFilter {
                actor: Some(nombre.into()),
                exclude_action: Some("heartbeat".into()),
                limit: Some(1),
                ..Default::default()
            })?
            .into_iter()
            .next();
        agentes.push(json!({
            "nombre": nombre,
            "tarea": en_curso.map(|t| json!({ "id": t.id, "titulo": t.title })),
            "ultimo": ultimo.map(|e| json!({ "ts": ts(e.ts), "accion": e.action })),
        }));
    }

    let propuestas_pendientes = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: None })?
        .len();

    Ok(Json(json!({
        "ahora": ts(OffsetDateTime::now_utc()),
        "servicios": servicios.iter().map(|sv| json!({
            "nombre": sv.nombre, "activo": sv.activo, "activo_s": sv.activo_s,
        })).collect::<Vec<_>>(),
        "orquestador": { "vivo": vivo, "ultimo_latido": ultimo_latido.map(ts) },
        "agentes": agentes,
        "tareas": por_estado,
        "propuestas_pendientes": propuestas_pendientes,
        "db": ea_store::resolve_db_path().ok().map(|p| p.display().to_string()),
    })))
}

// ------------------------------------------------------------------ tareas

#[derive(Deserialize)]
struct TareasQuery {
    estado: Option<String>,
    asignado_a: Option<String>,
    limite: Option<i64>,
}

async fn tareas_listar(State(s): Estado, Query(q): Query<TareasQuery>) -> ApiResult<Json<Value>> {
    let filter = TaskFilter {
        status: q.estado.as_deref().map(str::parse::<TaskStatus>).transpose()?,
        assigned_to: q.asignado_a.filter(|a| !a.is_empty()),
        limit: Some(limite(q.limite, 100, 500)),
    };
    let store = s.store.lock().await;
    let tareas = store.list_tasks(&filter)?;
    Ok(Json(json!(tareas.iter().map(tarea_json).collect::<Vec<_>>())))
}

async fn tarea_ver(State(s): Estado, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let store = s.store.lock().await;
    let tarea = store.get_task(id)?;
    let mut eventos = store.list_events_by_task(id)?;
    eventos.reverse(); // cronológico: la historia de la tarea
    Ok(Json(json!({
        "tarea": tarea_json(&tarea),
        "eventos": eventos.iter().map(evento_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct NuevaTarea {
    titulo: String,
    asignado_a: String,
    solicita: String,
}

async fn tareas_crear(
    State(s): Estado,
    Json(body): Json<NuevaTarea>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let nueva = NewTask {
        created_by: ACTOR.into(),
        assigned_to: texto_no_vacio(&body.asignado_a, "asignado_a", 64)?,
        origin: TaskOrigin::Boton,
        parent_task_id: None,
        title: texto_no_vacio(&body.titulo, "titulo", 200)?,
        request: texto_no_vacio(&body.solicita, "solicita", 10_000)?,
    };
    let mut store = s.store.lock().await;
    let tarea = store.create_task(nueva)?;
    Ok((StatusCode::CREATED, Json(tarea_json(&tarea))))
}

#[derive(Deserialize)]
struct CambioEstado {
    estado: String,
    resultado: Option<String>,
}

async fn tarea_estado(
    State(s): Estado,
    Path(id): Path<i64>,
    Json(body): Json<CambioEstado>,
) -> ApiResult<Json<Value>> {
    let nuevo: TaskStatus = body.estado.parse()?;
    let resultado = body.resultado.filter(|r| !r.trim().is_empty());
    let mut store = s.store.lock().await;
    let tarea = store.update_task_status(id, nuevo, ACTOR, resultado)?;
    Ok(Json(tarea_json(&tarea)))
}

// ------------------------------------------------------------------ diario

#[derive(Deserialize)]
struct DiarioQuery {
    tarea: Option<i64>,
    actor: Option<String>,
    desde: Option<String>,
    hasta: Option<String>,
    /// `1` incluye los heartbeats del orquestador (por defecto se omiten).
    latidos: Option<String>,
    limite: Option<i64>,
}

async fn diario(State(s): Estado, Query(q): Query<DiarioQuery>) -> ApiResult<Json<Value>> {
    let filter = EventFilter {
        task_id: q.tarea,
        actor: q.actor.filter(|a| !a.is_empty()),
        since: q.desde.as_deref().map(parse_fecha).transpose()?,
        until: q.hasta.as_deref().map(parse_fecha).transpose()?,
        exclude_action: (q.latidos.as_deref() != Some("1")).then(|| "heartbeat".to_string()),
        limit: Some(limite(q.limite, 200, 1000)),
    };
    let store = s.store.lock().await;
    let eventos = store.list_events(&filter)?;
    Ok(Json(json!(eventos.iter().map(evento_json).collect::<Vec<_>>())))
}

// ----------------------------------------------------------------- rutinas

async fn rutinas_listar(State(s): Estado) -> ApiResult<Json<Value>> {
    let dir = &s.config.rutinas_dir;
    let slugs = rutina::listar_slugs(dir)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut filas = Vec::with_capacity(slugs.len());
    for slug in slugs {
        let (descripcion, error) = match rutina::cargar(dir, &slug) {
            Ok(r) => (Some(r.descripcion), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let (ultima, ultimo_exito) = {
            let store = s.store.lock().await;
            (
                store.list_routine_runs(&slug, Some(1))?.into_iter().next(),
                store.last_successful_run(&slug)?,
            )
        };
        let proxima = if s.config.systemctl && slug_valido(&slug).is_ok() {
            estado::proxima_ejecucion(&slug).await
        } else {
            None
        };
        filas.push(json!({
            "slug": slug,
            "descripcion": descripcion,
            "error": error,
            "ultima": ultima.as_ref().map(run_json),
            "ultimo_exito": ultimo_exito.map(|r| ts(r.started_at)),
            "proxima": proxima,
        }));
    }
    Ok(Json(json!(filas)))
}

/// Pasos en orden de recorrido desde `inicio`, y al final los inalcanzables.
fn pasos_json(r: &Rutina) -> Vec<Value> {
    fn destinos(p: &Paso) -> Vec<String> {
        match p {
            Paso::Accion { siguiente, .. } => vec![siguiente.clone()],
            Paso::Decision { ramas, .. } => {
                let mut v: Vec<_> = ramas.iter().collect();
                v.sort();
                v.into_iter().map(|(_, d)| d.clone()).collect()
            }
            Paso::Propuesta { siguiente, .. } => siguiente.iter().cloned().collect(),
            Paso::Fin => vec![],
        }
    }

    let mut orden = Vec::new();
    let mut vistos = HashSet::new();
    let mut cola = VecDeque::from([r.inicio.clone()]);
    while let Some(id) = cola.pop_front() {
        if !vistos.insert(id.clone()) {
            continue;
        }
        if let Some(p) = r.pasos.get(&id) {
            cola.extend(destinos(p));
            orden.push(id);
        }
    }
    let mut sueltos: Vec<_> = r.pasos.keys().filter(|k| !vistos.contains(*k)).cloned().collect();
    sueltos.sort();
    orden.extend(sueltos);

    orden
        .into_iter()
        .map(|id| {
            let paso = &r.pasos[&id];
            let mut v = match paso {
                Paso::Accion { herramienta, argumentos, .. } => {
                    json!({ "tipo": "accion", "herramienta": herramienta, "argumentos": argumentos })
                }
                Paso::Decision { contexto, ramas } => {
                    let ramas: std::collections::BTreeMap<_, _> = ramas.iter().collect();
                    json!({ "tipo": "decision", "contexto": contexto, "ramas": ramas })
                }
                Paso::Propuesta { descripcion, .. } => {
                    json!({ "tipo": "propuesta", "descripcion": descripcion })
                }
                Paso::Fin => json!({ "tipo": "fin" }),
            };
            v["id"] = json!(id);
            v["destinos"] = json!(destinos(paso));
            v
        })
        .collect()
}

async fn rutina_ver(State(s): Estado, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    slug_valido(&slug)?;
    let r = rutina::cargar(&s.config.rutinas_dir, &slug)?;
    let ejecuciones = s.store.lock().await.list_routine_runs(&slug, Some(20))?;
    Ok(Json(json!({
        "slug": slug,
        "descripcion": r.descripcion,
        "inicio": r.inicio,
        "max_duracion_s": r.max_duracion_s,
        "max_pasos": r.max_pasos,
        "max_llamadas_modelo": r.max_llamadas_modelo,
        "ventana_inicial_dias": r.ventana_inicial_dias,
        "pasos": pasos_json(&r),
        "ejecuciones": ejecuciones.iter().map(run_json).collect::<Vec<_>>(),
    })))
}

/// Lo mismo que hace el disparador de systemd (`ea rutinas encolar`): no
/// ejecuta la rutina, solo encola la tarea que el orquestador recogerá.
async fn rutina_encolar(
    State(s): Estado,
    Path(slug): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    slug_valido(&slug)?;
    // Falla pronto si la rutina no existe o es inválida.
    rutina::cargar(&s.config.rutinas_dir, &slug)?;

    let mut store = s.store.lock().await;
    let tarea = store.create_task(NewTask {
        created_by: ACTOR.into(),
        assigned_to: "orquestador".into(),
        origin: TaskOrigin::Boton,
        parent_task_id: None,
        title: format!("Ejecutar rutina: {slug}"),
        request: json!({ "rutina": slug }).to_string(),
    })?;
    Ok((StatusCode::CREATED, Json(tarea_json(&tarea))))
}

// -------------------------------------------------------------- propuestas

#[derive(Deserialize)]
struct PropuestasQuery {
    estado: Option<String>,
}

async fn propuestas_listar(
    State(s): Estado,
    Query(q): Query<PropuestasQuery>,
) -> ApiResult<Json<Value>> {
    let filter = PropuestaFilter {
        estado: q.estado.as_deref().map(str::parse::<EstadoPropuesta>).transpose()?,
        limit: Some(100),
    };
    let store = s.store.lock().await;
    let filas = store
        .list_propuestas(&filter)?
        .iter()
        .map(|p| {
            let rutina = store.get_routine_run(p.routine_run_id).ok().map(|r| r.routine_slug);
            propuesta_json(p, rutina)
        })
        .collect::<Vec<_>>();
    Ok(Json(json!(filas)))
}

#[derive(Deserialize)]
struct Decision {
    decision: String,
}

/// Solo deja constancia de la decisión (igual que `ea propuestas decidir`):
/// ejecutar una propuesta aceptada requiere herramientas mutantes que aún
/// no existen.
async fn propuesta_decidir(
    State(s): Estado,
    Path(id): Path<i64>,
    Json(body): Json<Decision>,
) -> ApiResult<Json<Value>> {
    let estado = match body.decision.as_str() {
        "aceptar" => EstadoPropuesta::Aceptada,
        "rechazar" => EstadoPropuesta::Rechazada,
        _ => return Err(ApiError::bad("decision debe ser 'aceptar' o 'rechazar'")),
    };

    let store = s.store.lock().await;
    if store.get_propuesta(id)?.estado != EstadoPropuesta::Pendiente {
        return Err(ApiError::new(StatusCode::CONFLICT, format!("propuesta #{id} ya decidida")));
    }
    let p = store.decidir_propuesta(id, estado)?;
    store.append_event(NewEvent {
        actor: ACTOR.into(),
        action: "propuesta_decidida".into(),
        task_id: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(json!({ "propuesta": id, "estado": estado.as_str() }).to_string()),
    })?;
    let rutina = store.get_routine_run(p.routine_run_id).ok().map(|r| r.routine_slug);
    Ok(Json(propuesta_json(&p, rutina)))
}

// --------------------------------------------------------------------- voz

/// Reenvía al orquestador, que es quien posee el dispositivo de audio
/// (§6.3); la web no captura ni reproduce nada.
async fn voz_hablar(State(s): Estado) -> ApiResult<Json<Value>> {
    let url = format!("http://127.0.0.1:{}/v1/voz/hablar", s.config.voz_puerto);
    let resp = s.http.post(&url).send().await.map_err(|e| {
        ApiError::new(StatusCode::BAD_GATEWAY, format!("ea-orchestrator no responde: {e}"))
    })?;

    let status = resp.status();
    if !status.is_success() {
        let texto = resp.text().await.unwrap_or_default();
        return Err(ApiError::new(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            texto,
        ));
    }
    let cuerpo: Value = resp
        .json()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("respuesta no válida: {e}")))?;
    Ok(Json(cuerpo))
}

// ---------------------------------------------------------------- servicios

async fn servicio_reiniciar(State(s): Estado, Path(nombre): Path<String>) -> ApiResult<Json<Value>> {
    if !estado::SERVICIOS.contains(&nombre.as_str()) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "servicio desconocido"));
    }
    if !s.config.systemctl {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "systemctl desactivado"));
    }

    let resultado = estado::reiniciar(&nombre).await;
    let store = s.store.lock().await;
    store.append_event(NewEvent {
        actor: ACTOR.into(),
        action: "servicio_reiniciado".into(),
        task_id: None,
        outcome: Some(if resultado.is_ok() { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: None,
        detail: Some(json!({ "servicio": nombre, "error": resultado.as_ref().err() }).to_string()),
    })?;
    resultado
        .map(|()| Json(json!({ "ok": true })))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latido(ts: OffsetDateTime) -> Event {
        Event {
            id: 0,
            ts,
            actor: "orquestador".into(),
            action: "heartbeat".into(),
            task_id: None,
            outcome: None,
            duration_ms: None,
            detail: None,
        }
    }

    #[test]
    fn el_ultimo_latido_ignora_los_fechados_en_el_futuro() {
        let ahora = OffsetDateTime::now_utc();
        let real = ahora - time::Duration::seconds(10);
        let eventos = [
            latido(ahora + time::Duration::hours(1)), // reloj mal puesto al arrancar
            latido(real),
            latido(real - time::Duration::seconds(30)),
        ];
        assert_eq!(ultimo_latido(&eventos, ahora), Some(real));
        assert_eq!(ultimo_latido(&eventos[..1], ahora), None);
    }
}
