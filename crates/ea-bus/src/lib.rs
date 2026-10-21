//! Bus de ficheros entre el ejecutor y el planificador: una cola por rol,
//! el estado de cada trabajo y un semáforo que da a un agente acceso
//! exclusivo de escritura a todo el worktree. Los ficheros son la fuente de
//! verdad; SQLite (`ea-store`) guarda el historial y el diario.

mod bus;
mod candado;
pub mod config;
mod equipo;
mod error;
mod fs_atomico;
mod instantanea;
mod papelera;
mod presencia;
mod rutina;
pub mod skills;
mod tipos;
mod vista;
mod worktree;

pub use bus::{comprobar_registro, Bus, EntradaCola, ENV_ESTADO, raiz_estado};
pub use equipo::NuevoEquipo;
pub use vista::{LineaInforme, MiembroResumen, ResumenEquipo, VistaEquipo, VistaMiembro, VistaTrabajoEquipo};
pub use presencia::{Presencia, CADUCIDAD_LATIDO_S, INTERVALO_LATIDO_S};
pub use candado::{CandadoWorktree, Ocupante, listar_semaforos, listar_semaforos_en};
pub use config::{ConfigOrquestador, PerfilEjecutor, TipoAgente};
pub use skills::{bloque_prompt, ConfigSkills, Skill};
pub use rutina::{slug_valido, validar_entradas, CabeceraRutina, Paso, Rutina, MAX_DEFINICION, MAX_ENTRADAS, MAX_VALOR_ENTRADA};
pub use error::{ErrorBus, Resultado};
/// La papelera de las rutinas: retirar y restaurar sin tocar SQLite (spec 004, RF-011).
pub use papelera::{
    comprobar_lanzable, quitar_de_papelera, restaurar_rutina, retirar_rutina, ruta_activa, ruta_retirada, ubicacion_rutina,
    Ubicacion, DIR_PAPELERA,
};
/// Escritura atómica (temporal, `fsync` y renombrado), también para quien
/// escribe fuera del bus (el timer de una rutina, en `ea-acciones`).
pub use fs_atomico::escribir_atomico;
pub use instantanea::{
    arbol_de_fase, diff_stat_de_fase, leer_salida_agente, rango_diff_fase, resumen_salida_agente, tipo_ejecutor, EXCLUIR_DIFF,
    SALIDA_AGENTE, SALIDA_ANTIGUA, SKILLS_AGENTE,
};
pub use tipos::{
    EjecutadoPor, Equipo, Estado, EstadoEquipo, EstadoTrabajo, Evento, Fase, FaseInforme, Informe, Miembro, Modo, Orden,
    ResultadoInforme, Rol, RolEquipo, RutinaRef, Topes,
    transicion,
};
