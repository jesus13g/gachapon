//! La lógica de cada herramienta mutante supervisada: validar argumentos,
//! simular (describir sin hacer), revalidar contra la precondición y aplicar.
//! Es la "capa interna" de §7.3; el catálogo y las definiciones MCP solo
//! nombran estas herramientas.

use ea_store::{Store, TaskStatus};
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromoverNota {
    ruta: String,
    #[serde(default)]
    destino: Option<String>,
}

/// Valida solo el texto de las rutas de una promoción (sin mirar el disco).
fn comprobar_promocion(a: &PromoverNota) -> Result<(), ErrorAccion> {
    let partes = ea_boveda::ruta::validar_ruta(&a.ruta)?;
    if partes[0] == ea_boveda::ruta::REGION_COMPARTIDA {
        return Err(ErrorAccion::Argumentos(format!("{} ya está en la región compartida", a.ruta)));
    }
    if let Some(d) = a.destino.as_deref().filter(|d| !d.trim().is_empty()) {
        ea_boveda::ruta::en_region(ea_boveda::ruta::REGION_COMPARTIDA, d)?;
    }
    Ok(())
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

/// Bus del orquestador sobre la raíz de estado estándar (o `EA_ESTADO_DIR`).
fn bus_de(store: &Store, actor: &str) -> Result<ea_bus::Bus, ErrorAccion> {
    let err = |e: &dyn std::fmt::Display| ErrorAccion::Sistema(e.to_string());
    let bus = ea_bus::Bus::abrir().map_err(|e| err(&e))?;
    Ok(bus.con_store(store.duplicar()?).con_actor(actor))
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
        "ea_promover_nota" => comprobar_promocion(&parsear::<PromoverNota>(args)?),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Un argumento de una herramienta supervisada, descrito para quien tenga
/// que pedírselo a una persona (el formulario de la web).
#[derive(Debug, Clone, Copy)]
pub struct Campo {
    pub nombre: &'static str,
    pub entero: bool,
    pub obligatorio: bool,
    /// Dominio cerrado de valores; vacío si es libre.
    pub opciones: &'static [&'static str],
    pub ayuda: &'static str,
}

const ESTADOS_TAREA: &[&str] = &["pendiente", "en_progreso", "completada", "fallida", "cancelada"];

const fn campo(nombre: &'static str, ayuda: &'static str) -> Campo {
    Campo { nombre, entero: false, obligatorio: true, opciones: &[], ayuda }
}

const CAMPOS_ESTADO: &[Campo] = &[
    Campo { entero: true, ..campo("id", "número de la tarea") },
    Campo { opciones: ESTADOS_TAREA, ..campo("estado", "estado nuevo") },
    Campo { obligatorio: false, ..campo("resultado", "resultado (opcional)") },
];
const CAMPOS_SLUG: &[Campo] = &[campo("slug", "rutina")];
const CAMPOS_SERVICIO: &[Campo] =
    &[Campo { opciones: &SERVICIOS_REINICIABLES, ..campo("servicio", "servicio") }];
const CAMPOS_PROMOVER: &[Campo] = &[
    campo("ruta", "ruta de la nota (p. ej. claude-code/tema/nota.md)"),
    Campo { obligatorio: false, ..campo("destino", "destino en «compartida» (opcional)") },
];

/// Los argumentos que acepta una herramienta supervisada, en el orden en que
/// conviene pedirlos. Debe casar con las estructuras de arriba (lo vigila
/// una prueba).
pub fn campos(nombre: &str) -> &'static [Campo] {
    match nombre {
        "ea_cambiar_estado_tarea" => CAMPOS_ESTADO,
        "ea_encolar_rutina" | "ea_eliminar_rutina" => CAMPOS_SLUG,
        "ea_reiniciar_servicio" => CAMPOS_SERVICIO,
        "ea_promover_nota" => CAMPOS_PROMOVER,
        _ => &[],
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
            let version = store
                .ultima_rutina(&a.slug)?
                .ok_or_else(|| {
                    ErrorAccion::NoAplica(format!("la rutina '{}' no está registrada (ea rutina registrar)", a.slug))
                })?
                .version;
            let bus = bus_de(store, "simulacion")?;
            let activos = bus.trabajos_activos_de_rutina(&a.slug).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
            if let Some(t) = activos.first() {
                return Err(ErrorAccion::NoAplica(format!(
                    "la rutina '{}' ya tiene una ejecución en curso (trabajo {t})",
                    a.slug
                )));
            }
            (
                json!({ "slug": a.slug }),
                format!(
                    "Lanzaría la rutina '{}' (versión {version}) por el bus del orquestador: crea un \
                     trabajo para el agente local, que la ejecuta y solo vuelve a Claude Code si falla \
                     y tú lo confirmas.",
                    a.slug
                ),
                json!({ "trabajos_activos": 0, "version": version }),
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
        "ea_promover_nota" => {
            let a: PromoverNota = parsear(args)?;
            comprobar_promocion(&a)?;
            let destino = a.destino.filter(|d| !d.trim().is_empty());
            let plan = consulta.plan_promocion(&a.ruta, destino.as_deref())?;
            let mut norm = json!({ "ruta": plan.origen });
            if let Some(d) = &destino {
                norm["destino"] = json!(d);
            }
            (
                norm,
                format!(
                    "Copiaría la nota «{}» (de {}, {} bytes) de {} a {} en la región compartida y \
                     marcaría ambas como promovidas. El original se conserva. Desde ahí la leen \
                     todos los agentes como contenido de confianza.",
                    plan.titulo, plan.autor, plan.bytes, plan.origen, plan.destino
                ),
                json!({ "huella": plan.huella, "destino": plan.destino }),
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
            let bus = bus_de(store, actor)?;
            let id = bus.lanzar_rutina(&a.slug, &[]).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
            let _ = canal;
            Ok(Some(Aplicada {
                texto: format!("Rutina '{}' lanzada: trabajo {id} en la cola del agente local.", a.slug),
                tarea: None,
                datos: json!({ "trabajo": id, "rutina": a.slug }),
            }))
        }
        "ea_reiniciar_servicio" | "ea_eliminar_rutina" | "ea_promover_nota" => Ok(None),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Aplica la parte de la acción que actúa sobre el sistema operativo.
pub async fn aplicar_en_sistema<S: Sistema>(
    sistema: &S,
    store: &mut Store,
    actor: &str,
    canal: Canal,
    nombre: &str,
    args: &Value,
    precondicion: &Value,
) -> Result<Aplicada, ErrorAccion> {
    match nombre {
        "ea_promover_nota" => {
            let a: PromoverNota = parsear(args)?;
            let huella = precondicion["huella"].as_str().unwrap_or_default();
            let destino = a.destino.as_deref().filter(|d| !d.trim().is_empty());
            let plan = sistema.nota_promover(store, &a.ruta, destino, huella, actor, canal.nombre())?;
            Ok(Aplicada {
                texto: format!("Nota {} promovida a {}.", plan.origen, plan.destino),
                tarea: None,
                datos: json!({ "origen": plan.origen, "destino": plan.destino }),
            })
        }
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
