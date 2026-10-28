//! `ea_promover_nota`: copiar una nota a la región compartida de la bóveda,
//! que decide el usuario (decisión 9).

use serde::Deserialize;
use serde_json::json;

use super::{campo, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

pub(super) const CAMPOS: &[Campo] = &[
    campo("ruta", "ruta de la nota (p. ej. claude-code/tema/nota.md)"),
    Campo { obligatorio: false, ..campo("destino", "destino en «compartida» (opcional)") },
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    ruta: String,
    #[serde(default)]
    destino: Option<String>,
}

impl Args {
    fn destino(&self) -> Option<&str> {
        self.destino.as_deref().filter(|d| !d.trim().is_empty())
    }
}

/// Valida solo el texto de las rutas (sin mirar el disco).
pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    let partes = ea_boveda::ruta::validar_ruta(&a.ruta)?;
    if partes[0] == ea_boveda::ruta::REGION_COMPARTIDA {
        return Err(ErrorAccion::Argumentos(format!("{} ya está en la región compartida", a.ruta)));
    }
    if let Some(d) = a.destino() {
        ea_boveda::ruta::en_region(ea_boveda::ruta::REGION_COMPARTIDA, d)?;
    }
    Ok(())
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    let plan = p.consulta.plan_promocion(&a.ruta, a.destino())?;
    let mut norm = json!({ "ruta": plan.origen });
    if let Some(d) = a.destino() {
        norm["destino"] = json!(d);
    }
    Ok((
        norm,
        format!(
            "Copiaría la nota «{}» (de {}, {} bytes) de {} a {} en la región compartida y \
             marcaría ambas como promovidas. El original se conserva. Desde ahí la leen \
             todos los agentes como contenido de confianza.",
            plan.titulo, plan.autor, plan.bytes, plan.origen, plan.destino
        ),
        json!({ "huella": plan.huella, "destino": plan.destino }),
    ))
}

pub(super) fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    let huella = p.precondicion["huella"].as_str().unwrap_or_default();
    let plan = p.sistema.nota_promover(p.store, &a.ruta, a.destino(), huella, p.actor, p.canal.nombre())?;
    Ok(Aplicada {
        texto: format!("Nota {} promovida a {}.", plan.origen, plan.destino),
        trabajo: None,
        datos: json!({ "origen": plan.origen, "destino": plan.destino }),
    })
}
