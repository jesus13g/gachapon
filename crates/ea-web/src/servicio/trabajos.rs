//! Trabajos del bus, sus colas y semáforos, y las decisiones del usuario
//! sobre ellos.

use std::collections::HashSet;

use ea_bus::{Agente, Bus, Evento, Modo, Orden, Topes};
use serde::Deserialize;
use serde_json::Value;

use super::{bus_con_store, bus_lectura, texto_no_vacio, ErrorWeb, Resultado, ACTOR};
use crate::vistas::{
    ColaEntrada, Colas, Diff, EstadoTrabajo, FaseVista, FormularioTrabajo, Semaforo, TrabajoDetalle, TrabajoResumen,
};
use crate::AppState;

fn a_valor<T: serde::Serialize>(v: T) -> Resultado<Value> {
    serde_json::to_value(v).map_err(|e| ErrorWeb::Interno(e.to_string()))
}

/// `en_curso`: trabajos cuyo worktree tiene cogido el agente local.
fn resumen(bus: &Bus, id: &str, en_curso: &HashSet<String>) -> Option<TrabajoResumen> {
    let orden = bus.leer_orden(id).ok()?;
    let estado = bus.leer_estado(id).ok()?;
    let informe = bus.leer_informe(id).ok().flatten();
    Some(TrabajoResumen {
        cancelando: estado.estado == ea_bus::EstadoTrabajo::Cancelado && en_curso.contains(id),
        id: id.into(),
        modo: orden.modo.as_str(),
        estado: estado.estado.as_str(),
        fase_actual: estado.fase_actual,
        fases_total: orden.fases.len(),
        iteracion: estado.iteracion,
        detalle: estado.detalle,
        resultado: informe.as_ref().map(|i| match i.resultado {
            ea_bus::ResultadoInforme::Terminado => "terminado",
            ea_bus::ResultadoInforme::Bloqueado => "bloqueado",
        }),
        obra_completa: informe.as_ref().map(|i| i.obra_completa),
        rutina: a_valor(&orden.rutina).ok()?,
        titulo: orden.titulo,
        worktree: orden.worktree,
        creado: orden.creada_en,
        actualizado: estado.actualizado_en,
    })
}

/// El más reciente primero.
pub fn listar(s: &AppState) -> Resultado<Vec<TrabajoResumen>> {
    let bus = bus_lectura(s)?;
    // Un trabajo cancelado cuyo semáforo sigue cogido es que el ejecutor aún
    // no se ha enterado (sondea cada 2 s y espera al agente): «cancelando…».
    let en_curso: HashSet<String> = ea_bus::listar_semaforos_en(bus.raiz())
        .into_iter()
        .filter(|(_, o)| o.agente == "modelo-local")
        .map(|(_, o)| o.trabajo)
        .collect();
    let mut v: Vec<_> = bus.listar_trabajos()?.iter().filter_map(|id| resumen(&bus, id, &en_curso)).collect();
    v.reverse();
    Ok(v)
}

/// El resumen del agente se recorta a esto (caracteres).
const RESUMEN_MAX: usize = 8_000;

pub async fn ver(s: &AppState, id: &str) -> Resultado<TrabajoDetalle> {
    let mut d = ver_bus(s, id)?;
    d.fases = s
        .store
        .lock()
        .await
        .listar_fases(id)?
        .into_iter()
        .map(|f| FaseVista {
            diff_stat: ea_bus::diff_stat_de_fase(&f),
            instantanea: u32::try_from(f.numero).is_ok_and(|n| ea_bus::arbol_de_fase(std::slice::from_ref(&f), n).is_some()),
            numero: f.numero,
            titulo: f.titulo,
            estado: f.estado,
            cerrado_en: f.cerrado_en,
        })
        .collect();
    Ok(d)
}

