//! Ajustes del ecosistema: un único `ajustes.toml`
//! (`~/.config/ecosistema-asistente/ajustes.toml`) con los TOML antiguos
//! (`orquestador.toml`, `orchestrator.toml`, `boveda.toml`) como fuentes
//! heredadas de solo lectura. Precedencia por clave: entorno >
//! `ajustes.toml` > heredado > valor por defecto.
//!
//! Está en la base del grafo (no depende de ningún otro crate `ea-*`). Quién
//! puede cambiar qué lo dice el registro de claves (`claves`): lo que toca la
//! seguridad del agente local solo se cambia desde una terminal real, y los
//! topes solo se endurecen desde la web. Ni ea-mcp ni el orquestador tienen
//! forma de escribir aquí.

mod cargar;
pub mod claves;
mod error;
mod escribir;
mod fuentes;
mod validar;
mod vista;

pub use cargar::{cargar, cargar_de, Efectivos, Origen};
pub use claves::{clave, Aplica, Canal, Clave, Edicion, Tipo, CLAVES, SECCIONES};
pub use error::ErrorAjustes;
pub use escribir::{escribir_atomico, establecer, huella, interpretar, Aplicado};
pub use fuentes::{
    Agente, Ajustes, Entorno, Fuentes, Modelo, Orquestador, Red, Rutas, Skills, TechoTopes, Web, ENV_CONFIG, TECHO_DURO,
};
pub use validar::{expandir_tilde, validar_base_url};
pub use vista::{Cliente, FilaAjuste, Heredado, Resumen, SeccionAjustes};
