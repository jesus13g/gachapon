//! Las respuestas de la API, con nombre y forma fijas: el contrato que
//! consume la página. Solo datos; quien los rellena es `servicio` y quien
//! los serializa, `http`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

pub use crate::memoria::{FicheroMemoria, Memoria, Proyecto as ProyectoMemoria};
pub use crate::sesiones::Sesion;

// ------------------------------------------------------------------ estado

#[derive(Serialize)]
pub struct EstadoGeneral {
    pub ahora: String,
    pub servicios: Vec<ServicioVista>,
    pub orquestador: Orquestador,
    pub agentes: Vec<AgenteVista>,
    pub trabajos: TrabajosPorEstado,
    pub propuestas_pendientes: usize,
    pub db: Option<String>,
}

#[derive(Serialize)]
pub struct ServicioVista {
    pub nombre: &'static str,
    pub activo: String,
    pub activo_s: Option<i64>,
}

#[derive(Serialize)]
pub struct Orquestador {
    pub vivo: bool,
    pub ultimo_latido: Option<String>,
}

#[derive(Serialize)]
pub struct AgenteVista {
    pub nombre: &'static str,
    pub ultimo: Option<UltimoEvento>,
}

#[derive(Serialize)]
pub struct UltimoEvento {
    pub ts: String,
    pub accion: String,
}

/// Trabajos del bus por estado; todo a cero si el bus no se puede leer.
#[derive(Serialize, Default)]
pub struct TrabajosPorEstado {
    pub en_cola: usize,
    pub ejecutando: usize,
    pub en_revision: usize,
    pub en_espera: usize,
    pub cerrado: usize,
    pub cancelado: usize,
}

#[derive(Serialize)]
pub struct Inicio {
    pub propuestas: Vec<PropuestaVista>,
}

#[derive(Serialize)]
pub struct Sesiones {
    pub sesiones: Vec<Sesion>,
}

// ------------------------------------------------------------------ diario

#[derive(Serialize)]
pub struct EventoVista {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub accion: String,
    pub trabajo: Option<String>,
    pub resultado: Option<&'static str>,
    pub duracion_ms: Option<i64>,
    pub detalle: Option<String>,
    /// Las notas `usuario_nota` que corrigen este evento (el diario es
    /// append-only: corregir es añadir otro evento que apunta a este).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub corregido_por: Vec<i64>,
}

// -------------------------------------------------------------- propuestas

#[derive(Serialize)]
pub struct PropuestaVista {
    pub id: i64,
    pub proponente: Option<String>,
    pub creada: String,
    pub descripcion: String,
    pub estado: &'static str,
    pub decidida: Option<String>,
    pub accion: Option<AccionPropuesta>,
    pub ejecutada: Option<Ejecucion>,
    /// Solo en la respuesta de aceptarla, si la acción se ejecutó: lo que
    /// produjo, para enlazar con ello (como en la consola de acciones).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resultado: Option<ResultadoAccion>,
}

#[derive(Serialize)]
pub struct AccionPropuesta {
    pub herramienta: String,
    pub argumentos: Value,
    pub riesgo: String,
    pub simulacion: String,
}

#[derive(Serialize)]
pub struct Ejecucion {
    pub estado: &'static str,
    pub detalle: Option<String>,
    pub cuando: Option<String>,
}

// ----------------------------------------------------------------- rutinas

#[derive(Serialize)]
pub struct RutinaFila {
    pub slug: String,
    pub version: i64,
    pub descripcion: Option<String>,
    pub worktree: Option<String>,
    pub criterio_exito: Option<String>,
    pub entradas: Option<BTreeMap<String, String>>,
    pub error: Option<&'static str>,
    pub registrada: String,
    pub proxima: Option<i64>,
    /// El `OnCalendar` de su timer (`ea-rutina@<slug>.timer`), si lo tiene.
    pub calendario: Option<String>,
    /// Estado del timer en systemd (`active`, `inactive`…); `None` si no se
    /// pregunta (sin `systemctl`).
    pub timer: Option<String>,
    /// El trabajo más reciente de la rutina en el bus, si lo hay.
    pub ultima: Option<RutinaEjecucion>,
}

#[derive(Serialize)]
pub struct RutinaDetalle {
    pub slug: String,
    pub version: i64,
    pub contenido: String,
    /// Las últimas ejecuciones (trabajos del bus), la más reciente primero.
    pub ejecuciones: Vec<RutinaEjecucion>,
}

/// Una ejecución de una rutina: un trabajo del bus en modo rutina, con la
/// versión con la que se lanzó y, si ya entregó informe, su resultado y lo
/// que tardó el agente.
#[derive(Serialize)]
pub struct RutinaEjecucion {
    pub trabajo: String,
    pub estado: &'static str,
    pub version: u32,
    pub resultado: Option<&'static str>,
    pub duracion_s: Option<u64>,
    pub detalle: Option<String>,
    pub creado: String,
    pub actualizado: String,
}

