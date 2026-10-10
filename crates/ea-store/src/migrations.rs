use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::error::StoreResult;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/0001_initial.sql")),
        M::up(include_str!("../migrations/0002_routine_runs.sql")),
        M::up(include_str!("../migrations/0003_propuestas_acciones.sql")),
        M::up(include_str!("../migrations/0004_boveda_indice.sql")),
    ])
}

pub fn run_migrations(conn: &mut Connection) -> StoreResult<()> {
    migrations().to_latest(conn)?;
    Ok(())
}
