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
use ea_bus::{Agente, Bus, ErrorBus, Evento};
use ea_store::{EstadoPropuesta, Event, EventFilter, Propuesta, PropuestaFilter, StoreError};
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::{estado, tema, AppState};

const ACTOR: &str = "usuario";
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
            StoreError::EventNotFound(_) | StoreError::PropuestaNotFound(_) => StatusCode::NOT_FOUND,
            StoreError::InvalidStatus(_) | StoreError::InvalidOutcome(_) => StatusCode::BAD_REQUEST,
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
            StoreError::EventNotFound(_) | StoreError::PropuestaNotFound(_) => StatusCode::NOT_FOUND,
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

impl From<ErrorBus> for ApiError {
    fn from(e: ErrorBus) -> Self {
        let status = match &e {
            ErrorBus::NoExiste(_) => StatusCode::NOT_FOUND,
            ErrorBus::TransicionInvalida { .. } | ErrorBus::Ocupado => StatusCode::CONFLICT,
            ErrorBus::Argumentos(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
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
        .route("/api/inicio", get(inicio))
        .route("/api/diario", get(diario))
        .route("/api/rutinas", get(rutinas_listar))
        .route("/api/rutinas/{slug}", get(rutina_ver))
        .route("/api/rutinas/{slug}/lanzar", post(rutina_lanzar))
        .route("/api/trabajos", get(trabajos_listar))
        .route("/api/trabajos/{id}", get(trabajo_ver))
        .route("/api/trabajos/{id}/diff", get(trabajo_diff))
        .route("/api/trabajos/{id}/cerrar", post(trabajo_cerrar))
        .route("/api/trabajos/{id}/cancelar", post(trabajo_cancelar))
        .route("/api/trabajos/{id}/continuar", post(trabajo_continuar))
        .route("/api/trabajos/{id}/reformular", post(trabajo_reformular))
        .route("/api/trabajos/{id}/confirmar", post(trabajo_confirmar))
        .route("/api/colas", get(colas))
        .route("/api/semaforos", get(semaforos))
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

fn evento_json(e: &Event) -> Value {
    json!({
        "id": e.id,
        "ts": ts(e.ts),
        "actor": e.actor,
        "accion": e.action,
        "trabajo": e.trabajo,
        "resultado": e.outcome.map(|o| o.as_str()),
        "duracion_ms": e.duration_ms,
        "detalle": e.detail,
    })
}

fn propuesta_json(p: &Propuesta) -> Value {
    json!({
        "id": p.id,
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

// ------------------------------------------------------------------ estado

async fn estado_general(State(s): Estado) -> ApiResult<Json<Value>> {
    let servicios = estado::servicios(s.config.systemctl).await;

    let store = s.store.lock().await;
    let orquestador = store.estado_orquestador(s.config.umbral_heartbeat_s)?;

    let agentes: Vec<Value> = store
        .resumen_agentes()?
        .into_iter()
        .map(|a| {
            json!({
                "nombre": a.nombre,
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
        "trabajos": trabajos_por_estado(),
        "propuestas_pendientes": propuestas_pendientes,
        "db": ea_store::resolve_db_path().ok().map(|p| p.display().to_string()),
    })))
}

/// Las propuestas pendientes. Los trabajos del orquestador van por
/// `/api/trabajos`, `/api/colas` y `/api/semaforos`.
async fn inicio(State(s): Estado) -> ApiResult<Json<Value>> {
    let store = s.store.lock().await;
    let propuestas = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: Some(20) })?
        .iter()
        .map(propuesta_json)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "propuestas": propuestas })))
}

/// Trabajos del bus por estado; vacío si el bus no se puede leer.
fn trabajos_por_estado() -> Value {
    const ESTADOS: [&str; 6] = ["en_cola", "ejecutando", "en_revision", "en_espera", "cerrado", "cancelado"];
    let mut cuenta: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    if let Ok(bus) = Bus::abrir() {
        for id in bus.listar_trabajos().unwrap_or_default() {
            if let Ok(e) = bus.leer_estado(&id) {
                *cuenta.entry(e.estado.as_str()).or_default() += 1;
            }
        }
    }
    let m: serde_json::Map<String, Value> =
        ESTADOS.iter().map(|e| (e.to_string(), json!(cuenta.get(e).copied().unwrap_or(0)))).collect();
    Value::Object(m)
}

// ------------------------------------------------------------------ diario

#[derive(Deserialize)]
struct DiarioQuery {
    trabajo: Option<String>,
    actor: Option<String>,
    desde: Option<String>,
    hasta: Option<String>,
    /// `1` incluye los heartbeats del orquestador (por defecto se omiten).
    latidos: Option<String>,
    limite: Option<i64>,
}

async fn diario(State(s): Estado, Query(q): Query<DiarioQuery>) -> ApiResult<Json<Value>> {
    let filter = EventFilter {
        trabajo: q.trabajo.filter(|t| !t.is_empty()),
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
    let filas = s.store.lock().await.listar_rutinas()?;
    let mut out = Vec::with_capacity(filas.len());
    for f in filas {
        let r = ea_bus::Rutina::parsear(&f.definicion).ok();
        let proxima = if s.config.systemctl && ea_bus::slug_valido(&f.slug) {
            estado::proxima_ejecucion(&f.slug).await
        } else {
            None
        };
        out.push(json!({
            "slug": f.slug,
            "version": f.version,
            "descripcion": r.as_ref().map(|r| r.rutina.descripcion.clone()),
            "worktree": r.as_ref().map(|r| r.rutina.worktree.clone()),
            "criterio_exito": r.as_ref().map(|r| r.rutina.criterio_exito.clone()),
            "entradas": r.as_ref().map(|r| r.entradas.clone()),
            "error": if r.is_none() { Some("definición ilegible") } else { None },
            "registrada": f.creada_en,
            "proxima": proxima,
        }));
    }
    Ok(Json(json!(out)))
}

async fn rutina_ver(State(s): Estado, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    slug_valido(&slug)?;
    let fila = s
        .store
        .lock()
        .await
        .ultima_rutina(&slug)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("la rutina '{slug}' no está registrada")))?;
    Ok(Json(json!({ "slug": slug, "version": fila.version, "contenido": fila.definicion })))
}