/// Lo que sale de los ficheros del bus (sin las fases de SQLite).
fn ver_bus(s: &AppState, id: &str) -> Resultado<TrabajoDetalle> {
    let bus = bus_lectura(s)?;
    let orden = bus.leer_orden(id)?;
    let estado = bus.leer_estado(id)?;
    let mut informe = a_valor(bus.leer_informe(id)?)?;
    if let Some(o) = informe.as_object_mut() {
        o.remove("diff"); // el diff va aparte, bajo demanda
    }
    let dir = bus.raiz().join("trabajos").join(id);
    let historial = std::fs::read_to_string(dir.join("historial.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let paquete = std::fs::read_to_string(dir.join("paquete-correccion.md")).ok();
    let resumen_agente = std::fs::read_to_string(dir.join("salida-qwen.json"))
        .ok()
        .map(|b| ea_bus::resumen_salida_agente(&b))
        .map(|r| r.trim().chars().take(RESUMEN_MAX).collect::<String>())
        .filter(|r| !r.is_empty());
    Ok(TrabajoDetalle {
        id: id.into(),
        orden: a_valor(orden)?,
        estado: a_valor(estado)?,
        informe,
        historial,
        paquete,
        fases: vec![],
        resumen_agente,
    })
}

const DIFF_MAX: usize = 200_000;

/// Lo mismo que `ea trabajo diff [--fase N]`. Sin fase: lo cambiado desde
/// `commit_inicio` más los ficheros nuevos sin seguimiento. Con fase: entre
/// la instantánea de árbol de la fase anterior (o `commit_inicio` para la
/// primera) y la suya (`ea_bus::rango_diff_fase`); sin instantánea, 404.
/// Recortado a `DIFF_MAX` bytes. Solo lectura.
pub async fn diff(s: &AppState, id: &str, fase: Option<u32>) -> Resultado<Diff> {
    let bus = bus_lectura(s)?;
    let orden = bus.leer_orden(id)?;
    let inicio = bus.leer_informe(id)?.and_then(|i| i.commit_inicio);
    if let Some(n) = fase {
        let fases = s.store.lock().await.listar_fases(id)?;
        let (base, fin) = ea_bus::rango_diff_fase(inicio.as_deref(), &fases, n)?;
        let (texto, truncado) = crate::git::diff_entre(&orden.worktree, &base, &fin, DIFF_MAX).await?;
        return Ok(Diff { id: id.into(), fase: Some(n), commit_inicio: inicio, diff: texto, truncado });
    }
    let inicio =
        inicio.ok_or_else(|| ErrorWeb::Conflicto("el trabajo aún no tiene informe con commit_inicio".into()))?;
    let (texto, truncado) = crate::git::diff_desde(&orden.worktree, &inicio, DIFF_MAX).await?;
    Ok(Diff { id: id.into(), fase: None, commit_inicio: Some(inicio), diff: texto, truncado })
}

// ------------------------------------------------------------------- crear

/// Los modos que se crean desde la web. Las obras llegarán con T2, por el
/// mismo formulario y la misma ruta.
const MODOS_WEB: [&str; 1] = ["mision"];
const TITULO_MAX: usize = 200;
const OBJETIVO_MAX: usize = 20_000;
const CRITERIO_MAX: usize = 2_000;
const WORKTREE_MAX: usize = 4_096;
const SUGERENCIAS_MAX: usize = 30;

/// Cuerpo de `POST /api/trabajos`. Cualquier otro campo (`skills`,
/// `creada_por`, `fases`…) se rechaza: desde la web no se eligen.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NuevoTrabajo {
    #[serde(default)]
    modo: Option<String>,
    titulo: String,
    worktree: String,
    objetivo: String,
    criterio: String,
    #[serde(default)]
    topes: Option<TopesPedidos>,
}

/// Los topes que se pueden pedir desde el formulario; el resto, por defecto.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct TopesPedidos {
    iteraciones: Option<u32>,
    tiempo_s: Option<u64>,
}

/// El techo de los topes de los ajustes efectivos (se relee en cada petición).
fn techo(s: &AppState) -> Topes {
    ea_bus::ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&s.config.ajustes).ajustes).techo_topes
}

/// Lo que el formulario necesita: techo, topes por defecto y los worktrees
/// que se sugieren (los `cwd` de las sesiones vivas de Claude Code y los de
/// los trabajos anteriores, del más reciente al más antiguo, sin repetir).
pub fn formulario(s: &AppState) -> Resultado<FormularioTrabajo> {
    let techo = techo(s);
    let mut worktrees: Vec<String> = super::estado::sesiones(s).sesiones.into_iter().map(|x| x.cwd).collect();
    worktrees.extend(listar(s)?.into_iter().map(|t| t.worktree));
    let mut vistos = std::collections::HashSet::new();
    worktrees.retain(|w| !w.is_empty() && vistos.insert(w.clone()));
    worktrees.truncate(SUGERENCIAS_MAX);
    Ok(FormularioTrabajo { modos: MODOS_WEB.to_vec(), techo, defecto: Topes::default().acotar(&techo), worktrees })
}

/// Ruta absoluta, existente, directorio y dentro de un repo git; devuelve
/// la forma canónica, que es la que va a la orden (y al semáforo).
async fn validar_worktree(wt: &str) -> Resultado<String> {
    let wt = wt.trim();
    if wt.is_empty() {
        return Err(ErrorWeb::Invalido("'worktree' no puede estar vacío".into()));
    }
    if wt.len() > WORKTREE_MAX {
        return Err(ErrorWeb::Invalido(format!("'worktree' supera {WORKTREE_MAX} bytes")));
    }
    let ruta = std::path::Path::new(wt);
    if !ruta.is_absolute() {
        return Err(ErrorWeb::Invalido("'worktree' debe ser una ruta absoluta".into()));
    }
    let canon = std::fs::canonicalize(ruta)
        .map_err(|_| ErrorWeb::Invalido(format!("el worktree '{wt}' no existe")))?;
    if !canon.is_dir() {
        return Err(ErrorWeb::Invalido(format!("el worktree '{wt}' no es un directorio")));
    }
    let canon = canon
        .to_str()
        .ok_or_else(|| ErrorWeb::Invalido("la ruta del worktree no es UTF-8".into()))?
        .to_string();
    if !crate::git::es_repo(&canon).await? {
        return Err(ErrorWeb::Invalido(format!("'{canon}' no está dentro de un repositorio git")));
    }
    Ok(canon)
}

