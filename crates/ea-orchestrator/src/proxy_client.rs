//! Cliente HTTP hacia el endpoint de decisión de rutinas de `ea-proxy`
//! (`POST /v1/rama`). El orquestador nunca habla con `llama-server`
//! directamente: `ea-proxy` sigue siendo el único punto que conmuta
//! modelos de inferencia (§5 de la especificación).

use anyhow::{bail, Context};
use serde::Serialize;
use serde_json::Value;

use crate::ejecutor::EjecutorDecisiones;

pub struct ProxyClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct RamaRequest<'a> {
    contexto: &'a str,
    opciones: &'a [String],
}

impl ProxyClient {
    pub fn new(base_url: String) -> Self {
        Self { base_url, http: reqwest::Client::new() }
    }

    /// `POST /v1/chat/completions` — el mismo endpoint que ya expone
    /// `ea-proxy` para "tareas más abiertas" (§9.3): un mensaje de usuario
    /// suelto, sin decisión de rama acotada de por medio. Es la vía natural
    /// para el texto libre que produce una transcripción de voz — la tabla
    /// de actores del documento (§14.1) asigna la voz como invocación del
    /// modelo local, nunca de Claude Code.
    pub async fn completar(&self, texto: &str) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct Mensaje<'a> {
            role: &'static str,
            content: &'a str,
        }
        #[derive(Serialize)]
        struct Peticion<'a> {
            messages: [Mensaje<'a>; 1],
        }

        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&Peticion {
                messages: [Mensaje { role: "user", content: texto }],
            })
            .send()
            .await
            .context("no se pudo contactar con ea-proxy")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("ea-proxy respondió {status}: {text}");
        }

        let parsed: Value = resp.json().await.context("respuesta de ea-proxy no es JSON")?;
        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("respuesta de ea-proxy sin contenido: {parsed}"))
    }
}

impl EjecutorDecisiones for ProxyClient {
    async fn elegir_rama(&self, contexto: &str, opciones: &[String]) -> anyhow::Result<String> {
        let resp = self
            .http
            .post(format!("{}/v1/rama", self.base_url))
            .json(&RamaRequest { contexto, opciones })
            .send()
            .await
            .context("no se pudo contactar con ea-proxy")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("ea-proxy respondió {status}: {text}");
        }

        let parsed: Value = resp.json().await.context("respuesta de ea-proxy no es JSON")?;
        parsed["rama"]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("respuesta de ea-proxy sin campo 'rama': {parsed}"))
    }
}
