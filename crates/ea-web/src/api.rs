//! Rutas HTTP: la página estática y la API JSON. Todo se apoya en la
//! fachada `Store` y en los ficheros de rutina; aquí solo se traduce a
//! JSON y se validan las entradas.

use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ea_acciones::{Canal, ErrorAccion, SistemaReal};
use ea_store::{
    EstadoPropuesta, Event, EventFilter, Propuesta,
    PropuestaFilter, RoutineRun, StoreError, NewTask, Task, TaskFilter, TaskOrigin, TaskStatus,
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

impl From<ErrorAccion> for ApiError {
    fn from(e: ErrorAccion) -> Self {
        let status = match &e {
            ErrorAccion::Desconocida(_) | ErrorAccion::NoSupervisada(_) => StatusCode::NOT_FOUND,
            ErrorAccion::Argumentos(_) => StatusCode::BAD_REQUEST,
            ErrorAccion::NoAplica(_) => StatusCode::CONFLICT,
            ErrorAccion::Prohibida(_) => StatusCode::FORBIDDEN,
            ErrorAccion::Sistema(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorAccion::Store(StoreError::PropuestaYaDecidida(_)) => StatusCode::CONFLICT,
            ErrorAccion::Store(se) => return ApiError::from_store_ref(se, e.to_string()),
        };
        Self(status, e.to_string())
    }
}

impl ApiError {
    fn from_store_ref(se: &StoreError, msg: String) -> Self {
        let status = match se {
            StoreError::TaskNotFound(_)
            | StoreError::EventNotFound(_)
            | StoreError::RoutineRunNotFound(_)
            | StoreError::PropuestaNotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, msg)
    }
}

/// El sistema real para las acciones que confirma el usuario desde esta
/// página. `ea-web` no puede reiniciarse a sí mismo a mitad de la petición.
fn sistema(s: &AppState) -> SistemaReal {
    SistemaReal {
        rutinas_dir: s.config.rutinas_dir.clone(),
        boveda: s.boveda.clone(),
        propio: Some("ea-web"),
    }
}

/// Slugs de los ficheros `.toml` del directorio de rutinas (ordenados).
fn listar_slugs(dir: &std::path::Path) -> std::io::Result<Vec<String>> {
    let mut v = Vec::new();
    match std::fs::read_dir(dir) {
        Ok(it) => {
            for e in it.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "toml") {
                    if let Some(n) = p.file_stem().and_then(|n| n.to_str()) {
                        v.push(n.to_string());
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    v.sort();
    Ok(v)
}

/// El fichero de la rutina, o 404 si no existe.
fn leer_rutina(dir: &std::path::Path, slug: &str) -> ApiResult<String> {
    std::fs::read_to_string(dir.join(format!("{slug}.toml"))).map_err(|e| {
        let status = if e.kind() == std::io::ErrorKind::NotFound {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        ApiError::new(status, format!("rutina '{slug}': {e}"))
    })
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(app_css))
        .route("/app.js", get(app_js))
        .route("/tema.css", get(tema_css))
        .route("/api/estado", get(estado_general))
        .route("/api/inicio", get(inicio))
        .route("/api/tareas", get(tareas_listar).post(tareas_crear))
        .route("/api/tareas/{id}", get(tarea_ver))
        .route("/api/tareas/{id}/estado", post(tarea_estado))
        .route("/api/tareas/{id}/repetir", post(tarea_repetir))
        .route("/api/diario", get(diario))
        .route("/api/rutinas", get(rutinas_listar))
        .route("/api/rutinas/{slug}", get(rutina_ver))
        .route("/api/rutinas/{slug}/encolar", post(rutina_encolar))
        .route("/api/propuestas", get(propuestas_listar))
        .route("/api/propuestas/{id}/decidir", post(propuesta_decidir))
        .route("/api/acciones", get(acciones_listar))
        .route("/api/acciones/simular", post(accion_simular))
        .route("/api/acciones/ejecutar", post(accion_ejecutar))
        .route("/api/boveda", get(boveda_listar))
        .route("/api/boveda/nota", get(boveda_leer))
        .route("/api/boveda/promover", post(boveda_promover))
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
        "proponente": p.proponente,
        "creada": ts(p.creado_en),
        "descripcion": p.descripcion,
        "estado": p.estado.as_str(),
        "decidida": p.decidido_en.map(ts),
        "accion": p.accion.as_ref().map(|a| json!({
            "herramienta": a.herramienta,
            "argumentos": a.argumentos,
            "riesgo": a.riesgo,
            "simulacion": a.simulacion,
        })),
        "ejecutada": p.ejecucion.as_ref().map(|e| json!({
            "estado": e.estado.as_str(),
            "detalle": e.detalle,
            "cuando": e.ejecutada_en.map(ts),
        })),
    })
}

fn rutina_de(store: &ea_store::Store, p: &Propuesta) -> Option<String> {
    p.routine_run_id
        .and_then(|id| store.get_routine_run(id).ok())
        .map(|r| r.routine_slug)
}

// ------------------------------------------------------------------ estado

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

    let orquestador = store.estado_orquestador(s.config.umbral_heartbeat_s)?;

    let agentes: Vec<Value> = store
        .resumen_agentes()?
        .into_iter()
        .map(|a| {
            json!({
                "nombre": a.nombre,
                "tarea": a.tarea_en_curso.map(|t| json!({ "id": t.id, "titulo": t.title })),
                "ultimo": a.ultimo_evento.map(|e| json!({ "ts": ts(e.ts), "accion": e.action })),
            })
        })
        .collect();

    let propuestas_pendientes = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: None })?
        .len();

    Ok(Json(json!({
        "ahora": ts(OffsetDateTime::now_utc()),
        "servicios": servicios.iter().map(|sv| json!({
            "nombre": sv.nombre, "activo": sv.activo, "activo_s": sv.activo_s,
        })).collect::<Vec<_>>(),
        "orquestador": { "vivo": orquestador.vivo, "ultimo_latido": orquestador.ultimo_latido.map(ts) },
        "agentes": agentes,
        "tareas": por_estado,
        "propuestas_pendientes": propuestas_pendientes,
        "db": ea_store::resolve_db_path().ok().map(|p| p.display().to_string()),
    })))
}

