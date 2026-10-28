//! Trabajos del bus, sus colas y semáforos, y las decisiones del usuario
//! sobre ellos.

use std::collections::HashSet;

use ea_bus::{Bus, Rol, Evento, Orden, Topes};
use serde_json::Value;

use super::{bus_con_store, bus_lectura, texto_no_vacio, ErrorWeb, Resultado, ACTOR};
use crate::vistas::{
    CatalogoSkills, ColaEntrada, Colas, Diff, EstadoTrabajo, FaseVista, FormularioTrabajo, NuevoTrabajo, PruebaSkills, Semaforo,
    SkillVista, TrabajoDetalle, TrabajoResumen,
};
use crate::AppState;

fn a_valor<T: serde::Serialize>(v: T) -> Resultado<Value> {
    serde_json::to_value(v).map_err(|e| ErrorWeb::Interno(e.to_string()))
}

/// `en_curso`: trabajos cuyo worktree tiene cogido el agente local.
fn resumen(bus: &Bus, id: &str, en_curso: &HashSet<String>) -> Option<TrabajoResumen> {
    let r = bus.resumen_trabajo(id)?;
    Some(TrabajoResumen {
        cancelando: r.estado == ea_bus::EstadoTrabajo::Cancelado && en_curso.contains(id),
        modo: r.modo.as_str(),
        estado: r.estado.as_str(),
        fase_actual: r.fase_actual,
        fases_total: r.fases_total,
        iteracion: r.iteracion,
        detalle: r.detalle,
        ejecutado_por: r.ejecutado_por,
        resultado: r.informe.as_ref().map(|i| i.resultado.as_str()),
        obra_completa: r.informe.as_ref().map(|i| i.obra_completa),
        rutina: a_valor(&r.rutina).ok()?,
        id: r.id,
        titulo: r.titulo,
        worktree: r.worktree,
        creado: r.creada_en,
        actualizado: r.actualizado_en,
    })
}

/// El más reciente primero.
pub fn listar(s: &AppState) -> Resultado<Vec<TrabajoResumen>> {
    let bus = bus_lectura(s)?;
    // Un trabajo cancelado cuyo semáforo sigue cogido es que el ejecutor aún
    // no se ha enterado (sondea cada 2 s y espera al agente): «cancelando…».
    let en_curso: HashSet<String> = ea_bus::listar_semaforos_en(bus.raiz())
        .into_iter()
        .filter(|(_, o)| o.es_ejecutor())
        .map(|(_, o)| o.trabajo)
        .collect();
    let mut v: Vec<_> = bus.listar_trabajos()?.iter().filter_map(|id| resumen(&bus, id, &en_curso)).collect();
    v.reverse();
    Ok(v)
}

/// El resumen del agente se recorta a esto (caracteres).
const RESUMEN_MAX: usize = 8_000;

pub async fn ver(s: &AppState, id: &str) -> Resultado<TrabajoDetalle> {
    let mut d = ver_bus(s, id)?;
    d.fases = s
        .store
        .lock()
        .await
        .listar_fases(id)?
        .into_iter()
        .map(|f| FaseVista {
            diff_stat: ea_bus::diff_stat_de_fase(&f),
            instantanea: u32::try_from(f.numero).is_ok_and(|n| ea_bus::arbol_de_fase(std::slice::from_ref(&f), n).is_some()),
            numero: f.numero,
            titulo: f.titulo,
            estado: f.estado,
            cerrado_en: f.cerrado_en,
        })
        .collect();
    Ok(d)
}

/// Lo que sale de los ficheros del bus (sin las fases de SQLite).
fn ver_bus(s: &AppState, id: &str) -> Resultado<TrabajoDetalle> {
    let bus = bus_lectura(s)?;
    let orden = bus.leer_orden(id)?;
    let estado = bus.leer_estado(id)?;
    let mut informe = a_valor(bus.leer_informe(id)?)?;
    if let Some(o) = informe.as_object_mut() {
        o.remove("diff"); // el diff va aparte, bajo demanda
    }
    let artefactos = bus.artefactos(id)?;
    let historial = artefactos.historial();
    let paquete = artefactos.paquete_correccion();
    let tipo = ea_bus::tipo_ejecutor(estado.ejecutado_por.as_ref());
    let resumen_agente = artefactos
        .salida_agente()
        .map(|b| ea_bus::resumen_salida_agente(tipo, &b))
        .map(|r| r.trim().chars().take(RESUMEN_MAX).collect::<String>())
        .filter(|r| !r.is_empty());
    let worktree = if orden.worktree_propio { bus.info_worktree(id).ok().flatten() } else { None };
    Ok(TrabajoDetalle {
        id: id.into(),
        orden: a_valor(orden)?,
        estado: a_valor(estado)?,
        informe,
        historial,
        paquete,
        fases: vec![],
        resumen_agente,
        worktree,
    })
}

