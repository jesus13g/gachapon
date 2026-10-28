//! `ea trabajo`, `ea cola` y `ea semaforos`: el bus del orquestador.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use ea_bus::{Bus, Rol, Evento, Modo, Orden};

use super::{limpio, Contexto};

/// El bus con historial si la base abre (sin ella, sigue): el de `ea equipo`.
pub(super) fn bus() -> Result<Bus> {
    let rutas = ea_ajustes::cargar().rutas;
    let b = Bus::abrir_en(&rutas.estado)?;
    Ok(match ea_store::Store::open_at(&rutas.db) {
        Ok(s) => b.con_store(s, super::actor()),
        Err(_) => b,
    })
}

/// Lo que `ea trabajo crear` añade para un equipo (spec 019).
#[derive(Debug, Default)]
pub struct DeEquipo {
    pub equipo: Option<String>,
    pub asignar: Option<String>,
    pub revisor: Option<String>,
    pub worktree_propio: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn crear(
    ctx: &mut Contexto,
    fichero: Option<PathBuf>,
    modo: &str,
    titulo: Option<String>,
    worktree: Option<String>,
    objetivo: Option<String>,
    criterio: Option<String>,
    skills: Vec<String>,
    por: String,
    de_equipo: DeEquipo,
) -> Result<()> {
    let mut orden: Orden = if let Some(f) = fichero {
        serde_json::from_str(&std::fs::read_to_string(&f)?).map_err(|e| anyhow!("orden inválida ({}): {e}", f.display()))?
    } else {
        let modo = match modo {
            "mision" => Modo::Mision,
            "obra" => Modo::Obra,
            "rutina" => Modo::Rutina,
            otro => bail!("modo desconocido: {otro} (mision, obra o rutina)"),
        };
        let worktree = worktree.ok_or_else(|| anyhow!("falta --worktree (o --orden)"))?;
        // Aquí una ruta relativa lo es al directorio actual (`--worktree .`
        // desde el repo): se hace absoluta antes de validarla.
        let worktree = std::path::absolute(&worktree).map(|p| p.to_string_lossy().into_owned()).unwrap_or(worktree);
        let titulo = titulo.ok_or_else(|| anyhow!("falta --titulo"))?;
        let objetivo = objetivo.ok_or_else(|| anyhow!("falta --objetivo"))?;
        Orden {
            modo,
            skills,
            equipo: de_equipo.equipo,
            asignado_a: de_equipo.asignar,
            revisor: de_equipo.revisor,
            worktree_propio: de_equipo.worktree_propio,
            // Sin `--criterio`, el bus la rechaza: el criterio es obligatorio.
            ..Orden::mision(titulo, worktree, objetivo, criterio.unwrap_or_default(), por)
        }
    };
    // Como en la web: ruta absoluta, existente, repo git con commits. La orden
    // lleva la forma canónica (la del semáforo y el ejecutor). La estructura
    // (fases 1..N) y los criterios los valida el bus al crear.
    orden.worktree = ea_bus::validar_worktree(&orden.worktree, ea_bus::git::TOPE_LECTURA)?;
    println!("{}", ctx.bus()?.crear_trabajo(orden)?);
    Ok(())
}

pub fn listar() -> Result<()> {
    let b = Bus::abrir()?;
    for id in b.listar_trabajos()? {
        let (Ok(o), Ok(e)) = (b.leer_orden(&id), b.leer_estado(&id)) else { continue };
        println!("{id}  {:<12} {:<8} {}", e.estado.as_str(), o.modo.as_str(), limpio(&o.titulo));
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
    // Lo que no es del sistema (lo escriben el planificador, el ejecutor o su
    // criterio) pasa por `limpio`: sin secuencias de control en la terminal.
    println!("trabajo   {id}");
    println!("titulo    {}", limpio(&orden.titulo));
    println!("modo      {}", orden.modo.as_str());
    println!("estado    {}", estado.estado.as_str());
    let fase = estado.fase_actual.map(|f| format!(" · fase {f}/{}", orden.fases.len())).unwrap_or_default();
    println!("iteracion {}{fase}", estado.iteracion);
    if let Some(d) = &estado.detalle {
        println!("detalle   {}", limpio(d));
    }
    if let Some(e) = &estado.ejecutado_por {
        println!("ejecutor  {} ({})", limpio(&e.agente), limpio(&e.modelo));
    }
    println!("worktree  {}{}", limpio(&orden.worktree), if orden.worktree_propio { " (propio: se crea un git worktree para el trabajo)" } else { "" });
    if orden.worktree_propio {
        match b.info_worktree(id)? {
            Some(w) => println!(
                "propio    {} · rama {} · {}",
                w.ruta,
                w.rama,
                match w.limpio {
                    Some(true) => "limpio",
                    Some(false) => "con cambios sin guardar",
                    None => "estado desconocido",
                }
            ),
            None => println!("propio    (sin directorio: aún no se ha creado o ya se limpió; la rama ea/{id} se conserva si existe)"),
        }
    }
    if let Some(e) = &orden.equipo {
        println!("equipo    {}", limpio(e));
    }
    if let Some(a) = &orden.asignado_a {
        println!("asignado  {}", limpio(a));
    }
    if let Some(r) = &orden.revisor {
        println!("revisor   {}", limpio(r));
    }
    if let Some(c) = &orden.criterio_exito {
        println!("criterio  {}", limpio(c));
    }
    if !orden.skills.is_empty() {
        println!("skills    {}", limpio(&orden.skills.join(", ")));
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
        println!("motivo        {}", limpio(m));
    }
    if let Some(c) = &i.commit_inicio {
        println!("commit_inicio {}", limpio(c));
    }
    if let Some(e) = &i.ejecutado_por {
        println!("ejecutado_por {} ({})", limpio(&e.agente), limpio(&e.modelo));
    }
    if let Some(a) = &i.aviso {
        println!("aviso         {}", limpio(a));
    }
    for f in &i.fases {
        let por = f.ejecutado_por.as_ref().map(|e| format!("  · {} ({})", e.agente, e.modelo)).unwrap_or_default();
        println!("{}", limpio(&format!("fase {} [{}] {}{por}", f.numero, f.resultado.as_str(), f.titulo)));
    }
    println!("evidencia:\n{}", limpio(i.evidencia.trim_end()));
    if let Some(d) = &i.diff_stat {
        println!("diff stat:\n{}", limpio(d.trim_end()));
    }
    println!("(detalle: ea trabajo diff {id} [--fase N] · todo: ea trabajo ver {id} --json)");
    Ok(())
}

/// Lo cambiado desde `commit_inicio` y los ficheros nuevos sin seguimiento,
/// o el diff de una fase, con el `git` endurecido de `ea-bus`.
pub fn diff(id: &str, fase: Option<u32>) -> Result<()> {
    use ea_bus::git::{self, TOPE_LECTURA};
    let b = Bus::abrir()?;
    let orden = b.leer_orden(id)?;
    let wt = std::path::Path::new(&orden.worktree);
    let inicio = b.leer_informe(id)?.and_then(|i| i.commit_inicio);
    if let Some(n) = fase {
        // Árboles guardados por la obra en el informe de cada fase (SQLite);
        // el rango lo calcula `ea-bus`, igual que para `ea-web`.
        let store = ea_store::Store::open_at(&ea_ajustes::cargar().rutas.db).map_err(|e| anyhow!("sin acceso al store: {e}"))?;
        let fases = store.listar_fases(id)?;
        let (base, fin) = ea_bus::rango_diff_fase(inicio.as_deref(), &fases, n)?;
        print!("{}", git::diff_entre(wt, &base, &fin, false, TOPE_LECTURA)?);
        return Ok(());
    }
    let inicio = inicio.ok_or_else(|| anyhow!("el trabajo {id} no tiene informe con commit_inicio"))?;
    print!("{}", git::diff_desde(wt, &inicio, TOPE_LECTURA)?);
    for f in git::ficheros_nuevos(wt, TOPE_LECTURA)? {
        println!("{}", f.bloque());
    }
    Ok(())
}

pub fn evento(ctx: &mut Contexto, id: &str, ev: Evento) -> Result<()> {
    let nombre = ev.nombre();
    let e = ctx.bus()?.aplicar(id, ev)?;
    println!("{id}: {nombre} -> {}", e.estado.as_str());
    Ok(())
}

pub fn reformular(ctx: &mut Contexto, id: &str, nota: &str, fase: Option<u32>) -> Result<()> {
    let e = ctx.bus()?.reformular(id, nota, fase)?;
    match e.fase_actual.filter(|_| fase.is_some()) {
        Some(n) => println!("{id}: reformular desde la fase {n} -> {}", e.estado.as_str()),
        None => println!("{id}: reformular -> {}", e.estado.as_str()),
    }
    Ok(())
}

pub fn confirmar(ctx: &mut Contexto, id: &str, contexto: &str) -> Result<()> {
    let e = ctx.bus()?.confirmar(id, contexto)?;
    println!("{id}: confirmar -> {} (encolado en claude-code)", e.estado.as_str());
    Ok(())
}

pub fn cola(agente: Option<&str>, json: bool) -> Result<()> {
    let b = Bus::abrir()?;
    let agentes = match agente {
        None => Rol::TODOS.to_vec(),
        Some(nombre) => match Rol::desde(nombre) {
            Some(rol) => vec![rol],
            None => bail!("rol desconocido: {nombre} (planificador o ejecutor; también claude-code o modelo-local)"),
        },
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
        println!("[{} · cola {}]", a.nombre(), a.as_str());
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
