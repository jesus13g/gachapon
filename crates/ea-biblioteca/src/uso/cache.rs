//! Caché incremental del uso (RF-006): `<estado>/biblioteca/cache-uso.json`.
//!
//! Es un dato derivado: se puede borrar sin perder nada. Por cada fuente (un
//! fichero de transcripción, o la base de OpenCode) guarda tamaño, `mtime`,
//! cuánto se leyó y los recuentos parciales. Solo contiene tipo, nombre de
//! herramienta, número de usos y fecha: nunca texto de los mensajes.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Estadisticas, Hallazgo};
use crate::modelo::Tipo;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reg {
    pub tipo: Tipo,
    pub nombre: String,
    pub usos: u64,
    pub ultimo_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entrada {
    pub tam: u64,
    pub mtime_ns: u64,
    /// Bytes ya leídos (siempre en un salto de línea). En fuentes que no son
    /// ficheros de líneas, la marca de tiempo del último registro visto.
    pub offset: u64,
    pub lineas: u64,
    pub regs: Vec<Reg>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub fuentes: BTreeMap<String, Entrada>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache { version: VERSION, fuentes: BTreeMap::new() }
    }
}

impl Cache {
    /// Lee la caché; una caché ausente, ilegible o de otra versión es vacía.
    pub fn cargar(ruta: &Path) -> Cache {
        std::fs::read(ruta)
            .ok()
            .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
            .filter(|c| c.version == VERSION)
            .unwrap_or_default()
    }

    pub fn guardar(&self, ruta: &Path, avisos: &mut Vec<String>) {
        let Some(dir) = ruta.parent() else { return };
        let bytes = match serde_json::to_vec(self) {
            Ok(b) => b,
            Err(_) => return,
        };
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| ea_ajustes::escribir_atomico(ruta, &bytes)) {
            avisos.push(format!("no se pudo guardar la caché de uso ({}): {e}", ruta.display()));
        }
    }

    /// Quita las fuentes que ya no existen.
    pub fn retener(&mut self, vivas: &std::collections::HashSet<String>) {
        self.fuentes.retain(|k, _| vivas.contains(k));
    }
}

/// Suma hallazgos a los registros de una entrada.
pub fn acumular(regs: &mut Vec<Reg>, nuevos: BTreeMap<(Tipo, String), (u64, Option<i64>)>) {
    for ((tipo, nombre), (usos, ultimo)) in nuevos {
        match regs.iter_mut().find(|r| r.tipo == tipo && r.nombre == nombre) {
            Some(r) => {
                r.usos += usos;
                r.ultimo_ms = r.ultimo_ms.max(ultimo);
            }
            None => regs.push(Reg { tipo, nombre, usos, ultimo_ms: ultimo }),
        }
    }
}

fn mtime_ns(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Lee un fichero de líneas JSON con la caché: sin cambios, no lo abre; si
/// creció, lee desde el offset; si encogió o cambió sin crecer, lo relee
/// entero. Devuelve los recuentos de ese fichero.
pub fn escanear_lineas(
    cache: &mut Cache,
    clave: &str,
    ruta: &Path,
    extraer: fn(&str) -> Vec<Hallazgo>,
    est: &mut Estadisticas,
    avisos: &mut Vec<String>,
) {
    est.ficheros += 1;
    let Ok(meta) = std::fs::metadata(ruta) else { return };
    let (tam, mtime) = (meta.len(), mtime_ns(&meta));
    let previa = cache.fuentes.get(clave);
    let desde = match previa {
        Some(p) if p.tam == tam && p.mtime_ns == mtime => return,
        Some(p) if tam > p.tam && p.offset <= p.tam => Some(p.offset),
        _ => None,
    };
    let (mut offset, mut regs, mut lineas) = match (desde, previa) {
        (Some(o), Some(p)) => {
            est.ficheros_incrementales += 1;
            (o, p.regs.clone(), p.lineas)
        }
        _ => {
            est.ficheros_releidos += 1;
            (0, Vec::new(), 0)
        }
    };
    let mut nuevos: BTreeMap<(Tipo, String), (u64, Option<i64>)> = BTreeMap::new();
    let abierto = std::fs::File::open(ruta).and_then(|mut f| f.seek(SeekFrom::Start(offset)).map(|_| f));
    match abierto {
        Ok(f) => {
            // Solo hasta el tamaño medido: lo que se escriba después se verá la próxima vez.
            let mut lector = BufReader::new(f.take(tam.saturating_sub(offset)));
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match lector.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        // Una línea sin salto final puede estar a medio escribir: se deja para la próxima.
                        if buf.last() != Some(&b'\n') {
                            break;
                        }
                        offset += n as u64;
                        lineas += 1;
                        est.lineas_leidas += 1;
                        let linea = String::from_utf8_lossy(&buf);
                        for h in extraer(linea.trim_end()) {
                            let e = nuevos.entry((h.tipo, h.nombre)).or_insert((0, None));
                            e.0 += 1;
                            e.1 = e.1.max(h.ts_ms);
                        }
                    }
                    Err(e) => {
                        avisos.push(format!("{}: lectura interrumpida ({e})", ruta.display()));
                        break;
                    }
                }
            }
        }
        Err(e) => {
            avisos.push(format!("{}: no se pudo abrir ({e})", ruta.display()));
            return;
        }
    }
    acumular(&mut regs, nuevos);
    cache.fuentes.insert(clave.to_string(), Entrada { tam, mtime_ns: mtime, offset, lineas, regs });
}
