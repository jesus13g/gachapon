//! Las rutas JSON: extraen la petición, llaman a `servicio` y devuelven su
//! vista tal cual.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use super::ApiError;
use crate::servicio::trabajos::Decision;
use crate::servicio::{self, diario::Filtro};
use crate::vistas::*;
use crate::AppState;

type Estado = State<Arc<AppState>>;
type Api<T> = Result<Json<T>, ApiError>;

fn ok<T>(r: servicio::Resultado<T>) -> Api<T> {
    Ok(Json(r?))
}

// ------------------------------------------------------------------ estado

pub async fn estado_general(State(s): Estado) -> Api<EstadoGeneral> {
    ok(servicio::estado::general(&s).await)
}

pub async fn inicio(State(s): Estado) -> Api<Inicio> {
    ok(servicio::estado::inicio(&s).await)
}

pub async fn sesiones(State(s): Estado) -> Json<Sesiones> {
    Json(servicio::estado::sesiones(&s))
}

// ------------------------------------------------------------------ diario

#[derive(Deserialize)]
pub struct DiarioQuery {
    trabajo: Option<String>,
    actor: Option<String>,
    desde: Option<String>,
    hasta: Option<String>,
    /// `1` incluye los heartbeats del orquestador (por defecto se omiten).
    latidos: Option<String>,
    limite: Option<i64>,
}

pub async fn diario(State(s): Estado, Query(q): Query<DiarioQuery>) -> Api<Vec<EventoVista>> {
    let filtro = Filtro {
        trabajo: q.trabajo,
        actor: q.actor,
        desde: q.desde,
        hasta: q.hasta,
        latidos: q.latidos.as_deref() == Some("1"),
        limite: q.limite,
    };
    ok(servicio::diario::listar(&s, filtro).await)
}

// ----------------------------------------------------------------- rutinas

pub async fn rutinas_listar(State(s): Estado) -> Api<Vec<RutinaFila>> {
    ok(servicio::rutinas::listar(&s).await)
}

pub async fn rutina_ver(State(s): Estado, Path(slug): Path<String>) -> Api<RutinaDetalle> {
    ok(servicio::rutinas::ver(&s, &slug).await)
}

pub async fn rutina_lanzar(
    State(s): Estado,
    Path(slug): Path<String>,
) -> Result<(StatusCode, Json<RutinaLanzada>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::rutinas::lanzar(&s, &slug).await?)))
}

// ---------------------------------------------------------------- trabajos

pub async fn trabajos_listar(State(s): Estado) -> Api<Vec<TrabajoResumen>> {
    ok(servicio::trabajos::listar(&s))
}

pub async fn trabajo_ver(State(s): Estado, Path(id): Path<String>) -> Api<TrabajoDetalle> {
    ok(servicio::trabajos::ver(&s, &id))
}

pub async fn trabajo_diff(State(s): Estado, Path(id): Path<String>) -> Api<Diff> {
    ok(servicio::trabajos::diff(&s, &id).await)
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NotaUsuario {
    nota: String,
    contexto: String,
}

async fn decidir(s: &AppState, id: &str, d: Decision<'_>) -> Api<EstadoTrabajo> {
    ok(servicio::trabajos::decidir(s, id, d).await)
}

pub async fn trabajo_cerrar(State(s): Estado, Path(id): Path<String>) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Cerrar).await
}

pub async fn trabajo_cancelar(State(s): Estado, Path(id): Path<String>) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Cancelar).await
}

pub async fn trabajo_continuar(State(s): Estado, Path(id): Path<String>) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Continuar).await
}

pub async fn trabajo_reformular(
    State(s): Estado,
    Path(id): Path<String>,
    Json(b): Json<NotaUsuario>,
) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Reformular { nota: &b.nota }).await
}

pub async fn trabajo_confirmar(
    State(s): Estado,
    Path(id): Path<String>,
    Json(b): Json<NotaUsuario>,
) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Confirmar { contexto: &b.contexto }).await
}

pub async fn colas(State(s): Estado) -> Api<Colas> {
    ok(servicio::trabajos::colas(&s))
}

pub async fn semaforos(State(s): Estado) -> Json<Vec<Semaforo>> {
    Json(servicio::trabajos::semaforos(&s))
}

// -------------------------------------------------------------- propuestas

#[derive(Deserialize)]
pub struct PropuestasQuery {
    estado: Option<String>,
}

pub async fn propuestas_listar(State(s): Estado, Query(q): Query<PropuestasQuery>) -> Api<Vec<PropuestaVista>> {
    ok(servicio::propuestas::listar(&s, q.estado.as_deref()).await)
}

