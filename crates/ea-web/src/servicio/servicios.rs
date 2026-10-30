//! Los servicios residentes: el plan de `ea servicios ver` y el reinicio
//! (spec 020, H4). Instalar y desinstalar **no** se hacen desde aquí: exigen
//! una terminal real (art. IV.2) y la web solo la abre
//! ([`super::terminal`]).

use ea_acciones::servicios::{Backend, Entorno, plan_instalar};
use ea_acciones::{Canal, GestorServicios, SERVICIOS_INSTALABLES};
use serde_json::json;

use super::{ACTOR, ErrorWeb, Resultado, sistema};
use crate::AppState;
use crate::vistas::{Hecho, PlanServicios, ServicioPlan};

/// Solo lectura: no cambia nada. Si el gestor no responde (o no se consulta,
/// en las pruebas), cada servicio sale `desconocido`.
pub async fn plan(s: &AppState) -> Resultado<PlanServicios> {
    let env = Entorno::usuario().map_err(ErrorWeb::NoDisponible)?;
    let pasos = plan_instalar(&env).map_err(ErrorWeb::NoDisponible)?;
    let g = GestorServicios::usuario();
    let mut servicios = Vec::new();
    for nombre in SERVICIOS_INSTALABLES {
        let estado = if s.config.systemctl { g.estado(nombre).await.ok() } else { None };
        let binario = env.exe(nombre);
        servicios.push(ServicioPlan {
            nombre,
            unidad: env.unidad(nombre),
            existe: binario.is_file(),
            binario: binario.display().to_string(),
            instalado: estado.as_ref().map(|e| e.cargado),
            activo: match &estado {
                Some(e) if e.cargado => e.activo.clone(),
                _ => "desconocido".into(),
            },
        });
    }
    Ok(PlanServicios {
        sistema: ea_plataforma::Sistema::actual().nombre(),
        backend: env.backend.nombre(),
        gestor: env.backend.descripcion(),
        binarios: env.exes_dir.display().to_string(),
        unidades: env.unidades_dir.display().to_string(),
        faltan: env.faltan().iter().map(|p| p.display().to_string()).collect(),
        servicios,
        pasos: pasos.iter().map(|p| p.describir()).collect(),
        path_fijo: env.backend == Backend::Launchd && env.path.is_some(),
    })
}

pub async fn reiniciar(s: &AppState, nombre: &str) -> Resultado<Hecho> {
    if !ea_acciones::salud::SERVICIOS.contains(&nombre) {
        return Err(ErrorWeb::NoEncontrado("servicio desconocido".into()));
    }
    if !s.config.systemctl {
        return Err(ErrorWeb::NoDisponible("la consulta al gestor de servicios está desactivada".into()));
    }

    let sis = sistema(s);
    let mut store = s.acciones.lock().await;
    ea_acciones::ejecutar_directo(&mut store, &sis, ACTOR, Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": nombre })).await?;
    Ok(Hecho { ok: true })
}
