//! Las rutas JSON: extraen la petición, llaman a `servicio` y devuelven su
//! vista tal cual.

use std::sync::Arc;

use axum::body::Bytes;
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
    /// `ok` o `error`.
    resultado: Option<String>,
    /// Prefijo de la acción (`bus_`, `propuesta_`…): solo `[a-z_]+`.
    accion: Option<String>,
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
        resultado: q.resultado,
        accion: q.accion,
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

/// `{"entradas": {"clave": "valor"}}`; un cuerpo vacío es lo mismo que `{}`.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct EntradasBody {
    entradas: servicio::rutinas::Entradas,
}

fn entradas_de(cuerpo: &[u8]) -> Result<EntradasBody, ApiError> {
    if cuerpo.iter().all(u8::is_ascii_whitespace) {
        return Ok(EntradasBody::default());
    }
    serde_json::from_slice(cuerpo).map_err(|e| {
        ApiError::new(StatusCode::BAD_REQUEST, format!("cuerpo inválido: se espera {{\"entradas\": {{\"clave\": \"valor\"}}}} ({e})"))
    })
}

pub async fn rutina_lanzar(
    State(s): Estado,
    Path(slug): Path<String>,
    cuerpo: Bytes,
) -> Result<(StatusCode, Json<RutinaLanzada>), ApiError> {
    let b = entradas_de(&cuerpo)?;
    Ok((StatusCode::CREATED, Json(servicio::rutinas::lanzar(&s, &slug, &b.entradas).await?)))
}

pub async fn rutina_previa(State(s): Estado, Path(slug): Path<String>, cuerpo: Bytes) -> Api<RutinaPrevia> {
    let b = entradas_de(&cuerpo)?;
    ok(servicio::rutinas::previa(&s, &slug, &b.entradas).await)
}

/// `{"toml": "…"}`: la definición entera de una rutina.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TomlBody {
    toml: String,
}

fn toml_de(cuerpo: &[u8]) -> Result<String, ApiError> {
    // Holgura para el escapado JSON; el límite de verdad lo pone el bus.
    if cuerpo.len() > 4 * ea_bus::MAX_DEFINICION {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("cuerpo demasiado largo (la definición admite como mucho {} bytes)", ea_bus::MAX_DEFINICION),
        ));
    }
    serde_json::from_slice::<TomlBody>(cuerpo)
        .map(|b| b.toml)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("cuerpo inválido: se espera {{\"toml\": \"…\"}} ({e})")))
}

pub async fn rutina_validar(State(s): Estado, cuerpo: Bytes) -> Api<RutinaValidada> {
    ok(servicio::rutinas::validar(&s, &toml_de(&cuerpo)?).await)
}

pub async fn rutina_registrar(State(s): Estado, cuerpo: Bytes) -> Result<(StatusCode, Json<RutinaRegistrada>), ApiError> {
    let toml = toml_de(&cuerpo)?;
    Ok((StatusCode::CREATED, Json(servicio::rutinas::registrar(&s, &toml).await?)))
}

/// `/api/rutinas/validar` y `/registrar` son rutas fijas que tapan las de
/// una rutina con ese slug: su GET sigue siendo la ficha de la rutina.
pub async fn rutina_ver_validar(s: Estado) -> Api<RutinaDetalle> {
    rutina_ver(s, Path("validar".into())).await
}

pub async fn rutina_ver_registrar(s: Estado) -> Api<RutinaDetalle> {
    rutina_ver(s, Path("registrar".into())).await
}

pub async fn rutina_versiones(State(s): Estado, Path(slug): Path<String>) -> Api<Vec<RutinaVersionFila>> {
    ok(servicio::rutinas::versiones(&s, &slug).await)
}

pub async fn rutina_version(State(s): Estado, Path((slug, n)): Path<(String, String)>) -> Api<RutinaVersion> {
    ok(servicio::rutinas::version(&s, &slug, &n).await)
}

// ---------------------------------------------------------------- trabajos

pub async fn trabajos_listar(State(s): Estado) -> Api<Vec<TrabajoResumen>> {
    ok(servicio::trabajos::listar(&s))
}

pub async fn trabajo_crear(State(s): Estado, Json(b): Json<Value>) -> Result<(StatusCode, Json<EstadoTrabajo>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::trabajos::crear(&s, b).await?)))
}

pub async fn trabajo_formulario(State(s): Estado) -> Api<FormularioTrabajo> {
    ok(servicio::trabajos::formulario(&s))
}

