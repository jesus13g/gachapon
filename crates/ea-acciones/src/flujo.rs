//! El flujo propuesta -> aceptación -> ejecución, con el diario como
//! testigo. Cada paso que cambia algo deja un evento: `propuesta_encolada`,
//! `propuesta_decidida`, `accion_ejecutada` / `accion_fallida`.

use ea_store::{
    AccionPropuesta, EstadoEjecucion, EstadoPropuesta, EventOutcome, NewEvent, NuevaPropuesta,
    Propuesta, PropuestaFilter, Store,
};
use serde_json::{json, Value};

use crate::catalogo::{puede_confirmar, Canal, Riesgo};
use crate::error::ErrorAccion;
use crate::herramientas::{self, supervisada, Hechos};
use crate::sistema::{Consulta, Sistema};

/// Tope de propuestas con acción pendientes **por proponente**: un agente que
/// insiste no puede inundar la cola del usuario, ni llenarla para que no
/// quepan las de otro (U-08). El proponente es el actor verificado (en el MCP,
/// por la credencial del planificador).
pub const MAX_PENDIENTES_CON_ACCION: usize = 30;

#[derive(Debug, Clone)]
pub struct Resultado {
    pub texto: String,
    pub trabajo: Option<String>,
    pub datos: Value,
    /// El id del evento `accion_ejecutada` que lo dejó en el diario, si se
    /// pudo escribir (las interfaces enlazan con él).
    pub evento: Option<i64>,
}

/// Deja el evento en el diario y devuelve su id. Un diario que no se puede
/// escribir no deshace la acción: el id es `None`.
fn evento(store: &Store, actor: &str, action: &str, trabajo: Option<&str>, ok: bool, detalle: Value) -> Option<i64> {
    store.append_event(NewEvent {
        actor: actor.to_string(),
        action: action.to_string(),
        trabajo: trabajo.map(str::to_string),
        outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
        detail: Some(detalle.to_string()),
        ..Default::default()
    })
    .ok()
    .map(|e| e.id)
}

/// Encola una propuesta con acción estructurada. La simulación se hace aquí
/// (solo lectura) y queda guardada con la propuesta. Si ya hay una
/// pendiente idéntica (misma herramienta y argumentos) no se duplica: se
/// devuelve esa, con `false`. No ejecuta nada: ni puede, porque solo recibe
/// una [`Consulta`].
pub fn proponer<C: Consulta>(
    store: &Store,
    consulta: &C,
    hechos: &Hechos,
    proponente: &str,
    descripcion: Option<&str>,
    herramienta: &str,
    args: &Value,
) -> Result<(Propuesta, bool), ErrorAccion> {
    let sim = herramientas::simular_con(store, consulta, hechos, herramienta, args)?;

    let pendientes = store.list_propuestas(&PropuestaFilter {
        estado: Some(EstadoPropuesta::Pendiente),
        limit: None,
    })?;
    if let Some(igual) = pendientes.iter().find(|p| {
        p.accion
            .as_ref()
            .is_some_and(|a| a.herramienta == herramienta && a.argumentos == sim.argumentos)
    }) {
        return Ok((igual.clone(), false));
    }
    let propias = pendientes.iter().filter(|p| p.accion.is_some() && p.proponente.as_deref() == Some(proponente)).count();
    if propias >= MAX_PENDIENTES_CON_ACCION {
        return Err(ErrorAccion::NoAplica(format!(
            "{proponente} ya tiene {MAX_PENDIENTES_CON_ACCION} propuestas con acción pendientes; el \
             usuario debe decidir algunas antes de que encole más"
        )));
    }

    let descripcion = match descripcion {
        Some(d) if !d.trim().is_empty() => d.to_string(),
        _ => sim.texto.clone(),
    };
    let propuesta = store.crear_propuesta(NuevaPropuesta {
        proponente: proponente.to_string(),
        descripcion,
        accion: Some(AccionPropuesta {
            herramienta: herramienta.to_string(),
            argumentos: sim.argumentos.clone(),
            riesgo: sim.riesgo.as_str().to_string(),
            simulacion: sim.texto.clone(),
            precondicion: sim.precondicion.clone(),
        }),
    })?;
    evento(
        store,
        proponente,
        "propuesta_encolada",
        None,
        true,
        json!({
            "propuesta": propuesta.id, "herramienta": herramienta, "riesgo": sim.riesgo.as_str(),
            "argumentos": sim.argumentos, "simulacion": sim.texto,
        }),
    );
    Ok((propuesta, true))
}