/// Lo mismo que hace el disparador de systemd (`ea rutina lanzar`): crea el
/// trabajo en el bus y el orquestador lo recoge. `{ "trabajo": id, "rutina": slug }`.
async fn rutina_lanzar(
    State(s): Estado,
    Path(slug): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    slug_valido(&slug)?;
    let bus = bus_con_store(&s).await?;
    bus.rutina(&slug).map_err(|_| ApiError::new(StatusCode::NOT_FOUND, format!("la rutina '{slug}' no está registrada")))?;
    if !bus.trabajos_activos_de_rutina(&slug)?.is_empty() {
        return Err(ApiError::new(StatusCode::CONFLICT, format!("la rutina '{slug}' ya tiene una ejecución en curso")));
    }
    let id = bus.lanzar_rutina(&slug, &[])?;
    Ok((StatusCode::CREATED, Json(json!({ "trabajo": id, "rutina": slug }))))
}

// --------------------------------------------------------------- orquestador

/// Bus de solo lectura, sobre la raíz de estado estándar.
fn bus_lectura() -> ApiResult<Bus> {
    Ok(Bus::abrir()?)
}

/// Bus con el historial y el diario de SQLite, para las acciones del usuario.
async fn bus_con_store(s: &AppState) -> ApiResult<Bus> {
    let store = s.acciones.lock().await.duplicar()?;
    Ok(Bus::abrir()?.con_actor(ACTOR).con_store(store))
}

fn trabajo_resumen(bus: &Bus, id: &str) -> Option<Value> {
    let orden = bus.leer_orden(id).ok()?;
    let estado = bus.leer_estado(id).ok()?;
    let informe = bus.leer_informe(id).ok().flatten();
    Some(json!({
        "id": id,
        "modo": orden.modo.as_str(),
        "titulo": orden.titulo,
        "estado": estado.estado.as_str(),
        "fase_actual": estado.fase_actual,
        "fases_total": orden.fases.len(),
        "iteracion": estado.iteracion,
        "detalle": estado.detalle,
        "resultado": informe.as_ref().map(|i| match i.resultado {
            ea_bus::ResultadoInforme::Terminado => "terminado",
            ea_bus::ResultadoInforme::Bloqueado => "bloqueado",
        }),
        "obra_completa": informe.as_ref().map(|i| i.obra_completa),
        "rutina": orden.rutina,
        "creado": orden.creada_en,
        "actualizado": estado.actualizado_en,
    }))
}

async fn trabajos_listar() -> ApiResult<Json<Value>> {
    let bus = bus_lectura()?;
    let mut v: Vec<Value> = bus.listar_trabajos()?.iter().filter_map(|id| trabajo_resumen(&bus, id)).collect();
    v.reverse(); // el más reciente primero
    Ok(Json(json!(v)))
}

