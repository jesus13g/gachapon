//! Las rutas JSON: extraen la petición (los cuerpos, con [`Cuerpo`] y los
//! tipos de `vistas`), llaman a `servicio` y devuelven su vista tal cual.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::{ApiError, Cuerpo};
use crate::AppState;
use crate::servicio::trabajos::Decision;
use crate::servicio::{self, diario::Filtro};
use crate::vistas::*;

type Estado = State<Arc<AppState>>;
type Api<T> = Result<Json<T>, ApiError>;

fn ok<T>(r: servicio::Resultado<T>) -> Api<T> {
    Ok(Json(r?))
}

// ------------------------------------------------------------------ estado

pub async fn estado_general(State(s): Estado) -> Api<EstadoGeneral> {
    ok(servicio::estado::general(&s).await)
}

pub async fn agentes(State(s): Estado) -> Json<AgentesVista> {
    Json(servicio::estado::agentes(&s).await)
}

pub async fn inicio(State(s): Estado) -> Api<Inicio> {
    ok(servicio::estado::inicio(&s).await)
}

pub async fn sesiones(State(s): Estado) -> Json<Sesiones> {
    Json(servicio::estado::sesiones(&s))
}

/// Los agentes vivos y dónde está cada trabajo activo, para la escena.
pub async fn ecosistema(State(s): Estado) -> Json<Ecosistema> {
    Json(servicio::ecosistema::vista(&s).await)
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
    /// `1` incluye los heartbeats que el orquestador anotaba antes en el diario
    /// (por defecto se omiten; desde U-07 el latido va a un fichero del bus).
    latidos: Option<String>,
    /// Cursor «cargar más»: id del último evento ya cargado.
    antes_de: Option<String>,
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
        antes_de: q.antes_de,
        limite: q.limite,
    };
    ok(servicio::diario::listar(&s, filtro).await)
}

pub async fn diario_anotar(State(s): Estado, Cuerpo(b): Cuerpo<AnotarBody>) -> Result<(StatusCode, Json<EventoVista>), ApiError> {
    let nota = ea_store::NuevaNota { texto: b.texto, corrige: b.corrige, trabajo: b.trabajo };
    Ok((StatusCode::CREATED, Json(servicio::diario::anotar(&s, nota).await?)))
}

// ----------------------------------------------------------------- rutinas

/// `?retiradas=1`: las rutinas retiradas (en la papelera o sin fichero) en
/// vez de las activas.
#[derive(Deserialize)]
pub struct RutinasQuery {
    retiradas: Option<String>,
}

pub async fn rutinas_listar(State(s): Estado, Query(q): Query<RutinasQuery>) -> Api<Vec<RutinaFila>> {
    ok(servicio::rutinas::listar(&s, q.retiradas.as_deref() == Some("1")).await)
}

pub async fn rutina_ver(State(s): Estado, Path(slug): Path<String>) -> Api<RutinaDetalle> {
    ok(servicio::rutinas::ver(&s, &slug).await)
}

pub async fn rutina_lanzar(State(s): Estado, Path(slug): Path<String>, Cuerpo(b): Cuerpo<EntradasBody>) -> Result<(StatusCode, Json<RutinaLanzada>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::rutinas::lanzar(&s, &slug, &b.entradas).await?)))
}

pub async fn rutina_previa(State(s): Estado, Path(slug): Path<String>, Cuerpo(b): Cuerpo<EntradasBody>) -> Api<RutinaPrevia> {
    ok(servicio::rutinas::previa(&s, &slug, &b.entradas).await)
}

pub async fn rutina_validar(State(s): Estado, Cuerpo(b): Cuerpo<TomlBody>) -> Api<RutinaValidada> {
    ok(servicio::rutinas::validar(&s, &b.toml).await)
}

pub async fn rutina_registrar(State(s): Estado, Cuerpo(b): Cuerpo<TomlBody>) -> Result<(StatusCode, Json<RutinaRegistrada>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::rutinas::registrar(&s, &b.toml).await?)))
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