/// La portada: lo que espera una decisión, lo que está en marcha, las rutinas
/// y lo último que terminó. Una sola petición por refresco.
async fn inicio(State(s): Estado) -> ApiResult<Json<Value>> {
    const RECIENTES: usize = 8;

    let rutinas = rutinas_resumen(&s).await?;
    let store = s.store.lock().await;
    let propuestas = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: Some(20) })?
        .iter()
        .map(|p| propuesta_json(p, rutina_de(&store, p)))
        .collect::<Vec<_>>();

    let tareas = store.list_tasks(&TaskFilter::default())?;
    let activa = |t: &&Task| matches!(t.status, TaskStatus::Pendiente | TaskStatus::EnProgreso);
    let mut recientes: Vec<&Task> = tareas.iter().filter(|t| !activa(t)).collect();
    recientes.sort_by_key(|t| std::cmp::Reverse(t.updated_at));

    Ok(Json(json!({
        "propuestas": propuestas,
        "activas": tareas.iter().filter(activa).map(tarea_json).collect::<Vec<_>>(),
        "recientes": recientes.into_iter().take(RECIENTES).map(tarea_json).collect::<Vec<_>>(),
        "rutinas": rutinas,
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

    // Si la tarea es la ejecución de una rutina, su ejecución y lo que propuso.
    let ejecucion = match rutina_de_tarea(&tarea) {
        Some(slug) => store
            .list_routine_runs(&slug, Some(100))?
            .into_iter()
            .find(|r| r.task_id == Some(id)),
        None => None,
    };
    let propuestas: Vec<Value> = match &ejecucion {
        Some(run) => store
            .list_propuestas(&PropuestaFilter::default())?
            .iter()
            .filter(|p| p.routine_run_id == Some(run.id))
            .map(|p| json!({ "id": p.id, "descripcion": p.descripcion, "estado": p.estado.as_str() }))
            .collect(),
        None => Vec::new(),
    };
    let hijas: Vec<Value> = store
        .list_tasks(&TaskFilter::default())?
        .iter()
        .filter(|t| t.parent_task_id == Some(id))
        .map(|t| json!({ "id": t.id, "titulo": t.title, "estado": t.status.as_str() }))
        .collect();

    Ok(Json(json!({
        "tarea": tarea_json(&tarea),
        "eventos": eventos.iter().map(evento_json).collect::<Vec<_>>(),
        "ejecucion": ejecucion.as_ref().map(|r| {
            let mut v = run_json(r);
            v["rutina"] = json!(r.routine_slug);
            v
        }),
        "propuestas": propuestas,
        "hijas": hijas,
    })))
}

/// El slug, si la petición de la tarea es la de ejecutar una rutina
/// (`{"rutina": "<slug>"}`, lo que encola `ea_encolar_rutina`).
fn rutina_de_tarea(t: &Task) -> Option<String> {
    if t.assigned_to != "orquestador" {
        return None;
    }
    let v: Value = serde_json::from_str(&t.request).ok()?;
    v.get("rutina")?.as_str().map(str::to_string)
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

/// Vuelve a lanzar una tarea ya terminada: una tarea nueva con la misma
/// petición y la original como padre. La de una rutina se encola por la
/// herramienta de siempre, que rechaza duplicar una ejecución en cola.
async fn tarea_repetir(
    State(s): Estado,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    let original = store.get_task(id)?;
    if matches!(original.status, TaskStatus::Pendiente | TaskStatus::EnProgreso) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("la tarea #{id} sigue {}", original.status.as_str()),
        ));
    }

    let nueva = match rutina_de_tarea(&original) {
        Some(slug) => {
            slug_valido(&slug)?;
            let r = ea_acciones::ejecutar_directo(
                &mut store, &sis, ACTOR, Canal::Web, "ea_encolar_rutina", &json!({ "slug": slug }),
            )
            .await?;
            store.get_task(r.tarea.ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "sin tarea"))?)?
        }
        None => store.create_task(NewTask {
            created_by: ACTOR.into(),
            assigned_to: original.assigned_to.clone(),
            origin: TaskOrigin::Boton,
            parent_task_id: Some(id),
            title: original.title.clone(),
            request: original.request.clone(),
        })?,
    };
    Ok((StatusCode::CREATED, Json(tarea_json(&nueva))))
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
    let mut args = json!({ "id": id, "estado": body.estado });
    if let Some(r) = body.resultado.filter(|r| !r.trim().is_empty()) {
        args["resultado"] = json!(r);
    }
    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    store.get_task(id)?; // 404 si no existe
    ea_acciones::ejecutar_directo(&mut store, &sis, ACTOR, Canal::Web, "ea_cambiar_estado_tarea", &args)
        .await?;
    Ok(Json(tarea_json(&store.get_task(id)?)))
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
    Ok(Json(json!(rutinas_resumen(&s).await?)))
}

