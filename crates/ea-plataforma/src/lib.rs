//! Las diferencias entre Linux, macOS y Windows, concentradas en un solo
//! crate. Ningún otro crate del workspace debería usar `libc`, `windows-sys`,
//! `std::os::unix` ni `cfg(unix)`/`cfg(windows)`: lo pide aquí.
//!
//! No depende de ningún `ea-*`. Las funciones no escriben mensajes de
//! usuario salvo en los errores; quien llama los traduce a su dominio.

pub mod escritorio;
pub mod fs;
pub mod proceso;
pub mod reloj;
pub mod rutas;
pub mod shell;
pub mod usuario;
pub mod wsl;

#[cfg(feature = "tokio")]
pub mod parada;

/// El sistema operativo en el que corre el binario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sistema {
    Linux,
    MacOs,
    Windows,
    Otro,
}

impl Sistema {
    /// El sistema para el que se compiló este binario.
    pub const fn actual() -> Sistema {
        if cfg!(target_os = "linux") {
            Sistema::Linux
        } else if cfg!(target_os = "macos") {
            Sistema::MacOs
        } else if cfg!(windows) {
            Sistema::Windows
        } else {
            Sistema::Otro
        }
    }

    /// Nombre legible, para mensajes.
    pub const fn nombre(self) -> &'static str {
        match self {
            Sistema::Linux => "Linux",
            Sistema::MacOs => "macOS",
            Sistema::Windows => "Windows",
            Sistema::Otro => "otro sistema",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_sistema_actual_tiene_nombre() {
        let s = Sistema::actual();
        assert_ne!(s, Sistema::Otro);
        assert!(!s.nombre().is_empty());
    }
}
