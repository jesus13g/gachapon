//! Bóveda compartida (§8.3 de la especificación): el sustrato de notas que
//! comparten los dos agentes y el usuario.
//!
//! - **Escritor único serializado** ([`Boveda`], módulo `escritor`): todo
//!   fichero de la bóveda se crea o cambia aquí, bajo un candado de fichero
//!   (`flock`) que excluye a cualquier otro proceso, con escritura atómica
//!   (temporal + renombrado) y un evento en el diario por cada operación.
//! - **Partición por autor**: cada agente escribe solo en su región
//!   (`modelo-local/`, `claude-code/`); `compartida/` solo recibe notas por
//!   [`Boveda::promover`], que invocan las interfaces del usuario al aceptar
//!   una propuesta.
//! - **Recuperación indexada y acotada**: búsqueda FTS5 en la base de datos
//!   del ecosistema y lectura paginada, con [`Limites`]; nunca se expone el
//!   sistema de ficheros.
//! - **Contrato de cabecera** ([`cabecera`]).
//! - **Edición del usuario** (módulo `usuario`): las operaciones a mano del
//!   editor de `ea-web` (editar, crear en `compartida/`, renombrar con los
//!   enlaces al día, papelera). Ningún agente llega a ellas.

pub mod cabecera;
pub mod enlaces;
pub mod error;
pub mod limites;
pub mod ruta;

mod boveda;
mod usuario;

pub use boveda::{Boveda, NotaEscrita, NotaLeida, NuevaNota, PlanPromocion, ResumenSincronizacion, consulta_fts, huella};
pub use cabecera::Cabecera;
pub use error::{ErrorBoveda, Resultado};
pub use limites::Limites;
pub use usuario::{Arbol, CarpetaEnPapelera, CarpetaRenombrada, EnPapelera, NotaCompleta, Renombrada, ResumenPapelera};