async fn trabajo_ver(Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let bus = bus_lectura()?;
    let orden = bus.leer_orden(&id)?;
    let estado = bus.leer_estado(&id)?;
    let mut informe = serde_json::to_value(bus.leer_informe(&id)?)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(o) = informe.as_object_mut() {
        o.remove("diff"); // el diff va aparte, bajo demanda
    }
    let dir = bus.raiz().join("trabajos").join(&id);
    let historial: Vec<Value> = std::fs::read_to_string(dir.join("historial.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let paquete = std::fs::read_to_string(dir.join("paquete-correccion.md")).ok();
    Ok(Json(json!({
        "id": id,
        "orden": orden,
        "estado": estado,
        "informe": informe,
        "historial": historial,
        "paquete": paquete,
    })))
}

const DIFF_MAX: usize = 200_000;

async fn git(wt: &str, args: &[&str]) -> ApiResult<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(wt).args(args).kill_on_drop(true);
    let o = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
        .await
        .map_err(|_| ApiError::new(StatusCode::GATEWAY_TIMEOUT, "git no respondió"))?
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !o.status.success() {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Lo mismo que `ea trabajo diff`: lo cambiado desde `commit_inicio` más los
/// ficheros nuevos sin seguimiento. Recortado a `DIFF_MAX` bytes.
async fn trabajo_diff(Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let bus = bus_lectura()?;
    let orden = bus.leer_orden(&id)?;
    let inicio = bus
        .leer_informe(&id)?
        .and_then(|i| i.commit_inicio)
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "el trabajo aún no tiene informe con commit_inicio"))?;
    let wt = orden.worktree.as_str();
    let mut texto = git(wt, &["diff", &inicio, "--", ".", ":(exclude,glob)**/__pycache__/**"]).await?;
    let nuevos = git(wt, &["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    for f in nuevos.split('\0').filter(|f| !f.is_empty() && !f.contains("__pycache__")) {
        match std::fs::read(std::path::Path::new(wt).join(f)) {
            Ok(b) if std::str::from_utf8(&b).is_ok() => {
                texto.push_str(&format!("\n=== fichero nuevo sin seguimiento: {f} ===\n{}", String::from_utf8_lossy(&b)))
            }
            _ => texto.push_str(&format!("\n=== fichero nuevo sin seguimiento: {f} (binario o ilegible) ===\n")),
        }
    }
    let truncado = texto.len() > DIFF_MAX;
    if truncado {
        let mut fin = DIFF_MAX;
        while !texto.is_char_boundary(fin) {
            fin -= 1;
        }
        texto.truncate(fin);
    }
    Ok(Json(json!({ "id": id, "commit_inicio": inicio, "diff": texto, "truncado": truncado })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NotaUsuario {
    nota: String,
    contexto: String,
}

fn estado_json(id: &str, e: &ea_bus::Estado) -> Json<Value> {
    Json(json!({ "id": id, "estado": e.estado.as_str() }))
}

async fn trabajo_cerrar(State(s): Estado, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let e = bus_con_store(&s).await?.aplicar(&id, Evento::Cerrar)?;
    Ok(estado_json(&id, &e))
}

async fn trabajo_cancelar(State(s): Estado, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let e = bus_con_store(&s).await?.aplicar(&id, Evento::Cancelar)?;
    Ok(estado_json(&id, &e))
}

/// Aprueba una revisión intermedia: reformular sin nota (como `ea trabajo continuar`).
async fn trabajo_continuar(State(s): Estado, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let e = bus_con_store(&s).await?.reformular(&id, "")?;
    Ok(estado_json(&id, &e))
}

async fn trabajo_reformular(
    State(s): Estado,
    Path(id): Path<String>,
    Json(b): Json<NotaUsuario>,
) -> ApiResult<Json<Value>> {
    let nota = texto_no_vacio(&b.nota, "nota", 10_000)?;
    let e = bus_con_store(&s).await?.reformular(&id, &nota)?;
    Ok(estado_json(&id, &e))
}

/// El usuario confirma el paquete de corrección de una rutina en espera.
async fn trabajo_confirmar(
    State(s): Estado,
    Path(id): Path<String>,
    Json(b): Json<NotaUsuario>,
) -> ApiResult<Json<Value>> {
    if b.contexto.chars().count() > 10_000 {
        return Err(ApiError::bad("'contexto' supera 10000 caracteres"));
    }
    let e = bus_con_store(&s).await?.confirmar(&id, &b.contexto)?;
    Ok(estado_json(&id, &e))
}

async fn colas() -> ApiResult<Json<Value>> {
    let bus = bus_lectura()?;
    let mut m = serde_json::Map::new();
    for a in [Agente::ModeloLocal, Agente::ClaudeCode] {
        let v: Vec<Value> = bus
            .listar_cola(a)?
            .into_iter()
            .map(|e| {
                let titulo = bus.leer_orden(&e.trabajo).map(|o| o.titulo).ok();
                json!({ "id": e.id, "trabajo": e.trabajo, "titulo": titulo, "motivo": e.motivo, "encolado_en": e.encolado_en })
            })
            .collect();
        m.insert(a.as_str().into(), v.into());
    }
    Ok(Json(Value::Object(m)))
}

async fn semaforos() -> ApiResult<Json<Value>> {
    let v: Vec<Value> = ea_bus::listar_semaforos()
        .into_iter()
        .map(|(_, o)| json!({ "agente": o.agente, "trabajo": o.trabajo, "pid": o.pid, "desde": o.desde }))
        .collect();
    Ok(Json(json!(v)))
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
        .map(propuesta_json)
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
    Ok(Json(propuesta_json(&p)))
}

// ---------------------------------------------------------------- acciones

/// El catálogo completo, para verlo; ejecutables desde aquí solo las
/// supervisadas (las directas son de los agentes o ya tienen su sección).
async fn acciones_listar(State(s): Estado) -> ApiResult<Json<Value>> {
    let slugs: Vec<String> = s.store.lock().await.listar_rutinas()?.into_iter().map(|r| r.slug).collect();
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
    Ok(Json(json!({ "texto": r.texto, "trabajo": r.trabajo, "datos": r.datos })))
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