/// Los worktrees propios vivos (`ea worktree listar`). Solo lectura; quitarlos
/// es `ea_limpiar_worktree` por `/api/acciones`, con simulación y clave.
pub fn worktrees(s: &AppState) -> Resultado<Vec<ea_bus::InfoWorktree>> {
    Ok(bus_lectura(s)?.listar_worktrees()?)
}

const DIFF_MAX: usize = 200_000;

/// Lo mismo que `ea trabajo diff [--fase N]`. Sin fase: lo cambiado desde
/// `commit_inicio` más los ficheros nuevos sin seguimiento. Con fase: entre
/// la instantánea de árbol de la fase anterior (o `commit_inicio` para la
/// primera) y la suya (`ea_bus::rango_diff_fase`); sin instantánea, 404.
/// Recortado a `DIFF_MAX` bytes. Solo lectura.
pub async fn diff(s: &AppState, id: &str, fase: Option<u32>) -> Resultado<Diff> {
    let bus = bus_lectura(s)?;
    let orden = bus.leer_orden(id)?;
    let inicio = bus.leer_informe(id)?.and_then(|i| i.commit_inicio);
    if let Some(n) = fase {
        let fases = s.store.lock().await.listar_fases(id)?;
        let (base, fin) = ea_bus::rango_diff_fase(inicio.as_deref(), &fases, n)?;
        let (texto, truncado) = crate::git::diff_entre(&orden.worktree, &base, &fin, DIFF_MAX).await?;
        return Ok(Diff { id: id.into(), fase: Some(n), commit_inicio: inicio, diff: texto, truncado });
    }
    let inicio =
        inicio.ok_or_else(|| ErrorWeb::Conflicto("el trabajo aún no tiene informe con commit_inicio".into()))?;
    let (texto, truncado) = crate::git::diff_desde(&orden.worktree, &inicio, DIFF_MAX).await?;
    Ok(Diff { id: id.into(), fase: None, commit_inicio: Some(inicio), diff: texto, truncado })
}

// ------------------------------------------------------------------- crear

/// Los modos que se crean desde la web: misiones y obras, por el mismo
/// formulario y la misma ruta. Las rutinas se lanzan desde su ficha.
const MODOS_WEB: [&str; 2] = ["mision", "obra"];
const TITULO_MAX: usize = 200;
const OBJETIVO_MAX: usize = 20_000;
const CRITERIO_MAX: usize = 2_000;
const SUGERENCIAS_MAX: usize = 30;
const FASES_MAX: usize = 20;
const ENTREGABLE_MAX: usize = 2_000;

