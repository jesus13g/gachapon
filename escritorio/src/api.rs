//! Cliente mínimo de la API local de `ea-web` (solo GET, solo `127.0.0.1`).

use std::time::Duration;

use serde_json::Value;

fn agente() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build()
}

/// `GET /api/<ruta>` como JSON; `Err` con el motivo legible si no responde.
pub fn obtener(puerto: u16, ruta: &str) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{puerto}/api/{ruta}");
    match agente().get(&url).call() {
        Ok(r) => r.into_json::<Value>().map_err(|e| format!("respuesta ilegible: {e}")),
        Err(ureq::Error::Status(c, _)) => Err(format!("ea-web respondió {c}")),
        Err(e) => Err(format!("ea-web no responde en 127.0.0.1:{puerto} ({e})")),
    }
}
