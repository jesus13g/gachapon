//! La lógica de cada herramienta mutante supervisada: validar argumentos,
//! simular (describir sin hacer), revalidar contra la precondición y aplicar.
//! Es la "capa interna" de §7.3; el catálogo y las definiciones MCP solo
//! nombran estas herramientas.

use ea_store::{NewTask, Store, Task, TaskFilter, TaskOrigin, TaskStatus};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::catalogo::{self, Canal, Ejecucion, Herramienta, Riesgo};
use crate::error::ErrorAccion;
use crate::sistema::{slug_valido, Consulta, EstadoServicio, Sistema, SERVICIOS_REINICIABLES};

/// Qué haría una herramienta con unos argumentos, sin hacerlo.
#[derive(Debug, Clone)]
pub struct Simulacion {
    pub herramienta: &'static str,
    pub riesgo: Riesgo,
    /// Argumentos validados y normalizados (sin nulos): lo que se ejecutará.
    pub argumentos: Value,
    /// Descripción en español de lo que ocurriría.
    pub texto: String,
    /// Estado del mundo del que depende la acción; se revalida al ejecutar.
    pub precondicion: Value,
}

/// Resultado de aplicar una acción.
#[derive(Debug, Clone)]
pub struct Aplicada {
    pub texto: String,
    pub tarea: Option<i64>,
    pub datos: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CambiarEstado {
    id: i64,
    estado: String,
    #[serde(default)]
    resultado: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorSlug {
    slug: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorServicio {
    servicio: String,
}

fn parsear<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, ErrorAccion> {
    serde_json::from_value(args.clone()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
}

pub fn supervisada(nombre: &str) -> Result<&'static Herramienta, ErrorAccion> {
    let h = catalogo::buscar(nombre).ok_or_else(|| ErrorAccion::Desconocida(nombre.to_string()))?;
    if h.ejecucion != Ejecucion::Supervisada {
        return Err(ErrorAccion::NoSupervisada(nombre.to_string()));
    }
    Ok(h)
}

/// FNV-1a de 64 bits: huella estable (no depende de la versión de Rust) del
/// contenido de una rutina, para detectar que cambió desde la simulación.
fn huella(texto: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in texto.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn tarea_activa_de_rutina(store: &Store, slug: &str) -> Result<Option<Task>, ErrorAccion> {
    for estado in [TaskStatus::Pendiente, TaskStatus::EnProgreso] {
        let tareas = store.list_tasks(&TaskFilter {
            status: Some(estado),
            assigned_to: Some("orquestador".into()),
            limit: None,
        })?;
        for t in tareas {
            let rutina = serde_json::from_str::<Value>(&t.request)
                .ok()
                .and_then(|v| v["rutina"].as_str().map(str::to_string));
            if rutina.as_deref() == Some(slug) {
                return Ok(Some(t));
            }
        }
    }
    Ok(None)
}

fn comprobar_servicio(nombre: &str) -> Result<(), ErrorAccion> {
    if SERVICIOS_REINICIABLES.contains(&nombre) {
        Ok(())
    } else {
        Err(ErrorAccion::Argumentos(format!(
            "servicio '{nombre}' fuera de la lista cerrada ({})",
            SERVICIOS_REINICIABLES.join(", ")
        )))
    }
}

fn comprobar_slug(slug: &str) -> Result<(), ErrorAccion> {
    if slug_valido(slug) {
        Ok(())
    } else {
        Err(ErrorAccion::Argumentos(format!("slug de rutina inválido: '{slug}'")))
    }
}

/// Validación estructural de unos argumentos, sin mirar el estado del
/// mundo: la herramienta existe y es supervisada, la forma es la esperada y
/// los valores están en su dominio. La usa la carga de rutinas, para
/// rechazar al cargar una rutina mal escrita (no a las 3 de la mañana).
pub fn validar_argumentos(nombre: &str, args: &Value) -> Result<(), ErrorAccion> {
    supervisada(nombre)?;
    match nombre {
        "ea_cambiar_estado_tarea" => {
            let a: CambiarEstado = parsear(args)?;
            a.estado
                .parse::<TaskStatus>()
                .map(|_| ())
                .map_err(|_| ErrorAccion::Argumentos(format!("estado '{}' inválido", a.estado)))
        }
        "ea_encolar_rutina" | "ea_eliminar_rutina" => comprobar_slug(&parsear::<PorSlug>(args)?.slug),
        "ea_reiniciar_servicio" => comprobar_servicio(&parsear::<PorServicio>(args)?.servicio),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Lo que hay que preguntarle al sistema de forma asíncrona antes de
/// simular. Se obtiene con [`observar`] **sin** sostener la conexión a la
/// base de datos (que no se comparte entre hilos); luego [`simular_con`] es
/// síncrona.
#[derive(Debug, Default, Clone)]
pub struct Hechos {
    pub servicio: Option<EstadoServicio>,
}

/// Consulta al sistema lo que la simulación de `nombre` necesita.
pub async fn observar<C: Consulta>(consulta: &C, nombre: &str, args: &Value) -> Result<Hechos, ErrorAccion> {
    let mut h = Hechos::default();
    if nombre == "ea_reiniciar_servicio" {
        if let Some(srv) = args["servicio"].as_str().filter(|s| SERVICIOS_REINICIABLES.contains(s)) {
            h.servicio = Some(consulta.servicio_estado(srv).await.map_err(ErrorAccion::Sistema)?);
        }
    }
    Ok(h)
}

/// Valida los argumentos y describe lo que haría la herramienta, leyendo el
/// estado actual y sin escribir nada (ni en la base de datos ni en el
/// sistema). Falla con `NoAplica` si la acción no tendría sentido ahora.
pub fn simular_con<C: Consulta>(
    store: &Store,
    consulta: &C,
    hechos: &Hechos,
    nombre: &str,
    args: &Value,
) -> Result<Simulacion, ErrorAccion> {
    let h = supervisada(nombre)?;
    let (argumentos, texto, precondicion) = match nombre {
        "ea_cambiar_estado_tarea" => {
            let a: CambiarEstado = parsear(args)?;
            let nuevo: TaskStatus = a.estado.parse().map_err(|_| {
                ErrorAccion::Argumentos(format!(
                    "estado '{}' inválido (pendiente, en_progreso, completada, fallida, cancelada)",
                    a.estado
                ))
            })?;
            let tarea = store
                .get_task(a.id)
                .map_err(|_| ErrorAccion::NoAplica(format!("la tarea #{} no existe", a.id)))?;
            if tarea.status == nuevo {
                return Err(ErrorAccion::NoAplica(format!(
                    "la tarea #{} ya está en estado '{}'",
                    a.id,
                    nuevo.as_str()
                )));
            }
            let resultado = a.resultado.filter(|r| !r.trim().is_empty());
            let mut texto = format!(
                "Cambiaría la tarea #{} «{}» de '{}' a '{}'.",
                tarea.id,
                tarea.title,
                tarea.status.as_str(),
                nuevo.as_str()
            );
            if let Some(r) = &resultado {
                texto.push_str(&format!(" Resultado que se anotaría: «{r}»."));
            }
            let mut norm = json!({ "id": a.id, "estado": nuevo.as_str() });
            if let Some(r) = resultado {
                norm["resultado"] = json!(r);
            }
            (norm, texto, json!({ "estado_actual": tarea.status.as_str() }))
        }
        "ea_encolar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            consulta
                .rutina_contenido(&a.slug)
                .map_err(ErrorAccion::Sistema)?
                .ok_or_else(|| ErrorAccion::NoAplica(format!("la rutina '{}' no existe", a.slug)))?;
            if let Some(t) = tarea_activa_de_rutina(store, &a.slug)? {
                return Err(ErrorAccion::NoAplica(format!(
                    "la rutina '{}' ya tiene una ejecución en cola (tarea #{}, {})",
                    a.slug,
                    t.id,
                    t.status.as_str()
                )));
            }
            (
                json!({ "slug": a.slug }),
                format!(
                    "Crearía una tarea para el orquestador que ejecuta la rutina '{}' (de solo \
                     lectura; lo que necesite mutar quedará como propuestas).",
                    a.slug
                ),
                json!({ "tareas_activas": 0 }),
            )
        }
        "ea_reiniciar_servicio" => {
            let a: PorServicio = parsear(args)?;
            comprobar_servicio(&a.servicio)?;
            let estado = hechos.servicio.clone().ok_or_else(|| ErrorAccion::Sistema("servicio no observado".into()))?;
            if !estado.cargado {
                return Err(ErrorAccion::NoAplica(format!(
                    "la unidad {}.service no está instalada en systemd --user",
                    a.servicio
                )));
            }
            (
                json!({ "servicio": a.servicio }),
                format!(
                    "Reiniciaría {}.service (ahora: {}). Las conexiones y tareas en curso de ese \
                     servicio se interrumpirían unos segundos.",
                    a.servicio, estado.activo
                ),
                json!({ "cargado": true }),
            )
        }
        "ea_eliminar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            let contenido = consulta
                .rutina_contenido(&a.slug)
                .map_err(ErrorAccion::Sistema)?
                .ok_or_else(|| ErrorAccion::NoAplica(format!("la rutina '{}' no existe", a.slug)))?;
            (
                json!({ "slug": a.slug }),
                format!(
                    "Borraría PARA SIEMPRE el fichero de la rutina '{}' ({} bytes, {} líneas). \
                     No hay copia ni papelera. No desactiva su temporizador de systemd, si lo hay.",
                    a.slug,
                    contenido.len(),
                    contenido.lines().count()
                ),
                json!({ "huella": huella(&contenido), "bytes": contenido.len() }),
            )
        }
        otro => return Err(ErrorAccion::NoSupervisada(otro.to_string())),
    };
    Ok(Simulacion { herramienta: h.nombre, riesgo: h.riesgo, argumentos, texto, precondicion })
}

/// Comprueba que el mundo sigue como en la simulación; si no, falla con un
/// mensaje claro en vez de hacer otra cosa.
pub fn revalidar_con<C: Consulta>(
    store: &Store,
    consulta: &C,
    hechos: &Hechos,
    nombre: &str,
    args: &Value,
    precondicion: &Value,
) -> Result<(), ErrorAccion> {
    // Primero la validación completa de la simulación (la tarea existe, la
    // rutina existe, la unidad está instalada…), con el estado de ahora.
    let ahora = simular_con(store, consulta, hechos, nombre, args).map_err(|e| match e {
        ErrorAccion::NoAplica(m) => ErrorAccion::NoAplica(format!("el estado cambió desde la simulación: {m}")),
        otro => otro,
    })?;
    // Y después, que lo que dependía el mundo siga igual.
    for (clave, esperado) in precondicion.as_object().into_iter().flatten() {
        let actual = &ahora.precondicion[clave];
        if actual != esperado {
            return Err(ErrorAccion::NoAplica(format!(
                "el estado cambió desde la simulación: {clave} era {esperado} y ahora es {actual}"
            )));
        }
    }
    Ok(())
}

/// Aplica la parte de la acción que escribe en la base de datos. Devuelve
/// `None` si la herramienta actúa sobre el sistema (ver [`aplicar_en_sistema`]).
pub fn aplicar_en_store(
    store: &mut Store,
    actor: &str,
    canal: Canal,
    nombre: &str,
    args: &Value,
) -> Result<Option<Aplicada>, ErrorAccion> {
    match nombre {
        "ea_cambiar_estado_tarea" => {
            let a: CambiarEstado = parsear(args)?;
            let nuevo: TaskStatus = a.estado.parse().map_err(|_| ErrorAccion::Argumentos(a.estado.clone()))?;
            let resultado = a.resultado.filter(|r| !r.trim().is_empty());
            let t = store.update_task_status(a.id, nuevo, actor, resultado)?;
            Ok(Some(Aplicada {
                texto: format!("Tarea #{} ahora '{}'.", t.id, t.status.as_str()),
                tarea: Some(t.id),
                datos: json!({ "tarea": t.id, "estado": t.status.as_str() }),
            }))
        }
        "ea_encolar_rutina" => {
            let a: PorSlug = parsear(args)?;
            let origin = match canal {
                Canal::Web => TaskOrigin::Boton,
                Canal::Terminal => TaskOrigin::Manual,
            };
            let t = store.create_task(NewTask {
                created_by: actor.to_string(),
                assigned_to: "orquestador".into(),
                origin,
                parent_task_id: None,
                title: format!("Ejecutar rutina: {}", a.slug),
                request: json!({ "rutina": a.slug }).to_string(),
            })?;
            Ok(Some(Aplicada {
                texto: format!("Tarea #{} encolada para la rutina '{}'.", t.id, a.slug),
                tarea: Some(t.id),
                datos: json!({ "tarea": t.id, "rutina": a.slug }),
            }))
        }
        "ea_reiniciar_servicio" | "ea_eliminar_rutina" => Ok(None),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Aplica la parte de la acción que actúa sobre el sistema operativo.
pub async fn aplicar_en_sistema<S: Sistema>(
    sistema: &S,
    nombre: &str,
    args: &Value,
) -> Result<Aplicada, ErrorAccion> {
    match nombre {
        "ea_reiniciar_servicio" => {
            let a: PorServicio = parsear(args)?;
            sistema.servicio_reiniciar(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
            let ahora = sistema.servicio_estado(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("{}.service reiniciado (ahora: {}).", a.servicio, ahora.activo),
                tarea: None,
                datos: json!({ "servicio": a.servicio, "activo": ahora.activo }),
            })
        }
        "ea_eliminar_rutina" => {
            let a: PorSlug = parsear(args)?;
            sistema.rutina_eliminar(&a.slug).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("Rutina '{}' eliminada.", a.slug),
                tarea: None,
                datos: json!({ "rutina": a.slug }),
            })
        }
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}
