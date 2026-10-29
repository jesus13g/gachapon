//! Propuestas de los agentes y su decisión.

use ea_acciones::Canal;
use ea_store::{EstadoPropuesta, Propuesta, PropuestaFilter};

use super::{ACTOR, ErrorWeb, Resultado, sistema, ts};
use crate::AppState;
use crate::vistas::{AccionPropuesta, Ejecucion, PropuestaVista, ResultadoAccion};

pub(super) fn vista(p: &Propuesta) -> PropuestaVista {
    PropuestaVista {
        id: p.id,
        proponente: p.proponente.clone(),
        creada: ts(p.creado_en),
        descripcion: p.descripcion.clone(),
        estado: p.estado.as_str(),
        decidida: p.decidido_en.map(ts),
        accion: p.accion.as_ref().map(|a| AccionPropuesta {
            herramienta: a.herramienta.clone(),
            argumentos: a.argumentos.clone(),
            riesgo: a.riesgo.clone(),
            simulacion: a.simulacion.clone(),
        }),
        ejecutada: p.ejecucion.as_ref().map(|e| Ejecucion { estado: e.estado.as_str(), detalle: e.detalle.clone(), cuando: e.ejecutada_en.map(ts) }),
        resultado: None,
    }
}

/// `estado`: `pendiente`, `aceptada`… o todas si es `None`.
pub async fn listar(s: &AppState, estado: Option<&str>) -> Resultado<Vec<PropuestaVista>> {
    let filter = PropuestaFilter { estado: estado.map(str::parse::<EstadoPropuesta>).transpose()?, limit: Some(100) };
    let store = s.store.lock().await;
    Ok(store.list_propuestas(&filter)?.iter().map(vista).collect())
}

/// Aceptar una propuesta con acción la ejecuta (a través de la capa interna,
/// con revalidación; el resultado queda en la propuesta y en el diario). Las
/// irreversibles no se confirman desde la web. Las antiguas de texto libre
/// solo dejan constancia.
pub async fn decidir(s: &AppState, id: i64, decision: &str) -> Resultado<PropuestaVista> {
    let aceptar = match decision {
        "aceptar" => true,
        "rechazar" => false,
        _ => return Err(ErrorWeb::Invalido("decision debe ser 'aceptar' o 'rechazar'".into())),
    };

    let sis = sistema(s);
    let mut store = s.acciones.lock().await;
    let (p, r) = if aceptar {
        ea_acciones::aceptar_propuesta_con_resultado(&mut store, &sis, id, ACTOR, Canal::Web).await?
    } else {
        (ea_acciones::rechazar_propuesta(&store, id, ACTOR, Canal::Web)?, None)
    };
    let mut v = vista(&p);
    v.resultado = r.map(ResultadoAccion::from);
    Ok(v)
}