async fn rutinas_resumen(s: &AppState) -> ApiResult<Vec<Value>> {
    let dir = &s.config.rutinas_dir;
    let slugs = listar_slugs(dir)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut filas = Vec::with_capacity(slugs.len());
    for slug in slugs {
        let (descripcion, error): (Option<String>, Option<String>) = (None, None);
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
    Ok(filas)
}

async fn rutina_ver(State(s): Estado, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    slug_valido(&slug)?;
    let contenido = leer_rutina(&s.config.rutinas_dir, &slug)?;
    let ejecuciones = s.store.lock().await.list_routine_runs(&slug, Some(20))?;
    Ok(Json(json!({
        "slug": slug,
        "contenido": contenido,
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
    // Falla pronto si la rutina no existe.
    leer_rutina(&s.config.rutinas_dir, &slug)?;

    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar_directo(
        &mut store, &sis, ACTOR, Canal::Web, "ea_encolar_rutina", &json!({ "slug": slug }),
    )
    .await?;
    let tarea = store.get_task(r.tarea.ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "sin tarea"))?)?;
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
            propuesta_json(p, rutina_de(&store, p))
        })
        .collect::<Vec<_>>();
    Ok(Json(json!(filas)))
}

#[derive(Deserialize)]
struct Decision {
    decision: String,
}

/// Aceptar una propuesta con acción la ejecuta (a través de la capa interna,
/// con revalidación; el resultado queda en la propuesta y en el diario). Las
/// irreversibles no se confirman desde la web: 403. Las antiguas de texto
/// libre solo dejan constancia.
async fn propuesta_decidir(
    State(s): Estado,
    Path(id): Path<i64>,
    Json(body): Json<Decision>,
) -> ApiResult<Json<Value>> {
    let aceptar = match body.decision.as_str() {
        "aceptar" => true,
        "rechazar" => false,
        _ => return Err(ApiError::bad("decision debe ser 'aceptar' o 'rechazar'")),
    };

    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    let p = if aceptar {
        ea_acciones::aceptar_propuesta(&mut store, &sis, id, ACTOR, Canal::Web).await?
    } else {
        ea_acciones::rechazar_propuesta(&store, id, ACTOR, Canal::Web)?
    };
    Ok(Json(propuesta_json(&p, rutina_de(&store, &p))))
}

// ---------------------------------------------------------------- acciones

/// El catálogo completo, para verlo; ejecutables desde aquí solo las
/// supervisadas (las directas son de los agentes o ya tienen su sección).
async fn acciones_listar(State(s): Estado) -> ApiResult<Json<Value>> {
    let slugs = listar_slugs(&s.config.rutinas_dir).unwrap_or_default();
    let filas: Vec<Value> = ea_acciones::CATALOGO
        .iter()
        .map(|h| {
            let supervisada = h.ejecucion == ea_acciones::Ejecucion::Supervisada;
            let campos: Vec<Value> = ea_acciones::campos(h.nombre)
                .iter()
                .map(|c| {
                    // Las rutinas no son un dominio fijo: las que hay ahora.
                    let opciones: Vec<&str> = if c.nombre == "slug" {
                        slugs.iter().map(String::as_str).collect()
                    } else {
                        c.opciones.to_vec()
                    };
                    json!({
                        "nombre": c.nombre, "entero": c.entero, "obligatorio": c.obligatorio,
                        "opciones": opciones, "ayuda": c.ayuda,
                    })
                })
                .collect();
            json!({
                "nombre": h.nombre,
                "riesgo": h.riesgo.as_str(),
                "supervisada": supervisada,
                "local": h.local,
                "resumen": h.resumen,
                "campos": campos,
                "confirmable": supervisada && Canal::Web.puede_confirmar(h.riesgo).is_ok(),
            })
        })
        .collect();
    Ok(Json(json!(filas)))
}

#[derive(Deserialize)]
struct AccionBody {
    herramienta: String,
    #[serde(default)]
    argumentos: Value,
    /// Lo que devolvió la simulación; al ejecutar se revalida contra ello.
    #[serde(default)]
    precondicion: Value,
}

impl AccionBody {
    /// Sin `systemctl` no hay ni estado que simular ni forma de reiniciar.
    fn comprobar(&self, s: &AppState) -> ApiResult<()> {
        if self.herramienta == "ea_reiniciar_servicio" && !s.config.systemctl {
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "systemctl desactivado"));
        }
        Ok(())
    }
}

