//! Trabajos del bus, sus colas y semáforos, y las decisiones del usuario
//! sobre ellos.

use std::collections::HashSet;

use ea_bus::{Bus, Rol, Evento, Modo, Orden, Topes};
use serde::Deserialize;
use serde_json::Value;

use super::{bus_con_store, bus_lectura, texto_no_vacio, ErrorWeb, Resultado, ACTOR};
use crate::vistas::{
    ColaEntrada, Colas, Diff, EstadoTrabajo, FaseVista, FormularioTrabajo, Semaforo, TrabajoDetalle, TrabajoResumen,
};
use crate::AppState;

fn a_valor<T: serde::Serialize>(v: T) -> Resultado<Value> {
    serde_json::to_value(v).map_err(|e| ErrorWeb::Interno(e.to_string()))
}

/// `en_curso`: trabajos cuyo worktree tiene cogido el agente local.
fn resumen(bus: &Bus, id: &str, en_curso: &HashSet<String>) -> Option<TrabajoResumen> {
    let orden = bus.leer_orden(id).ok()?;
    let estado = bus.leer_estado(id).ok()?;
    let informe = bus.leer_informe(id).ok().flatten();
    Some(TrabajoResumen {
        cancelando: estado.estado == ea_bus::EstadoTrabajo::Cancelado && en_curso.contains(id),
        id: id.into(),
        modo: orden.modo.as_str(),
        estado: estado.estado.as_str(),
        fase_actual: estado.fase_actual,
        fases_total: orden.fases.len(),
        iteracion: estado.iteracion,
        detalle: estado.detalle,
        resultado: informe.as_ref().map(|i| match i.resultado {
            ea_bus::ResultadoInforme::Terminado => "terminado",
            ea_bus::ResultadoInforme::Bloqueado => "bloqueado",
        }),
        obra_completa: informe.as_ref().map(|i| i.obra_completa),
        rutina: a_valor(&orden.rutina).ok()?,
        titulo: orden.titulo,
        worktree: orden.worktree,
        creado: orden.creada_en,
        actualizado: estado.actualizado_en,
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
    let dir = bus.raiz().join("trabajos").join(id);
    let historial = std::fs::read_to_string(dir.join("historial.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let paquete = std::fs::read_to_string(dir.join("paquete-correccion.md")).ok();
    let tipo = ea_bus::tipo_ejecutor(estado.ejecutado_por.as_ref());
    let resumen_agente = ea_bus::leer_salida_agente(&dir)
        .map(|b| ea_bus::resumen_salida_agente(tipo, &b))
        .map(|r| r.trim().chars().take(RESUMEN_MAX).collect::<String>())
        .filter(|r| !r.is_empty());
    Ok(TrabajoDetalle {
        id: id.into(),
        orden: a_valor(orden)?,
        estado: a_valor(estado)?,
        informe,
        historial,
        paquete,
        fases: vec![],
        resumen_agente,
    })
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
const WORKTREE_MAX: usize = 4_096;
const SUGERENCIAS_MAX: usize = 30;
const FASES_MAX: usize = 20;
const ENTREGABLE_MAX: usize = 2_000;

/// Cuerpo de `POST /api/trabajos`. Cualquier otro campo (`skills`,
/// `creada_por`…) se rechaza: desde la web no se eligen. La misión lleva
/// `criterio`; la obra, `fases` (cada una con el suyo) y `revisar_cada`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NuevoTrabajo {
    #[serde(default)]
    modo: Option<String>,
    titulo: String,
    worktree: String,
    objetivo: String,
    #[serde(default)]
    criterio: Option<String>,
    #[serde(default)]
    topes: Option<TopesPedidos>,
    #[serde(default)]
    fases: Option<Vec<FasePedida>>,
    /// Fuera: no se envió; `Some(None)`: `null`, solo al final;
    /// `Some(Some(n))`: cada `n` fases. En la obra es obligatorio (art. IV.4).
    #[serde(default, deserialize_with = "presente")]
    revisar_cada: Option<Option<u32>>,
}

/// Distingue un campo ausente (`None`, por el `default`) de uno a `null`.
fn presente<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<u32>>, D::Error> {
    Option::<u32>::deserialize(d)
        .map(Some)
        .map_err(|e| serde::de::Error::custom(format!("'revisar_cada' debe ser un entero positivo o null ({e})")))
}

/// Una fase de la obra tal como llega del formulario. El número lo pone el
/// orden de la lista; las skills no se eligen desde la web.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FasePedida {
    titulo: String,
    instrucciones: String,
    entregable: String,
    /// Vacío llega al bus, que lo rechaza («la fase N no tiene criterio»).
    #[serde(default)]
    criterio: String,
}

/// Los topes que se pueden pedir desde el formulario; el resto, por defecto.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct TopesPedidos {
    iteraciones: Option<u32>,
    tiempo_s: Option<u64>,
}

/// El techo de los topes de los ajustes efectivos (se relee en cada petición).
fn techo(s: &AppState) -> Topes {
    ea_bus::ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&s.config.ajustes).ajustes).techo_topes
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

/// Ruta absoluta, existente, directorio y dentro de un repo git; devuelve
/// la forma canónica, que es la que va a la orden (y al semáforo).
async fn validar_worktree(wt: &str) -> Resultado<String> {
    let wt = wt.trim();
    if wt.is_empty() {
        return Err(ErrorWeb::Invalido("'worktree' no puede estar vacío".into()));
    }
    if wt.len() > WORKTREE_MAX {
        return Err(ErrorWeb::Invalido(format!("'worktree' supera {WORKTREE_MAX} bytes")));
    }
    let ruta = std::path::Path::new(wt);
    if !ruta.is_absolute() {
        return Err(ErrorWeb::Invalido("'worktree' debe ser una ruta absoluta".into()));
    }
    let canon = std::fs::canonicalize(ruta)
        .map_err(|_| ErrorWeb::Invalido(format!("el worktree '{wt}' no existe")))?;
    if !canon.is_dir() {
        return Err(ErrorWeb::Invalido(format!("el worktree '{wt}' no es un directorio")));
    }
    let canon = canon
        .to_str()
        .ok_or_else(|| ErrorWeb::Invalido("la ruta del worktree no es UTF-8".into()))?
        .to_string();
    if !crate::git::es_repo(&canon).await? {
        return Err(ErrorWeb::Invalido(format!("'{canon}' no está dentro de un repositorio git")));
    }
    if !crate::git::tiene_commits(&canon).await? {
        return Err(ErrorWeb::Invalido(format!(
            "el repo no tiene ningún commit ('{canon}'): haz un commit inicial antes de encargarle trabajo"
        )));
    }
    Ok(canon)
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
pub async fn crear(s: &AppState, cuerpo: Value) -> Resultado<EstadoTrabajo> {
    if cuerpo.get("skills").is_some() {
        return Err(ErrorWeb::Invalido("las skills no se eligen desde la web".into()));
    }
    let n: NuevoTrabajo =
        serde_json::from_value(cuerpo).map_err(|e| ErrorWeb::Invalido(format!("petición inválida: {e}")))?;
    let modo = n.modo.as_deref().unwrap_or("mision");
    if !MODOS_WEB.contains(&modo) {
        return Err(ErrorWeb::Invalido(format!("modo '{modo}': desde la web solo se crean misiones y obras")));
    }
    let titulo = una_linea(&n.titulo, "titulo")?;
    let objetivo = texto_no_vacio(&n.objetivo, "objetivo", OBJETIVO_MAX)?;
    let (modo, criterio, fases, revisar_cada) = if modo == "obra" {
        let (fases, revisar_cada) = fases_de_obra(&n)?;
        (Modo::Obra, None, fases, revisar_cada)
    } else {
        if n.fases.is_some() || n.revisar_cada.is_some() {
            return Err(ErrorWeb::Invalido("'fases' y 'revisar_cada' son de las obras: usa modo 'obra'".into()));
        }
        let criterio = texto_no_vacio(n.criterio.as_deref().unwrap_or(""), "criterio", CRITERIO_MAX)?;
        (Modo::Mision, Some(criterio), vec![], None)
    };
    let pedidos = n.topes.unwrap_or_default();
    if pedidos.iteraciones == Some(0) || pedidos.tiempo_s == Some(0) {
        return Err(ErrorWeb::Invalido("los topes deben ser al menos 1".into()));
    }
    let worktree = validar_worktree(&n.worktree).await?;
    let d = Topes::default();
    let topes = Topes {
        iteraciones: pedidos.iteraciones.unwrap_or(d.iteraciones),
        tiempo_s: pedidos.tiempo_s.unwrap_or(d.tiempo_s),
        ..d
    }
    .acotar(&techo(s));
    let orden = Orden {
        id: String::new(),
        modo,
        titulo,
        worktree,
        objetivo,
        criterio_exito: criterio,
        fases,
        revisar_cada,
        topes,
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        creada_en: String::new(),
        creada_por: ACTOR.into(),
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
