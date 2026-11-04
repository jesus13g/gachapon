//! Capa interna de herramientas del ecosistema (§7.3 de la especificación).
//!
//! Un solo catálogo ([`catalogo`]) declara cada herramienta con su nivel de
//! riesgo y qué agentes la ven; la lógica de sistema operativo vive aquí
//! detrás de los traits de [`sistema`], nunca en las definiciones de
//! herramienta de `ea-mcp` ni en las interfaces. Las herramientas mutantes
//! tienen siempre simulación previa ([`herramientas::simular`]).
//!
//! Regla central (§11, decisión 16): **la confirmación la da el usuario,
//! nunca un agente**. Un agente solo puede [`flujo::proponer`] (encolar una
//! propuesta con acción estructurada); ejecutar ocurre únicamente al aceptar
//! la propuesta desde una interfaz del usuario ([`flujo::aceptar_propuesta`]),
//! con el nivel de riesgo limitando qué canal puede confirmar.

pub mod ajustes;
pub mod calendario;
pub mod catalogo;
mod diff;
pub mod error;
pub mod flujo;
pub mod herramientas;
mod plist;
pub mod salud;
pub mod servicios;
pub mod sesion;
pub mod sistema;
pub mod temporizador;
pub mod terminal;

pub use catalogo::{CATALOGO, Canal, Ejecucion, Herramienta, Riesgo, autorizar_mcp, buscar, puede_confirmar, terminal_interactiva};
pub use diff::diff_lineas;
pub use error::ErrorAccion;
pub use flujo::{Resultado, aceptar_propuesta, aceptar_propuesta_con_resultado, ejecutar, ejecutar_directo, proponer, rechazar_propuesta};
pub use herramientas::{Campo, Hechos, Simulacion, Supervisada, campos, observar, simular_con, validar_argumentos};
pub use servicios::{Backend, GestorServicios, InfoServicio, SERVICIOS_INSTALABLES};
pub use sesion::lanzar_sesion;
#[cfg(feature = "dobles")]
pub use sistema::SistemaSimulado;
pub use sistema::{Consulta, EstadoServicio, Sistema, SistemaReal, SoloConsulta};
pub use temporizador::{Calendario, EstadoTimer, Temporizadores};
#[cfg(any(test, feature = "dobles"))]
pub use terminal::TerminalSimulada;
pub use terminal::{OrdenTerminal, Terminal, TerminalSistema, lanzar_en_terminal};