/// Vacío o solo espacios cuenta como «no enviado» (los selectores del
/// formulario mandan `""`).
fn opcional(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// El techo de los topes de los ajustes efectivos (se relee en cada petición).
fn techo(s: &AppState) -> Topes {
    Topes::from(&ea_ajustes::cargar_de(&s.config.ajustes).ajustes.agente.techo_topes)
}

/// Lo que el formulario necesita: techo, topes por defecto y los worktrees
/// que se sugieren (los `cwd` de las sesiones vivas de Claude Code y los de
/// los trabajos anteriores, del más reciente al más antiguo, sin repetir).
pub fn formulario(s: &AppState) -> Resultado<FormularioTrabajo> {
    let techo = techo(s);
    let mut worktrees: Vec<String> = super::estado::sesiones(s).sesiones.into_iter().map(|x| x.cwd).collect();
    worktrees.extend(listar(s)?.into_iter().map(|t| t.worktree));
    let mut vistos = std::collections::HashSet::new();
    worktrees.retain(|w| !w.is_empty() && vistos.insert(w.clone()));
    worktrees.truncate(SUGERENCIAS_MAX);
    Ok(FormularioTrabajo { modos: MODOS_WEB.to_vec(), techo, defecto: Topes::default().acotar(&techo), worktrees })
}

/// La configuración de skills de los ajustes efectivos; el catálogo por
/// defecto cuelga del directorio de configuración de los ajustes del panel.
fn config_skills(s: &AppState) -> ea_bus::ConfigSkills {
    let mut c = ea_bus::ConfigSkills::desde_ajustes(&ea_ajustes::cargar_de(&s.config.ajustes).ajustes);
    if c.dir.is_empty() {
        c.dir = s.config.ajustes.dir_config.join("qwen-skills").to_string_lossy().into_owned();
    }
    c
}

/// El catálogo de skills del ejecutor, sus topes y la llave (solo lectura).
pub fn catalogo_skills(s: &AppState) -> Resultado<CatalogoSkills> {
    let c = config_skills(s);
    let skills = c
        .catalogo()?
        .into_iter()
        .map(|k| SkillVista {
            caracteres: k.cuerpo.chars().count(),
            nombre: k.nombre,
            descripcion: k.descripcion,
            habilitada: k.habilitada,
        })
        .collect();
    Ok(CatalogoSkills {
        skills,
        max_por_trabajo: c.max_por_trabajo,
        max_caracteres: c.max_caracteres,
        usuario_tiene_llave: c.tiene_llave(ACTOR),
        pueden_elegir: c.pueden_elegir,
    })
}

/// Comprueba una selección con la misma validación que `ea qwen-skills
/// probar` (`ConfigSkills::resolver`): llave de `por` (por defecto, el actor
/// de la web), existencia, habilitada y topes. No escribe nada.
pub fn probar_skills(s: &AppState, skills: &[String], por: Option<&str>) -> Resultado<PruebaSkills> {
    let por = por.map(str::trim).filter(|p| !p.is_empty()).unwrap_or(ACTOR);
    let elegidas = config_skills(s).resolver(por, skills)?;
    Ok(PruebaSkills {
        por: por.into(),
        caracteres: elegidas.iter().map(|k| k.cuerpo.chars().count()).sum(),
        skills: elegidas.into_iter().map(|k| k.nombre).collect(),
    })
}

/// Texto no vacío de una sola línea, de hasta `TITULO_MAX` caracteres.
fn una_linea(valor: &str, campo: &str) -> Resultado<String> {
    let v = texto_no_vacio(valor, campo, TITULO_MAX)?;
    if v.chars().any(char::is_control) {
        return Err(ErrorWeb::Invalido(format!("'{campo}' debe ser una sola línea")));
    }
    Ok(v)
}

/// Las fases y `revisar_cada` de una obra. Sin criterio general (cada fase
/// lleva el suyo); `revisar_cada` lo elige el usuario expresamente: `null`
/// (solo al final) o entre 1 y el número de fases, nunca un valor oculto.
/// Que cada fase tenga criterio lo comprueba el bus antes de escribir nada.
fn fases_de_obra(n: &NuevoTrabajo) -> Resultado<(Vec<ea_bus::Fase>, Option<u32>)> {
    if n.criterio.as_deref().is_some_and(|c| !c.trim().is_empty()) {
        return Err(ErrorWeb::Invalido("en una obra el criterio va en cada fase, no en 'criterio'".into()));
    }
    let pedidas = n.fases.as_deref().unwrap_or_default();
    if pedidas.is_empty() {
        return Err(ErrorWeb::Invalido("una obra necesita al menos una fase en 'fases'".into()));
    }
    if pedidas.len() > FASES_MAX {
        return Err(ErrorWeb::Invalido(format!("una obra tiene como mucho {FASES_MAX} fases")));
    }
    let revisar_cada = n.revisar_cada.ok_or_else(|| {
        ErrorWeb::Invalido("elige 'revisar_cada': cada cuántas fases revisa Claude Code (null: solo al final)".into())
    })?;
    if let Some(k) = revisar_cada
        && (k == 0 || k as usize > pedidas.len())
    {
        return Err(ErrorWeb::Invalido(format!(
            "'revisar_cada' debe estar entre 1 y {} (o null: solo al final)",
            pedidas.len()
        )));
    }
    let mut fases = Vec::with_capacity(pedidas.len());
    for (i, f) in pedidas.iter().enumerate() {
        let numero = i as u32 + 1;
        let campo = |c: &str| format!("fase {numero}: {c}");
        if f.criterio.chars().count() > CRITERIO_MAX {
            return Err(ErrorWeb::Invalido(format!("'{}' supera {CRITERIO_MAX} caracteres", campo("criterio"))));
        }
        fases.push(ea_bus::Fase {
            numero,
            titulo: una_linea(&f.titulo, &campo("titulo"))?,
            instrucciones: texto_no_vacio(&f.instrucciones, &campo("instrucciones"), OBJETIVO_MAX)?,
            entregable: texto_no_vacio(&f.entregable, &campo("entregable"), ENTREGABLE_MAX)?,
            criterio: f.criterio.trim().to_string(),
            skills: vec![],
        });
    }
    Ok((fases, revisar_cada))
}

/// Crea una misión o una obra encargada por el usuario desde la página. Todo se valida
/// antes de tocar el bus; después, `Bus::crear_trabajo` (que vuelve a
/// exigir el criterio y valida skills) la escribe, la encola en
/// `modelo-local` y apunta `bus_crear` en el diario con actor `usuario`.
/// El criterio no se ejecuta aquí: solo el ejecutor lo corre (arts. III y V).
pub async fn crear(s: &AppState, n: NuevoTrabajo) -> Resultado<EstadoTrabajo> {
    if n.skills.is_some() {
        return Err(ErrorWeb::Invalido(
            "las skills no se eligen desde la web: las elige quien tiene la llave (`pueden_elegir`, el rol planificador); prueba una selección con POST /api/skills/probar".into(),
        ));
    }
    let modo = n.modo.as_deref().unwrap_or("mision");
    if !MODOS_WEB.contains(&modo) {
        return Err(ErrorWeb::Invalido(format!("modo '{modo}': desde la web solo se crean misiones y obras")));
    }
    let titulo = una_linea(&n.titulo, "titulo")?;
    let objetivo = texto_no_vacio(&n.objetivo, "objetivo", OBJETIVO_MAX)?;
    // Misión u obra, sin el worktree todavía (se valida después, con git).
    let base = if modo == "obra" {
        let (fases, revisar_cada) = fases_de_obra(&n)?;
        Orden::obra(titulo, String::new(), objetivo, fases, revisar_cada, ACTOR)
    } else {
        if n.fases.is_some() || n.revisar_cada.is_some() {
            return Err(ErrorWeb::Invalido("'fases' y 'revisar_cada' son de las obras: usa modo 'obra'".into()));
        }
        let criterio = texto_no_vacio(n.criterio.as_deref().unwrap_or(""), "criterio", CRITERIO_MAX)?;
        Orden::mision(titulo, String::new(), objetivo, criterio, ACTOR)
    };
    let pedidos = n.topes.unwrap_or_default();
    if pedidos.iteraciones == Some(0) || pedidos.tiempo_s == Some(0) {
        return Err(ErrorWeb::Invalido("los topes deben ser al menos 1".into()));
    }
    let worktree = crate::git::validar_worktree(&n.worktree).await?;
    let d = Topes::default();
    let topes = Topes {
        iteraciones: pedidos.iteraciones.unwrap_or(d.iteraciones),
        tiempo_s: pedidos.tiempo_s.unwrap_or(d.tiempo_s),
        ..d
    }
    .acotar(&techo(s));
    let orden = Orden {
        worktree,
        topes,
        equipo: opcional(&n.equipo),
        asignado_a: opcional(&n.asignado_a),
        revisor: opcional(&n.revisor),
        worktree_propio: n.worktree_propio,
        ..base
    };
    let id = bus_con_store(s).await?.crear_trabajo(orden)?;
    Ok(EstadoTrabajo { id, estado: ea_bus::EstadoTrabajo::EnCola.as_str() })
}

/// Lo que el usuario decide sobre un trabajo entregado o en espera.
pub enum Decision<'a> {
    Cerrar,
    Cancelar,
    /// Aprueba una revisión intermedia: reformular sin nota (como `ea trabajo continuar`).
    Continuar,
    /// `fase`: en una obra, la fase desde la que se rehace (por defecto, la pendiente).
    Reformular { nota: &'a str, fase: Option<u32> },
    /// Confirma el paquete de corrección de una rutina en espera.
    Confirmar { contexto: &'a str },
}

pub async fn decidir(s: &AppState, id: &str, d: Decision<'_>) -> Resultado<EstadoTrabajo> {
    let e = match d {
        Decision::Cerrar => bus_con_store(s).await?.aplicar(id, Evento::Cerrar)?,
        Decision::Cancelar => bus_con_store(s).await?.aplicar(id, Evento::Cancelar)?,
        Decision::Continuar => bus_con_store(s).await?.reformular(id, "", None)?,
        Decision::Reformular { nota, fase } => {
            let nota = texto_no_vacio(nota, "nota", 10_000)?;
            bus_con_store(s).await?.reformular(id, &nota, fase)?
        }
        Decision::Confirmar { contexto } => {
            if contexto.chars().count() > 10_000 {
                return Err(ErrorWeb::Invalido("'contexto' supera 10000 caracteres".into()));
            }
            bus_con_store(s).await?.confirmar(id, contexto)?
        }
    };
    Ok(EstadoTrabajo { id: id.into(), estado: e.estado.as_str() })
}

pub fn colas(s: &AppState) -> Resultado<Colas> {
    let bus = bus_lectura(s)?;
    let mut m = Colas::new();
    for a in Rol::TODOS {
        let v = bus
            .listar_cola(a)?
            .into_iter()
            .map(|e| ColaEntrada {
                titulo: bus.leer_orden(&e.trabajo).map(|o| o.titulo).ok(),
                id: e.id,
                trabajo: e.trabajo,
                motivo: e.motivo,
                encolado_en: e.encolado_en,
            })
            .collect();
        m.insert(a.as_str(), v);
    }
    Ok(m)
}

pub fn semaforos(s: &AppState) -> Vec<Semaforo> {
    ea_bus::listar_semaforos_en(&s.config.estado_dir)
        .into_iter()
        .map(|(_, o)| Semaforo { agente: o.agente, trabajo: o.trabajo, pid: o.pid, desde: o.desde })
        .collect()
}
