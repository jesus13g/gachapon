//! Bus de ficheros entre el ejecutor y el planificador: una cola por rol,
//! el estado de cada trabajo y un semáforo que da a un agente acceso
//! exclusivo de escritura a todo el worktree. Los ficheros son la fuente de
//! verdad; SQLite (`ea-store`) guarda el historial y el diario.

mod bus;
mod candado;
mod equipo;
mod error;
mod fs_atomico;
/// El `git` endurecido y con tope que el sistema ejecuta fuera del sandbox.
pub mod git;
mod instantanea;
mod papelera;
mod presencia;
mod rutina;
pub mod skills;
/// Las marcas del hook de subagentes de Claude Code, bajo la raíz del bus.
pub mod subagentes;
mod tipos;
mod vista;
mod worktree;

pub use worktree::InfoWorktree;
pub use git::validar_worktree;
pub use bus::{comprobar_registro, id_valido, Bus, Entrega, EntradaCola, raiz_estado};
pub use equipo::{comprobar_disponibles, parse_miembro, repartir_todos, validar_equipo, Miembro, NuevoEquipo, RolEquipo};
pub use vista::{LineaInforme, MiembroResumen, ResumenEquipo, ResumenTrabajo, VistaEquipo, VistaMiembro, VistaTrabajoEquipo};
pub use presencia::{Latido, Presencia, CADUCIDAD_LATIDO_S, INTERVALO_LATIDO_S};
pub use candado::{CandadoWorktree, Ocupante, listar_semaforos, listar_semaforos_en};
pub use skills::{bloque_prompt, ConfigSkills, Skill};
pub use rutina::{slug_valido, validar_entradas, CabeceraRutina, Paso, Rutina, MAX_DEFINICION, MAX_ENTRADAS, MAX_VALOR_ENTRADA};
pub use error::{ErrorBus, Resultado};
/// La papelera de las rutinas: retirar y restaurar sin tocar SQLite (spec 004, RF-011).
pub use papelera::{
    comprobar_lanzable, quitar_de_papelera, restaurar_rutina, retirar_rutina, ruta_activa, ruta_retirada, rutinas_por_ubicacion,
    ubicacion_rutina, Ubicacion, DIR_PAPELERA,
};
/// Escritura atómica (temporal, `fsync` y renombrado), también para quien
/// escribe fuera del bus (el timer de una rutina, en `ea-acciones`).
pub use fs_atomico::escribir_atomico;
pub use instantanea::{
    arbol_de_fase, diff_stat_de_fase, leer_salida_agente, rango_diff_fase, resumen_salida_agente, tipo_ejecutor, EXCLUIR_DIFF,
    SALIDA_AGENTE, SALIDA_ANTIGUA, SKILLS_AGENTE,
};
/// El adaptador de cada agente ejecutor, con el que se lee su salida; es parte
/// de la ficha del catálogo (`ea-ajustes`).
pub use ea_ajustes::TipoAgente;
pub use tipos::{
    EjecutadoPor, Equipo, Estado, EstadoEquipo, EstadoTrabajo, Evento, Fase, FaseInforme, Informe, Modo, Orden,
    ResultadoInforme, Rol, RutinaRef, Topes,
    transicion,
};
