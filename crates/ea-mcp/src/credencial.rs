//! Credencial del planificador (U-02): 256 bits aleatorios en
//! `<ejecucion>/ea-mcp/credencial` (directorio 0700, fichero 0600). El cliente
//! MCP del planificador la manda en `X-EA-Credencial` con cada petición; en
//! Claude Code la saca `ea mcp cabeceras` (su `headersHelper`), que corre
//! fuera del sandbox y la lee en cada conexión.
//!
//! Lo que protege: que el agente local, que corre en el sandbox con el
//! directorio de ejecución tapado (`bwrap` y Seatbelt, también el criterio),
//! se presente como el planificador con solo declarar su nombre. Lo que no:
//! un proceso sin sandbox de la misma cuenta lee el fichero igual que el
//! `headersHelper`.
//!
//! La credencial no sale nunca en un mensaje, en el diario ni en los
//! registros: `Debug` no la enseña.

use std::fs;
use std::path::{Path, PathBuf};

use ea_plataforma::fs::{Permisos, crear_dir_privado, es_privado, escribir_atomico};

const BYTES: usize = 32;

pub struct Credencial(String);

impl std::fmt::Debug for Credencial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credencial(…)")
    }
}

impl Credencial {
    /// Credencial nueva del generador aleatorio del sistema operativo, en
    /// hexadecimal (64 caracteres: `ea_ajustes::credencial_mcp_valida`).
    pub fn nueva() -> std::io::Result<Self> {
        let mut b = [0u8; BYTES];
        getrandom::getrandom(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Self(b.iter().map(|x| format!("{x:02x}")).collect()))
    }

    /// Una credencial dada (pruebas); `None` si no está bien formada.
    pub fn de_texto(texto: &str) -> Option<Self> {
        ea_ajustes::credencial_mcp_valida(texto).then(|| Self(texto.to_string()))
    }

    /// Comparación en tiempo constante respecto al contenido (la longitud es
    /// pública: siempre 64).
    pub fn coincide(&self, otra: &[u8]) -> bool {
        let mia = self.0.as_bytes();
        if mia.len() != otra.len() {
            return false;
        }
        mia.iter().zip(otra).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// La credencial de `<ejecucion>/ea-mcp/credencial`: la que haya si está bien
/// formada y es privada (así sobrevive a reiniciar `ea-mcp` y un cliente que
/// la leyó al arrancar sigue valiendo); si no, una nueva, escrita de forma
/// atómica en un fichero que nace 0600 dentro de un directorio 0700. El
/// directorio de ejecución se vacía al cerrar la sesión: entonces se crea
/// otra. Devuelve la credencial y la ruta del fichero.
pub fn leer_o_crear(ejecucion: &Path) -> std::io::Result<(Credencial, PathBuf)> {
    crear_dir_privado(&ejecucion.join(ea_ajustes::DIR_MCP))?;
    let ruta = ea_ajustes::fichero_credencial_mcp(ejecucion);
    if es_privado(&ruta).unwrap_or(false)
        && let Some(c) = fs::read_to_string(&ruta).ok().and_then(|t| Credencial::de_texto(&t))
    {
        return Ok((c, ruta));
    }
    let c = Credencial::nueva()?;
    escribir_atomico(&ruta, c.0.as_bytes(), Permisos::Privados)?;
    Ok((c, ruta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn es_aleatoria_se_compara_entera_y_no_se_ensena() {
        let a = Credencial::nueva().unwrap();
        let b = Credencial::nueva().unwrap();
        assert!(ea_ajustes::credencial_mcp_valida(&a.0));
        assert_ne!(a.0, b.0);
        assert!(a.coincide(a.0.as_bytes()));
        assert!(!a.coincide(b.0.as_bytes()));
        assert!(!a.coincide(&a.0.as_bytes()[..63]));
        assert!(!a.coincide(b""));
        assert!(!format!("{a:?}").contains(&a.0));
        assert!(Credencial::de_texto("corta").is_none());
    }

    #[test]
    fn se_reutiliza_si_es_valida_y_privada_y_si_no_se_rehace() {
        let d = tempfile::tempdir().unwrap();
        let (c, ruta) = leer_o_crear(d.path()).unwrap();
        assert_eq!(ruta, d.path().join("ea-mcp/credencial"));
        assert_eq!(fs::read_to_string(&ruta).unwrap(), c.0);
        #[cfg(unix)]
        {
            let modo = |p: &Path| ea_plataforma::fs::permisos_unix(&fs::metadata(p).unwrap()).unwrap() & 0o777;
            assert_eq!(modo(&ruta), 0o600);
            assert_eq!(modo(&d.path().join("ea-mcp")), 0o700);
        }
        // Reiniciar `ea-mcp` no la cambia.
        let (otra, _) = leer_o_crear(d.path()).unwrap();
        assert_eq!(otra.0, c.0);
        // Una mal formada se sustituye.
        fs::write(&ruta, "no-vale").unwrap();
        let (nueva, _) = leer_o_crear(d.path()).unwrap();
        assert!(ea_ajustes::credencial_mcp_valida(&nueva.0));
        assert_ne!(nueva.0, c.0);
        // Una legible por otros también: pudo leerla cualquiera.
        #[cfg(unix)]
        {
            ea_plataforma::fs::poner_permisos_unix(&ruta, 0o644).unwrap();
            let (rehecha, _) = leer_o_crear(d.path()).unwrap();
            assert_ne!(rehecha.0, nueva.0);
            assert!(ea_plataforma::fs::es_privado(&ruta).unwrap());
        }
        assert_eq!(fs::read_dir(d.path().join("ea-mcp")).unwrap().count(), 1, "sin temporales");
    }
}
