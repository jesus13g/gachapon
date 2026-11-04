//! `ea_lanzar_sesion` (spec 018, F5): abrir en una terminal un agente con
//! solo las herramientas elegidas de la biblioteca.

use serde::Deserialize;
use serde_json::json;

use super::{Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular, campo, huella};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

pub(super) const CAMPOS: &[Campo] = &[
    Campo { opciones: &ea_biblioteca::AGENTES_CON_SESION, ..campo("agente", "agente que se lanza") },
    Campo { largo: true, ..campo("herramientas", "ids de la biblioteca (ea biblioteca listar), uno por línea") },
    Campo { obligatorio: false, ..campo("directorio", "carpeta del proyecto, ruta absoluta (opcional)") },
];

const MAX_HERRAMIENTAS_SESION: usize = 40;

/// Los ids: una lista, o un texto con ids separados por espacios, comas o
/// saltos de línea (el formulario de la web).
#[derive(Deserialize)]
#[serde(untagged)]
enum IdsSesion {
    Lista(Vec<String>),
    Texto(String),
}

/// El agente, las herramientas de la biblioteca (ids) y, si se quiere, la
/// carpeta del proyecto.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    agente: String,
    herramientas: IdsSesion,
    #[serde(default)]
    directorio: Option<String>,
}

impl Args {
    fn ids(&self) -> Vec<String> {
        match &self.herramientas {
            IdsSesion::Lista(l) => l.iter().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()).collect(),
            IdsSesion::Texto(t) => t.split(|c: char| c.is_whitespace() || c == ',').filter(|i| !i.is_empty()).map(str::to_string).collect(),
        }
    }

    fn directorio(&self) -> Option<&str> {
        self.directorio.as_deref().map(str::trim).filter(|d| !d.is_empty())
    }
}

/// Validación sin mirar el mundo: forma y tamaños. Que los ids existan y
/// tengan sentido lo decide el plan de la biblioteca.
pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    if !ea_ajustes::IDS_AGENTES.contains(&a.agente.as_str()) {
        return Err(ErrorAccion::Argumentos(format!("agente desconocido '{}' (uno de: {})", a.agente, ea_ajustes::IDS_AGENTES.join(", "))));
    }
    let ids = a.ids();
    if ids.is_empty() {
        return Err(ErrorAccion::Argumentos("hay que elegir al menos una herramienta de la biblioteca".into()));
    }
    if ids.len() > MAX_HERRAMIENTAS_SESION {
        return Err(ErrorAccion::Argumentos(format!("demasiadas herramientas ({}; como mucho {MAX_HERRAMIENTAS_SESION})", ids.len())));
    }
    if let Some(i) = ids.iter().find(|i| i.len() > 300 || i.chars().any(char::is_control)) {
        return Err(ErrorAccion::Argumentos(format!("id de herramienta no válido: {i:?}")));
    }
    if let Some(d) = a.directorio()
        && (d.len() > 1024 || d.chars().any(char::is_control))
    {
        return Err(ErrorAccion::Argumentos("directorio no válido".into()));
    }
    Ok(())
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    let plan = p.consulta.sesion_plan(&a.agente, &a.ids(), a.directorio()).map_err(ErrorAccion::NoAplica)?;
    // Siempre con su carpeta: la pedida o, si no, la que se resolvió
    // (el proyecto de quien simula, o el HOME). Así quien la acepte,
    // desde otro proceso y otro directorio, planea lo mismo que se vio.
    let argumentos = json!({
        "agente": plan.agente, "herramientas": plan.herramientas, "directorio": plan.carpeta().to_string_lossy(),
    });
    let ficheros: Vec<String> = plan
        .ficheros
        .iter()
        .map(|f| f.ruta.strip_prefix(&plan.directorio).unwrap_or(&f.ruta).display().to_string())
        .chain(plan.enlaces.iter().map(|e| format!("{} (enlace)", e.ruta.strip_prefix(&plan.directorio).unwrap_or(&e.ruta).display())))
        .collect();
    let entorno =
        if plan.entorno.is_empty() { "ninguna".to_string() } else { plan.entorno.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ") };
    let texto = format!(
        "Abriría una terminal con {} y solo estas {} herramientas de la biblioteca: {}. Comando (sin shell): {}. \
         Escribe en {} (0700) los ficheros {} con modo 0600 —las definiciones completas de los MCP, con sus \
         secretos, se releen de su fuente solo al aceptar y solo van a esos ficheros—; caducan a los 7 días. \
         Variables de entorno: {entorno}. No cambia la configuración de ningún agente.",
        plan.agente,
        plan.herramientas.len(),
        plan.herramientas.join(", "),
        plan.comando(),
        plan.directorio.display(),
        if ficheros.is_empty() { "(ninguno)".to_string() } else { ficheros.join(", ") },
    );
    let huella = huella(&serde_json::to_string(&plan).map_err(|e| ErrorAccion::Sistema(e.to_string()))?);
    Ok((argumentos, texto, json!({ "plan": huella })))
}

pub(super) fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    validar(&a)?;
    let plan = p.sistema.sesion_plan(&a.agente, &a.ids(), a.directorio()).map_err(ErrorAccion::NoAplica)?;
    p.sistema.sesion_lanzar(&plan).map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: format!("Sesión de {} abierta en una terminal con {} herramientas de la biblioteca ({}).", plan.agente, plan.herramientas.len(), plan.id),
        trabajo: None,
        // Sin secretos: ids y argv (rutas de ficheros 0600, nunca su contenido).
        datos: json!({
            "sesion": plan.id, "agente": plan.agente, "herramientas": plan.herramientas,
            "argv": plan.argv_terminal(),
        }),
    })
}
