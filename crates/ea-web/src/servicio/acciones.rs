//! La consola de acciones: el catálogo, simular y ejecutar lo simulado.

use ea_acciones::Canal;
use serde_json::Value;

use super::{sistema, ts, ErrorWeb, Resultado, ACTOR};
use crate::vistas::{AccionCatalogo, CampoVista, EventoHerramienta, ResultadoAccion, Simulacion};
use crate::AppState;

/// El catálogo completo, para verlo; ejecutables desde aquí solo las
/// supervisadas (las directas son de los agentes o ya tienen su sección).
pub async fn catalogo(s: &AppState) -> Resultado<Vec<AccionCatalogo>> {
    let slugs: Vec<String> = s.store.lock().await.listar_rutinas()?.into_iter().map(|r| r.slug).collect();
    Ok(ea_acciones::CATALOGO
        .iter()
        .map(|h| {
            let supervisada = h.ejecucion == ea_acciones::Ejecucion::Supervisada;
            let campos = ea_acciones::campos(h.nombre)
                .iter()
                .map(|c| CampoVista {
                    nombre: c.nombre,
                    entero: c.entero,
                    largo: c.largo,
                    obligatorio: c.obligatorio,
                    // Las rutinas no son un dominio fijo: las que hay ahora.
                    opciones: if c.nombre == "slug" {
                        slugs.clone()
                    } else {
                        c.opciones.iter().map(|o| o.to_string()).collect()
                    },
                    ayuda: c.ayuda,
                })
                .collect();
            AccionCatalogo {
                nombre: h.nombre,
                riesgo: h.riesgo.as_str(),
                supervisada,
                local: h.local,
                resumen: h.resumen,
                campos,
                confirmable: supervisada && Canal::Web.puede_confirmar(h.riesgo).is_ok(),
            }
        })
        .collect())
}

/// Sin `systemctl` no hay ni estado que simular ni forma de reiniciar.
fn comprobar(s: &AppState, herramienta: &str) -> Resultado<()> {
    if herramienta == "ea_reiniciar_servicio" && !s.config.systemctl {
        return Err(ErrorWeb::NoDisponible("systemctl desactivado".into()));
    }
    Ok(())
}

/// Describe lo que haría la herramienta ahora, sin hacer nada.
pub async fn simular(s: &AppState, herramienta: &str, argumentos: &Value) -> Resultado<Simulacion> {
    comprobar(s, herramienta)?;
    let sis = sistema(s);
    let hechos = ea_acciones::observar(&sis, herramienta, argumentos).await?;
    let store = s.store.lock().await;
    let sim = ea_acciones::simular_con(&store, &sis, &hechos, herramienta, argumentos)?;
    Ok(Simulacion {
        herramienta: sim.herramienta,
        riesgo: sim.riesgo.as_str(),
        argumentos: sim.argumentos,
        texto: sim.texto,
        precondicion: sim.precondicion,
        confirmable: Canal::Web.puede_confirmar(sim.riesgo).is_ok(),
    })
}

/// Ejecuta lo que se acaba de simular. Si el mundo ha cambiado desde la
/// simulación, la revalidación falla y no se hace nada; las irreversibles no
/// se confirman desde la web. Todo queda en el diario.
pub async fn ejecutar(
    s: &AppState,
    herramienta: &str,
    argumentos: &Value,
    precondicion: &Value,
) -> Resultado<ResultadoAccion> {
    comprobar(s, herramienta)?;
    let sis = sistema(s);
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar(&mut store, &sis, ACTOR, Canal::Web, herramienta, argumentos, precondicion, None)
        .await?;
    Ok(ResultadoAccion { texto: r.texto, trabajo: r.trabajo, datos: r.datos })
}

/// Cuántos eventos lleva el historial de una herramienta.
const HISTORIAL_HERRAMIENTA: i64 = 10;

/// Los últimos eventos de una herramienta del catálogo, supervisada o
/// directa (A3): ejecuciones y fallos, propuestas y sus decisiones, e
/// invocaciones por MCP (también simuladas y denegadas). Solo lectura.
pub async fn historial(s: &AppState, herramienta: &str) -> Resultado<Vec<EventoHerramienta>> {
    if ea_acciones::buscar(herramienta).is_none() {
        return Err(ErrorWeb::NoEncontrado(format!("herramienta desconocida: «{herramienta}»")));
    }
    let eventos = s.store.lock().await.list_events_herramienta(herramienta, HISTORIAL_HERRAMIENTA)?;
    Ok(eventos
        .into_iter()
        .map(|e| {
            let d: Value = e.detail.as_deref().and_then(|d| serde_json::from_str(d).ok()).unwrap_or(Value::Null);
            let texto = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_string);
            let canal = texto("canal").or_else(|| e.action.starts_with("mcp_").then(|| "mcp".to_string()));
            EventoHerramienta {
                id: e.id,
                ts: ts(e.ts),
                canal,
                proponente: texto("proponente"),
                propuesta: d.get("propuesta").and_then(Value::as_i64),
                error: texto("error"),
                actor: e.actor,
                accion: e.action,
                resultado: e.outcome.map(|o| o.as_str()),
                trabajo: e.trabajo,
                duracion_ms: e.duration_ms,
            }
        })
        .collect())
}
