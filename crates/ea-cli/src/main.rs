mod cli;
mod commands;
mod tui;

use anyhow::Result;
use clap::Parser;
use ea_store::Store;

use cli::{AccionesAccion, BovedaAccion, Cli, Command, DiarioAccion, PropuestasAccion, RutinaAccion, TareasAccion, TrabajoAccion};

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
        Command::Rutina { accion } => match accion {
            RutinaAccion::Registrar { fichero } => commands::rutinas::registrar(&fichero)?,
            RutinaAccion::Listar => commands::rutinas::listar(&store)?,
            RutinaAccion::Ver { slug } => commands::rutinas::ver(&store, &slug)?,
            RutinaAccion::Lanzar { slug, entrada } => commands::rutinas::lanzar(&slug, &entrada)?,
        },
        Command::Propuestas { accion } => match accion {
            PropuestasAccion::Listar { estado } => commands::propuestas::listar(&store, estado)?,
            PropuestasAccion::Ver { id } => commands::propuestas::ver(&store, id)?,
            PropuestasAccion::Decidir { id, aceptar, rechazar } => {
                commands::propuestas::decidir(&mut store, id, aceptar, rechazar)?
            }
        },
        Command::Acciones { accion } => match accion {
            AccionesAccion::Listar => commands::acciones::listar(),
            AccionesAccion::Simular { herramienta, args } => {
                commands::acciones::simular(&store, &herramienta, &args)?
            }
            AccionesAccion::Ejecutar { herramienta, args } => {
                commands::acciones::ejecutar(&mut store, &herramienta, &args)?
            }
        },
        Command::Boveda { accion } => match accion {
            BovedaAccion::Estado => commands::boveda::estado(&mut store)?,
            BovedaAccion::Listar { region, limite } => commands::boveda::listar(&mut store, region, limite)?,
            BovedaAccion::Buscar { consulta, region, limite } => {
                commands::boveda::buscar(&mut store, &consulta, region, limite)?
            }
            BovedaAccion::Leer { ruta, desde } => commands::boveda::leer(&ruta, desde)?,
            BovedaAccion::Promover { ruta, destino } => commands::boveda::promover(&mut store, &ruta, destino)?,
            BovedaAccion::Reindexar => commands::boveda::reindexar(&mut store)?,
        },
        Command::Trabajo { accion } => match accion {
            TrabajoAccion::Crear { orden, modo, titulo, worktree, objetivo, criterio } => {
                commands::trabajo::crear(orden, &modo, titulo, worktree, objetivo, criterio)?
            }
            TrabajoAccion::Listar => commands::trabajo::listar()?,
            TrabajoAccion::Ver { id } => commands::trabajo::ver(&id)?,
            TrabajoAccion::Cerrar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cerrar)?,
            TrabajoAccion::Continuar { id } => commands::trabajo::reformular(&id, "")?,
            TrabajoAccion::Confirmar { id, contexto } => commands::trabajo::confirmar(&id, &contexto.unwrap_or_default())?,
            TrabajoAccion::Reformular { id, nota } => commands::trabajo::reformular(&id, &nota)?,
            TrabajoAccion::Cancelar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cancelar)?,
        },
        Command::Cola { agente } => commands::trabajo::cola(agente.as_deref())?,
        Command::Semaforos => commands::trabajo::semaforos(),
        Command::Tui => tui::run(&mut store)?,
    }

    Ok(())
}