pub async fn trabajo_crear(State(s): Estado, Cuerpo(b): Cuerpo<NuevoTrabajo>) -> Result<(StatusCode, Json<EstadoTrabajo>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::trabajos::crear(&s, b).await?)))
}

pub async fn trabajo_formulario(State(s): Estado) -> Api<FormularioTrabajo> {
    ok(servicio::trabajos::formulario(&s))
}

pub async fn skills_catalogo(State(s): Estado) -> Api<CatalogoSkills> {
    ok(servicio::trabajos::catalogo_skills(&s))
}

pub async fn skills_probar(State(s): Estado, Cuerpo(b): Cuerpo<ProbarSkills>) -> Api<PruebaSkills> {
    ok(servicio::trabajos::probar_skills(&s, &b.skills, b.por.as_deref()))
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

pub async fn trabajo_reformular(State(s): Estado, Path(id): Path<String>, Cuerpo(b): Cuerpo<ReformularBody>) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Reformular { nota: &b.nota, fase: b.fase }).await
}

pub async fn trabajo_confirmar(State(s): Estado, Path(id): Path<String>, Cuerpo(b): Cuerpo<ConfirmarBody>) -> Api<EstadoTrabajo> {
    decidir(&s, &id, Decision::Confirmar { contexto: &b.contexto }).await
}

pub async fn colas(State(s): Estado) -> Api<Colas> {
    ok(servicio::trabajos::colas(&s))
}

pub async fn semaforos(State(s): Estado) -> Json<Vec<Semaforo>> {
    Json(servicio::trabajos::semaforos(&s))
}

// -------------------------------------------------------------- worktrees

pub async fn worktrees_listar(State(s): Estado) -> Api<Vec<InfoWorktree>> {
    ok(servicio::trabajos::worktrees(&s))
}

// ----------------------------------------------------------------- equipos

pub async fn equipos_listar(State(s): Estado) -> Api<Vec<ResumenEquipo>> {
    ok(servicio::equipos::listar(&s))
}

pub async fn equipo_ver(State(s): Estado, Path(id): Path<String>) -> Api<VistaEquipo> {
    ok(servicio::equipos::ver(&s, &id))
}

pub async fn equipo_despertar(State(s): Estado, Cuerpo(b): Cuerpo<DespertarEquipo>) -> Result<(StatusCode, Json<Despierto>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::equipos::despertar(&s, b).await?)))
}

pub async fn equipo_dormir(State(s): Estado, Path(id): Path<String>) -> Api<VistaEquipo> {
    ok(servicio::equipos::dormir(&s, &id).await)
}

// -------------------------------------------------------------- propuestas

#[derive(Deserialize)]
pub struct PropuestasQuery {
    estado: Option<String>,
}

pub async fn propuestas_listar(State(s): Estado, Query(q): Query<PropuestasQuery>) -> Api<Vec<PropuestaVista>> {
    ok(servicio::propuestas::listar(&s, q.estado.as_deref()).await)
}

pub async fn propuesta_decidir(State(s): Estado, Path(id): Path<i64>, Cuerpo(b): Cuerpo<DecisionBody>) -> Api<PropuestaVista> {
    ok(servicio::propuestas::decidir(&s, id, &b.decision).await)
}

// ---------------------------------------------------------------- acciones

pub async fn acciones_listar(State(s): Estado) -> Api<Vec<AccionCatalogo>> {
    ok(servicio::acciones::catalogo(&s).await)
}

pub async fn accion_simular(State(s): Estado, Cuerpo(b): Cuerpo<AccionBody>) -> Api<Simulacion> {
    ok(servicio::acciones::simular(&s, &b.herramienta, &b.argumentos).await)
}

pub async fn accion_ejecutar(State(s): Estado, Cuerpo(b): Cuerpo<AccionBody>) -> Api<ResultadoAccion> {
    ok(servicio::acciones::ejecutar(&s, &b.herramienta, &b.argumentos, &b.precondicion).await)
}