/// Crea una misión encargada por el usuario desde la página. Todo se valida
/// antes de tocar el bus; después, `Bus::crear_trabajo` (que vuelve a
/// exigir el criterio y valida skills) la escribe, la encola en
/// `modelo-local` y apunta `bus_crear` en el diario con actor `usuario`.
/// El criterio no se ejecuta aquí: solo el ejecutor lo corre (arts. III y V).
pub async fn crear(s: &AppState, cuerpo: Value) -> Resultado<EstadoTrabajo> {
    if cuerpo.get("skills").is_some() {
        return Err(ErrorWeb::Invalido("las skills no se eligen desde la web".into()));
    }
    let n: NuevoTrabajo =
        serde_json::from_value(cuerpo).map_err(|e| ErrorWeb::Invalido(format!("petición inválida: {e}")))?;
    let modo = n.modo.as_deref().unwrap_or("mision");
    if !MODOS_WEB.contains(&modo) {
        return Err(ErrorWeb::Invalido(format!("modo '{modo}': desde la web solo se crean misiones")));
    }
    let titulo = texto_no_vacio(&n.titulo, "titulo", TITULO_MAX)?;
    if titulo.chars().any(char::is_control) {
        return Err(ErrorWeb::Invalido("'titulo' debe ser una sola línea".into()));
    }
    let objetivo = texto_no_vacio(&n.objetivo, "objetivo", OBJETIVO_MAX)?;
    let criterio = texto_no_vacio(&n.criterio, "criterio", CRITERIO_MAX)?;
    let pedidos = n.topes.unwrap_or_default();
    if pedidos.iteraciones == Some(0) || pedidos.tiempo_s == Some(0) {
        return Err(ErrorWeb::Invalido("los topes deben ser al menos 1".into()));
    }
    let worktree = validar_worktree(&n.worktree).await?;
    let d = Topes::default();
    let topes = Topes {
        iteraciones: pedidos.iteraciones.unwrap_or(d.iteraciones),
        tiempo_s: pedidos.tiempo_s.unwrap_or(d.tiempo_s),
        ..d
    }
    .acotar(&techo(s));
    let orden = Orden {
        id: String::new(),
        modo: Modo::Mision,
        titulo,
        worktree,
        objetivo,
        criterio_exito: Some(criterio),
        fases: vec![],
        revisar_cada: None,
        topes,
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        creada_en: String::new(),
        creada_por: ACTOR.into(),
    };
    let id = bus_con_store(s).await?.crear_trabajo(orden)?;
    Ok(EstadoTrabajo { id, estado: ea_bus::EstadoTrabajo::EnCola.as_str() })
}

/// Lo que el usuario decide sobre un trabajo entregado o en espera.
pub enum Decision<'a> {
    Cerrar,
    Cancelar,
    /// Aprueba una revisión intermedia: reformular sin nota (como `ea trabajo continuar`).
    Continuar,
    Reformular { nota: &'a str },
    /// Confirma el paquete de corrección de una rutina en espera.
    Confirmar { contexto: &'a str },
}

pub async fn decidir(s: &AppState, id: &str, d: Decision<'_>) -> Resultado<EstadoTrabajo> {
    let e = match d {
        Decision::Cerrar => bus_con_store(s).await?.aplicar(id, Evento::Cerrar)?,
        Decision::Cancelar => bus_con_store(s).await?.aplicar(id, Evento::Cancelar)?,
        Decision::Continuar => bus_con_store(s).await?.reformular(id, "")?,
        Decision::Reformular { nota } => {
            let nota = texto_no_vacio(nota, "nota", 10_000)?;
            bus_con_store(s).await?.reformular(id, &nota)?
        }
        Decision::Confirmar { contexto } => {
            if contexto.chars().count() > 10_000 {
                return Err(ErrorWeb::Invalido("'contexto' supera 10000 caracteres".into()));
            }
            bus_con_store(s).await?.confirmar(id, contexto)?
        }
    };
    Ok(EstadoTrabajo { id: id.into(), estado: e.estado.as_str() })
}

pub fn colas(s: &AppState) -> Resultado<Colas> {
    let bus = bus_lectura(s)?;
    let mut m = Colas::new();
    for a in [Agente::ModeloLocal, Agente::ClaudeCode] {
        let v = bus
            .listar_cola(a)?
            .into_iter()
            .map(|e| ColaEntrada {
                titulo: bus.leer_orden(&e.trabajo).map(|o| o.titulo).ok(),
                id: e.id,
                trabajo: e.trabajo,
                motivo: e.motivo,
                encolado_en: e.encolado_en,
            })
            .collect();
        m.insert(a.as_str(), v);
    }
    Ok(m)
}

pub fn semaforos(s: &AppState) -> Vec<Semaforo> {
    ea_bus::listar_semaforos_en(&s.config.estado_dir)
        .into_iter()
        .map(|(_, o)| Semaforo { agente: o.agente, trabajo: o.trabajo, pid: o.pid, desde: o.desde })
        .collect()
}
