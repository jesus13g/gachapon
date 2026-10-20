//! Clave de sesión del panel: 256 bits aleatorios que `ea-web` genera al
//! arrancar y deja en `$XDG_RUNTIME_DIR/ea-web/clave` (directorio 0700,
//! fichero 0600). `ea-web-open` la pasa a la página en el fragmento de la
//! URL y la página la manda en `X-EA-Clave` con cada escritura.
//!
//! Lo que protege: que otro proceso local que no pueda leer ese fichero (el
//! agente local dentro de `bwrap`, donde el directorio está tapado) escriba
//! por la API. Lo que no: un proceso sin sandbox de la misma cuenta (Claude
//! Code, por ejemplo) lee el fichero igual que `ea-web-open`.

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Cabecera con la que la página manda la clave.
pub const CABECERA: &str = "x-ea-clave";

const BYTES: usize = 32;

pub struct ClaveSesion(String);

impl ClaveSesion {
    /// Clave nueva de `/dev/urandom`, en hexadecimal (64 caracteres).
    pub fn nueva() -> std::io::Result<Self> {
        let mut b = [0u8; BYTES];
        fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
        Ok(Self(b.iter().map(|x| format!("{x:02x}")).collect()))
    }

    pub fn texto(&self) -> &str {
        &self.0
    }

    /// Comparación en tiempo constante (respecto al contenido; la longitud
    /// es pública: siempre 64).
    pub fn coincide(&self, otra: &[u8]) -> bool {
        let mia = self.0.as_bytes();
        if mia.len() != otra.len() {
            return false;
        }
        mia.iter().zip(otra).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// Escribe la clave en `<ejecucion>/ea-web/clave`: crea el directorio con
/// 0700 (y lo deja en 0700 si ya existía), escribe un temporal 0600, lo
/// sincroniza y lo renombra encima. Devuelve la ruta del fichero.
pub fn guardar(ejecucion: &Path, clave: &ClaveSesion) -> std::io::Result<PathBuf> {
    let dir = ejecucion.join(ea_ajustes::DIR_WEB);
    DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    fs::set_permissions(&dir, Permissions::from_mode(0o700))?;
    let destino = ea_ajustes::fichero_clave_web(ejecucion);
    let temporal = dir.join(format!(".{}.{}.tmp", ea_ajustes::FICHERO_CLAVE, std::process::id()));
    let _ = fs::remove_file(&temporal);
    let r = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporal)?;
        f.write_all(clave.texto().as_bytes())?;
        f.sync_all()?;
        fs::rename(&temporal, &destino)
    })();
    if let Err(e) = r {
        let _ = fs::remove_file(&temporal);
        return Err(e);
    }
    if let Ok(d) = fs::File::open(&dir) {
        let _ = d.sync_all();
    }
    Ok(destino)
}

/// Borra el fichero al parar, si sigue siendo el de esta clave (otro
/// `ea-web` arrancado después podría haberlo reemplazado).
pub fn retirar(fichero: &Path, clave: &ClaveSesion) {
    if fs::read(fichero).is_ok_and(|c| clave.coincide(&c)) {
        let _ = fs::remove_file(fichero);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clave_es_aleatoria_y_se_compara_entera() {
        let a = ClaveSesion::nueva().unwrap();
        let b = ClaveSesion::nueva().unwrap();
        assert_eq!(a.texto().len(), 64);
        assert!(a.texto().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.texto(), b.texto());
        assert!(a.coincide(a.texto().as_bytes()));
        assert!(!a.coincide(b.texto().as_bytes()));
        assert!(!a.coincide(&a.texto().as_bytes()[..63]));
        assert!(!a.coincide(b""));
    }

    #[test]
    fn se_guarda_con_permisos_privados_y_se_retira() {
        let d = tempfile::tempdir().unwrap();
        // Un directorio previo con permisos abiertos queda en 0700.
        fs::create_dir(d.path().join("ea-web")).unwrap();
        fs::set_permissions(d.path().join("ea-web"), Permissions::from_mode(0o755)).unwrap();
        let c = ClaveSesion::nueva().unwrap();
        let f = guardar(d.path(), &c).unwrap();
        assert_eq!(f, d.path().join("ea-web/clave"));
        assert_eq!(fs::read_to_string(&f).unwrap(), c.texto());
        assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(d.path().join("ea-web")).unwrap().permissions().mode() & 0o777, 0o700);
        // Reescribirla la reemplaza sin dejar temporales.
        let c2 = ClaveSesion::nueva().unwrap();
        guardar(d.path(), &c2).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), c2.texto());
        assert_eq!(fs::read_dir(d.path().join("ea-web")).unwrap().count(), 1);
        // Solo se retira si es la suya.
        retirar(&f, &c);
        assert!(f.exists());
        retirar(&f, &c2);
        assert!(!f.exists());
    }
}
