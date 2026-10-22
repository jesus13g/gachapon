//! `ea ajustes`: ver, obtener y establecer los ajustes del ecosistema
//! (`ajustes.toml`). Establecer exige una terminal interactiva real: es el
//! único canal que puede tocar lo que afecta a la seguridad del agente local
//! (sandbox, binario de Qwen, rutas, quién elige skills) y subir los techos.

use anyhow::{bail, Context, Result};
use comfy_table::Table;
use ea_ajustes::{cargar_de, clave, establecer as escribir, interpretar, sin_alias, Canal, Fuentes, SECCIONES};
use ea_store::{EventOutcome, NewEvent, Store};
use serde_json::{json, Value};

const ACTOR: &str = "usuario";

/// Un valor tal como se escribiría en la terminal.
fn mostrar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(l) => l.iter().map(mostrar).collect::<Vec<_>>().join(","),
        otro => otro.to_string(),
    }
}

pub fn ver(json: bool) -> Result<()> {
    let f = Fuentes::sistema();
    let e = cargar_de(&f);
    if json {
        println!("{}", serde_json::to_string_pretty(&e.resumen(&f))?);
        return Ok(());
    }
    println!("Ajustes: {}{}", e.fichero.display(), if e.existe { "" } else { " (no existe: todo sale de otras capas)" });
    for (id, titulo) in SECCIONES {
        let mut t = Table::new();
        t.set_header(vec!["Clave", "Valor", "Origen", "Edición", "Aplica"]);
        for (nombre, c) in e.claves.iter().filter(|(_, c)| c.seccion == *id) {
            let origen = &e.origenes[nombre];
            let edicion = match origen {
                ea_ajustes::Origen::Entorno { variable } => format!("fijada por {variable}"),
                _ if c.endurecible && c.edicion == ea_ajustes::Edicion::Web => "web (solo bajar)".into(),
                _ => c.edicion.nombre().into(),
            };
            let aplica = match c.aplica {
                ea_ajustes::Aplica::EnCaliente => "en caliente".to_string(),
                ea_ajustes::Aplica::Reinicio(s) => format!("reiniciar {}", s.join(", ")),
            };
            t.add_row(vec![nombre.clone(), mostrar(&e.valores[nombre]), origen.legible(), edicion, aplica]);
        }
        println!("\n{titulo}\n{t}");
    }
    if !e.avisos.is_empty() {
        println!("\nAvisos:");
        for a in &e.avisos {
            println!("  ! {a}");
        }
    }
    Ok(())
}

pub fn obtener(nombre: &str) -> Result<()> {
    let nombre = sin_alias(nombre.trim());
    clave(nombre).with_context(|| format!("no existe el ajuste {nombre:?} (mira `ea ajustes ver`)"))?;
    let e = cargar_de(&Fuentes::sistema());
    let v = e.valores.get(nombre).with_context(|| format!("el perfil de {nombre:?} no existe o se ha descartado (mira los avisos de `ea ajustes ver`)"))?;
    println!("{}", mostrar(v));
    Ok(())
}

pub fn ruta() -> Result<()> {
    let f = Fuentes::sistema();
    let existe = |p: &std::path::Path| if p.exists() { "existe" } else { "no existe" };
    println!("ajustes.toml       {} ({})", f.fichero().display(), existe(&f.fichero()));
    for (nombre, ruta) in f.heredados() {
        println!("{:<18} {} ({}, heredado: solo lectura)", nombre, ruta.display(), existe(&ruta));
    }
    Ok(())
}

pub fn establecer(cambios: &[String], quitar: &[String]) -> Result<()> {
    // Lo primero: una persona delante de un teclado, no un agente con `ea`
    // como subproceso.
    ea_acciones::Canal::terminal_interactiva()?;
    if cambios.is_empty() && quitar.is_empty() {
        bail!("nada que cambiar: `ea ajustes establecer clave=valor ...` o `--quitar clave`");
    }
    let mut lote: Vec<(String, Option<Value>)> = Vec::new();
    for c in cambios {
        let (k, v) = c.split_once('=').with_context(|| format!("{c:?}: se espera clave=valor"))?;
        let k = sin_alias(k.trim());
        let def = clave(k).with_context(|| format!("no existe el ajuste {k:?} (mira `ea ajustes ver`)"))?;
        lote.push((k.to_string(), Some(interpretar(def, v)?)));
    }
    for k in quitar {
        lote.push((k.trim().to_string(), None));
    }
    let f = Fuentes::sistema();
    let aplicados = escribir(&f, Canal::Terminal, None, &lote)?;
    // Antes de imprimir: si la salida se corta, el diario ya lo tiene.
    apuntar(&f, &aplicados);
    for aviso in ea_ajustes::aplicar_efectos(&aplicados) {
        eprintln!("aviso: {aviso}");
    }
    let mut reiniciar: Vec<&str> = Vec::new();
    for a in &aplicados {
        println!("  {}: {} → {}", a.clave, mostrar(&a.antes), mostrar(&a.despues));
        for s in a.aplica.servicios() {
            if !reiniciar.contains(s) {
                reiniciar.push(s);
            }
        }
    }
    println!("Guardado en {}.", f.fichero().display());
    if !reiniciar.is_empty() {
        println!("Requiere reiniciar: {} (systemctl --user restart {})", reiniciar.join(", "), reiniciar.join(" "));
    }
    Ok(())
}

/// Al diario, si la base abre (mejor esfuerzo: el cambio ya está escrito).
pub(super) fn apuntar(f: &Fuentes, aplicados: &[ea_ajustes::Aplicado]) {
    let cambios: Vec<Value> =
        aplicados.iter().map(|a| json!({ "clave": a.clave, "antes": a.antes, "despues": a.despues })).collect();
    let detalle = json!({ "canal": "terminal", "fichero": f.fichero().display().to_string(), "cambios": cambios });
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: "ajustes_cambiados".into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(detalle.to_string()),
    };
    if let Err(e) = Store::open().map_err(anyhow::Error::from).and_then(|s| Ok(s.append_event(evento)?)) {
        eprintln!("aviso: no se pudo apuntar el cambio en el diario: {e}");
    }
}
