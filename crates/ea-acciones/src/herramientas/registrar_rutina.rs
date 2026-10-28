//! `ea_registrar_rutina`: registrar una rutina nueva o una versión nueva de
//! una registrada (lo mismo que `ea rutina registrar`).

use serde::Deserialize;
use serde_json::json;

use super::{campo, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::diff::diff_lineas;
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

pub(super) const CAMPOS: &[Campo] =
    &[Campo { largo: true, ..campo("toml", "definición TOML de la rutina ([rutina], [entradas], [[pasos]])") }];

/// La definición completa, tal como se registraría.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    toml: String,
}

/// Validación sin mirar el mundo: tamaño y formato (RF-001 y RF-016 de la
/// spec 004). Que la versión sea mayor se mira al simular.
pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    if a.toml.len() > ea_bus::MAX_DEFINICION {
        return Err(ErrorAccion::Argumentos(format!(
            "la definición es demasiado larga ({} bytes; como mucho {})",
            a.toml.len(),
            ea_bus::MAX_DEFINICION
        )));
    }
    ea_bus::Rutina::parsear(&a.toml).map(|_| ()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    // Lo que no es de formato depende del mundo (la versión
    // registrada ahora): NoAplica, como en el resto de herramientas.
    let (r, anterior) = ea_bus::comprobar_registro(p.store, &a.toml).map_err(|e| ErrorAccion::NoAplica(e.to_string()))?;
    let orden = r.a_orden(&[]).map_err(|e| ErrorAccion::Argumentos(e.to_string()))?;
    let c = &r.rutina;
    let (que, cambios) = match &anterior {
        Some(f) => {
            let d = diff_lineas(&f.definicion, &a.toml);
            (
                format!("la versión {} de la rutina '{}' (ahora está en la {})", c.version, c.slug, f.version),
                if d.is_empty() {
                    "La definición es la misma salvo la versión.".to_string()
                } else {
                    format!("Cambios frente a la versión {}:\n{d}", f.version)
                },
            )
        }
        None => (
            format!("la rutina nueva '{}' en la versión {}", c.slug, c.version),
            format!("Definición completa:\n{}", a.toml.trim_end()),
        ),
    };
    Ok((
        json!({ "toml": a.toml }),
        format!(
            "Registraría {que}: la guarda en SQLite y copia el TOML a rutinas_dir/{}.toml, como \
             `ea rutina registrar`. No lanza nada; la próxima ejecución usará esta versión. \
             Worktree: {}. Criterio que ejecutará el sistema (entradas por defecto): `{}`.\n\n{cambios}",
            c.slug,
            c.worktree,
            orden.criterio_exito.as_deref().unwrap_or_default()
        ),
        json!({ "slug": c.slug, "version_registrada": anterior.map(|f| f.version) }),
    ))
}

pub(super) fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    let (slug, version) = p.sistema.rutina_registrar(p.store, &a.toml, p.actor).map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: format!("Rutina '{slug}' registrada en la versión {version}."),
        trabajo: None,
        datos: json!({ "rutina": slug, "version": version }),
    })
}
