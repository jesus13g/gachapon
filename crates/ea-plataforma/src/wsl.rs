//! WSL2 de Windows (spec 023): lo que comparten el instalador, los ajustes y
//! la app de escritorio. Puro: se prueba en cualquier sistema.

/// ¿Es un nombre de distribución de WSL razonable (`[A-Za-z0-9._-]{1,64}`)?
/// Basta para pasarlo como argumento a `wsl.exe` sin comillas ni sorpresas.
pub fn nombre_distro_valido(t: &str) -> bool {
    !t.is_empty() && t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// La salida de `wsl.exe` como texto. `wsl.exe` escribe en UTF-16LE: leída
/// como UTF-8 con pérdida quedan NUL (y a veces un BOM) entre los
/// caracteres, que aquí se quitan.
pub fn limpiar_salida(salida: &[u8]) -> String {
    String::from_utf8_lossy(salida).chars().filter(|c| *c != '\0' && *c != '\u{feff}').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Como la escribe `wsl.exe` (UTF-16LE) leída como UTF-8: un NUL tras
    /// cada carácter ASCII y un BOM delante.
    fn utf16(t: &str) -> Vec<u8> {
        let mut s = String::from('\u{feff}');
        for c in t.chars() {
            s.push(c);
            s.push('\0');
        }
        s.into_bytes()
    }

    #[test]
    fn nombres_de_distro() {
        assert!(nombre_distro_valido("Ubuntu"));
        assert!(nombre_distro_valido("Ubuntu-24.04"));
        assert!(nombre_distro_valido("kali_linux"));
        assert!(!nombre_distro_valido(""));
        assert!(!nombre_distro_valido("a b"));
        assert!(!nombre_distro_valido("x';calc;'"));
        assert!(!nombre_distro_valido("x\";calc"));
        assert!(!nombre_distro_valido(&"a".repeat(65)));
        assert!(nombre_distro_valido(&"a".repeat(64)));
    }

    #[test]
    fn limpia_la_salida_utf16_de_wsl() {
        assert_eq!(limpiar_salida(&utf16("si")), "si");
        assert_eq!(limpiar_salida(&utf16("Ubuntu\r\nkali-linux\r\n")), "Ubuntu\r\nkali-linux\r\n");
        // Lo que ya es texto pasa igual.
        assert_eq!(limpiar_salida(b"1000\n"), "1000\n");
    }
}
