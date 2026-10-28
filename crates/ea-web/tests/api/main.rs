//! La API JSON del panel (`ea-web`), un módulo por área. Es un solo binario
//! de pruebas: lo común (montar el panel sobre un estado temporal y hacer las
//! peticiones) está en `comun`.

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

mod comun;

mod acciones;
mod ajustes;
mod biblioteca;
mod boveda;
mod contrato_escritorio;
mod diario;
mod ecosistema;
mod equipos;
mod guardia;
mod memoria;
mod rutinas;
mod terminal;
mod trabajos;
