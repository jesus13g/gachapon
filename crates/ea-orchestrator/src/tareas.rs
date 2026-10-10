//! Ejecución de las tareas dirigidas al modelo local. Quien crea una tarea
//! para `modelo-local` (el panel web, la CLI, otro agente por MCP) la deja
//! pendiente; el sondeo la recoge aquí, le pasa la petición a `ea-proxy` —
//! el mismo camino que recorre una orden de voz (§9.3) — y anota la
//! respuesta como resultado. Las de voz no pasan por aquí: su turno
//! (`turno.rs`) las crea y las resuelve él mismo.

use std::future::Future;
use std::time::Instant;

use ea_store::{EventOutcome, NewEvent, Store, StoreResult, Task, TaskFilter, TaskOrigin, TaskStatus};
use serde_json::json;

use crate::proxy_client::ProxyClient;

const ORQUESTADOR: &str = "orquestador";
pub const AGENTE: &str = "modelo-local";

/// Quien responde a una petición en texto libre (`ea-proxy`; en las pruebas,
/// un doble).
pub trait Completador {
    fn completar(&self, texto: &str) -> impl Future<Output = anyhow::Result<String>> + Send;
}

impl Completador for ProxyClient {
    async fn completar(&self, texto: &str) -> anyhow::Result<String> {
        ProxyClient::completar(self, texto).await
    }
}

/// Ejecuta, de la más antigua a la más nueva, las tareas pendientes del
/// modelo local. Devuelve cuántas ha ejecutado.
pub async fn ejecutar_pendientes<C: Completador>(store: &mut Store, modelo: &C) -> StoreResult<usize> {
    let pendientes = store.list_tasks(&TaskFilter {
        status: Some(TaskStatus::Pendiente),
        assigned_to: Some(AGENTE.to_string()),
        ..Default::default()
    })?;

    let mut ejecutadas = 0;
    for tarea in pendientes.into_iter().rev().filter(|t| t.origin != TaskOrigin::Voz) {
        if ejecutar_una(store, modelo, &tarea).await? {
            ejecutadas += 1;
        }
    }
    Ok(ejecutadas)
}

async fn ejecutar_una<C: Completador>(store: &mut Store, modelo: &C, tarea: &Task) -> StoreResult<bool> {
    // Entre el listado y este punto puede haber pasado una respuesta larga
    // de la tarea anterior: el usuario ha podido cancelarla mientras tanto.
    if store.get_task(tarea.id)?.status != TaskStatus::Pendiente {
        return Ok(false);
    }
    store.update_task_status(tarea.id, TaskStatus::EnProgreso, ORQUESTADOR, None)?;

    let inicio = Instant::now();
    let respuesta = modelo.completar(&tarea.request).await;
    let duracion_ms = inicio.elapsed().as_millis() as i64;

    store.append_event(NewEvent {
        actor: AGENTE.to_string(),
        action: "modelo_respuesta".to_string(),
        task_id: Some(tarea.id),
        outcome: Some(if respuesta.is_ok() { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: Some(duracion_ms),
        detail: respuesta.as_ref().err().map(|e| json!({ "error": e.to_string() }).to_string()),
    })?;

    // Si alguien la cerró mientras el modelo respondía (una cancelación
    // desde el panel), su decisión se respeta: la respuesta queda en el
    // diario pero no pisa el estado.
    if store.get_task(tarea.id)?.status != TaskStatus::EnProgreso {
        return Ok(true);
    }
    match respuesta {
        Ok(texto) => store.update_task_status(tarea.id, TaskStatus::Completada, AGENTE, Some(texto))?,
        Err(e) => store.update_task_status(tarea.id, TaskStatus::Fallida, ORQUESTADOR, Some(e.to_string()))?,
    };
    Ok(true)
}
