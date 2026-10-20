mod cli;
mod commands;
mod tui;

use anyhow::Result;
use clap::Parser;
use ea_store::Store;

use cli::{AccionesAccion, AjustesAccion, BovedaAccion, Cli, Command, DiarioAccion, PropuestasAccion, QwenSkillsAccion, RutinaAccion, TrabajoAccion};

fn main() -> Result<()> {
    let args = Cli::parse();
    // Los ajustes no necesitan la base de datos (y dicen dónde está).
    let args = match args.command {
        Command::Ajustes { accion } => {
            return match accion {
                AjustesAccion::Ver { json } => commands::ajustes::ver(json),
                AjustesAccion::Obtener { clave } => commands::ajustes::obtener(&clave),
                AjustesAccion::Establecer { cambios, quitar } => commands::ajustes::establecer(&cambios, &quitar),
                AjustesAccion::Ruta => commands::ajustes::ruta(),
            };
        }
        otro => Cli { command: otro },
    };
    let mut store = Store::open()?;

    match args.command {
        Command::Status => commands::status::mostrar(&store)?,
        Command::Diario { accion } => match accion {
            DiarioAccion::Listar {
                trabajo,
                actor,
                desde,
                hasta,
                limite,
            } => commands::diario::listar(&store, trabajo, actor, desde, hasta, limite)?,
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
        Command::QwenSkills { accion } => match accion {
            QwenSkillsAccion::Listar { json } => commands::qwen_skills::listar(json)?,
            QwenSkillsAccion::Ver { nombre } => commands::qwen_skills::ver(&nombre)?,
            QwenSkillsAccion::Probar { skills, por } => commands::qwen_skills::probar(&por, &skills)?,
        },
        Command::Trabajo { accion } => match accion {
            TrabajoAccion::Crear { orden, modo, titulo, worktree, objetivo, criterio, skills, por } => {
                commands::trabajo::crear(orden, &modo, titulo, worktree, objetivo, criterio, skills, por)?
            }
            TrabajoAccion::Listar => commands::trabajo::listar()?,
            TrabajoAccion::Ver { id, json } => commands::trabajo::ver(&id, json)?,
            TrabajoAccion::Diff { id, fase } => commands::trabajo::diff(&id, fase)?,
            TrabajoAccion::Cerrar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cerrar)?,
            TrabajoAccion::Continuar { id } => commands::trabajo::reformular(&id, "")?,
            TrabajoAccion::Confirmar { id, contexto } => commands::trabajo::confirmar(&id, &contexto.unwrap_or_default())?,
            TrabajoAccion::Reformular { id, nota } => commands::trabajo::reformular(&id, &nota)?,
            TrabajoAccion::Cancelar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cancelar)?,
        },
        Command::Cola { agente, json } => commands::trabajo::cola(agente.as_deref(), json)?,
        Command::Semaforos => commands::trabajo::semaforos(),
        Command::Tui => tui::run(&mut store)?,
        Command::Ajustes { .. } => unreachable!("se atiende antes de abrir la base"),
    }

    Ok(())
}