#[derive(Serialize)]
pub struct RutinaLanzada {
    pub trabajo: String,
    pub rutina: String,
}

/// La orden que recibiría el agente con unas entradas, sin crear trabajo.
#[derive(Serialize)]
pub struct RutinaPrevia {
    pub rutina: String,
    pub version: u32,
    pub objetivo: String,
    pub criterio: String,
}

/// La versión registrada de una rutina, para comparar con la que se edita.
#[derive(Serialize)]
pub struct RutinaAnterior {
    pub version: i64,
    pub definicion: String,
}

/// Lo que registraría una definición (`POST /api/rutinas/validar`), sin
/// escribir nada: su slug y versión, la registrada ahora y la orden que
/// recibiría el agente con las entradas por defecto.
#[derive(Serialize)]
pub struct RutinaValidada {
    pub rutina: String,
    pub version: u32,
    pub anterior: Option<RutinaAnterior>,
    pub previa: RutinaPrevia,
}

#[derive(Serialize)]
pub struct RutinaRegistrada {
    pub rutina: String,
    pub version: u32,
    /// La versión que había antes, o `None` si la rutina es nueva.
    pub anterior: Option<i64>,
}

/// Una versión registrada de una rutina, sin su definición
/// (`GET /api/rutinas/{slug}/versiones`).
#[derive(Serialize)]
pub struct RutinaVersionFila {
    pub version: i64,
    pub creada_en: String,
    pub creada_por: String,
}

/// Una versión concreta con su definición
/// (`GET /api/rutinas/{slug}/versiones/{n}`).
#[derive(Serialize)]
pub struct RutinaVersion {
    pub slug: String,
    pub version: i64,
    pub creada_en: String,
    pub creada_por: String,
    pub contenido: String,
}

// ---------------------------------------------------------------- trabajos

#[derive(Serialize)]
pub struct TrabajoResumen {
    pub id: String,
    pub modo: &'static str,
    pub titulo: String,
    pub estado: &'static str,
    pub fase_actual: Option<u32>,
    pub fases_total: usize,
    pub iteracion: u32,
    pub detalle: Option<String>,
    pub resultado: Option<&'static str>,
    pub obra_completa: Option<bool>,
    /// Cancelado, pero el ejecutor aún tiene cogido su worktree (está
    /// parando al agente).
    pub cancelando: bool,
    pub rutina: Value,
    pub worktree: String,
    pub creado: String,
    pub actualizado: String,
}

/// Lo que guarda el bus de un trabajo, tal cual (el informe sin el diff).
#[derive(Serialize)]
pub struct TrabajoDetalle {
    pub id: String,
    pub orden: Value,
    pub estado: Value,
    pub informe: Value,
    pub historial: Vec<Value>,
    pub paquete: Option<String>,
    /// Las fases de una obra en `trabajo_fases` (vacío en misiones y rutinas).
    pub fases: Vec<FaseVista>,
    /// El texto final del agente en su última ejecución (`salida-qwen.json`).
    pub resumen_agente: Option<String>,
}

/// El estado de una fase en `trabajo_fases`; el plan (instrucciones,
/// entregable, criterio) está en `orden.fases`.
#[derive(Serialize)]
pub struct FaseVista {
    pub numero: i64,
    pub titulo: String,
    pub estado: String,
    pub cerrado_en: Option<String>,
    pub diff_stat: Option<String>,
    /// Si guardó instantánea de árbol, es decir, si tiene diff de fase.
    pub instantanea: bool,
}

#[derive(Serialize)]
pub struct Diff {
    pub id: String,
    /// La fase, si es el diff de una fase (`?fase=N`).
    pub fase: Option<u32>,
    /// `null` en el diff de una fase de una obra que aún no ha entregado.
    pub commit_inicio: Option<String>,
    pub diff: String,
    pub truncado: bool,
}

#[derive(Serialize)]
pub struct EstadoTrabajo {
    pub id: String,
    pub estado: &'static str,
}

/// Lo que necesita el formulario «＋ misión»: los modos que se pueden crear
/// desde la web, el techo de los topes (`ajustes.toml`), los topes por
/// defecto ya recortados a ese techo y los worktrees que se sugieren.
#[derive(Serialize)]
pub struct FormularioTrabajo {
    pub modos: Vec<&'static str>,
    pub techo: ea_bus::Topes,
    pub defecto: ea_bus::Topes,
    pub worktrees: Vec<String>,
}

/// Por agente (`modelo-local`, `claude-code`).
pub type Colas = BTreeMap<&'static str, Vec<ColaEntrada>>;

#[derive(Serialize)]
pub struct ColaEntrada {
    pub id: String,
    pub trabajo: String,
    pub titulo: Option<String>,
    pub motivo: String,
    pub encolado_en: String,
}

#[derive(Serialize)]
pub struct Semaforo {
    pub agente: String,
    pub trabajo: String,
    pub pid: u32,
    pub desde: String,
}

// ---------------------------------------------------------------- acciones