/// Describe lo que haría la herramienta ahora, sin hacer nada.
async fn accion_simular(State(s): Estado, Json(b): Json<AccionBody>) -> ApiResult<Json<Value>> {
    b.comprobar(&s)?;
    let sis = sistema(&s);
    let hechos = ea_acciones::observar(&sis, &b.herramienta, &b.argumentos).await?;
    let store = s.store.lock().await;
    let sim = ea_acciones::simular_con(&store, &sis, &hechos, &b.herramienta, &b.argumentos)?;
    Ok(Json(json!({
        "herramienta": sim.herramienta,
        "riesgo": sim.riesgo.as_str(),
        "argumentos": sim.argumentos,
        "texto": sim.texto,
        "precondicion": sim.precondicion,
        "confirmable": Canal::Web.puede_confirmar(sim.riesgo).is_ok(),
    })))
}

/// Ejecuta lo que se acaba de simular. Si el mundo ha cambiado desde la
/// simulación, la revalidación falla (409) y no se hace nada; las
/// irreversibles no se confirman desde la web (403). Todo queda en el diario.
async fn accion_ejecutar(State(s): Estado, Json(b): Json<AccionBody>) -> ApiResult<Json<Value>> {
    b.comprobar(&s)?;
    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar(
        &mut store, &sis, ACTOR, Canal::Web, &b.herramienta, &b.argumentos, &b.precondicion, None,
    )
    .await?;
    Ok(Json(json!({ "texto": r.texto, "tarea": r.tarea, "datos": r.datos })))
}

// ------------------------------------------------------------------ bóveda

