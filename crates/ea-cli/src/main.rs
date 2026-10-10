mod cli;
mod commands;
mod tui;

use anyhow::Result;
use clap::Parser;
use ea_store::Store;

use cli::{Cli, Command, DiarioAccion, PropuestasAccion, RutinasAccion, TareasAccion, VozAccion};

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut store = Store::open()?;

    match args.command {
        Command::Status => commands::status::mostrar(&store)?,
        Command::Tareas { accion } => match accion {
            TareasAccion::Crear {
                titulo,
                asignado_a,
                solicita,
                origen,
                creado_por,
                tarea_padre,
            } => commands::tareas::crear(
                &mut store,
                titulo,
                asignado_a,
                solicita,
                origen,
                creado_por,
                tarea_padre,
            )?,
            TareasAccion::Listar {
                estado,
                asignado_a,
                limite,
            } => commands::tareas::listar(&store, estado, asignado_a, limite)?,
            TareasAccion::Ver { id } => commands::tareas::ver(&store, id)?,
            TareasAccion::Actualizar {
                id,
                estado,
                resultado,
                actor,
            } => commands::tareas::actualizar(&mut store, id, estado, resultado, actor)?,
        },
        Command::Diario { accion } => match accion {
            DiarioAccion::Listar {
                tarea,
                actor,
                desde,
                hasta,
                limite,
            } => commands::diario::listar(&store, tarea, actor, desde, hasta, limite)?,
            DiarioAccion::Ver { id } => commands::diario::ver(&store, id)?,
        },
        Command::Rutinas { accion } => match accion {
            RutinasAccion::Listar => commands::rutinas::listar(&store)?,
            RutinasAccion::Ver { slug } => commands::rutinas::ver(&store, &slug)?,
            RutinasAccion::Encolar { slug } => commands::rutinas::encolar(&mut store, &slug)?,
        },
        Command::Propuestas { accion } => match accion {
            PropuestasAccion::Listar { estado } => commands::propuestas::listar(&store, estado)?,
            PropuestasAccion::Ver { id } => commands::propuestas::ver(&store, id)?,
            PropuestasAccion::Decidir { id, aceptar, rechazar } => {
                commands::propuestas::decidir(&mut store, id, aceptar, rechazar)?
            }
        },
        Command::Voz { accion } => match accion {
            VozAccion::Hablar => commands::voz::hablar()?,
        },
        Command::Tui => tui::run(&store)?,
    }

    Ok(())
}
