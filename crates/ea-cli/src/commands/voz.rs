//! Cliente del control de voz de `ea-orchestrator` (`POST /v1/voz/hablar`,
//! `GET /v1/voz/estado`).
//! Esta CLI nunca captura ni reproduce audio: solo dispara la escucha y
//! muestra en terminal lo que el orquestador transcribió y respondió — el
//! propio orquestador es quien posee el dispositivo (§6.3 de la
//! especificación) y quien habla la respuesta.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct HablarResponse {
    transcripcion: Option<String>,
    respuesta: Option<String>,
    aviso: Option<String>,
}

pub fn hablar() -> Result<()> {
    let puerto = ea_orchestrator::config::load().voz_puerto;
    let url = format!("http://127.0.0.1:{puerto}/v1/voz/hablar");

    println!("Escuchando… (habla ahora; se corta sola tras el silencio)");

    let cliente = reqwest::blocking::Client::builder()
        // El ciclo completo (grabar + transcribir + preguntar al modelo
        // local + sintetizar) puede tardar bastante más que una petición
        // HTTP normal.
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("no se pudo crear el cliente HTTP")?;

    let resp = cliente
        .post(&url)
        .send()
        .with_context(|| format!("no se pudo contactar con ea-orchestrator en {url} (¿está arrancado el servicio?)"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let texto = resp.text().unwrap_or_default();
        anyhow::bail!("ea-orchestrator respondió {status}: {texto}");
    }

    let cuerpo: HablarResponse = resp.json().context("respuesta de ea-orchestrator no es JSON válido")?;

    if let Some(aviso) = cuerpo.aviso {
        println!("{aviso}");
        return Ok(());
    }
    if let Some(transcripcion) = cuerpo.transcripcion {
        println!("Tú:      {transcripcion}");
    }
    if let Some(respuesta) = cuerpo.respuesta {
        println!("Sistema: {respuesta}");
    }
    Ok(())
}

pub fn estado() -> Result<()> {
    let puerto = ea_orchestrator::config::load().voz_puerto;
    let url = format!("http://127.0.0.1:{puerto}/v1/voz/estado");
    let resp = reqwest::blocking::get(&url)
        .with_context(|| format!("no se pudo contactar con ea-orchestrator en {url} (¿está arrancado el servicio?)"))?;
    let cuerpo: serde_json::Value = resp.json().context("respuesta de ea-orchestrator no es JSON válido")?;

    let motor = cuerpo["motor"].as_bool().unwrap_or(false);
    println!("Motor de voz:          {}", if motor { "cargado" } else { "no disponible (faltan modelos, ver README)" });
    let palabra = &cuerpo["palabra_activacion"];
    match palabra["estado"].as_str() {
        Some("escuchando") => println!(
            "Palabra de activación: escuchando «{}» (micrófono abierto de forma continua)",
            palabra["palabra"].as_str().unwrap_or("?")
        ),
        Some("no_disponible") => println!(
            "Palabra de activación: activada en voz.toml pero no disponible: {}",
            palabra["motivo"].as_str().unwrap_or("?")
        ),
        _ => println!("Palabra de activación: desactivada"),
    }
    Ok(())
}
