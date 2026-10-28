mod cli;
mod commands;
mod instalacion;
mod tui;

use anyhow::Result;
use clap::Parser;

use cli::{AccionesAccion, AgentesAccion, AjustesAccion, BibliotecaAccion, BovedaAccion, Cli, Command, DiarioAccion, EquipoAccion, HookAccion, McpAccion, PropuestasAccion, QwenSkillsAccion, RutinaAccion, ServiciosAccion, TrabajoAccion, WorktreeAccion};

fn main() -> Result<()> {
    salir_en_silencio_si_se_cierra_la_tuberia();
    // Cada orden pide lo que usa (los ajustes, la base, los timers) y solo eso
    // se abre: los ajustes y los agentes no necesitan la base de datos (y los
    // ajustes dicen dónde está). Las que la usan después de leer o enseñar
    // algo, o tras `--esperar`, la abren antes que nada: si no abre, fallan
    // sin más (y sin esperar a Intro).
    let mut ctx = commands::Contexto::default();
    match Cli::parse().command {
        Command::Ajustes { accion } => match accion {
            AjustesAccion::Ver { json } => commands::ajustes::ver(json)?,
            AjustesAccion::Obtener { clave } => commands::ajustes::obtener(&clave)?,
            AjustesAccion::Establecer { cambios, quitar } => commands::ajustes::establecer(&cambios, &quitar)?,
            AjustesAccion::Ruta => commands::ajustes::ruta()?,
        },
        Command::Agentes { accion, json } => match accion {
            None => commands::agentes::listar(json)?,
            Some(AgentesAccion::Rol { rol, agente }) => commands::agentes::rol(rol.0, &agente)?,
        },
        Command::Hook { accion: HookAccion::Subagentes { accion } } => commands::hook::subagentes(&accion)?,
        Command::Mcp { accion: McpAccion::Cabeceras } => commands::mcp::cabeceras()?,
        Command::Servicios { accion } => match accion {
            ServiciosAccion::Ver => commands::servicios::ver()?,
            ServiciosAccion::Instalar { esperar } => commands::al_final(esperar, commands::servicios::instalar())?,
            ServiciosAccion::Desinstalar { esperar } => commands::al_final(esperar, commands::servicios::desinstalar())?,
        },
        Command::Instalar { via, distro, sin_escritorio, sin_dependencias, continuar } => {
            let via = via.as_deref().map(commands::instalar::Via::desde).transpose()?;
            commands::instalar::instalar(commands::instalar::Opciones { via, distro, sin_escritorio, sin_dependencias, continuar })?
        }
        Command::Desinstalar { esperar } => commands::al_final(esperar, commands::instalar::desinstalar())?,
        Command::Equipo { accion } => match accion {
            EquipoAccion::Despertar { todos, miembro, titulo, objetivo, worktree, sin_terminal } => {
                commands::equipo::despertar(&miembro, todos, &titulo, &objetivo, &worktree, sin_terminal)?
            }
            EquipoAccion::Listar { json } => commands::equipo::listar(json)?,
            EquipoAccion::Ver { id, json } => commands::equipo::ver(&id, json)?,
            EquipoAccion::Dormir { id } => commands::equipo::dormir(&id)?,
            EquipoAccion::Sesion { id, agente } => commands::equipo::sesion(&id, &agente)?,
        },
        Command::Worktree { accion } => match accion {
            WorktreeAccion::Listar { json } => commands::worktree::listar(json)?,
            WorktreeAccion::Limpiar { id } => commands::worktree::limpiar(&mut ctx, &id)?,
        },
        Command::Status => commands::status::mostrar(ctx.store()?)?,
        Command::Diario { accion } => match accion {
            DiarioAccion::Listar {
                trabajo,
                actor,
                desde,
                hasta,
                resultado,
                accion,
                antes_de,
                limite,
            } => commands::diario::listar(
                ctx.store()?,
                ea_store::FiltroDiario {
                    trabajo,
                    actor,
                    desde,
                    hasta,
                    resultado,
                    accion,
                    antes_de: antes_de.map(|n| n.to_string()),
                    limite: Some(limite),
                },
            )?,
            DiarioAccion::Ver { id } => commands::diario::ver(ctx.store()?, id)?,
            DiarioAccion::Anotar { texto, corrige, trabajo } => {
                commands::diario::anotar(ctx.store()?, ea_store::NuevaNota { texto, corrige, trabajo })?
            }
        },
        Command::Rutina { accion } => match accion {
            RutinaAccion::Registrar { fichero } => {
                ctx.store()?;
                commands::rutinas::registrar(&mut ctx, &fichero)?
            }
            RutinaAccion::Listar { retiradas } => commands::rutinas::listar(&mut ctx, retiradas)?,
            RutinaAccion::Ver { slug, version: None } => commands::rutinas::ver(&mut ctx, &slug)?,
            RutinaAccion::Ver { slug, version: Some(n) } => commands::rutinas::ver_version(ctx.store()?, &slug, n)?,
            RutinaAccion::Lanzar { slug, entrada } => {
                ctx.store()?;
                commands::rutinas::lanzar(&mut ctx, &slug, &entrada)?
            }
            RutinaAccion::Retirar { slug } => commands::rutinas::retirar(&mut ctx, &slug)?,
            RutinaAccion::Restaurar { slug } => commands::rutinas::restaurar(&mut ctx, &slug)?,
        },
        Command::Propuestas { accion } => match accion {
            PropuestasAccion::Listar { estado } => commands::propuestas::listar(ctx.store()?, estado)?,
            PropuestasAccion::Ver { id } => commands::propuestas::ver(ctx.store()?, id)?,
            PropuestasAccion::Decidir { id, aceptar, rechazar, esperar } => {
                ctx.store()?;
                commands::al_final(esperar, commands::propuestas::decidir(&mut ctx, id, aceptar, rechazar))?
            }
        },
        Command::Acciones { accion } => match accion {
            AccionesAccion::Listar => commands::acciones::listar(),
            AccionesAccion::Simular { herramienta, args } => commands::acciones::simular(&mut ctx, &herramienta, &args)?,
            AccionesAccion::Ejecutar { herramienta, args, esperar } => {
                ctx.store()?;
                commands::al_final(esperar, commands::acciones::ejecutar(&mut ctx, &herramienta, &args))?
            }
        },
        Command::Boveda { accion } => match accion {
            BovedaAccion::Estado => commands::boveda::estado(ctx.store()?)?,
            BovedaAccion::Listar { region, limite } => commands::boveda::listar(ctx.store()?, region, limite)?,
            BovedaAccion::Buscar { consulta, region, limite } => {
                commands::boveda::buscar(ctx.store()?, &consulta, region, limite)?
            }
            BovedaAccion::Leer { ruta, desde } => commands::boveda::leer(&ruta, desde)?,
            BovedaAccion::Promover { ruta, destino } => {
                ctx.store()?;
                commands::boveda::promover(&mut ctx, &ruta, destino)?
            }
            BovedaAccion::Reindexar => commands::boveda::reindexar(ctx.store()?)?,
            BovedaAccion::VaciarPapelera { esperar } => {
                let store = ctx.store()?;
                commands::al_final(esperar, commands::boveda::vaciar_papelera(store))?
            }
        },
        Command::QwenSkills { accion } => match accion {
            QwenSkillsAccion::Listar { json } => commands::qwen_skills::listar(json)?,
            QwenSkillsAccion::Ver { nombre } => commands::qwen_skills::ver(&nombre)?,
            QwenSkillsAccion::Probar { skills, por } => commands::qwen_skills::probar(&por, &skills)?,
        },
        Command::Trabajo { accion } => match accion {
            TrabajoAccion::Crear { orden, modo, titulo, worktree, objetivo, criterio, skills, por, equipo, asignar, revisor, worktree_propio } => {
                ctx.store()?;
                commands::trabajo::crear(&mut ctx, orden, modo.0, titulo, worktree, objetivo, criterio, skills, por, commands::trabajo::DeEquipo { equipo, asignar, revisor, worktree_propio })?
            }
            TrabajoAccion::Listar => commands::trabajo::listar()?,
            TrabajoAccion::Ver { id, json } => commands::trabajo::ver(&id, json)?,
            TrabajoAccion::Diff { id, fase } => commands::trabajo::diff(&id, fase)?,
            TrabajoAccion::Cerrar { id } => commands::trabajo::evento(&mut ctx, &id, ea_bus::Evento::Cerrar)?,
            TrabajoAccion::Continuar { id } => commands::trabajo::reformular(&mut ctx, &id, "", None)?,
            // U-04: confirmar no exige terminal (`tests/tty.rs`).
            TrabajoAccion::Confirmar { id, contexto } => commands::trabajo::confirmar(&mut ctx, &id, &contexto.unwrap_or_default())?,
            TrabajoAccion::Reformular { id, nota, fase } => commands::trabajo::reformular(&mut ctx, &id, &nota, fase)?,
            TrabajoAccion::Cancelar { id } => commands::trabajo::evento(&mut ctx, &id, ea_bus::Evento::Cancelar)?,
        },
        Command::Cola { agente, json } => commands::trabajo::cola(agente.map(|r| r.0), json)?,
        Command::Semaforos => commands::trabajo::semaforos(),
        // La biblioteca lee el disco; solo proponer necesita la base.
        Command::Biblioteca { accion } => match accion {
            BibliotecaAccion::Listar { agente, tipo, categoria, activados, desactivados, ids, json } => {
                commands::biblioteca::listar(agente, tipo, categoria, activados, desactivados, ids, json)?
            }
            BibliotecaAccion::Ver { id, json } => commands::biblioteca::ver(&id, json)?,
            BibliotecaAccion::Uso { id, json } => commands::biblioteca::uso(id, json)?,
            BibliotecaAccion::Categoria { id, categoria, quitar } => commands::biblioteca::categoria(&id, categoria.as_deref(), quitar)?,
            BibliotecaAccion::Activar { id, esperar } => {
                ctx.store()?;
                commands::al_final(esperar, commands::biblioteca::cambiar(&mut ctx, &id, true, esperar))?
            }
            BibliotecaAccion::Desactivar { id, esperar } => {
                ctx.store()?;
                commands::al_final(esperar, commands::biblioteca::cambiar(&mut ctx, &id, false, esperar))?
            }
            BibliotecaAccion::Sesion { agente, ids, directorio, esperar } => {
                ctx.store()?;
                commands::al_final(esperar, commands::biblioteca::sesion(&mut ctx, &agente, &ids, directorio, esperar))?
            }
        },
        Command::Tui => tui::run(ctx.store()?)?,
    }

    Ok(())
}

/// Rust ignora SIGPIPE, así que `ea ... | head` acababa en pánico al escribir
/// en una tubería cerrada. Con la acción por defecto el proceso termina sin
/// ruido, como cualquier herramienta de Unix.
fn salir_en_silencio_si_se_cierra_la_tuberia() {
    ea_plataforma::proceso::ignorar_sigpipe_por_defecto();
}