#[derive(Serialize)]
pub struct AccionCatalogo {
    pub nombre: &'static str,
    pub riesgo: &'static str,
    pub supervisada: bool,
    pub local: bool,
    pub resumen: &'static str,
    pub campos: Vec<CampoVista>,
    pub confirmable: bool,
}

#[derive(Serialize)]
pub struct CampoVista {
    pub nombre: &'static str,
    pub entero: bool,
    /// Texto de varias líneas: la consola pinta un área de texto.
    pub largo: bool,
    pub obligatorio: bool,
    pub opciones: Vec<String>,
    pub ayuda: &'static str,
}

#[derive(Serialize)]
pub struct Simulacion {
    pub herramienta: &'static str,
    pub riesgo: &'static str,
    pub argumentos: Value,
    pub texto: String,
    pub precondicion: Value,
    pub confirmable: bool,
}

#[derive(Serialize)]
pub struct ResultadoAccion {
    pub texto: String,
    pub trabajo: Option<String>,
    pub datos: Value,
    /// El evento `accion_ejecutada` del diario (para «≡ en el diario»).
    pub evento: Option<i64>,
}

// ------------------------------------------------------------------ bóveda

#[derive(Serialize)]
pub struct ListadoBoveda {
    pub raiz: String,
    pub notas: Vec<NotaResumen>,
}

#[derive(Serialize)]
pub struct NotaResumen {
    pub ruta: String,
    pub region: String,
    pub autor: Option<String>,
    pub titulo: String,
    pub actualizada: Option<String>,
    pub promocion: Option<String>,
    pub fragmento: String,
}

#[derive(Serialize)]
pub struct NotaVista {
    pub ruta: String,
    pub region: String,
    pub cabecera: Option<CabeceraVista>,
    pub aviso_cabecera: Option<String>,
    pub contenido: String,
    pub total_caracteres: usize,
    pub truncado: bool,
    pub siguiente_desde: Option<usize>,
}

#[derive(Serialize)]
pub struct CabeceraVista {
    pub titulo: String,
    pub autor: String,
    pub creada: String,
    pub actualizada: String,
    pub origen: String,
    pub promocion: String,
    pub tags: Vec<String>,
    pub promovida_de: Option<String>,
    pub promovida_a: Option<String>,
}

/// El árbol del editor de la bóveda: regiones, carpetas (también vacías) y
/// notas.
#[derive(Serialize)]
pub struct ArbolBoveda {
    pub raiz: String,
    pub regiones: Vec<&'static str>,
    pub carpetas: Vec<String>,
    pub notas: Vec<NotaResumen>,
    pub en_papelera: usize,
}

/// Una nota entera para el editor, con la versión que hay que devolver al
/// guardar (misma forma que una memoria: `contenido` y `version`).
#[derive(Serialize)]
pub struct NotaFichero {
    pub ruta: String,
    pub region: String,
    pub contenido: String,
    pub version: String,
    pub cabecera: Option<CabeceraVista>,
    pub aviso_cabecera: Option<String>,
    pub bytes: u64,
    pub modificada: Option<String>,
    /// Ruta absoluta del fichero (para copiarla).
    pub absoluta: String,
}

#[derive(Serialize)]
pub struct RenombradoBoveda {
    pub nota: NotaFichero,
    /// Las otras notas cuyos enlaces se reescribieron.
    pub actualizadas: Vec<String>,
    /// Notas con `[[enlaces]]` ambiguos al nombre viejo, que no se tocaron.
    pub ambiguas: Vec<String>,
}

#[derive(Serialize)]
pub struct EnPapelera {
    pub id: String,
    pub ruta: String,
    pub borrada: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct Promocion {
    pub texto: String,
    pub datos: Value,
}

// --------------------------------------------------------------- servicios

#[derive(Serialize)]
pub struct Hecho {
    pub ok: bool,
}

// ----------------------------------------------------------------- memoria

#[derive(Serialize)]
pub struct ListadoMemoria {
    pub raiz: String,
    pub proyectos: Vec<ProyectoMemoria>,
}

#[derive(Serialize)]
pub struct Renombrado {
    pub memoria: Memoria,
    /// Las otras memorias del proyecto cuyos enlaces se reescribieron.
    pub actualizadas: Vec<String>,
}

// ----------------------------------------------------------------- ajustes

/// `GET /api/ajustes`: la misma forma que `ea ajustes ver --json`.
pub use ea_ajustes::{FilaAjuste as ClaveVista, Resumen as AjustesVista, SeccionAjustes};

#[derive(Serialize)]
pub struct AjusteAplicado {
    pub clave: &'static str,
    pub antes: Value,
    pub despues: Value,
    /// `caliente` o `reinicio`.
    pub aplica: &'static str,
    pub reinicio: &'static [&'static str],
}

#[derive(Serialize)]
pub struct AjustesCambiados {
    pub aplicados: Vec<AjusteAplicado>,
    pub ajustes: AjustesVista,
}