/// Cotas de la interfaz del usuario: más holgadas que las de los agentes,
/// pero acotadas (la página no pinta cientos de notas).
const LIMITES_WEB: ea_boveda::Limites = ea_boveda::Limites { max_resultados: 50, ..ea_boveda::Limites::AMPLIOS };

impl From<ea_boveda::ErrorBoveda> for ApiError {
    fn from(e: ea_boveda::ErrorBoveda) -> Self {
        ApiError::from(ErrorAccion::from(e))
    }
}

#[derive(Deserialize)]
struct BovedaQuery {
    q: Option<String>,
    region: Option<String>,
    limite: Option<usize>,
}

fn nota_resumen_json(r: &ea_store::ResultadoBusqueda) -> Value {
    json!({
        "ruta": r.ruta, "region": r.region, "autor": r.autor, "titulo": r.titulo,
        "actualizada": r.actualizada, "promocion": r.promocion, "fragmento": r.fragmento,
    })
}

/// Con `q`, búsqueda de texto completo; sin ella, las notas más recientes.
async fn boveda_listar(State(s): Estado, Query(q): Query<BovedaQuery>) -> ApiResult<Json<Value>> {
    let mut store = s.store.lock().await;
    let region = q.region.as_deref().filter(|r| !r.is_empty());
    let filas = match q.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        // Una consulta sin términos buscables (solo signos) no es un error aquí:
        // se escribe letra a letra en el cuadro de filtro.
        Some(texto) => match s.boveda.buscar(&mut store, texto, region, q.limite, &LIMITES_WEB) {
            Err(ea_boveda::ErrorBoveda::Argumentos(_)) => Vec::new(),
            otro => otro?,
        },
        None => s.boveda.listar(&mut store, region, q.limite, &LIMITES_WEB)?,
    };
    Ok(Json(json!({
        "raiz": s.boveda.raiz().display().to_string(),
        "notas": filas.iter().map(nota_resumen_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct NotaQuery {
    ruta: String,
    desde: Option<usize>,
}

async fn boveda_leer(State(s): Estado, Query(q): Query<NotaQuery>) -> ApiResult<Json<Value>> {
    let n = s.boveda.leer(&q.ruta, q.desde.unwrap_or(0), &LIMITES_WEB)?;
    Ok(Json(json!({
        "ruta": n.ruta, "region": n.region,
        "cabecera": n.cabecera.as_ref().map(|c| json!({
            "titulo": c.titulo, "autor": c.autor, "creada": c.creada, "actualizada": c.actualizada,
            "origen": c.origen, "promocion": c.promocion, "tags": c.tags,
            "promovida_de": c.promovida_de, "promovida_a": c.promovida_a,
        })),
        "aviso_cabecera": n.aviso_cabecera,
        "contenido": n.contenido, "total_caracteres": n.total_caracteres,
        "truncado": n.truncado, "siguiente_desde": n.siguiente_desde,
    })))
}

#[derive(Deserialize)]
struct PromoverBody {
    ruta: String,
    destino: Option<String>,
}

/// Promoción directa del usuario desde la página (el gesto es la
/// confirmación; la web la pide con un segundo clic). Misma herramienta, mismo
/// flujo y mismo rastro en el diario que aceptar la propuesta de un agente.
async fn boveda_promover(State(s): Estado, Json(b): Json<PromoverBody>) -> ApiResult<Json<Value>> {
    let sis = sistema(&s);
    let mut args = json!({ "ruta": b.ruta });
    if let Some(d) = b.destino.filter(|d| !d.trim().is_empty()) {
        args["destino"] = json!(d);
    }
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar_directo(&mut store, &sis, ACTOR, Canal::Web, "ea_promover_nota", &args).await?;
    Ok(Json(json!({ "texto": r.texto, "datos": r.datos })))
}

// ---------------------------------------------------------------- servicios

async fn servicio_reiniciar(State(s): Estado, Path(nombre): Path<String>) -> ApiResult<Json<Value>> {
    if !estado::SERVICIOS.contains(&nombre.as_str()) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "servicio desconocido"));
    }
    if !s.config.systemctl {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "systemctl desactivado"));
    }

    let sis = sistema(&s);
    let mut store = s.acciones.lock().await;
    ea_acciones::ejecutar_directo(
        &mut store, &sis, ACTOR, Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": nombre }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
