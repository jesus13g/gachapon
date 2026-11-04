//! Bus de ficheros entre el ejecutor y el planificador: una cola por rol,
//! el estado de cada trabajo y un semáforo que da a un agente acceso
//! exclusivo de escritura a todo el worktree. Los ficheros son la fuente de
//! verdad; SQLite (`ea-store`) guarda el historial y el diario.

mod artefactos;
mod bus;
mod candado;
mod equipo;
mod error;
mod fs_atomico;
/// El `git` endurecido y con tope que el sistema ejecuta fuera del sandbox.
pub mod git;
mod instantanea;
mod orquestador;
mod papelera;
mod presencia;
mod rutina;
pub mod skills;
/// Las marcas del hook de subagentes de Claude Code, bajo la raíz del bus.
pub mod subagentes;
mod tipos;
mod vista;
mod worktree;

/// Los ficheros de cada trabajo además de orden, estado e informe.
pub use artefactos::{Artefactos, SALIDA_AGENTE, SKILLS_AGENTE};
pub use bus::{Bus, Cola, EntradaCola, Entrega, comprobar_registro, id_valido, raiz_estado};
pub use candado::{CandadoWorktree, Ocupante, listar_semaforos, listar_semaforos_en};
/// El adaptador de cada agente ejecutor, con el que se lee su salida; es parte
/// de la ficha del catálogo (`ea-ajustes`).
pub use ea_ajustes::TipoAgente;
pub use equipo::{Miembro, NuevoEquipo, RolEquipo, comprobar_disponibles, parse_miembro, repartir_todos, validar_equipo};
pub use error::{ErrorBus, Resultado};
/// Escritura atómica (temporal, `fsync` y renombrado), también para quien
/// escribe fuera del bus (el timer de una rutina, en `ea-acciones`).
pub use fs_atomico::escribir_atomico;
pub use git::validar_worktree;
pub use instantanea::{EXCLUIR_DIFF, arbol_de_fase, diff_stat_de_fase, rango_diff_fase, resumen_salida_agente, tipo_ejecutor};
pub use orquestador::{EstadoOrquestador, FICHERO_LATIDO, LatidoOrquestador, SandboxInfo, estado_orquestador, latido_orquestador};
/// La papelera de las rutinas: retirar y restaurar sin tocar SQLite (spec 004, RF-011).
pub use papelera::{
    DIR_PAPELERA, Ubicacion, comprobar_lanzable, quitar_de_papelera, restaurar_rutina, retirar_rutina, ruta_activa, ruta_retirada, rutinas_por_ubicacion,
    ubicacion_rutina,
};
pub use presencia::{CADUCIDAD_LATIDO_S, INTERVALO_LATIDO_S, Latido, Presencia};
pub use rutina::{CabeceraRutina, MAX_DEFINICION, MAX_ENTRADAS, MAX_VALOR_ENTRADA, Paso, Rutina, slug_valido, validar_entradas};
pub use skills::{ConfigSkills, Skill, bloque_prompt};
pub use tipos::{
    EjecutadoPor, Equipo, Estado, EstadoEquipo, EstadoFase, EstadoTrabajo, Evento, Fase, FaseInforme, Informe, InformeFase, Modo, Orden, ResultadoInforme, Rol,
    RutinaRef, Topes, transicion,
};
pub use vista::{LineaInforme, MiembroResumen, ResumenEquipo, ResumenTrabajo, VistaEquipo, VistaMiembro, VistaTrabajoEquipo};
pub use worktree::InfoWorktree;