pub async fn accion_historial(State(s): Estado, Path(nombre): Path<String>) -> Api<Vec<EventoHerramienta>> {
    ok(servicio::acciones::historial(&s, &nombre).await)
}

// ---------------------------------------------------------------- terminal

/// `{propuesta}`, `{servicios}`, `{herramienta, argumentos}` o `{boveda}`;
/// cualquier otra cosa (texto libre incluido) es un 400.
pub async fn terminal_abrir(State(s): Estado, Cuerpo(b): Cuerpo<AbrirTerminal>) -> Api<TerminalAbierta> {
    ok(servicio::terminal::abrir(&s, &b).await)
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

pub async fn boveda_promover(State(s): Estado, Cuerpo(b): Cuerpo<PromoverBody>) -> Api<Promocion> {
    ok(servicio::boveda::promover(&s, &b.ruta, b.destino.as_deref()).await)
}

pub async fn boveda_indice(State(s): Estado) -> Api<IndiceBoveda> {
    ok(servicio::boveda::indice(&s).await)
}

pub async fn boveda_reindexar(State(s): Estado) -> Api<IndiceBoveda> {
    ok(servicio::boveda::reindexar(&s).await)
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

pub async fn boveda_guardar(State(s): Estado, Cuerpo(b): Cuerpo<GuardarNotaBody>) -> Api<NotaFichero> {
    ok(servicio::boveda::guardar(&s, &b.ruta, &b.contenido, &b.version).await)
}

pub async fn boveda_crear(State(s): Estado, Cuerpo(b): Cuerpo<CrearNotaBody>) -> Result<(StatusCode, Json<NotaFichero>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::boveda::crear(&s, &b.ruta, &b.contenido).await?)))
}

pub async fn boveda_renombrar(State(s): Estado, Cuerpo(b): Cuerpo<RenombrarNotaBody>) -> Api<RenombradoBoveda> {
    ok(servicio::boveda::renombrar(&s, &b.ruta, &b.nuevo, &b.version).await)
}

pub async fn boveda_eliminar(State(s): Estado, Cuerpo(b): Cuerpo<EliminarNotaBody>) -> Api<EnPapelera> {
    ok(servicio::boveda::eliminar(&s, &b.ruta, &b.version).await)
}

pub async fn boveda_carpeta(State(s): Estado, Cuerpo(b): Cuerpo<RutaBody>) -> Result<(StatusCode, Json<Hecho>), ApiError> {
    Ok((StatusCode::CREATED, Json(servicio::boveda::carpeta(&s, &b.ruta).await?)))
}

pub async fn boveda_carpeta_renombrar(State(s): Estado, Cuerpo(b): Cuerpo<RenombrarCarpetaBody>) -> Api<CarpetaRenombradaVista> {
    ok(servicio::boveda::renombrar_carpeta(&s, &b.ruta, &b.nuevo).await)
}

pub async fn boveda_carpeta_eliminar(State(s): Estado, Cuerpo(b): Cuerpo<RutaBody>) -> Api<CarpetaBorradaVista> {
    ok(servicio::boveda::eliminar_carpeta(&s, &b.ruta).await)
}

pub async fn boveda_mostrar(State(s): Estado, Cuerpo(b): Cuerpo<RutaBody>) -> Api<Hecho> {
    ok(servicio::boveda::mostrar(&s, &b.ruta).await)
}

pub async fn boveda_papelera(State(s): Estado) -> Api<Vec<EnPapelera>> {
    ok(servicio::boveda::papelera(&s))
}

pub async fn boveda_restaurar(State(s): Estado, Cuerpo(b): Cuerpo<RestaurarBody>) -> Api<NotaFichero> {
    ok(servicio::boveda::restaurar(&s, &b.id).await)
}

// ----------------------------------------------------------------- memoria

pub async fn memoria_listar(State(s): Estado) -> Api<ListadoMemoria> {
    ok(servicio::memoria::listar(&s))
}

