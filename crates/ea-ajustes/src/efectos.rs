//! Los efectos de un cambio de ajustes que no son solo el fichero. Hoy uno:
//! `escritorio.al_iniciar` escribe o quita el autoarranque de `ea-escritorio`
//! (`ea-plataforma`). Lo llaman quienes aplican cambios de verdad (`ea ajustes
//! establecer` y el panel), no `establecer`, para que escribir el fichero siga
//! sin tocar nada fuera de él.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::escribir::Aplicado;

/// Clave del ajuste de autoarranque.
pub const CLAVE_AL_INICIAR: &str = "escritorio.al_iniciar";

/// El binario `ea-escritorio`: junto al ejecutable actual (los binarios del
/// ecosistema viven juntos) y, si no, en el `PATH`.
pub fn binario_escritorio() -> Option<PathBuf> {
    ea_plataforma::rutas::binario_hermano("ea-escritorio").or_else(|| ea_plataforma::rutas::buscar_en_path("ea-escritorio"))
}

/// Aplica a la plataforma el nuevo valor de `escritorio.al_iniciar`.
/// `binario` solo hace falta para activarlo.
pub fn aplicar_al_iniciar(activar: bool, binario: Option<&Path>) -> Result<(), String> {
    let destino = match (activar, binario) {
        (true, None) => return Err("no se encuentra el binario ea-escritorio (compílalo e instálalo junto a ea)".into()),
        (_, Some(b)) => b,
        (false, None) => Path::new("ea-escritorio"),
    };
    ea_plataforma::escritorio::autoarranque(activar, destino).map_err(|e| format!("autoarranque de ea-escritorio: {e}"))
}

/// Aplica los efectos de un lote ya escrito. Devuelve un aviso por cada efecto
/// que falló (el ajuste queda guardado igualmente; el efecto se reintenta al
/// volver a guardarlo).
pub fn aplicar_efectos(aplicados: &[Aplicado]) -> Vec<String> {
    let mut avisos = Vec::new();
    for a in aplicados.iter().filter(|a| a.clave == CLAVE_AL_INICIAR) {
        // Quitar la clave deja el valor por defecto: apagado.
        let activar = matches!(a.despues, Value::Bool(true));
        if let Err(e) = aplicar_al_iniciar(activar, binario_escritorio().as_deref()) {
            avisos.push(e);
        }
    }
    avisos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activar_sin_binario_falla_y_desactivar_no_lo_necesita() {
        assert!(aplicar_al_iniciar(true, None).unwrap_err().contains("ea-escritorio"));
    }
}
