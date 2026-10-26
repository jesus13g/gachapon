//! De dónde salen los binarios y adónde van (spec 023, D-2): desde un
//! paquete descargado se copian a una carpeta estable del usuario; desde
//! `target/release` de un clon del repo se quedan donde están.

use std::path::{Path, PathBuf};

use ea_plataforma::Sistema;

use super::{Destinos, Origen};

/// Los binarios que lleva un paquete, sin extensión. `ea-escritorio` es
/// opcional: el paquete puede no traerlo (p. ej. dentro de WSL).
pub const BINARIOS: [&str; 4] = ["ea", "ea-web", "ea-mcp", "ea-orchestrator"];
pub const BINARIO_ESCRITORIO: &str = "ea-escritorio";

/// El nombre real de un binario en `sistema` (`ea-web.exe` en Windows). Va
/// por el sistema del plan, no por el de compilación, para que los planes
/// de los tres sistemas se prueben en cualquiera.
pub fn nombre_binario(sistema: Sistema, base: &str) -> String {
    if sistema == Sistema::Windows { format!("{base}.exe") } else { base.to_string() }
}

/// `Clon` si `exes` es `target/release` (o `target/debug`) de un clon del
/// repo: su `Cargo.toml` declara el workspace y existe `crates/ea-cli`.
pub fn origen_de(exes: &Path) -> Origen {
    let es_target = exes.file_name().is_some_and(|n| n == "release" || n == "debug")
        && exes.parent().and_then(Path::file_name).is_some_and(|n| n == "target");
    if !es_target {
        return Origen::Paquete;
    }
    let Some(repo) = exes.parent().and_then(Path::parent) else { return Origen::Paquete };
    let workspace = std::fs::read_to_string(repo.join("Cargo.toml")).is_ok_and(|c| c.contains("[workspace]"));
    if workspace && repo.join("crates/ea-cli").is_dir() {
        Origen::Clon { repo: repo.to_path_buf() }
    } else {
        Origen::Paquete
    }
}

/// La carpeta base de lo instalado desde un paquete: Linux
/// `$XDG_DATA_HOME/ecosistema-asistente` (o `~/.local/share/…`), macOS
/// `~/Library/Application Support/ecosistema-asistente`, Windows
/// `%LOCALAPPDATA%\Programs\gachapon`.
fn base(sistema: Sistema, home: &Path, var: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let absoluta = |n: &str| var(n).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute() || sistema == Sistema::Windows);
    match sistema {
        Sistema::MacOs => home.join("Library/Application Support/ecosistema-asistente"),
        Sistema::Windows => {
            absoluta("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local")).join("Programs").join("gachapon")
        }
        _ => absoluta("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join("ecosistema-asistente"),
    }
}

/// Adónde va cada cosa. Con `Paquete`, los binarios a `<base>/bin` (en
/// Windows, a `<base>` mismo); con `Clon`, se quedan en `target/release` y
/// las guías son las del repo.
pub fn destinos(sistema: Sistema, origen: &Origen, home: &Path, var: impl Fn(&str) -> Option<String>) -> Destinos {
    let base = base(sistema, home, &var);
    let bin_paquete = if sistema == Sistema::Windows { base.clone() } else { base.join("bin") };
    match origen {
        Origen::Paquete => Destinos { bin: bin_paquete, flujo: base.join("flujo"), datos: base },
        Origen::Clon { repo } => Destinos {
            bin: repo.join("target").join("release"),
            flujo: repo.join("agentes").join("flujo"),
            datos: base,
        },
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn sin_var(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn un_target_release_de_un_clon_es_clon() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::create_dir_all(d.path().join("crates/ea-cli")).unwrap();
        let exes = d.path().join("target/release");
        std::fs::create_dir_all(&exes).unwrap();
        assert_eq!(origen_de(&exes), Origen::Clon { repo: d.path().to_path_buf() });
        assert_eq!(origen_de(&d.path().join("target/debug")), Origen::Clon { repo: d.path().to_path_buf() });
    }

    #[test]
    fn lo_demas_es_paquete() {
        let d = tempfile::tempdir().unwrap();
        // Una carpeta cualquiera.
        assert_eq!(origen_de(d.path()), Origen::Paquete);
        // Un target/release sin workspace (otro proyecto de Rust).
        std::fs::write(d.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(origen_de(&d.path().join("target/release")), Origen::Paquete);
        // Con workspace pero sin crates/ea-cli.
        std::fs::write(d.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(origen_de(&d.path().join("target/release")), Origen::Paquete);
    }

    #[test]
    fn destinos_de_un_paquete_por_sistema() {
        let home = Path::new("/home/ana");
        let l = destinos(Sistema::Linux, &Origen::Paquete, home, sin_var);
        assert_eq!(l.bin.to_string_lossy().replace('\\', "/"), "/home/ana/.local/share/ecosistema-asistente/bin");
        assert_eq!(l.flujo.to_string_lossy().replace('\\', "/"), "/home/ana/.local/share/ecosistema-asistente/flujo");
        assert_eq!(l.datos.to_string_lossy().replace('\\', "/"), "/home/ana/.local/share/ecosistema-asistente");

        let x = destinos(Sistema::Linux, &Origen::Paquete, home, |n| (n == "XDG_DATA_HOME").then(|| "/datos".to_string()));
        assert_eq!(x.bin.to_string_lossy().replace('\\', "/"), "/datos/ecosistema-asistente/bin");
        // Una XDG_DATA_HOME relativa no vale (la especificación XDG la ignora).
        let r = destinos(Sistema::Linux, &Origen::Paquete, home, |n| (n == "XDG_DATA_HOME").then(|| "datos".to_string()));
        assert_eq!(r.bin, l.bin);

        let m = destinos(Sistema::MacOs, &Origen::Paquete, Path::new("/Users/ana"), sin_var);
        assert_eq!(m.bin.to_string_lossy().replace('\\', "/"), "/Users/ana/Library/Application Support/ecosistema-asistente/bin");

        let w = destinos(Sistema::Windows, &Origen::Paquete, Path::new("C:/Users/ana"), |n| {
            (n == "LOCALAPPDATA").then(|| "C:/Users/ana/AppData/Local".to_string())
        });
        assert_eq!(w.bin, PathBuf::from("C:/Users/ana/AppData/Local").join("Programs").join("gachapon"));
        assert_eq!(w.datos, w.bin);
        assert_eq!(w.flujo, w.bin.join("flujo"));
    }

    #[test]
    fn destinos_de_un_clon_se_quedan_en_el_repo() {
        let repo = PathBuf::from("/src/gachapon");
        let d = destinos(Sistema::Linux, &Origen::Clon { repo: repo.clone() }, Path::new("/home/ana"), sin_var);
        assert_eq!(d.bin, repo.join("target/release"));
        assert_eq!(d.flujo, repo.join("agentes/flujo"));
        assert_eq!(d.datos.to_string_lossy().replace('\\', "/"), "/home/ana/.local/share/ecosistema-asistente");
    }

    #[test]
    fn los_nombres_llevan_exe_en_windows() {
        assert_eq!(nombre_binario(Sistema::Windows, "ea-web"), "ea-web.exe");
        assert_eq!(nombre_binario(Sistema::Linux, "ea-web"), "ea-web");
    }
}
