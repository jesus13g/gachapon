//! Ajustes del ecosistema: un único `ajustes.toml`
//! (`~/.config/ecosistema-asistente/ajustes.toml`) con los TOML antiguos
//! (`orquestador.toml`, `orchestrator.toml`, `boveda.toml`) como fuentes
//! heredadas de solo lectura. Precedencia por clave: entorno >
//! `ajustes.toml` > heredado > valor por defecto.
//!
//! Está en la base del grafo (de los crates `ea-*` solo depende de
//! `ea-plataforma`). Quién
//! puede cambiar qué lo dice el registro de claves (`claves`): lo que toca la
//! seguridad del ejecutor solo se cambia desde una terminal real, y los
//! topes solo se endurecen desde la web. Qué agente ocupa cada rol y con qué
//! perfil de modelo también vive aquí (spec 017), con el catálogo cerrado de
//! agentes (`agentes`). Ni ea-mcp ni el orquestador tienen
//! forma de escribir aquí.

mod agentes;
mod cargar;
pub mod descargas;
pub mod claves;
mod ejecucion;
mod equipo;
mod efectos;
mod error;
mod escribir;
mod fuentes;
mod validar;
mod vista;

pub use agentes::{buscar_binario, ficha, id_perfil_valido, por_cliente_mcp, FichaAgente, Rol, VistaAgente, CATALOGO, IDS_AGENTES};
pub use cargar::{cargar, cargar_de, Efectivos, Origen};
pub use claves::{clave, concretas, sin_alias, titulo, Aplica, Canal, Clave, Edicion, Tipo, ALIAS, CLAVES, SECCIONES};
pub use equipo::{comprobar_disponibles, parse_miembro, repartir_todos, validar_equipo, Miembro, RolEquipo};
pub use ejecucion::{
    clave_web_valida, dir_ejecucion, fichero_clave_web, fichero_presencia_escritorio, DIR_ESCRITORIO, DIR_WEB, FICHERO_CLAVE,
    FICHERO_PRESENCIA,
};
pub use efectos::{aplicar_al_iniciar, aplicar_efectos, binario_escritorio, CLAVE_AL_INICIAR};
pub use error::ErrorAjustes;
pub use escribir::{escribir_atomico, establecer, huella, interpretar, Aplicado};
pub use fuentes::{
    Agente, Ajustes, AjustesBiblioteca, ConfigAgente, Entorno, Escritorio, Fuentes, Orquestador, PerfilModelo, Roles, Red, Rutas, Skills, TechoTopes, Web, ENV_CONFIG, TECHO_DURO,
};
pub use validar::{expandir_tilde, validar_base_url, validar_perfil, validar_roles, validar_url_remota};
pub use vista::{Cliente, FilaAjuste, Heredado, Resumen, SeccionAjustes};
