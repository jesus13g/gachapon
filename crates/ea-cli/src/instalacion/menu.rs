//! La entrada de la app de escritorio en el menú del sistema (spec 023):
//! un `.desktop` en Linux, `~/Applications/gachapon.app` en macOS y un
//! acceso directo del menú Inicio en Windows.

use std::path::{Path, PathBuf};

use ea_acciones::servicios::Paso;
use ea_plataforma::Sistema;

use super::origen::{BINARIO_ESCRITORIO, nombre_binario};
use super::recursos::ICONO_PNG;
use super::ruta_usuario::literal_ps;
use super::{Contexto, Origen};

/// `~/.local/share/applications/ea-escritorio.desktop`.
pub fn ruta_desktop(home: &Path) -> PathBuf {
    home.join(".local/share/applications/ea-escritorio.desktop")
}

/// `~/Applications/gachapon.app`.
pub fn ruta_app(home: &Path) -> PathBuf {
    home.join("Applications").join("gachapon.app")
}

/// El acceso directo del menú Inicio del usuario.
pub fn ruta_lnk(home: &Path) -> PathBuf {
    home.join("AppData").join("Roaming").join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("gachapon.lnk")
}

/// Un valor de `Exec=` de un `.desktop`: entre comillas dobles, con `"`,
/// `` ` ``, `$` y `\` escapados (especificación de Desktop Entry).
fn exec_desktop(ruta: &Path) -> String {
    let mut s = String::from("\"");
    for c in ruta.display().to_string().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    // El `%` es un código de campo: se dobla.
    s.replace('%', "%%")
}

pub fn contenido_desktop(binario: &Path, icono: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=gachapon\nComment=Panel del ecosistema de agentes\n\
         Exec={}\nIcon={}\nTerminal=false\nCategories=Development;Utility;\nStartupWMClass=ea-escritorio\n",
        exec_desktop(binario),
        icono.display()
    )
}

/// El `Info.plist` mínimo del `.app`.
pub fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>gachapon</string>
  <key>CFBundleDisplayName</key><string>gachapon</string>
  <key>CFBundleIdentifier</key><string>es.gachapon.ea-escritorio</string>
  <key>CFBundleExecutable</key><string>{BINARIO_ESCRITORIO}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{}</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
        env!("CARGO_PKG_VERSION")
    )
}

/// Guion de PowerShell que crea el acceso directo.
pub fn guion_lnk(lnk: &Path, binario: &Path, carpeta: &Path) -> String {
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.Description = 'gachapon'; $s.Save()",
        literal_ps(&lnk.display().to_string()),
        literal_ps(&binario.display().to_string()),
        literal_ps(&carpeta.display().to_string()),
    )
}

/// Los pasos para poner la app en el menú. Vacío sin app de escritorio.
pub fn plan_menu(ctx: &Contexto) -> Vec<Paso> {
    if !ctx.con_escritorio {
        return vec![];
    }
    let binario = ctx.destinos.bin.join(nombre_binario(ctx.sistema, BINARIO_ESCRITORIO));
    match ctx.sistema {
        Sistema::Windows => {
            let lnk = ruta_lnk(&ctx.home);
            let carpeta = lnk.parent().map(Path::to_path_buf).unwrap_or_default();
            vec![
                Paso::CrearDir(carpeta),
                Paso::Ejecutar {
                    argv: vec![
                        "powershell".into(),
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        guion_lnk(&lnk, &binario, &ctx.destinos.bin),
                    ],
                    tolerar: false,
                    reintentos: 0,
                },
            ]
        }
        Sistema::MacOs => {
            let app = ruta_app(&ctx.home);
            let destino = app.join("Contents/MacOS").join(BINARIO_ESCRITORIO);
            // Desde un clon, el binario del `.app` sigue al que se compila.
            let binario_app = match ctx.origen {
                Origen::Clon { .. } => Paso::Enlazar { origen: binario, destino },
                Origen::Paquete => Paso::Copiar { origen: binario, destino },
            };
            vec![Paso::Escribir { ruta: app.join("Contents/Info.plist"), contenido: info_plist() }, binario_app]
        }
        _ => {
            let icono = ctx.destinos.datos.join("icono.png");
            vec![
                Paso::EscribirBytes { ruta: icono.clone(), contenido: ICONO_PNG.to_vec() },
                Paso::Escribir { ruta: ruta_desktop(&ctx.home), contenido: contenido_desktop(&binario, &icono) },
            ]
        }
    }
}

/// Los pasos para quitarla.
pub fn plan_quitar_menu(ctx: &Contexto) -> Vec<Paso> {
    match ctx.sistema {
        Sistema::Windows => vec![Paso::Borrar(ruta_lnk(&ctx.home))],
        Sistema::MacOs => {
            let app = ruta_app(&ctx.home);
            // Solo carpetas vacías: si el usuario metió algo, se queda.
            let rmdir = Paso::QuitarDirVacio;
            vec![
                Paso::Borrar(app.join("Contents/MacOS").join(BINARIO_ESCRITORIO)),
                Paso::Borrar(app.join("Contents/Info.plist")),
                rmdir(app.join("Contents/MacOS")),
                rmdir(app.join("Contents")),
                rmdir(app),
            ]
        }
        _ => vec![Paso::Borrar(ruta_desktop(&ctx.home)), Paso::Borrar(ctx.destinos.datos.join("icono.png"))],
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn el_desktop_cita_la_ruta() {
        let c = contenido_desktop(Path::new("/home/a b/$x/ea-escritorio"), Path::new("/i.png"));
        assert!(c.contains("Exec=\"/home/a b/\\$x/ea-escritorio\"\n"));
        assert!(c.contains("StartupWMClass=ea-escritorio"));
        assert!(c.contains("Icon=/i.png"));
    }

    #[test]
    fn el_plist_nombra_el_binario() {
        let p = info_plist();
        assert!(p.contains("<string>ea-escritorio</string>"));
        assert!(p.contains("es.gachapon.ea-escritorio"));
    }

    #[test]
    fn el_lnk_va_citado() {
        let g = guion_lnk(Path::new("C:\\M'nu\\g.lnk"), Path::new("C:\\b\\ea-escritorio.exe"), Path::new("C:\\b"));
        assert!(g.contains("CreateShortcut('C:\\M''nu\\g.lnk')"));
        assert!(g.contains("TargetPath = 'C:\\b\\ea-escritorio.exe'"));
    }
}
