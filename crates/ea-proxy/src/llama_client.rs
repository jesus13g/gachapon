//! Cliente HTTP hacia el endpoint crudo de `llama-server`
//! (OpenAI-compatible). No confía en `tool_calls`/`tool_choice` nativos —
//! validado empíricamente en esta sesión como no fiable contra la plantilla
//! de Qwen2.5-Coder-7B servida por `llama-server` (`chat_format:
//! "Content-only"`). Usa siempre `response_format: json_schema` para
//! cualquier paso que deba producir una decisión estructurada, y una
//! llamada sin schema solo para la respuesta final en lenguaje natural.

use anyhow::{Context, bail};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

pub struct LlamaClient {
    base_url: String,
    http: reqwest::Client,
}

impl LlamaClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    /// Llamada acotada por gramática: el modelo solo puede producir JSON
    /// válido contra `schema`. Siempre a `temperature: 0` — estos pasos son
    /// decisiones, no generación creativa.
    pub async fn constrained(
        &self,
        messages: &[ChatMessage],
        schema_name: &str,
        schema: Value,
    ) -> anyhow::Result<Value> {
        let body = json!({
            "model": "qwen2.5-coder-7b",
            "messages": messages,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "strict": true,
                    "schema": schema,
                }
            },
            "temperature": 0,
        });
        let content = self.chat_completion(body).await?;
        serde_json::from_str(&content)
            .with_context(|| format!("respuesta del modelo no es JSON válido: {content}"))
    }

    /// Llamada sin acotar, para la respuesta final en lenguaje natural.
    /// Aplica los parámetros del perfil activo.
    pub async fn free(
        &self,
        messages: &[ChatMessage],
        temperature: f32,
        max_tokens: u32,
    ) -> anyhow::Result<String> {
        let body = json!({
            "model": "qwen2.5-coder-7b",
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        });
        self.chat_completion(body).await
    }

    async fn chat_completion(&self, body: Value) -> anyhow::Result<String> {
        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .context("no se pudo contactar con llama-server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("llama-server respondió {status}: {text}");
        }

        let parsed: Value = resp.json().await.context("respuesta de llama-server no es JSON")?;
        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("respuesta de llama-server sin content: {parsed}"))
    }
}
