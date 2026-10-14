//! Escritura atómica (mismo patrón que `ea-boveda`): temporal oculto en el
//! mismo directorio, `fsync` y renombrado.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::error::{ErrorBus, Resultado};

static CONTADOR: AtomicU64 = AtomicU64::new(0);

pub fn escribir_atomico(destino: &Path, contenido: &[u8]) -> Resultado<()> {
    let dir = destino.parent().ok_or_else(|| ErrorBus::Argumentos("ruta sin directorio".into()))?;
    fs::create_dir_all(dir)?;
    let nombre = destino.file_name().and_then(|n| n.to_str()).unwrap_or("fichero");
    let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
    let temporal = dir.join(format!(".{nombre}.{}.{n}.tmp", std::process::id()));
    let resultado = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&temporal)?;
        f.write_all(contenido)?;
        f.sync_all()?;
        fs::rename(&temporal, destino)
    })();
    if let Err(e) = resultado {
        let _ = fs::remove_file(&temporal);
        return Err(e.into());
    }
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

pub fn escribir_json<T: Serialize>(destino: &Path, valor: &T) -> Resultado<()> {
    escribir_atomico(destino, &serde_json::to_vec_pretty(valor)?)
}

/// FNV-1a de 64 bits.
pub fn fnv(texto: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in texto.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
