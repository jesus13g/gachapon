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

pub fn ver(id: &str, json: bool) -> Result<()> {
    let b = Bus::abrir()?;
    let orden = b.leer_orden(id)?;
    let estado = b.leer_estado(id)?;
    let informe = b.leer_informe(id)?;
    if json {
        let v = serde_json::json!({ "orden": orden, "estado": estado, "informe": informe });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("trabajo   {id}");
    println!("titulo    {}", orden.titulo);
    println!("modo      {}", orden.modo.as_str());
    println!("estado    {}", estado.estado.as_str());
    let fase = estado.fase_actual.map(|f| format!(" · fase {f}/{}", orden.fases.len())).unwrap_or_default();
    println!("iteracion {}{fase}", estado.iteracion);
    if let Some(d) = &estado.detalle {
        println!("detalle   {d}");
    }
    println!("worktree  {}", orden.worktree);
    if let Some(c) = &orden.criterio_exito {
        println!("criterio  {c}");
    }
    if let Some(r) = orden.revisar_cada {
        println!("revisar_cada {r}");
    }
    if !orden.notas_revision.is_empty() {
        println!("notas de revision: {}", orden.notas_revision.len());
    }
    let Some(i) = informe else {
        println!("informe   (todavia no hay)");
        return Ok(());
    };
    println!("--- informe ---");
    println!("resultado     {:?}", i.resultado);
    println!("obra_completa {}", i.obra_completa);
    println!("fases_hechas  {}  ·  duracion {}s", i.fases_hechas, i.duracion_s);
    if let Some(m) = &i.motivo {
        println!("motivo        {m}");
    }
    if let Some(c) = &i.commit_inicio {
        println!("commit_inicio {c}");
    }
    for f in &i.fases {
        println!("fase {} [{}] {}", f.numero, f.resultado, f.titulo);
    }
    println!("evidencia:\n{}", i.evidencia.trim_end());
    if let Some(d) = &i.diff_stat {
        println!("diff stat:\n{}", d.trim_end());
    }
    println!("(detalle: ea trabajo diff {id} [--fase N] · todo: ea trabajo ver {id} --json)");
    Ok(())
}

fn git(wt: &str, args: &[&str]) -> Result<String> {
    let o = std::process::Command::new("git").arg("-C").arg(wt).args(args).output()?;
    if !o.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn diff(id: &str, fase: Option<u32>) -> Result<()> {
    let b = Bus::abrir()?;
    let orden = b.leer_orden(id)?;
    let wt = orden.worktree.as_str();
    let inicio = b
        .leer_informe(id)?
        .and_then(|i| i.commit_inicio)
        .ok_or_else(|| anyhow!("el trabajo {id} no tiene informe con commit_inicio"))?;
    let excl = ":(exclude,glob)**/__pycache__/**";
    if let Some(n) = fase {
        // Árboles guardados por la obra en el informe de cada fase (SQLite).
        let store = ea_store::Store::open().map_err(|e| anyhow!("sin acceso al store: {e}"))?;
        let fases = store.listar_fases(id)?;
        let arbol = |num: i64| -> Option<String> {
            let f = fases.iter().find(|f| f.numero == num)?;
            let v: serde_json::Value = serde_json::from_str(f.informe.as_deref()?).ok()?;
            v.get("arbol")?.as_str().map(String::from)
        };
        let Some(fin) = arbol(i64::from(n)) else {
            bail!("no hay instantánea de árbol de la fase {n} (la obra no la guardó o el store no la tiene)");
        };
        let base = if n <= 1 { format!("{inicio}^{{tree}}") } else { arbol(i64::from(n) - 1).ok_or_else(|| anyhow!("no hay instantánea de la fase {}", n - 1))? };
        print!("{}", git(wt, &["diff", &base, &fin, "--", ".", excl])?);
        return Ok(());
    }
    print!("{}", git(wt, &["diff", &inicio, "--", ".", excl])?);
    let nuevos = git(wt, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for f in nuevos.split('\0').filter(|f| !f.is_empty() && !f.contains("__pycache__")) {
        let ruta = std::path::Path::new(wt).join(f);
        match std::fs::read(&ruta) {
            Ok(bytes) if std::str::from_utf8(&bytes).is_ok() => {
                println!("\n=== fichero nuevo sin seguimiento: {f} ===\n{}", String::from_utf8_lossy(&bytes))
            }
            _ => println!("\n=== fichero nuevo sin seguimiento: {f} (binario o ilegible) ==="),
        }
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

pub fn cola(agente: Option<&str>, json: bool) -> Result<()> {
    let b = Bus::abrir()?;
    let agentes = match agente {
        None => vec![Agente::ModeloLocal, Agente::ClaudeCode],
        Some("modelo-local") => vec![Agente::ModeloLocal],
        Some("claude-code") => vec![Agente::ClaudeCode],
        Some(otro) => bail!("agente desconocido: {otro} (modelo-local o claude-code)"),
    };
    if json {
        let mut m = serde_json::Map::new();
        for a in agentes {
            let v: Vec<_> = b
                .listar_cola(a)?
                .into_iter()
                .map(|e| serde_json::json!({ "id": e.id, "trabajo": e.trabajo, "motivo": e.motivo, "encolado_en": e.encolado_en }))
                .collect();
            m.insert(a.as_str().into(), v.into());
        }
        println!("{}", serde_json::to_string_pretty(&m)?);
        return Ok(());
    }
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
