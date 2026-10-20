//! Trabajos del bus, sus colas y semáforos, y las decisiones del usuario
//! sobre ellos.

use std::collections::HashSet;

use ea_bus::{Agente, Bus, Evento};
use serde_json::Value;

use super::{bus_con_store, bus_lectura, texto_no_vacio, ErrorWeb, Resultado};
use crate::vistas::{ColaEntrada, Colas, Diff, EstadoTrabajo, Semaforo, TrabajoDetalle, TrabajoResumen};
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

pub fn ver(s: &AppState, id: &str) -> Resultado<TrabajoDetalle> {
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
    Ok(TrabajoDetalle { id: id.into(), orden: a_valor(orden)?, estado: a_valor(estado)?, informe, historial, paquete })
}

const DIFF_MAX: usize = 200_000;

/// Lo mismo que `ea trabajo diff`: lo cambiado desde `commit_inicio` más los
/// ficheros nuevos sin seguimiento. Recortado a `DIFF_MAX` bytes.
pub async fn diff(s: &AppState, id: &str) -> Resultado<Diff> {
    let bus = bus_lectura(s)?;
    let orden = bus.leer_orden(id)?;
    let inicio = bus
        .leer_informe(id)?
        .and_then(|i| i.commit_inicio)
        .ok_or_else(|| ErrorWeb::Conflicto("el trabajo aún no tiene informe con commit_inicio".into()))?;
    let (texto, truncado) = crate::git::diff_desde(&orden.worktree, &inicio, DIFF_MAX).await?;
    Ok(Diff { id: id.into(), commit_inicio: inicio, diff: texto, truncado })
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
