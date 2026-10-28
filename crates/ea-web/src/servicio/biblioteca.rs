//! La biblioteca de herramientas (spec 018, F7): el inventario de lo que
//! cada agente tiene (skills, plugins, subagentes, MCP, comandos y
//! extensiones), su uso, y los gestos del usuario sobre ello: activar,
//! desactivar y abrir una sesión con herramientas acotadas.
//!
//! Lo que sale de aquí es lo que ya da `ea-biblioteca` (redactado: nunca un
//! secreto) y la simulación de las herramientas del catálogo. Las tres
//! herramientas de cambio pasan por la misma capa de acciones que el resto
//! (simular → ejecutar con revalidación, diario); lo único propio de la web es
//! de dónde salen las raíces del inventario y la terminal (`WebConfig`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use ea_acciones::SistemaReal;
use ea_biblioteca::uso::{calcular_uso, FuentesUso, InformeUso};
use ea_biblioteca::{Activado, Biblioteca, Contexto, Elemento, Filtro, Inventario, Mecanismo, Raices, Tipo, AGENTES_CON_SESION};
use serde_json::Value;

use super::{ErrorWeb, Resultado};
use crate::vistas::{BibliotecaVista, CambioPosible, ElementoVista, FichaVista, ResultadoAccion, Simulacion};
use crate::AppState;

/// Las únicas herramientas del catálogo que se simulan y ejecutan desde esta sección.
pub const HERRAMIENTAS: [&str; 3] = ["ea_activar_herramienta", "ea_desactivar_herramienta", "ea_lanzar_sesion"];

/// Cuánto vale un informe de uso ya calculado (la página sondea cada pocos segundos).
const VIGENCIA_USO: Duration = Duration::from_secs(20);

/// Lo que pide `GET /api/biblioteca` (cada campo es opcional).
#[derive(Default)]
pub struct Peticion {
    pub agente: Option<String>,
    pub tipo: Option<String>,
    pub categoria: Option<String>,
    pub activado: Option<String>,
}

fn invalido(m: impl Into<String>) -> ErrorWeb {
    ErrorWeb::Invalido(m.into())
}

fn filtro(c: &Peticion) -> Resultado<Filtro> {
    Ok(Filtro {
        agente: c.agente.clone().filter(|a| !a.is_empty()),
        tipo: match c.tipo.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => Some(Tipo::desde(t).ok_or_else(|| invalido(format!("tipo desconocido: «{t}»")))?),
            None => None,
        },
        categoria: c.categoria.clone().filter(|a| !a.is_empty()),
        activado: match c.activado.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => Some(Activado::desde(t).ok_or_else(|| invalido("«activado» es si, no o desconocido"))?),
            None => None,
        },
    })
}

// ------------------------------------------------------------------ entorno

fn raices(s: &AppState) -> Raices {
    s.config.biblioteca_raices.clone().unwrap_or_else(Raices::desde_entorno)
}

/// La biblioteca en `raices` con los ajustes de la configuración. El canal de
/// ajustes es Web: las skills del ejecutor solo se cambian desde una terminal.
pub(super) fn contexto_en(s: &AppState, raices: Raices) -> Contexto {
    Contexto::nuevo(raices, s.config.ajustes.clone(), ea_ajustes::Canal::Web)
}

/// Lee el disco (muchos ficheros): fuera del hilo asíncrono. El inventario ya
/// trae lo que la biblioteca desactivó moviéndolo, para poder reactivarlo.
async fn inventario(s: &AppState) -> Resultado<Inventario> {
    let r = raices(s);
    let a = ea_ajustes::cargar_de(&s.config.ajustes).ajustes;
    tokio::task::spawn_blocking(move || Biblioteca::cargar(&r, &a))
    .await
    .map_err(|e| ErrorWeb::Interno(format!("la lectura de la biblioteca falló: {e}")))
}

fn cambio_posible(e: &Elemento) -> CambioPosible {
    let no = |m: &str| CambioPosible { posible: false, motivo: Some(m.to_string()) };
    match e.mecanismo {
        Mecanismo::SoloSesion => no("solo se puede acotar en una sesión (su configuración no se edita): usa «▶ sesión con…»"),
        Mecanismo::No => no(e.nota.as_deref().unwrap_or("este elemento no se puede activar ni desactivar")),
        Mecanismo::Ajustes => no(
            "solo desde una terminal real: lo que cambia es lo que puede hacer el ejecutor (agente.skills.habilitadas)",
        ),
        _ if e.activado == Activado::Desconocido => no("no se sabe si está activado, así que no hay qué cambiar"),
        _ => CambioPosible { posible: true, motivo: None },
    }
}