/// Ejecuta una herramienta mutante supervisada: comprueba que el canal puede
/// confirmar ese riesgo, revalida contra la precondición, aplica, y deja el
/// evento (`accion_ejecutada` o `accion_fallida`) con argumentos y resultado.
/// Quien llama es responsable de que `actor` sea quien confirmó.
#[allow(clippy::too_many_arguments)]
pub async fn ejecutar<S: Sistema>(
    store: &mut Store,
    sistema: &S,
    actor: &str,
    canal: Canal,
    herramienta: &str,
    args: &Value,
    precondicion: &Value,
    propuesta: Option<&Propuesta>,
) -> Result<Resultado, ErrorAccion> {
    let s = supervisada(herramienta)?;
    let h = s.ficha();
    let contexto = |extra: Value| {
        let mut v = json!({
            "herramienta": herramienta, "riesgo": h.riesgo.as_str(), "canal": canal.nombre(),
            "argumentos": args,
            "propuesta": propuesta.map(|p| p.id),
            "proponente": propuesta.and_then(|p| p.proponente.clone()),
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };

    let intento: Result<herramientas::Aplicada, ErrorAccion> = async {
        puede_confirmar(canal, h.riesgo)?;
        let hechos = herramientas::observar(sistema, herramienta, args).await?;
        herramientas::revalidar_con(store, sistema, &hechos, herramienta, args, precondicion)?;
        herramientas::aplicar(s, sistema, store, actor, canal, args, precondicion).await
    }
    .await;

    match intento {
        Ok(a) => {
            let ev = evento(
                store,
                actor,
                "accion_ejecutada",
                a.trabajo.as_deref(),
                true,
                contexto(json!({ "resultado": a.datos, "texto": a.texto })),
            );
            Ok(Resultado { texto: a.texto, trabajo: a.trabajo, datos: a.datos, evento: ev })
        }
        Err(e) => {
            evento(
                store,
                actor,
                "accion_fallida",
                None,
                false,
                contexto(json!({ "error": e.to_string() })),
            );
            Err(e)
        }
    }
}

/// Ejecución directa por el usuario presente (un botón, un comando de
/// terminal): simula y ejecuta enseguida. El gesto del usuario es la
/// confirmación; el canal sigue limitando los irreversibles.
pub async fn ejecutar_directo<S: Sistema>(
    store: &mut Store,
    sistema: &S,
    actor: &str,
    canal: Canal,
    herramienta: &str,
    args: &Value,
) -> Result<Resultado, ErrorAccion> {
    let h = supervisada(herramienta)?.ficha();
    if let Err(e) = puede_confirmar(canal, h.riesgo) {
        evento(
            store,
            actor,
            "accion_fallida",
            None,
            false,
            json!({
                "herramienta": herramienta, "riesgo": h.riesgo.as_str(), "canal": canal.nombre(),
                "argumentos": args, "error": e.to_string(),
            }),
        );
        return Err(e);
    }
    let hechos = herramientas::observar(sistema, herramienta, args).await;
    let sim = match hechos.and_then(|h| herramientas::simular_con(store, sistema, &h, herramienta, args)) {
        Ok(s) => s,
        Err(e) => {
            evento(
                store,
                actor,
                "accion_fallida",
                None,
                false,
                json!({
                    "herramienta": herramienta, "riesgo": h.riesgo.as_str(), "canal": canal.nombre(),
                    "argumentos": args, "error": e.to_string(),
                }),
            );
            return Err(e);
        }
    };
    ejecutar(store, sistema, actor, canal, herramienta, &sim.argumentos, &sim.precondicion, None).await
}

fn riesgo_de(p: &Propuesta, a: &AccionPropuesta) -> Result<Riesgo, ErrorAccion> {
    Riesgo::parse(&a.riesgo)
        .filter(|r| r.es_mutante())
        .ok_or_else(|| ErrorAccion::NoAplica(format!("propuesta #{} con riesgo inválido '{}'", p.id, a.riesgo)))
}

/// El usuario acepta una propuesta. Sin acción (de texto libre): solo constancia. Con acción: se reclama de forma atómica (aceptar
/// dos veces no ejecuta dos veces), se ejecuta por la capa interna con
/// revalidación y se anota el resultado en la propuesta. Una acción que
/// falla no es un `Err`: la propuesta queda aceptada con ejecución
/// `fallida` y el detalle. `Err` significa que nada se reclamó (propuesta
/// inexistente, ya decidida, o canal no autorizado para ese riesgo).
pub async fn aceptar_propuesta<S: Sistema>(
    store: &mut Store,
    sistema: &S,
    id: i64,
    actor: &str,
    canal: Canal,
) -> Result<Propuesta, ErrorAccion> {
    Ok(aceptar_propuesta_con_resultado(store, sistema, id, actor, canal).await?.0)
}

/// Como [`aceptar_propuesta`], y además devuelve el [`Resultado`] de la
/// acción cuando se ejecutó bien (trabajo, datos y evento del diario), para
/// que la interfaz enlace con lo que produjo.
pub async fn aceptar_propuesta_con_resultado<S: Sistema>(
    store: &mut Store,
    sistema: &S,
    id: i64,
    actor: &str,
    canal: Canal,
) -> Result<(Propuesta, Option<Resultado>), ErrorAccion> {
    let previa = store.get_propuesta(id)?;
    let Some(accion) = previa.accion.clone() else {
        let p = store.decidir_propuesta(id, EstadoPropuesta::Aceptada)?;
        evento(
            store,
            actor,
            "propuesta_decidida",
            None,
            true,
            json!({ "propuesta": id, "estado": "aceptada", "canal": canal.nombre() }),
        );
        return Ok((p, None));
    };

    // La política de canal se comprueba antes de reclamar: una web que
    // intenta aceptar un irreversible no consume la propuesta.
    if let Err(e) = puede_confirmar(canal, riesgo_de(&previa, &accion)?) {
        evento(
            store,
            actor,
            "accion_fallida",
            None,
            false,
            json!({
                "propuesta": id, "herramienta": accion.herramienta, "riesgo": accion.riesgo,
                "canal": canal.nombre(), "error": e.to_string(),
            }),
        );
        return Err(e);
    }
    let reclamada = store.reclamar_propuesta(id)?;
    evento(
        store,
        actor,
        "propuesta_decidida",
        None,
        true,
        json!({
            "propuesta": id, "estado": "aceptada", "canal": canal.nombre(),
            "herramienta": accion.herramienta, "riesgo": accion.riesgo,
        }),
    );

    let (estado, detalle, resultado) = match ejecutar(
        store,
        sistema,
        actor,
        canal,
        &accion.herramienta,
        &accion.argumentos,
        &accion.precondicion,
        Some(&reclamada),
    )
    .await
    {
        Ok(r) => (EstadoEjecucion::Ejecutada, r.texto.clone(), Some(r)),
        Err(e) => (EstadoEjecucion::Fallida, e.to_string(), None),
    };
    Ok((store.registrar_ejecucion_propuesta(id, estado, &detalle)?, resultado))
}

pub fn rechazar_propuesta(
    store: &Store,
    id: i64,
    actor: &str,
    canal: Canal,
) -> Result<Propuesta, ErrorAccion> {
    let p = store.decidir_propuesta(id, EstadoPropuesta::Rechazada)?;
    evento(
        store,
        actor,
        "propuesta_decidida",
        None,
        true,
        json!({ "propuesta": id, "estado": "rechazada", "canal": canal.nombre() }),
    );
    Ok(p)
}
