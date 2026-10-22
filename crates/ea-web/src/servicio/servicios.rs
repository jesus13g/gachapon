//! Los servicios residentes: el plan de `ea servicios ver` y el reinicio
//! (spec 020, H4). Instalar y desinstalar **no** se hacen desde aquí: exigen
//! una terminal real (art. IV.2) y la web solo la abre
//! ([`super::terminal`]).

use ea_acciones::servicios::{plan_instalar, Backend, Entorno};
use ea_acciones::{Canal, GestorServicios, SERVICIOS_INSTALABLES};
use serde::Serialize;
use serde_json::json;

use super::{sistema, ErrorWeb, Resultado, ACTOR};
use crate::vistas::Hecho;
use crate::AppState;

#[derive(Serialize)]
pub struct ServicioPlan {
    pub nombre: &'static str,
    /// La unidad en el gestor (`ea-web.service`, `es.gachapon.ea-web.plist`
    /// o la tarea `ecosistema-asistente\ea-web`).
    pub unidad: String,
    /// La ruta real del binario que arrancaría.
    pub binario: String,
    pub existe: bool,
    /// `None` si no se pudo preguntar al gestor.
    pub instalado: Option<bool>,
    /// `active`, `inactive`, `failed`… o `desconocido`.
    pub activo: String,
}

/// Lo que cuenta `ea servicios ver`, sin `PATH` ni secretos.
#[derive(Serialize)]
pub struct PlanServicios {
    pub sistema: &'static str,
    /// `systemd`, `launchd` o `schtasks`.
    pub backend: &'static str,
    /// El gestor, en una frase.
    pub gestor: &'static str,
    pub binarios: String,
    pub unidades: String,
    /// Los binarios que faltan junto a este (`ea` incluido).
    pub faltan: Vec<String>,
    pub servicios: Vec<ServicioPlan>,
    /// Qué haría `instalar`, paso a paso.
    pub pasos: Vec<String>,
    /// launchd fija el `PATH` de quien instala (el valor no se enseña).
    pub path_fijo: bool,
}

fn gestor(b: Backend) -> &'static str {
    match b {
        Backend::Systemd => "systemd --user (unidades en ~/.config/systemd/user)",
        Backend::Launchd => "launchd (LaunchAgents en ~/Library/LaunchAgents)",
        Backend::Schtasks => "Programador de tareas de Windows (tareas «ecosistema-asistente\\…» al iniciar sesión)",
    }
}

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
        backend: match env.backend {
            Backend::Systemd => "systemd",
            Backend::Launchd => "launchd",
            Backend::Schtasks => "schtasks",
        },
        gestor: gestor(env.backend),
        binarios: env.exes_dir.display().to_string(),
        unidades: env.unidades_dir.display().to_string(),
        faltan: env.faltan().iter().map(|p| p.display().to_string()).collect(),
        servicios,
        pasos: pasos.iter().map(|p| p.describir()).collect(),
        path_fijo: env.backend == Backend::Launchd && env.path.is_some(),
    })
}

pub async fn reiniciar(s: &AppState, nombre: &str) -> Resultado<Hecho> {
    if !crate::estado::SERVICIOS.contains(&nombre) {
        return Err(ErrorWeb::NoEncontrado("servicio desconocido".into()));
    }
    if !s.config.systemctl {
        return Err(ErrorWeb::NoDisponible("la consulta al gestor de servicios está desactivada".into()));
    }

    let sis = sistema(s);
    let mut store = s.acciones.lock().await;
    ea_acciones::ejecutar_directo(
        &mut store, &sis, ACTOR, Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": nombre }),
    )
    .await?;
    Ok(Hecho { ok: true })
}
