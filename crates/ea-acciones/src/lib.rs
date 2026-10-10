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

pub mod catalogo;
pub mod error;
pub mod flujo;
pub mod herramientas;
pub mod sistema;

pub use ea_boveda::Boveda;
pub use catalogo::{autorizar_mcp, buscar, Canal, Ejecucion, Herramienta, Riesgo, CATALOGO};
pub use error::ErrorAccion;
pub use flujo::{aceptar_propuesta, ejecutar, ejecutar_directo, proponer, rechazar_propuesta, Resultado};
pub use herramientas::{observar, simular_con, validar_argumentos, Hechos, Simulacion};
pub use sistema::{Consulta, EstadoServicio, Sistema, SistemaReal, SistemaSimulado, SoloConsulta};