pub async fn memoria_leer(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>) -> Api<Memoria> {
    ok(servicio::memoria::leer(&s, &proyecto, &fichero))
}

pub async fn memoria_guardar(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>, Cuerpo(b): Cuerpo<MemoriaBody>) -> Api<Memoria> {
    ok(servicio::memoria::guardar(&s, &proyecto, &fichero, &b.contenido, &b.version).await)
}

pub async fn memoria_crear(
    State(s): Estado,
    Path(proyecto): Path<String>,
    Cuerpo(b): Cuerpo<CrearMemoriaBody>,
) -> Result<(StatusCode, Json<Memoria>), ApiError> {
    let n = crate::memoria::NuevaMemoria { fichero: &b.fichero, titulo: &b.titulo, descripcion: &b.descripcion, tipo: &b.tipo, cuerpo: &b.cuerpo };
    Ok((StatusCode::CREATED, Json(servicio::memoria::crear(&s, &proyecto, &n).await?)))
}

pub async fn memoria_eliminar(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>, Cuerpo(b): Cuerpo<VersionBody>) -> Api<Hecho> {
    ok(servicio::memoria::eliminar(&s, &proyecto, &fichero, &b.version).await)
}

pub async fn memoria_renombrar(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>, Cuerpo(b): Cuerpo<RenombrarBody>) -> Api<Renombrado> {
    ok(servicio::memoria::renombrar(&s, &proyecto, &fichero, &b.nuevo, &b.version).await)
}

pub async fn memoria_mostrar(State(s): Estado, Path((proyecto, fichero)): Path<(String, String)>) -> Api<Hecho> {
    ok(servicio::memoria::mostrar(&s, &proyecto, &fichero).await)
}

// --------------------------------------------------------------- servicios

pub async fn servicios_plan(State(s): Estado) -> Api<PlanServicios> {
    ok(servicio::servicios::plan(&s).await)
}

pub async fn servicio_reiniciar(State(s): Estado, Path(nombre): Path<String>) -> Api<Hecho> {
    ok(servicio::servicios::reiniciar(&s, &nombre).await)
}

// ----------------------------------------------------------------- ajustes

pub async fn ajustes_ver(State(s): Estado) -> Api<AjustesVista> {
    Ok(Json(servicio::ajustes::ver(&s)))
}

pub async fn ajustes_cambiar(State(s): Estado, Cuerpo(b): Cuerpo<CuerpoAjustes>) -> Api<AjustesCambiados> {
    ok(servicio::ajustes::cambiar(&s, &b.version, b.cambios).await)
}

// -------------------------------------------------------------- biblioteca

#[derive(Deserialize)]
pub struct BibliotecaQuery {
    agente: Option<String>,
    tipo: Option<String>,
    categoria: Option<String>,
    activado: Option<String>,
}

pub async fn biblioteca_listar(State(s): Estado, Query(q): Query<BibliotecaQuery>) -> Api<BibliotecaVista> {
    let p = servicio::biblioteca::Peticion { agente: q.agente, tipo: q.tipo, categoria: q.categoria, activado: q.activado };
    ok(servicio::biblioteca::listar(&s, &p).await)
}

pub async fn biblioteca_ficha(State(s): Estado, Path(id): Path<String>) -> Api<FichaVista> {
    ok(servicio::biblioteca::ficha(&s, &id).await)
}

pub async fn biblioteca_uso(State(s): Estado) -> Api<InformeUso> {
    Ok(Json((*servicio::biblioteca::uso(&s).await?).clone()))
}

pub async fn biblioteca_simular(State(s): Estado, Cuerpo(b): Cuerpo<AccionBody>) -> Api<Simulacion> {
    ok(servicio::biblioteca::simular(&s, &b.herramienta, &b.argumentos).await)
}

pub async fn biblioteca_ejecutar(State(s): Estado, Cuerpo(b): Cuerpo<AccionBody>) -> Api<ResultadoAccion> {
    ok(servicio::biblioteca::ejecutar(&s, &b.herramienta, &b.argumentos, &b.precondicion).await)
}
