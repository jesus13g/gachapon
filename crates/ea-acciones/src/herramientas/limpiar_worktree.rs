//! `ea_limpiar_worktree` (spec 019, anexo A): quitar el worktree propio de un
//! trabajo terminado y limpio, sin `--force`; la rama se conserva.

use serde::Deserialize;
use serde_json::json;

use super::{campo, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

pub(super) const CAMPOS: &[Campo] = &[campo("trabajo", "id del trabajo con worktree propio")];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    trabajo: String,
}

pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    if a.trabajo.len() <= 64 && ea_bus::id_valido(&a.trabajo) {
        Ok(())
    } else {
        Err(ErrorAccion::Argumentos(format!("id de trabajo inválido: '{}'", a.trabajo)))
    }
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    let no = |m: String| Err(ErrorAccion::NoAplica(m));
    let Some(w) = p.consulta.worktree_info(&a.trabajo).map_err(ErrorAccion::Sistema)? else {
        return no(format!("el trabajo {} no tiene worktree propio vivo (nada que limpiar)", a.trabajo));
    };
    if !w.trabajo_terminado() {
        return no(format!(
            "el trabajo {} no ha terminado (estado: {}): solo se limpia un trabajo cerrado o cancelado",
            a.trabajo,
            w.estado_trabajo.map_or("desconocido", ea_bus::EstadoTrabajo::as_str)
        ));
    }
    if w.ocupado {
        return no(format!("el ejecutor tiene cogido el semáforo del trabajo {}: espera a que suelte", a.trabajo));
    }
    match w.limpio {
        Some(true) => {}
        Some(false) => {
            return no(format!(
                "el worktree {} tiene cambios sin guardar o ficheros nuevos: nunca se fuerza. Guárdalos \
                 (git add -A && git commit en el worktree, y fusiona la rama {} si los quieres) o descártalos \
                 tú a mano, y vuelve a simular",
                w.ruta, w.rama
            ))
        }
        None => return no(format!("git no pudo decir si {} está limpio; revísalo a mano", w.ruta)),
    }
    Ok((
        json!({ "trabajo": a.trabajo }),
        format!(
            "Quitaría el worktree propio del trabajo {} ({}) con `git worktree remove`, sin --force; está \
             limpio y el trabajo está {}. La rama {} se conserva (borrarla o fusionarla es cosa tuya) y el \
             directorio se vuelve a sacar de ella si hiciera falta.",
            a.trabajo,
            w.ruta,
            w.estado_trabajo.map_or("terminado", ea_bus::EstadoTrabajo::as_str),
            w.rama
        ),
        json!({ "ruta": w.ruta, "rama": w.rama, "estado_trabajo": w.estado_trabajo }),
    ))
}

pub(super) fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    let ruta = p.precondicion["ruta"].as_str().unwrap_or_default().to_string();
    let habia = p.sistema.worktree_limpiar(&a.trabajo).map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: if habia {
            format!("Worktree del trabajo {} quitado; la rama ea/{} sigue en el repo.", a.trabajo, a.trabajo)
        } else {
            format!("El trabajo {} ya no tenía worktree.", a.trabajo)
        },
        trabajo: Some(a.trabajo.clone()),
        datos: json!({ "trabajo": a.trabajo, "ruta": ruta, "rama": format!("ea/{}", a.trabajo), "quitado": habia }),
    })
}
