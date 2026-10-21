//! `ea agentes`: el catálogo de agentes con el rol que ocupa cada uno, su
//! binario, su versión y si responde su modelo (spec 017). `ea agentes rol`
//! es un atajo de `ea ajustes establecer roles.<rol>=<agente>`, con la misma
//! exigencia de terminal interactiva.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use comfy_table::Table;
use ea_ajustes::{Rol, IDS_AGENTES};
use ea_web::agentes::{disponibilidad, AgenteDisponible};

const LIMITE_VERSION: Duration = Duration::from_secs(5);

pub fn listar(json: bool) -> Result<()> {
    let ajustes = ea_ajustes::cargar();
    let agentes = super::bloquear(disponibilidad(&ajustes));
    if json {
        println!("{}", serde_json::to_string_pretty(&agentes)?);
        return Ok(());
    }
    let mut t = Table::new();
    t.set_header(vec!["Agente", "Rol", "Admite", "Binario", "Versión", "Modelo"]);
    for a in &agentes {
        let v = &a.vista;
        let binario = match &v.ruta {
            Some(r) => corta_home(r),
            None => format!("{} (no está)", v.bin),
        };
        let version = v.ruta.as_deref().and_then(version).unwrap_or_else(|| "—".into());
        t.add_row(vec![
            format!("{} ({})", v.nombre, v.id),
            v.rol.unwrap_or("—").to_string(),
            v.admite.join(", "),
            binario,
            version,
            modelo(a),
        ]);
    }
    println!("{t}");
    println!("Cambiar de agente: ea agentes rol <planificador|ejecutor> <agente>");
    Ok(())
}

/// `$HOME/...` como `~/...`, para que la tabla quepa.
fn corta_home(ruta: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && ruta.starts_with(&format!("{h}/")) => format!("~{}", &ruta[h.len()..]),
        _ => ruta.to_string(),
    }
}

/// El modelo de su perfil y si su endpoint responde.
fn modelo(a: &AgenteDisponible) -> String {
    let Some(m) = &a.vista.modelo else {
        return "el suyo propio".into();
    };
    let donde = if m.remoto {
        if m.base_url.is_empty() { "remoto".to_string() } else { format!("remoto, {}", m.base_url) }
    } else {
        m.base_url.clone()
    };
    let estado = match a.modelo_accesible {
        Some(true) => " · responde",
        Some(false) => " · no responde",
        None => "",
    };
    format!("{} [{}] ({donde}){estado}", m.nombre, a.vista.perfil)
}

/// La primera línea de `<bin> --version`, si sale antes del límite. No
/// arranca ninguna sesión del agente: solo pregunta la versión.
fn version(bin: &str) -> Option<String> {
    let mut hijo = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let inicio = Instant::now();
    loop {
        match hijo.try_wait().ok()? {
            Some(_) => break,
            None if inicio.elapsed() > LIMITE_VERSION => {
                let _ = hijo.kill();
                let _ = hijo.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let salida = hijo.wait_with_output().ok()?;
    let texto = String::from_utf8_lossy(&salida.stdout);
    texto.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

pub fn rol(rol: &str, agente: &str) -> Result<()> {
    let Some(rol) = Rol::desde(rol) else {
        bail!("rol desconocido {rol:?}: planificador o ejecutor");
    };
    if !IDS_AGENTES.contains(&agente) {
        bail!("agente desconocido {agente:?}: {}", IDS_AGENTES.join(", "));
    }
    super::ajustes::establecer(&[format!("roles.{}={agente}", rol.nombre())], &[])
}
