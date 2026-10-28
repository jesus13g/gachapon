//! `ea mcp cabeceras`: el `headersHelper` del MCP de Claude Code (lo pone
//! `ea instalar`). Escribe en la salida estándar `{"X-EA-Credencial": "…"}`
//! con la credencial del planificador que `ea-mcp` deja en
//! `<ejecucion>/ea-mcp/credencial`; Claude Code la corre en cada conexión.
//!
//! Dentro del sandbox del ejecutor el directorio de ejecución está tapado:
//! ahí no hay credencial que leer y esta orden falla. Un error nunca lleva el
//! contenido del fichero.

use std::path::Path;

use anyhow::Context;

pub fn cabeceras() -> anyhow::Result<()> {
    let ejecucion = ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso)
        .context("sin directorio de ejecución no hay credencial del MCP")?;
    println!("{}", cabeceras_de(&ejecucion)?);
    Ok(())
}

/// El JSON de las cabeceras con la credencial de este directorio de ejecución.
pub(crate) fn cabeceras_de(ejecucion: &Path) -> anyhow::Result<String> {
    let ruta = ea_ajustes::fichero_credencial_mcp(ejecucion);
    let texto = std::fs::read_to_string(&ruta).with_context(|| format!("no se pudo leer {} (¿está arrancado ea-mcp?)", ruta.display()))?;
    let credencial = texto.trim();
    anyhow::ensure!(ea_ajustes::credencial_mcp_valida(credencial), "{} no tiene una credencial válida: reinicia ea-mcp", ruta.display());
    Ok(serde_json::json!({ ea_ajustes::CABECERA_CREDENCIAL_MCP: credencial }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_la_cabecera_y_sus_errores_no_ensenan_el_fichero() {
        let d = tempfile::tempdir().unwrap();
        let error = cabeceras_de(d.path()).unwrap_err().to_string();
        assert!(error.contains("ea-mcp"), "{error}");
        std::fs::create_dir(d.path().join("ea-mcp")).unwrap();
        let ruta = d.path().join("ea-mcp/credencial");
        std::fs::write(&ruta, "secreto-mal-formado").unwrap();
        let error = format!("{:#}", cabeceras_de(d.path()).unwrap_err());
        assert!(error.contains("no tiene una credencial válida") && !error.contains("secreto-mal-formado"), "{error}");
        let buena = "a1".repeat(32);
        std::fs::write(&ruta, &buena).unwrap();
        let v: serde_json::Value = serde_json::from_str(&cabeceras_de(d.path()).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({ "X-EA-Credencial": buena }));
    }
}