pub async fn trabajo_ver(State(s): Estado, Path(id): Path<String>) -> Api<TrabajoDetalle> {
    ok(servicio::trabajos::ver(&s, &id).await)
}

/// `?fase=N`: el diff de esa fase de una obra (entero ≥ 1; si no, 400).
#[derive(Deserialize)]
pub struct DiffQuery {
    fase: Option<u32>,
}

pub async fn trabajo_diff(State(s): Estado, Path(id): Path<String>, Query(q): Query<DiffQuery>) -> Api<Diff> {
    ok(servicio::trabajos::diff(&s, &id, q.fase).await)
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NotaUsuario {
    nota: String,
    contexto: String,
    /// Reformular una obra: fase desde la que se rehace.
    fase: Option<u32>,
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
    decidir(&s, &id, Decision::Reformular { nota: &b.nota, fase: b.fase }).await
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

pub async fn boveda_arbol(State(s): Estado) -> Api<ArbolBoveda> {
    ok(servicio::boveda::arbol(&s).await)
}

#[derive(Deserialize)]
pub struct RutaQuery {
    ruta: String,
}

pub async fn boveda_fichero(State(s): Estado, Query(q): Query<RutaQuery>) -> Api<NotaFichero> {
    ok(servicio::boveda::fichero(&s, &q.ruta))
}

#[derive(Deserialize)]
pub struct GuardarNotaBody {
    ruta: String,
    contenido: String,
    /// La versión con la que se abrió: si en disco ya es otra, 409.
    version: String,
}

pub async fn boveda_guardar(State(s): Estado, Json(b): Json<GuardarNotaBody>) -> Api<NotaFichero> {
    ok(servicio::boveda::guardar(&s, &b.ruta, &b.contenido, &b.version).await)
}

#[derive(Deserialize)]
pub struct CrearNotaBody {
    ruta: String,
    #[serde(default)]
    contenido: String,
}

pub async fn boveda_crear(State(s): Estado, Json(b): Json<CrearNotaBody>) -> Result<(StatusCode, Json<NotaFichero>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::boveda::crear(&s, &b.ruta, &b.contenido).await?)))
}

#[derive(Deserialize)]
pub struct RenombrarNotaBody {
    ruta: String,
    nuevo: String,
    version: String,
}

pub async fn boveda_renombrar(State(s): Estado, Json(b): Json<RenombrarNotaBody>) -> Api<RenombradoBoveda> {
    ok(servicio::boveda::renombrar(&s, &b.ruta, &b.nuevo, &b.version).await)
}

#[derive(Deserialize)]
pub struct EliminarNotaBody {
    ruta: String,
    version: String,
}

pub async fn boveda_eliminar(State(s): Estado, Json(b): Json<EliminarNotaBody>) -> Api<EnPapelera> {
    ok(servicio::boveda::eliminar(&s, &b.ruta, &b.version).await)
}

#[derive(Deserialize)]
pub struct RutaBody {
    ruta: String,
}

pub async fn boveda_carpeta(State(s): Estado, Json(b): Json<RutaBody>) -> Result<(StatusCode, Json<Hecho>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::boveda::carpeta(&s, &b.ruta).await?)))
}

pub async fn boveda_mostrar(State(s): Estado, Json(b): Json<RutaBody>) -> Api<Hecho> {
    ok(servicio::boveda::mostrar(&s, &b.ruta).await)
}

pub async fn boveda_papelera(State(s): Estado) -> Api<Vec<EnPapelera>> {
    ok(servicio::boveda::papelera(&s))
}

#[derive(Deserialize)]
pub struct RestaurarBody {
    id: String,
}

pub async fn boveda_restaurar(State(s): Estado, Json(b): Json<RestaurarBody>) -> Api<NotaFichero> {
    ok(servicio::boveda::restaurar(&s, &b.id).await)
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

// ----------------------------------------------------------------- ajustes

pub async fn ajustes_ver(State(s): Estado) -> Api<AjustesVista> {
    Ok(Json(servicio::ajustes::ver(&s)))
}

#[derive(Deserialize)]
pub struct CuerpoAjustes {
    /// Huella de `ajustes.toml` que se leyó ("" si no existía).
    version: String,
    /// Clave → valor nuevo; `null` la quita (vuelve a la capa siguiente).
    cambios: std::collections::BTreeMap<String, Option<Value>>,
}

pub async fn ajustes_cambiar(State(s): Estado, Json(b): Json<CuerpoAjustes>) -> Api<AjustesCambiados> {
    ok(servicio::ajustes::cambiar(&s, &b.version, b.cambios).await)
}
