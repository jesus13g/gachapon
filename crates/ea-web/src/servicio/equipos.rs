//! Equipos de agentes (spec 019): verlos, despertarlos y dormirlos. Despertar
//! y dormir son gestos del usuario (la ruta exige la clave de sesión del
//! panel); ningún agente los hace. Las reglas son las de `ea equipo`:
//! `validar_equipo`, disponibilidad de los binarios, tope `orquestador.paralelo`
//! y un agente en un solo equipo despierto, todo dentro de `Bus::crear_equipo`.

use ea_ajustes::ficha;
use ea_bus::{comprobar_disponibles, parse_miembro, repartir_todos, Miembro, NuevoEquipo, ResumenEquipo, RolEquipo, VistaEquipo};
use ea_store::{EventOutcome, NewEvent};
use serde::{Deserialize, Serialize};

use super::terminal::lanzar_argv;
use super::{bus_con_store, bus_lectura, texto_no_vacio, ErrorWeb, Resultado, ACTOR};
use crate::AppState;

/// Cuántos trabajos del equipo enseña la ficha (como `ea equipo ver`).
const TRABAJOS_VISTOS: usize = 20;
const TITULO_MAX: usize = 200;
const OBJETIVO_MAX: usize = 4_000;

pub fn listar(s: &AppState) -> Resultado<Vec<ResumenEquipo>> {
    Ok(bus_lectura(s)?.resumen_equipos()?)
}

pub fn ver(s: &AppState, id: &str) -> Resultado<VistaEquipo> {
    Ok(bus_lectura(s)?.vista_equipo(id, TRABAJOS_VISTOS)?)
}

/// Un miembro tal como lo manda el formulario: `{agente, rol}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MiembroPedido {
    agente: String,
    rol: String,
}

/// Cuerpo de `POST /api/equipos`: `miembros` o `todos`, no las dos cosas.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Despertar {
    titulo: String,
    objetivo: String,
    worktree: String,
    #[serde(default)]
    miembros: Vec<MiembroPedido>,
    #[serde(default)]
    todos: bool,
    /// Abrir una terminal con la sesión de cada planificador y revisor.
    #[serde(default)]
    terminal: bool,
}

/// Qué pasó con la sesión de un planificador o revisor.
#[derive(Serialize)]
pub struct SesionEquipo {
    pub agente: String,
    pub abierta: bool,
    /// El comando que abre (o abriría) la sesión a mano.
    pub comando: String,
    /// Por qué no se abrió, si se pidió y falló.
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Despierto {
    pub equipo: VistaEquipo,
    /// Agentes que «todos» dejó fuera (sin binario) y por qué.
    pub omitidos: Vec<String>,
    pub sesiones: Vec<SesionEquipo>,
}

/// Despierta un equipo con el mismo flujo que `ea equipo despertar`.
pub async fn despertar(s: &AppState, cuerpo: serde_json::Value) -> Resultado<Despierto> {
    let p: Despertar =
        serde_json::from_value(cuerpo).map_err(|e| ErrorWeb::Invalido(format!("petición inválida: {e}")))?;
    if p.todos == !p.miembros.is_empty() {
        return Err(ErrorWeb::Invalido("indica 'todos' o una lista de 'miembros' (no las dos cosas)".into()));
    }
    let titulo = texto_no_vacio(&p.titulo, "titulo", TITULO_MAX)?;
    let objetivo = texto_no_vacio(&p.objetivo, "objetivo", OBJETIVO_MAX)?;
    let worktree = crate::git::validar_worktree(&p.worktree).await?;
    let ajustes = ea_ajustes::cargar_de(&s.config.ajustes).ajustes;
    let path = std::env::var_os("PATH");
    let (lista, omitidos): (Vec<Miembro>, Vec<String>) = if p.todos {
        repartir_todos(&ajustes, path.as_deref())?
    } else {
        let lista = p
            .miembros
            .iter()
            .map(|m| parse_miembro(&format!("{}:{}", m.agente, m.rol)))
            .collect::<Result<Vec<_>, _>>()?;
        (lista, vec![])
    };
    comprobar_disponibles(&lista, &ajustes, path.as_deref())?;
    let equipo = bus_con_store(s)
        .await?
        .crear_equipo(NuevoEquipo { titulo, objetivo, worktree_base: worktree, miembros: lista, creado_por: ACTOR.into() }, &ajustes)?;

    let mut sesiones = Vec::new();
    for m in equipo.miembros.iter().filter(|m| m.rol != RolEquipo::Ejecutor) {
        // Lista cerrada: `ea equipo sesion <id del equipo> <agente del catálogo>`.
        if ficha(&m.agente).is_none() {
            continue;
        }
        let args: Vec<String> = ["equipo", "sesion", &equipo.id, &m.agente].map(String::from).to_vec();
        let comando = format!("ea {}", args.join(" "));
        let (abierta, error) = if p.terminal {
            match lanzar_argv(s, &args) {
                Ok(()) => {
                    anotar_sesion(s, &equipo.id, &m.agente).await;
                    (true, None)
                }
                Err(e) => (false, Some(e.to_string())),
            }
        } else {
            (false, None)
        };
        sesiones.push(SesionEquipo { agente: m.agente.clone(), abierta, comando, error });
    }
    Ok(Despierto { equipo: ver(s, &equipo.id)?, omitidos, sesiones })
}

/// Abrir la terminal es un gesto del usuario: queda en el diario.
async fn anotar_sesion(s: &AppState, equipo: &str, agente: &str) {
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: "equipo_sesion_abierta".into(),
        outcome: Some(EventOutcome::Ok),
        detail: Some(serde_json::json!({ "equipo": equipo, "agente": agente }).to_string()),
        ..Default::default()
    };
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, "no se pudo apuntar la sesión de equipo en el diario");
    }
}

/// Duerme el equipo: lo pendiente de sus ejecutores vuelve a la cola de rol.
pub async fn dormir(s: &AppState, id: &str) -> Resultado<VistaEquipo> {
    bus_con_store(s).await?.dormir_equipo(id)?;
    ver(s, id)
}