#[derive(Deserialize)]
pub struct DecisionBody {
    decision: String,
}

pub async fn propuesta_decidir(
    State(s): Estado,
    Path(id): Path<i64>,
    Json(b): Json<DecisionBody>,
) -> Api<PropuestaVista> {
    ok(servicio::propuestas::decidir(&s, id, &b.decision).await)
}

// ---------------------------------------------------------------- acciones

pub async fn acciones_listar(State(s): Estado) -> Api<Vec<AccionCatalogo>> {
    ok(servicio::acciones::catalogo(&s).await)
}

#[derive(Deserialize)]
pub struct AccionBody {
    herramienta: String,
    #[serde(default)]
    argumentos: Value,
    /// Lo que devolvió la simulación; al ejecutar se revalida contra ello.
    #[serde(default)]
    precondicion: Value,
}

pub async fn accion_simular(State(s): Estado, Json(b): Json<AccionBody>) -> Api<Simulacion> {
    ok(servicio::acciones::simular(&s, &b.herramienta, &b.argumentos).await)
}

pub async fn accion_ejecutar(State(s): Estado, Json(b): Json<AccionBody>) -> Api<ResultadoAccion> {
    ok(servicio::acciones::ejecutar(&s, &b.herramienta, &b.argumentos, &b.precondicion).await)
}

// ------------------------------------------------------------------ bóveda

#[derive(Deserialize)]
pub struct BovedaQuery {
    q: Option<String>,
    region: Option<String>,
    limite: Option<usize>,
}

pub async fn boveda_listar(State(s): Estado, Query(q): Query<BovedaQuery>) -> Api<ListadoBoveda> {
    ok(servicio::boveda::listar(&s, q.q.as_deref(), q.region.as_deref(), q.limite).await)
}

#[derive(Deserialize)]
pub struct NotaQuery {
    ruta: String,
    desde: Option<usize>,
}

pub async fn boveda_leer(State(s): Estado, Query(q): Query<NotaQuery>) -> Api<NotaVista> {
    ok(servicio::boveda::leer(&s, &q.ruta, q.desde.unwrap_or(0)))
}

#[derive(Deserialize)]
pub struct PromoverBody {
    ruta: String,
    destino: Option<String>,
}

pub async fn boveda_promover(State(s): Estado, Json(b): Json<PromoverBody>) -> Api<Promocion> {
    ok(servicio::boveda::promover(&s, &b.ruta, b.destino.as_deref()).await)
}

// ----------------------------------------------------------------- memoria

pub async fn memoria_listar(State(s): Estado) -> Api<ListadoMemoria> {
    ok(servicio::memoria::listar(&s))
}

pub async fn memoria_leer(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>) -> Api<Memoria> {
    ok(servicio::memoria::leer(&s, &proyecto, &fichero))
}

#[derive(Deserialize)]
pub struct MemoriaBody {
    contenido: String,
    /// La versión con la que se abrió: si en disco ya es otra, 409.
    version: String,
}

pub async fn memoria_guardar(
    State(s): Estado,
    Path((proyecto, fichero)): Path<(String, String)>,
    Json(b): Json<MemoriaBody>,
) -> Api<Memoria> {
    ok(servicio::memoria::guardar(&s, &proyecto, &fichero, &b.contenido, &b.version).await)
}

#[derive(Deserialize)]
pub struct VersionBody {
    version: String,
}

pub async fn memoria_eliminar(
    State(s): Estado,
    Path((proyecto, fichero)): Path<(String, String)>,
    Json(b): Json<VersionBody>,
) -> Api<Hecho> {
    ok(servicio::memoria::eliminar(&s, &proyecto, &fichero, &b.version).await)
}

#[derive(Deserialize)]
pub struct RenombrarBody {
    nuevo: String,
    version: String,
}

pub async fn memoria_renombrar(
    State(s): Estado,
    Path((proyecto, fichero)): Path<(String, String)>,
    Json(b): Json<RenombrarBody>,
) -> Api<Renombrado> {
    ok(servicio::memoria::renombrar(&s, &proyecto, &fichero, &b.nuevo, &b.version).await)
}

pub async fn memoria_mostrar(
    State(s): Estado,
    Path((proyecto, fichero)): Path<(String, String)>,
    Json(_): Json<Value>,
) -> Api<Hecho> {
    ok(servicio::memoria::mostrar(&s, &proyecto, &fichero).await)
}

// --------------------------------------------------------------- servicios

pub async fn servicio_reiniciar(State(s): Estado, Path(nombre): Path<String>) -> Api<Hecho> {
    ok(servicio::servicios::reiniciar(&s, &nombre).await)
}
