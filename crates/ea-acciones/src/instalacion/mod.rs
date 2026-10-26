//! `ea instalar` y `ea desinstalar` (spec 023): la instalación en un paso.
//!
//! Aquí solo se **planifica**: cada función devuelve los [`Paso`]s que se
//! enseñan al usuario y que `servicios::aplicar` ejecuta tras su
//! confirmación en una terminal real (lo hace `ea-cli`). Lo que se mira del
//! sistema pasa por una [`Sonda`], para que las pruebas no dependan de la
//! máquina.

use std::path::{Path, PathBuf};

use ea_plataforma::Sistema;

use crate::servicios::Paso;

pub mod agentes;
pub mod dependencias;
pub mod menu;
pub mod origen;
pub mod recursos;
pub mod ruta_usuario;
pub mod wsl;

/// De dónde salen los binarios del `ea` que corre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origen {
    /// Un paquete descargado (Release): se copian a una carpeta estable.
    Paquete,
    /// `target/release` (o `target/debug`) de un clon del repo: se quedan
    /// donde están (modo desarrollo).
    Clon { repo: PathBuf },
}

/// Adónde va cada cosa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destinos {
    /// Los binarios (con origen clon, `target/release`).
    pub bin: PathBuf,
    /// Las guías `agentes/flujo/*.md` (con origen clon, las del repo).
    pub flujo: PathBuf,
    /// Base de lo demás que se instala (icono de la app…).
    pub datos: PathBuf,
}

/// Todo lo que un plan necesita saber.
#[derive(Debug, Clone)]
pub struct Contexto {
    pub sistema: Sistema,
    pub origen: Origen,
    /// Carpeta del `ea` que corre (de ahí se copian los binarios).
    pub exes_origen: PathBuf,
    pub destinos: Destinos,
    pub home: PathBuf,
    /// `agente.skills.dir` efectivo.
    pub skills_dir: PathBuf,
    /// `red.puerto_mcp` efectivo.
    pub puerto_mcp: u16,
    /// `false` con `--sin-escritorio` o dentro de WSL.
    pub con_escritorio: bool,
}

/// Lo que los planes miran del sistema. En producción, [`SondaReal`]; en
/// las pruebas, una falsa.
pub trait Sonda {
    /// La ruta de un programa del `PATH`.
    fn binario(&self, nombre: &str) -> Option<PathBuf>;
    fn existe(&self, ruta: &Path) -> bool;
    /// El contenido de un fichero de texto, si se puede leer.
    fn leer(&self, ruta: &Path) -> Option<String>;
    /// La salida estándar de una orden que termina con 0.
    fn salida(&self, argv: &[&str]) -> Option<String>;
    /// ¿Responde con 2xx un GET a esta URL local (tope corto)?
    fn http_ok(&self, url: &str) -> bool;
}

/// La sonda de verdad.
pub struct SondaReal;

impl Sonda for SondaReal {
    fn binario(&self, nombre: &str) -> Option<PathBuf> {
        ea_plataforma::rutas::buscar_en_path(nombre)
    }

    fn existe(&self, ruta: &Path) -> bool {
        // Un enlace roto también «existe»: hay algo que sustituir.
        std::fs::symlink_metadata(ruta).is_ok()
    }

    fn leer(&self, ruta: &Path) -> Option<String> {
        std::fs::read_to_string(ruta).ok()
    }

    fn salida(&self, argv: &[&str]) -> Option<String> {
        let (prog, args) = argv.split_first()?;
        let s = std::process::Command::new(prog).args(args).stdin(std::process::Stdio::null()).output().ok()?;
        s.status.success().then(|| String::from_utf8_lossy(&s.stdout).into_owned())
    }

    fn http_ok(&self, url: &str) -> bool {
        http_ok(url, std::time::Duration::from_secs(2))
    }
}

/// GET mínimo a `http://127.0.0.1:<puerto>/<ruta>` sin dependencias: ¿la
/// respuesta empieza por `HTTP/1.x 2`? Solo para URLs locales en claro.
pub fn http_ok(url: &str, tope: std::time::Duration) -> bool {
    use std::io::{Read, Write};
    let Some(resto) = url.strip_prefix("http://") else { return false };
    let (hostport, ruta) = resto.split_once('/').map(|(h, r)| (h, format!("/{r}"))).unwrap_or((resto, "/".into()));
    let Ok(mut dirs) = std::net::ToSocketAddrs::to_socket_addrs(hostport) else { return false };
    let Some(dir) = dirs.next() else { return false };
    let Ok(mut c) = std::net::TcpStream::connect_timeout(&dir, tope) else { return false };
    let _ = c.set_read_timeout(Some(tope));
    let _ = c.set_write_timeout(Some(tope));
    let host = hostport.split(':').next().unwrap_or("localhost");
    if write!(c, "GET {ruta} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 16];
    let n = c.read(&mut buf).unwrap_or(0);
    let cabeza = String::from_utf8_lossy(&buf[..n]);
    cabeza.starts_with("HTTP/1.1 2") || cabeza.starts_with("HTTP/1.0 2")
}

/// El plan completo de `ea instalar` (sin la vía WSL ni las ofertas de
/// dependencias, que van aparte). Orden en el plan de la spec 023.
pub fn plan_instalar_todo(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let _ = (ctx, sonda);
    Err("sin implementar (spec 023, T-002)".into())
}

/// El plan de `ea desinstalar`: lo inverso, sin tocar datos.
pub fn plan_desinstalar_todo(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let _ = (ctx, sonda);
    Err("sin implementar (spec 023, T-002)".into())
}
