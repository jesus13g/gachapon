mod cli;
mod commands;
mod tui;

use anyhow::Result;
use clap::Parser;
use ea_store::Store;

use cli::{AccionesAccion, AgentesAccion, AjustesAccion, BibliotecaAccion, BovedaAccion, Cli, Command, DiarioAccion, EquipoAccion, HookAccion, PropuestasAccion, QwenSkillsAccion, RutinaAccion, ServiciosAccion, TrabajoAccion, WorktreeAccion};

fn main() -> Result<()> {
    salir_en_silencio_si_se_cierra_la_tuberia();
    let args = Cli::parse();
    // Los ajustes y los agentes no necesitan la base de datos (y los ajustes
    // dicen dónde está).
    let args = match args.command {
        Command::Ajustes { accion } => {
            return match accion {
                AjustesAccion::Ver { json } => commands::ajustes::ver(json),
                AjustesAccion::Obtener { clave } => commands::ajustes::obtener(&clave),
                AjustesAccion::Establecer { cambios, quitar } => commands::ajustes::establecer(&cambios, &quitar),
                AjustesAccion::Ruta => commands::ajustes::ruta(),
            };
        }
        Command::Agentes { accion, json } => {
            return match accion {
                None => commands::agentes::listar(json),
                Some(AgentesAccion::Rol { rol, agente }) => commands::agentes::rol(&rol, &agente),
            };
        }
        Command::Hook { accion } => {
            return match accion {
                HookAccion::Subagentes { accion } => commands::hook::subagentes(&accion),
            };
        }
        Command::Servicios { accion } => {
            return match accion {
                ServiciosAccion::Ver => commands::servicios::ver(),
                ServiciosAccion::Instalar { esperar } => commands::al_final(esperar, commands::servicios::instalar()),
                ServiciosAccion::Desinstalar { esperar } => commands::al_final(esperar, commands::servicios::desinstalar()),
            };
        }
        Command::Equipo { accion } => {
            return match accion {
                EquipoAccion::Despertar { todos, miembro, titulo, objetivo, worktree, sin_terminal } => {
                    commands::equipo::despertar(&miembro, todos, &titulo, &objetivo, &worktree, sin_terminal)
                }
                EquipoAccion::Listar { json } => commands::equipo::listar(json),
                EquipoAccion::Ver { id, json } => commands::equipo::ver(&id, json),
                EquipoAccion::Dormir { id } => commands::equipo::dormir(&id),
                EquipoAccion::Sesion { id, agente } => commands::equipo::sesion(&id, &agente),
            };
        }
        Command::Worktree { accion: WorktreeAccion::Listar { json } } => return commands::worktree::listar(json),
        // La biblioteca lee el disco; solo proponer necesita la base.
        Command::Biblioteca { accion } => match accion {
            BibliotecaAccion::Listar { agente, tipo, categoria, activados, desactivados, ids, json } => {
                return commands::biblioteca::listar(agente, tipo, categoria, activados, desactivados, ids, json);
            }
            BibliotecaAccion::Ver { id, json } => return commands::biblioteca::ver(&id, json),
            BibliotecaAccion::Uso { id, json } => return commands::biblioteca::uso(id, json),
            BibliotecaAccion::Categoria { id, categoria, quitar } => {
                return commands::biblioteca::categoria(&id, categoria.as_deref(), quitar);
            }
            accion => Cli { command: Command::Biblioteca { accion } },
        },
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
                resultado,
                accion,
                antes_de,
                limite,
            } => commands::diario::listar(
                &store,
                commands::diario::Filtro { trabajo, actor, desde, hasta, resultado, accion, antes_de },
                limite,
            )?,
            DiarioAccion::Ver { id } => commands::diario::ver(&store, id)?,
            DiarioAccion::Anotar { texto, corrige, trabajo } => {
                commands::diario::anotar(&store, ea_store::NuevaNota { texto, corrige, trabajo })?
            }
        },
        Command::Rutina { accion } => match accion {
            RutinaAccion::Registrar { fichero } => commands::rutinas::registrar(&fichero)?,
            RutinaAccion::Listar { retiradas } => commands::rutinas::listar(&store, retiradas)?,
            RutinaAccion::Ver { slug, version: None } => commands::rutinas::ver(&store, &slug)?,
            RutinaAccion::Ver { slug, version: Some(n) } => commands::rutinas::ver_version(&store, &slug, n)?,
            RutinaAccion::Lanzar { slug, entrada } => commands::rutinas::lanzar(&slug, &entrada)?,
            RutinaAccion::Retirar { slug } => commands::rutinas::retirar(&mut store, &slug)?,
            RutinaAccion::Restaurar { slug } => commands::rutinas::restaurar(&mut store, &slug)?,
        },
        Command::Propuestas { accion } => match accion {
            PropuestasAccion::Listar { estado } => commands::propuestas::listar(&store, estado)?,
            PropuestasAccion::Ver { id } => commands::propuestas::ver(&store, id)?,
            PropuestasAccion::Decidir { id, aceptar, rechazar, esperar } => {
                commands::al_final(esperar, commands::propuestas::decidir(&mut store, id, aceptar, rechazar))?
            }
        },
        Command::Acciones { accion } => match accion {
            AccionesAccion::Listar => commands::acciones::listar(),
            AccionesAccion::Simular { herramienta, args } => {
                commands::acciones::simular(&store, &herramienta, &args)?
            }
            AccionesAccion::Ejecutar { herramienta, args, esperar } => {
                commands::al_final(esperar, commands::acciones::ejecutar(&mut store, &herramienta, &args))?
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
            BovedaAccion::VaciarPapelera { esperar } => {
                commands::al_final(esperar, commands::boveda::vaciar_papelera(&mut store))?
            }
        },
        Command::QwenSkills { accion } => match accion {
            QwenSkillsAccion::Listar { json } => commands::qwen_skills::listar(json)?,
            QwenSkillsAccion::Ver { nombre } => commands::qwen_skills::ver(&nombre)?,
            QwenSkillsAccion::Probar { skills, por } => commands::qwen_skills::probar(&por, &skills)?,
        },
        Command::Trabajo { accion } => match accion {
            TrabajoAccion::Crear { orden, modo, titulo, worktree, objetivo, criterio, skills, por, equipo, asignar, revisor, worktree_propio } => {
                commands::trabajo::crear(orden, &modo, titulo, worktree, objetivo, criterio, skills, por, commands::trabajo::DeEquipo { equipo, asignar, revisor, worktree_propio })?
            }
            TrabajoAccion::Listar => commands::trabajo::listar()?,
            TrabajoAccion::Ver { id, json } => commands::trabajo::ver(&id, json)?,
            TrabajoAccion::Diff { id, fase } => commands::trabajo::diff(&id, fase)?,
            TrabajoAccion::Cerrar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cerrar)?,
            TrabajoAccion::Continuar { id } => commands::trabajo::reformular(&id, "", None)?,
            TrabajoAccion::Confirmar { id, contexto } => commands::trabajo::confirmar(&id, &contexto.unwrap_or_default())?,
            TrabajoAccion::Reformular { id, nota, fase } => commands::trabajo::reformular(&id, &nota, fase)?,
            TrabajoAccion::Cancelar { id } => commands::trabajo::evento(&id, ea_bus::Evento::Cancelar)?,
        },
        Command::Cola { agente, json } => commands::trabajo::cola(agente.as_deref(), json)?,
        Command::Semaforos => commands::trabajo::semaforos(),
        Command::Biblioteca { accion } => match accion {
            BibliotecaAccion::Activar { id, esperar } => {
                commands::al_final(esperar, commands::biblioteca::cambiar(&mut store, &id, true, esperar))?
            }
            BibliotecaAccion::Desactivar { id, esperar } => {
                commands::al_final(esperar, commands::biblioteca::cambiar(&mut store, &id, false, esperar))?
            }
            BibliotecaAccion::Sesion { agente, ids, directorio, esperar } => {
                commands::al_final(esperar, commands::biblioteca::sesion(&mut store, &agente, &ids, directorio, esperar))?
            }
            _ => unreachable!("se atiende antes de abrir la base"),
        },
        Command::Worktree { accion: WorktreeAccion::Limpiar { id } } => commands::worktree::limpiar(&mut store, &id)?,
        Command::Tui => tui::run(&mut store)?,
        Command::Ajustes { .. } | Command::Agentes { .. } | Command::Servicios { .. } | Command::Hook { .. } | Command::Equipo { .. } | Command::Worktree { accion: WorktreeAccion::Listar { .. } } => unreachable!("se atiende antes de abrir la base"),
    }

    Ok(())
}

/// Rust ignora SIGPIPE, así que `ea ... | head` acababa en pánico al escribir
/// en una tubería cerrada. Con la acción por defecto el proceso termina sin
/// ruido, como cualquier herramienta de Unix.
fn salir_en_silencio_si_se_cierra_la_tuberia() {
    ea_plataforma::proceso::ignorar_sigpipe_por_defecto();
}
