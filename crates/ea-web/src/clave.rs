//! Clave de sesión del panel: 256 bits aleatorios que `ea-web` genera al
//! arrancar y deja en `$XDG_RUNTIME_DIR/ea-web/clave` (directorio 0700,
//! fichero 0600). `ea-web-open` la pasa a la página en el fragmento de la
//! URL y la página la manda en `X-EA-Clave` con cada escritura.
//!
//! Lo que protege: que otro proceso local que no pueda leer ese fichero (el
//! agente local dentro de `bwrap`, donde el directorio está tapado) escriba
//! por la API. Lo que no: un proceso sin sandbox de la misma cuenta (Claude
//! Code, por ejemplo) lee el fichero igual que `ea-web-open`.

use std::fs;
use std::path::{Path, PathBuf};

use ea_plataforma::fs::{Permisos, crear_dir_privado, escribir_atomico};

/// Cabecera con la que la página manda la clave.
pub const CABECERA: &str = "x-ea-clave";

const BYTES: usize = 32;

pub struct ClaveSesion(String);

impl ClaveSesion {
    /// Clave nueva del generador aleatorio del sistema operativo, en
    /// hexadecimal (64 caracteres: `ea_ajustes::clave_web_valida`, lo que
    /// acepta la app de escritorio).
    pub fn nueva() -> std::io::Result<Self> {
        let mut b = [0u8; BYTES];
        getrandom::getrandom(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
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
/// 0700 en Unix (y lo deja en 0700 si ya existía) y la escribe de forma
/// atómica en un fichero que nace 0600. Devuelve la ruta del fichero.
pub fn guardar(ejecucion: &Path, clave: &ClaveSesion) -> std::io::Result<PathBuf> {
    crear_dir_privado(&ejecucion.join(ea_ajustes::DIR_WEB))?;
    let destino = ea_ajustes::fichero_clave_web(ejecucion);
    escribir_atomico(&destino, clave.texto().as_bytes(), Permisos::Privados)?;
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
        assert!(ea_ajustes::clave_web_valida(a.texto()), "{}", a.texto());
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
        ea_plataforma::fs::poner_permisos_unix(&d.path().join("ea-web"), 0o755).unwrap();
        let c = ClaveSesion::nueva().unwrap();
        let f = guardar(d.path(), &c).unwrap();
        assert_eq!(f, d.path().join("ea-web/clave"));
        assert_eq!(fs::read_to_string(&f).unwrap(), c.texto());
        #[cfg(unix)]
        {
            assert!(ea_plataforma::fs::es_privado(&f).unwrap());
            let modo = |p: &Path| ea_plataforma::fs::permisos_unix(&fs::metadata(p).unwrap()).unwrap() & 0o777;
            assert_eq!(modo(&f), 0o600);
            assert_eq!(modo(&d.path().join("ea-web")), 0o700);
        }
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
