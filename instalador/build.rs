//! Mete dentro del instalador los binarios de la carpeta que da la variable
//! `GACHAPON_BINARIOS` al compilar (la usa el workflow de release). Sin la
//! variable compila igual, sin binarios: el `.exe` resultante lo dice al
//! ejecutarse (sirve para la CI y para probar).

use std::path::PathBuf;

/// Los binarios que lleva el instalador, en este orden.
const BINARIOS: [&str; 5] = ["ea.exe", "ea-web.exe", "ea-mcp.exe", "ea-orchestrator.exe", "ea-escritorio.exe"];

fn main() {
    println!("cargo:rerun-if-env-changed=GACHAPON_BINARIOS");
    let salida = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo da OUT_DIR")).join("binarios.rs");
    let codigo = match std::env::var_os("GACHAPON_BINARIOS").filter(|v| !v.is_empty()) {
        None => "pub const BINARIOS: &[(&str, &[u8])] = &[];\n".to_string(),
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let mut filas = Vec::new();
            for b in BINARIOS {
                let ruta = dir.join(b);
                if !ruta.is_file() {
                    panic!("GACHAPON_BINARIOS={}: falta {b}; el instalador necesita los cinco binarios ({})", dir.display(), BINARIOS.join(", "));
                }
                println!("cargo:rerun-if-changed={}", ruta.display());
                let absoluta = ruta.canonicalize().unwrap_or(ruta);
                filas.push(format!("    ({b:?}, include_bytes!({:?})),\n", absoluta.display().to_string()));
            }
            format!("pub const BINARIOS: &[(&str, &[u8])] = &[\n{}];\n", filas.concat())
        }
    };
    std::fs::write(&salida, codigo).expect("se puede escribir en OUT_DIR");
}
