use std::path::Path;

use crate::connection;
use crate::error::StoreResult;
use crate::models::{
    EstadoPropuesta, Event, NewEvent, NewTask, Propuesta, RoutineRun, RunOutcome, Task, TaskStatus,
};
use crate::repository::events::{self, EventFilter};
use crate::repository::propuestas::{self, PropuestaFilter};
use crate::repository::routine_runs;
use crate::repository::tasks::{self, TaskFilter};

/// Fachada pública sobre la base de datos embebida. Único punto de acceso
/// para todo consumidor (CLI hoy; proceso orquestador y servidor MCP en
/// fases futuras) — ninguno ve tipos de `rusqlite`, solo tipos de dominio.
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    /// Abre la base de datos en su ruta XDG estándar (o `$EA_DB_PATH` si está
    /// definida) y aplica migraciones pendientes.
    pub fn open() -> StoreResult<Self> {
        Ok(Self {
            conn: connection::open()?,
        })
    }

    /// Igual que [`Store::open`] pero sobre una ruta explícita (`":memory:"`
    /// para tests).
    pub fn open_at(path: &Path) -> StoreResult<Self> {
        Ok(Self {
            conn: connection::open_at(path)?,
        })
    }

    pub fn create_task(&mut self, new_task: NewTask) -> StoreResult<Task> {
        tasks::create_task(&mut self.conn, new_task)
    }

    pub fn update_task_status(
        &mut self,
        task_id: i64,
        new_status: TaskStatus,
        actor: &str,
        result: Option<String>,
    ) -> StoreResult<Task> {
        tasks::update_status(&mut self.conn, task_id, new_status, actor, result)
    }

    pub fn get_task(&self, id: i64) -> StoreResult<Task> {
        tasks::get_task(&self.conn, id)
    }

    pub fn list_tasks(&self, filter: &TaskFilter) -> StoreResult<Vec<Task>> {
        tasks::list_tasks(&self.conn, filter)
    }

    pub fn append_event(&self, new_event: NewEvent) -> StoreResult<Event> {
        events::append_event(&self.conn, new_event)
    }

    pub fn get_event(&self, id: i64) -> StoreResult<Event> {
        events::get_event(&self.conn, id)
    }

    pub fn list_events(&self, filter: &EventFilter) -> StoreResult<Vec<Event>> {
        events::list_events(&self.conn, filter)
    }

    pub fn list_events_by_task(&self, task_id: i64) -> StoreResult<Vec<Event>> {
        events::list_events_by_task(&self.conn, task_id)
    }

    pub fn start_routine_run(
        &self,
        routine_slug: &str,
        task_id: Option<i64>,
    ) -> StoreResult<RoutineRun> {
        routine_runs::start_routine_run(&self.conn, routine_slug, task_id)
    }

    pub fn finish_routine_run(
        &self,
        id: i64,
        outcome: RunOutcome,
        pasos_ejecutados: i64,
        llamadas_modelo: i64,
        detalle: Option<String>,
    ) -> StoreResult<RoutineRun> {
        routine_runs::finish_routine_run(
            &self.conn,
            id,
            outcome,
            pasos_ejecutados,
            llamadas_modelo,
            detalle,
        )
    }

    pub fn get_routine_run(&self, id: i64) -> StoreResult<RoutineRun> {
        routine_runs::get_routine_run(&self.conn, id)
    }

    pub fn last_successful_run(&self, routine_slug: &str) -> StoreResult<Option<RoutineRun>> {
        routine_runs::last_successful_run(&self.conn, routine_slug)
    }

    pub fn list_routine_runs(
        &self,
        routine_slug: &str,
        limit: Option<i64>,
    ) -> StoreResult<Vec<RoutineRun>> {
        routine_runs::list_routine_runs(&self.conn, routine_slug, limit)
    }

    pub fn create_propuesta(&self, routine_run_id: i64, descripcion: &str) -> StoreResult<Propuesta> {
        propuestas::create_propuesta(&self.conn, routine_run_id, descripcion)
    }

    pub fn get_propuesta(&self, id: i64) -> StoreResult<Propuesta> {
        propuestas::get_propuesta(&self.conn, id)
    }

    pub fn list_propuestas(&self, filter: &PropuestaFilter) -> StoreResult<Vec<Propuesta>> {
        propuestas::list_propuestas(&self.conn, filter)
    }

    pub fn decidir_propuesta(&self, id: i64, estado: EstadoPropuesta) -> StoreResult<Propuesta> {
        propuestas::decidir_propuesta(&self.conn, id, estado)
    }
}