fn vista(e: &Elemento) -> ElementoVista {
    ElementoVista {
        cambio: cambio_posible(e),
        sesion: AGENTES_CON_SESION.iter().copied().filter(|a| ea_biblioteca::sesion::admite(a, e).is_ok()).collect(),
        elemento: e.clone(),
    }
}

// ------------------------------------------------------------------ lectura

pub async fn listar(s: &AppState, peticion: &Peticion) -> Resultado<BibliotecaVista> {
    let f = filtro(peticion)?;
    let inv = inventario(s).await?;
    Ok(BibliotecaVista {
        total: inv.elementos.len(),
        elementos: inv.filtrar(&f).into_iter().map(vista).collect(),
        categorias: inv.categorias(),
        avisos: inv.avisos.clone(),
        agentes_sesion: AGENTES_CON_SESION.to_vec(),
    })
}

/// El informe de uso: caro la primera vez (transcripciones), con la caché
/// incremental en disco de `calcular_uso`; además se reutiliza unos segundos.
/// Se calcula fuera del hilo asíncrono y de uno en uno.
pub async fn uso(s: &AppState) -> Resultado<Arc<InformeUso>> {
    let mut guarda = s.biblioteca_uso.lock().await;
    if let Some((cuando, informe)) = guarda.as_ref()
        && cuando.elapsed() < VIGENCIA_USO
    {
        return Ok(informe.clone());
    }
    let inv = inventario(s).await?;
    let r = raices(s);
    let aislado = s.config.biblioteca_raices.is_some();
    let informe = tokio::task::spawn_blocking(move || {
        let mut fuentes = FuentesUso::desde_raices(&r);
        if aislado {
            // Con raíces de prueba, ni el diario real ni otro fuera de ellas.
            fuentes.diario = r.home.join("ecosistema.db");
        }
        calcular_uso(&r, &inv, &fuentes)
    })
    .await
    .map_err(|e| ErrorWeb::Interno(format!("el cálculo del uso falló: {e}")))?;
    let informe = Arc::new(informe);
    *guarda = Some((Instant::now(), informe.clone()));
    Ok(informe)
}

pub async fn ficha(s: &AppState, id: &str) -> Resultado<FichaVista> {
    let inv = inventario(s).await?;
    let e = inv.buscar(id).ok_or_else(|| ErrorWeb::NoEncontrado(format!("no existe el elemento «{id}» en la biblioteca")))?;
    let elemento = vista(e);
    let uso = uso(s).await?.buscar(id).cloned();
    Ok(FichaVista { elemento, uso })
}

// ------------------------------------------------- el sistema de la sección

/// El sistema real con la biblioteca de la sección: las raíces de
/// `WebConfig` (o las del entorno) y sus ajustes. El plan de una sesión toma
/// el `directorio` tal cual: [`comprobar`] solo deja pasar el de ea-web.
fn sistema_biblioteca(s: &AppState) -> SistemaReal {
    super::sistema_con(s, Some(contexto_en(s, raices(s))))
}

fn comprobar(s: &AppState, herramienta: &str, argumentos: &Value) -> Resultado<()> {
    if !HERRAMIENTAS.contains(&herramienta) {
        return Err(invalido(format!("desde la biblioteca solo se simula {}", HERRAMIENTAS.join(", "))));
    }
    // El plan de sesión usa el proyecto de ea-web: nada de directorios a
    // pedido. Solo vale el suyo, el que su propia simulación deja en los
    // argumentos (el proyecto o, sin él, el HOME).
    if let Some(d) = argumentos.get("directorio") {
        let r = raices(s);
        let propio = r.proyecto.as_ref().unwrap_or(&r.home).to_string_lossy().into_owned();
        if d.as_str().map(str::trim) != Some(propio.as_str()) {
            return Err(invalido("la sesión se abre en el proyecto de ea-web: no admite otro «directorio»"));
        }
    }
    Ok(())
}

/// Describe lo que haría la herramienta ahora, sin hacer nada (la simulación
/// de una sesión lleva el comando sin secretos).
pub async fn simular(s: &AppState, herramienta: &str, argumentos: &Value) -> Resultado<Simulacion> {
    comprobar(s, herramienta, argumentos)?;
    super::acciones::simular_en(s, &sistema_biblioteca(s), herramienta, argumentos).await
}

/// Ejecuta lo simulado: el gesto del usuario es la confirmación; la
/// revalidación y el diario son los de siempre. Tras un cambio, el informe de
/// uso en caché se descarta (el inventario ya no es el mismo).
pub async fn ejecutar(
    s: &AppState,
    herramienta: &str,
    argumentos: &Value,
    precondicion: &Value,
) -> Resultado<ResultadoAccion> {
    comprobar(s, herramienta, argumentos)?;
    let r = super::acciones::ejecutar_en(s, &sistema_biblioteca(s), herramienta, argumentos, precondicion).await?;
    *s.biblioteca_uso.lock().await = None;
    Ok(r)
}
