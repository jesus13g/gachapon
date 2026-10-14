//! `ea trabajo`, `ea cola` y `ea semaforos`: el bus del orquestador.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use ea_bus::{Agente, Bus, Evento, Modo, Orden, Topes};

pub(super) fn bus() -> Result<Bus> {
    let b = Bus::abrir()?.con_actor("ea-cli");
    Ok(match ea_store::Store::open() {
        Ok(s) => b.con_store(s),
        Err(_) => b,
    })
}

pub fn crear(
    fichero: Option<PathBuf>,
    modo: &str,
    titulo: Option<String>,
    worktree: Option<String>,
    objetivo: Option<String>,
    criterio: Option<String>,
) -> Result<()> {
    let orden: Orden = if let Some(f) = fichero {
        serde_json::from_str(&std::fs::read_to_string(&f)?).map_err(|e| anyhow!("orden inválida ({}): {e}", f.display()))?
    } else {
        let modo = match modo {
            "mision" => Modo::Mision,
            "obra" => Modo::Obra,
            "rutina" => Modo::Rutina,
            otro => bail!("modo desconocido: {otro} (mision, obra o rutina)"),
        };
        let worktree = worktree.ok_or_else(|| anyhow!("falta --worktree (o --orden)"))?;
        let worktree = std::fs::canonicalize(&worktree)?.to_string_lossy().into_owned();
        Orden {
            id: String::new(),
            modo,
            titulo: titulo.ok_or_else(|| anyhow!("falta --titulo"))?,
            worktree,
            objetivo: objetivo.ok_or_else(|| anyhow!("falta --objetivo"))?,
            criterio_exito: criterio,
            fases: vec![],
            revisar_cada: None,
            topes: Topes::default(),
            rutina: None,
            notas_revision: vec![],
            creada_en: String::new(),
            creada_por: "ea-cli".into(),
        }
    };
    println!("{}", bus()?.crear_trabajo(orden)?);
    Ok(())
}

pub fn listar() -> Result<()> {
    let b = Bus::abrir()?;
    for id in b.listar_trabajos()? {
        let (Ok(o), Ok(e)) = (b.leer_orden(&id), b.leer_estado(&id)) else { continue };
        println!("{id}  {:<12} {:<8} {}", e.estado.as_str(), o.modo.as_str(), o.titulo);
    }
    Ok(())
}

pub fn ver(id: &str) -> Result<()> {
    let b = Bus::abrir()?;
    println!("== orden ==\n{}", serde_json::to_string_pretty(&b.leer_orden(id)?)?);
    println!("== estado ==\n{}", serde_json::to_string_pretty(&b.leer_estado(id)?)?);
    match b.leer_informe(id)? {
        Some(i) => println!("== informe ==\n{}", serde_json::to_string_pretty(&i)?),
        None => println!("== informe ==\n(todavía no hay)"),
    }
    Ok(())
}

pub fn evento(id: &str, ev: Evento) -> Result<()> {
    let nombre = ev.nombre();
    let e = bus()?.aplicar(id, ev)?;
    println!("{id}: {nombre} -> {}", e.estado.as_str());
    Ok(())
}

pub fn reformular(id: &str, nota: &str) -> Result<()> {
    let e = bus()?.reformular(id, nota)?;
    println!("{id}: reformular -> {}", e.estado.as_str());
    Ok(())
}

pub fn confirmar(id: &str, contexto: &str) -> Result<()> {
    let e = bus()?.confirmar(id, contexto)?;
    println!("{id}: confirmar -> {} (encolado en claude-code)", e.estado.as_str());
    Ok(())
}

pub fn cola(agente: Option<&str>) -> Result<()> {
    let b = Bus::abrir()?;
    let agentes = match agente {
        None => vec![Agente::ModeloLocal, Agente::ClaudeCode],
        Some("modelo-local") => vec![Agente::ModeloLocal],
        Some("claude-code") => vec![Agente::ClaudeCode],
        Some(otro) => bail!("agente desconocido: {otro} (modelo-local o claude-code)"),
    };
    for a in agentes {
        println!("[{}]", a.as_str());
        for e in b.listar_cola(a)? {
            println!("  {}  {}  ({})", e.encolado_en, e.trabajo, e.motivo);
        }
    }
    Ok(())
}

pub fn semaforos() {
    let v = ea_bus::listar_semaforos();
    if v.is_empty() {
        println!("(ningún worktree ocupado)");
    }
    for (_, o) in v {
        println!("{}  trabajo {}  pid {}  desde {}", o.agente, o.trabajo, o.pid, o.desde);
    }
}
