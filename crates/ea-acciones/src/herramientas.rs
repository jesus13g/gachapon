//! La lógica de cada herramienta mutante supervisada: validar argumentos,
//! simular (describir sin hacer), revalidar contra la precondición y aplicar.
//! Es la "capa interna" de §7.3; el catálogo y las definiciones MCP solo
//! nombran estas herramientas.

use ea_biblioteca::{Cambio, Id};
use ea_store::{EventOutcome, NewEvent, Store};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::catalogo::{self, Canal, Ejecucion, Herramienta, Riesgo};
use crate::diff::diff_lineas;
use crate::error::ErrorAccion;
use crate::sistema::{slug_valido, Consulta, EstadoServicio, Sistema, SERVICIOS_REINICIABLES};
use crate::temporizador::{comprobar_calendario, unidad_timer, Calendario, EstadoTimer};

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
    pub trabajo: Option<String>,
    pub datos: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorSlug {
    slug: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorTrabajo {
    trabajo: String,
}

fn comprobar_trabajo(id: &str) -> Result<(), ErrorAccion> {
    if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(())
    } else {
        Err(ErrorAccion::Argumentos(format!("id de trabajo inválido: '{id}'")))
    }
}

/// `ea_encolar_rutina`: la rutina y, si se quieren cambiar, sus entradas
/// (`{clave: valor}`, con la política de `ea_bus::validar_entradas`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncolarRutina {
    slug: String,
    #[serde(default)]
    entradas: std::collections::BTreeMap<String, String>,
}

impl EncolarRutina {
    fn pares(&self) -> Vec<(String, String)> {
        self.entradas.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Validación sin mirar el mundo: slug y límites de las entradas.
    fn comprobar(&self) -> Result<(), ErrorAccion> {
        comprobar_slug(&self.slug)?;
        ea_bus::validar_entradas(&self.pares()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
    }
}

/// `ea_registrar_rutina`: la definición completa, tal como se registraría.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrarRutina {
    toml: String,
}

impl RegistrarRutina {
    /// Validación sin mirar el mundo: tamaño y formato (RF-001 y RF-016 de
    /// la spec 004). Que la versión sea mayor se mira al simular.
    fn comprobar(&self) -> Result<ea_bus::Rutina, ErrorAccion> {
        if self.toml.len() > ea_bus::MAX_DEFINICION {
            return Err(ErrorAccion::Argumentos(format!(
                "la definición es demasiado larga ({} bytes; como mucho {})",
                self.toml.len(),
                ea_bus::MAX_DEFINICION
            )));
        }
        ea_bus::Rutina::parsear(&self.toml).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
    }
}

/// `ea_programar_rutina`: la rutina y su `OnCalendar`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramarRutina {
    slug: String,
    calendario: String,
}

impl ProgramarRutina {
    /// Validación sin mirar el mundo: slug y texto del calendario (sin
    /// caracteres de control, `%` ni `\`). Devuelve el calendario recortado.
    fn comprobar(&self) -> Result<&str, ErrorAccion> {
        comprobar_slug(&self.slug)?;
        comprobar_calendario(&self.calendario).map_err(ErrorAccion::Argumentos)
    }
}

/// Los ids de `ea_lanzar_sesion`: una lista, o un texto con ids separados por
/// espacios, comas o saltos de línea (el formulario de la web).
#[derive(Deserialize)]
#[serde(untagged)]
enum IdsSesion {
    Lista(Vec<String>),
    Texto(String),
}

/// `ea_lanzar_sesion`: el agente, las herramientas de la biblioteca (ids) y,
/// si se quiere, la carpeta del proyecto.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanzarSesion {
    agente: String,
    herramientas: IdsSesion,
    #[serde(default)]
    directorio: Option<String>,
}

const MAX_HERRAMIENTAS_SESION: usize = 40;

impl LanzarSesion {
    fn ids(&self) -> Vec<String> {
        match &self.herramientas {
            IdsSesion::Lista(l) => l.iter().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()).collect(),
            IdsSesion::Texto(t) => t.split(|c: char| c.is_whitespace() || c == ',').filter(|i| !i.is_empty()).map(str::to_string).collect(),
        }
    }

    fn directorio(&self) -> Option<&str> {
        self.directorio.as_deref().map(str::trim).filter(|d| !d.is_empty())
    }

    /// Validación sin mirar el mundo: forma y tamaños. Que los ids existan y
    /// tengan sentido lo decide el plan de la biblioteca.
    fn comprobar(&self) -> Result<(), ErrorAccion> {
        if !ea_ajustes::IDS_AGENTES.contains(&self.agente.as_str()) {
            return Err(ErrorAccion::Argumentos(format!(
                "agente desconocido '{}' (uno de: {})",
                self.agente,
                ea_ajustes::IDS_AGENTES.join(", ")
            )));
        }
        let ids = self.ids();
        if ids.is_empty() {
            return Err(ErrorAccion::Argumentos("hay que elegir al menos una herramienta de la biblioteca".into()));
        }
        if ids.len() > MAX_HERRAMIENTAS_SESION {
            return Err(ErrorAccion::Argumentos(format!("demasiadas herramientas ({}; como mucho {MAX_HERRAMIENTAS_SESION})", ids.len())));
        }
        if let Some(i) = ids.iter().find(|i| i.len() > 300 || i.chars().any(char::is_control)) {
            return Err(ErrorAccion::Argumentos(format!("id de herramienta no válido: {i:?}")));
        }
        if let Some(d) = self.directorio()
            && (d.len() > 1024 || d.chars().any(char::is_control))
        {
            return Err(ErrorAccion::Argumentos("directorio no válido".into()));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorServicio {
    servicio: String,
}

/// `ea_activar_herramienta` y `ea_desactivar_herramienta`: el id de un
/// elemento de la biblioteca (`agente/tipo/ámbito/nombre`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorElemento {
    elemento: String,
}

impl PorElemento {
    /// Solo ids con la forma del inventario, nunca texto libre.
    fn comprobar(&self) -> Result<(), ErrorAccion> {
        Id::analizar(&self.elemento).map(|_| ()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
    }
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

/// Huella estable (FNV-1a, la de `ea-plataforma`) del contenido de una
/// rutina, para detectar que cambió desde la simulación.
fn huella(texto: &str) -> String {
    ea_plataforma::fs::huella(texto.as_bytes())
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

/// Dónde está el TOML de la rutina según la [`Consulta`]: la misma regla que
/// [`ea_bus::ubicacion_rutina`] (manda el de `rutinas_dir`), con el
/// contenido de ese fichero.
fn ubicacion<C: Consulta>(consulta: &C, slug: &str) -> Result<(ea_bus::Ubicacion, Option<String>), ErrorAccion> {
    if let Some(c) = consulta.rutina_contenido(slug).map_err(ErrorAccion::Sistema)? {
        return Ok((ea_bus::Ubicacion::Activa, Some(c)));
    }
    Ok(match consulta.rutina_retirada(slug).map_err(ErrorAccion::Sistema)? {
        Some(c) => (ea_bus::Ubicacion::Retirada, Some(c)),
        None => (ea_bus::Ubicacion::SinFichero, None),
    })
}

/// Falla con `NoAplica` si la rutina no se puede lanzar ni programar
/// (retirada o sin fichero), con el texto de [`ea_bus::Ubicacion::motivo_no_lanzable`].
fn exigir_lanzable<C: Consulta>(consulta: &C, slug: &str) -> Result<(), ErrorAccion> {
    match ubicacion(consulta, slug)?.0.motivo_no_lanzable(slug) {
        None => Ok(()),
        Some(m) => Err(ErrorAccion::NoAplica(m)),
    }
}

/// Validación estructural de unos argumentos, sin mirar el estado del
/// mundo: la herramienta existe y es supervisada, la forma es la esperada y
/// los valores están en su dominio. La usa la carga de rutinas, para
/// rechazar al cargar una rutina mal escrita (no a las 3 de la mañana).
pub fn validar_argumentos(nombre: &str, args: &Value) -> Result<(), ErrorAccion> {
    supervisada(nombre)?;
    match nombre {
        "ea_encolar_rutina" => parsear::<EncolarRutina>(args)?.comprobar(),
        "ea_eliminar_rutina" | "ea_retirar_rutina" | "ea_restaurar_rutina" => {
            comprobar_slug(&parsear::<PorSlug>(args)?.slug)
        }
        "ea_registrar_rutina" => parsear::<RegistrarRutina>(args)?.comprobar().map(|_| ()),
        "ea_reiniciar_servicio" => comprobar_servicio(&parsear::<PorServicio>(args)?.servicio),
        "ea_promover_nota" => comprobar_promocion(&parsear::<PromoverNota>(args)?),
        "ea_programar_rutina" => parsear::<ProgramarRutina>(args)?.comprobar().map(|_| ()),
        "ea_desprogramar_rutina" => comprobar_slug(&parsear::<PorSlug>(args)?.slug),
        "ea_limpiar_worktree" => comprobar_trabajo(&parsear::<PorTrabajo>(args)?.trabajo),
        "ea_activar_herramienta" | "ea_desactivar_herramienta" => parsear::<PorElemento>(args)?.comprobar(),
        "ea_lanzar_sesion" => parsear::<LanzarSesion>(args)?.comprobar(),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Un argumento de una herramienta supervisada, descrito para quien tenga
/// que pedírselo a una persona (el formulario de la web).
#[derive(Debug, Clone, Copy)]
pub struct Campo {
    pub nombre: &'static str,
    pub entero: bool,
    /// Texto de varias líneas (un área de texto, no un campo de una línea).
    pub largo: bool,
    pub obligatorio: bool,
    /// Dominio cerrado de valores; vacío si es libre.
    pub opciones: &'static [&'static str],
    pub ayuda: &'static str,
}

const fn campo(nombre: &'static str, ayuda: &'static str) -> Campo {
    Campo { nombre, entero: false, largo: false, obligatorio: true, opciones: &[], ayuda }
}

const CAMPOS_SLUG: &[Campo] = &[campo("slug", "rutina")];
const CAMPOS_TRABAJO: &[Campo] = &[campo("trabajo", "id del trabajo con worktree propio")];
const CAMPOS_SERVICIO: &[Campo] =
    &[Campo { opciones: &SERVICIOS_REINICIABLES, ..campo("servicio", "servicio") }];
const CAMPOS_REGISTRAR: &[Campo] =
    &[Campo { largo: true, ..campo("toml", "definición TOML de la rutina ([rutina], [entradas], [[pasos]])") }];
const CAMPOS_PROGRAMAR: &[Campo] = &[
    campo("slug", "rutina"),
    campo("calendario", "OnCalendar de systemd (p. ej. *-*-* 08:00:00 o Mon *-*-* 09:00)"),
];
const CAMPOS_PROMOVER: &[Campo] = &[
    campo("ruta", "ruta de la nota (p. ej. claude-code/tema/nota.md)"),
    Campo { obligatorio: false, ..campo("destino", "destino en «compartida» (opcional)") },
];

const CAMPOS_ELEMENTO: &[Campo] = &[campo("elemento", "id del elemento de la biblioteca (agente/tipo/ámbito/nombre, p. ej. codex/mcp/usuario/docs)")];
const CAMPOS_SESION: &[Campo] = &[
    Campo { opciones: &ea_biblioteca::AGENTES_CON_SESION, ..campo("agente", "agente que se lanza") },
    Campo { largo: true, ..campo("herramientas", "ids de la biblioteca (ea biblioteca listar), uno por línea") },
    Campo { obligatorio: false, ..campo("directorio", "carpeta del proyecto, ruta absoluta (opcional)") },
];

/// Los argumentos que acepta una herramienta supervisada, en el orden en que
/// conviene pedirlos. Debe casar con las estructuras de arriba (lo vigila
/// una prueba).
pub fn campos(nombre: &str) -> &'static [Campo] {
    match nombre {
        "ea_encolar_rutina" | "ea_eliminar_rutina" | "ea_desprogramar_rutina" => CAMPOS_SLUG,
        "ea_retirar_rutina" | "ea_restaurar_rutina" => CAMPOS_SLUG,
        "ea_limpiar_worktree" => CAMPOS_TRABAJO,
        "ea_programar_rutina" => CAMPOS_PROGRAMAR,
        "ea_reiniciar_servicio" => CAMPOS_SERVICIO,
        "ea_registrar_rutina" => CAMPOS_REGISTRAR,
        "ea_promover_nota" => CAMPOS_PROMOVER,
        "ea_activar_herramienta" | "ea_desactivar_herramienta" => CAMPOS_ELEMENTO,
        "ea_lanzar_sesion" => CAMPOS_SESION,
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
    /// El timer de la rutina (programar y desprogramar).
    pub timer: Option<EstadoTimer>,
    /// Lo que dice `systemd-analyze calendar` del calendario pedido.
    pub calendario: Option<Result<Calendario, String>>,
}

/// Consulta al sistema lo que la simulación de `nombre` necesita.
pub async fn observar<C: Consulta>(consulta: &C, nombre: &str, args: &Value) -> Result<Hechos, ErrorAccion> {
    let mut h = Hechos::default();
    if nombre == "ea_reiniciar_servicio"
        && let Some(srv) = args["servicio"].as_str().filter(|s| SERVICIOS_REINICIABLES.contains(s)) {
        h.servicio = Some(consulta.servicio_estado(srv).await.map_err(ErrorAccion::Sistema)?);
    }
    // Retirar también quita el timer: su simulación lo dice.
    if matches!(nombre, "ea_programar_rutina" | "ea_desprogramar_rutina" | "ea_retirar_rutina")
        && let Some(slug) = args["slug"].as_str().filter(|s| slug_valido(s)) {
        h.timer = Some(consulta.timer_estado(slug).await.map_err(ErrorAccion::Sistema)?);
    }
    if nombre == "ea_programar_rutina"
        && let Some(c) = args["calendario"].as_str().filter(|c| comprobar_calendario(c).is_ok()) {
        h.calendario = Some(consulta.calendario_validar(c).await.map_err(ErrorAccion::Sistema)?);
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
        "ea_encolar_rutina" => {
            let a: EncolarRutina = parsear(args)?;
            a.comprobar()?;
            // Retirada o sin fichero: no se lanza (el mismo texto que la web y la CLI).
            exigir_lanzable(consulta, &a.slug)?;
            let version = store
                .ultima_rutina(&a.slug)?
                .ok_or_else(|| {
                    ErrorAccion::NoAplica(format!("la rutina '{}' no está registrada (ea rutina registrar)", a.slug))
                })?
                .version;
            let bus = bus_de(store, "simulacion")?;
            // La orden que saldría, con las mismas comprobaciones que al
            // lanzar (entrada no declarada, límites): así el usuario ve el
            // criterio que ejecutará el sistema antes de aceptar.
            let orden = bus
                .rutina(&a.slug)
                .and_then(|r| r.a_orden(&a.pares()))
                .map_err(|e| ErrorAccion::Argumentos(e.to_string()))?;
            let activos = bus.trabajos_activos_de_rutina(&a.slug).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
            if let Some(t) = activos.first() {
                return Err(ErrorAccion::NoAplica(format!(
                    "la rutina '{}' ya tiene una ejecución en curso (trabajo {t})",
                    a.slug
                )));
            }
            let entradas = if a.entradas.is_empty() {
                "con las entradas por defecto".to_string()
            } else {
                let lista: Vec<String> = a.entradas.iter().map(|(k, v)| format!("{k} = {v:?}")).collect();
                format!("con las entradas {}", lista.join(", "))
            };
            // Sin entradas, los argumentos quedan como antes ({slug}).
            let argumentos =
                if a.entradas.is_empty() { json!({ "slug": a.slug }) } else { json!({ "slug": a.slug, "entradas": a.entradas }) };
            (
                argumentos,
                format!(
                    "Lanzaría la rutina '{}' (versión {version}) {entradas} por el bus del orquestador: crea un \
                     trabajo para el agente local, que la ejecuta y solo vuelve a Claude Code si falla \
                     y tú lo confirmas. Criterio que ejecutará el sistema: `{}`.",
                    a.slug,
                    orden.criterio_exito.as_deref().unwrap_or_default()
                ),
                json!({ "trabajos_activos": 0, "version": version }),
            )
        }
        "ea_registrar_rutina" => {
            let a: RegistrarRutina = parsear(args)?;
            a.comprobar()?;
            // Lo que no es de formato depende del mundo (la versión
            // registrada ahora): NoAplica, como en el resto de herramientas.
            let (r, anterior) =
                ea_bus::comprobar_registro(store, &a.toml).map_err(|e| ErrorAccion::NoAplica(e.to_string()))?;
            let orden = r.a_orden(&[]).map_err(|e| ErrorAccion::Argumentos(e.to_string()))?;
            let c = &r.rutina;
            let (que, cambios) = match &anterior {
                Some(f) => {
                    let d = diff_lineas(&f.definicion, &a.toml);
                    (
                        format!("la versión {} de la rutina '{}' (ahora está en la {})", c.version, c.slug, f.version),
                        if d.is_empty() {
                            "La definición es la misma salvo la versión.".to_string()
                        } else {
                            format!("Cambios frente a la versión {}:\n{d}", f.version)
                        },
                    )
                }
                None => (
                    format!("la rutina nueva '{}' en la versión {}", c.slug, c.version),
                    format!("Definición completa:\n{}", a.toml.trim_end()),
                ),
            };
            (
                json!({ "toml": a.toml }),
                format!(
                    "Registraría {que}: la guarda en SQLite y copia el TOML a rutinas_dir/{}.toml, como \
                     `ea rutina registrar`. No lanza nada; la próxima ejecución usará esta versión. \
                     Worktree: {}. Criterio que ejecutará el sistema (entradas por defecto): `{}`.\n\n{cambios}",
                    c.slug,
                    c.worktree,
                    orden.criterio_exito.as_deref().unwrap_or_default()
                ),
                json!({ "slug": c.slug, "version_registrada": anterior.map(|f| f.version) }),
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
                    "Reiniciaría {}.service (ahora: {}). Las conexiones y trabajos en curso de ese \
                     servicio se interrumpirían unos segundos.",
                    a.servicio, estado.activo
                ),
                json!({ "cargado": true }),
            )
        }
        "ea_eliminar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            // Solo se borra para siempre lo que ya está en la papelera: el
            // paso reversible (retirar) va antes, como en la bóveda.
            let contenido = match ubicacion(consulta, &a.slug)? {
                (ea_bus::Ubicacion::Retirada, Some(c)) => c,
                (ea_bus::Ubicacion::Activa, _) => {
                    return Err(ErrorAccion::NoAplica(format!(
                        "la rutina '{}' está activa: retírala antes (ea_retirar_rutina); solo se borra para \
                         siempre una rutina retirada",
                        a.slug
                    )))
                }
                _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está en la papelera", a.slug))),
            };
            (
                json!({ "slug": a.slug }),
                format!(
                    "Borraría PARA SIEMPRE la copia retirada de la rutina '{}' (rutinas_dir/{}/{}.toml, {} bytes, \
                     {} líneas): ese fichero no se puede recuperar. Sus versiones registradas siguen en SQLite \
                     como historial; registrar una versión nueva la vuelve a poner en uso.",
                    a.slug,
                    ea_bus::DIR_PAPELERA,
                    a.slug,
                    contenido.len(),
                    contenido.lines().count()
                ),
                json!({ "huella": huella(&contenido), "bytes": contenido.len() }),
            )
        }
        "ea_retirar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            let contenido = match ubicacion(consulta, &a.slug)? {
                (ea_bus::Ubicacion::Activa, Some(c)) => c,
                (ea_bus::Ubicacion::Retirada, _) => {
                    return Err(ErrorAccion::NoAplica(format!("la rutina '{}' ya está retirada", a.slug)))
                }
                _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no tiene fichero en rutinas_dir", a.slug))),
            };
            let version = store
                .ultima_rutina(&a.slug)?
                .map(|f| format!(" (versión {} registrada)", f.version))
                .unwrap_or_else(|| " (no registrada)".to_string());
            let timer = hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
            let del_timer = if timer.programado() {
                let horario = timer.calendario.as_deref().map(|c| format!(" (OnCalendar={c})")).unwrap_or_default();
                format!(
                    "Antes desprograma su timer{horario}: `systemctl --user disable --now {}`, borra el fichero \
                     del timer y daemon-reload.",
                    unidad_timer(&a.slug)
                )
            } else {
                "No tiene timer que desprogramar.".to_string()
            };
            let en_curso = bus_de(store, "simulacion")?
                .trabajos_activos_de_rutina(&a.slug)
                .map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
            let en_curso = match en_curso.first() {
                Some(t) => format!(" El trabajo {t}, ya creado, sigue su curso: retirar no lo cancela."),
                None => String::new(),
            };
            (
                json!({ "slug": a.slug }),
                format!(
                    "Retiraría la rutina '{}'{version}: mueve rutinas_dir/{}.toml ({} bytes) a la papelera \
                     (rutinas_dir/{}/{}.toml). Deja de listarse y de poder lanzarse (CLI, web, \
                     ea_encolar_rutina y el timer). {del_timer} Sus versiones siguen en SQLite. Se deshace \
                     con ea_restaurar_rutina, que no vuelve a programar el timer.{en_curso}",
                    a.slug,
                    a.slug,
                    contenido.len(),
                    ea_bus::DIR_PAPELERA,
                    a.slug
                ),
                json!({ "huella": huella(&contenido), "calendario_actual": timer.calendario, "fichero": timer.fichero }),
            )
        }
        "ea_restaurar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            let contenido = match ubicacion(consulta, &a.slug)? {
                (ea_bus::Ubicacion::Retirada, Some(c)) => c,
                (ea_bus::Ubicacion::Activa, _) => {
                    return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está retirada", a.slug)))
                }
                _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está en la papelera", a.slug))),
            };
            let lanzable = match store.ultima_rutina(&a.slug)? {
                Some(f) => format!("vuelve a listarse y a poder lanzarse con la última versión registrada ({})", f.version),
                None => "vuelve a listarse, pero no está registrada: para lanzarla hay que registrarla".to_string(),
            };
            (
                json!({ "slug": a.slug }),
                format!(
                    "Restauraría la rutina '{}': devuelve rutinas_dir/{}/{}.toml ({} bytes) a rutinas_dir/{}.toml; \
                     {lanzable}. No vuelve a programar su timer: si lo quieres, prográmalo después \
                     (ea_programar_rutina).",
                    a.slug,
                    ea_bus::DIR_PAPELERA,
                    a.slug,
                    contenido.len(),
                    a.slug
                ),
                json!({ "huella": huella(&contenido) }),
            )
        }
        "ea_limpiar_worktree" => {
            let a: PorTrabajo = parsear(args)?;
            comprobar_trabajo(&a.trabajo)?;
            let no = |m: String| Err(ErrorAccion::NoAplica(m));
            let Some(w) = consulta.worktree_info(&a.trabajo).map_err(ErrorAccion::Sistema)? else {
                return no(format!("el trabajo {} no tiene worktree propio vivo (nada que limpiar)", a.trabajo));
            };
            if !w.trabajo_terminado() {
                return no(format!(
                    "el trabajo {} no ha terminado (estado: {}): solo se limpia un trabajo cerrado o cancelado",
                    a.trabajo,
                    w.estado_trabajo.map_or("desconocido", ea_bus::EstadoTrabajo::as_str)
                ));
            }
            if w.ocupado {
                return no(format!("el ejecutor tiene cogido el semáforo del trabajo {}: espera a que suelte", a.trabajo));
            }
            match w.limpio {
                Some(true) => {}
                Some(false) => {
                    return no(format!(
                        "el worktree {} tiene cambios sin guardar o ficheros nuevos: nunca se fuerza. Guárdalos \
                         (git add -A && git commit en el worktree, y fusiona la rama {} si los quieres) o descártalos \
                         tú a mano, y vuelve a simular",
                        w.ruta, w.rama
                    ))
                }
                None => return no(format!("git no pudo decir si {} está limpio; revísalo a mano", w.ruta)),
            }
            (
                json!({ "trabajo": a.trabajo }),
                format!(
                    "Quitaría el worktree propio del trabajo {} ({}) con `git worktree remove`, sin --force; está \
                     limpio y el trabajo está {}. La rama {} se conserva (borrarla o fusionarla es cosa tuya) y el \
                     directorio se vuelve a sacar de ella si hiciera falta.",
                    a.trabajo,
                    w.ruta,
                    w.estado_trabajo.map_or("terminado", ea_bus::EstadoTrabajo::as_str),
                    w.rama
                ),
                json!({ "ruta": w.ruta, "rama": w.rama, "estado_trabajo": w.estado_trabajo }),
            )
        }
        "ea_lanzar_sesion" => {
            let a: LanzarSesion = parsear(args)?;
            a.comprobar()?;
            let plan = consulta.sesion_plan(&a.agente, &a.ids(), a.directorio()).map_err(ErrorAccion::NoAplica)?;
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
            let entorno = if plan.entorno.is_empty() {
                "ninguna".to_string()
            } else {
                plan.entorno.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
            };
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
            (argumentos, texto, json!({ "plan": huella }))
        }
        "ea_programar_rutina" => {
            let a: ProgramarRutina = parsear(args)?;
            let c = a.comprobar()?.to_string();
            // Una rutina retirada no se programa: el timer la lanzaría y
            // `ea rutina lanzar` la rechazaría.
            exigir_lanzable(consulta, &a.slug)?;
            // Solo las rutinas registradas: el timer lanza la última versión.
            let version = store
                .ultima_rutina(&a.slug)?
                .ok_or_else(|| {
                    ErrorAccion::NoAplica(format!("la rutina '{}' no está registrada (ea rutina registrar)", a.slug))
                })?
                .version;
            let cal = match &hechos.calendario {
                Some(Ok(cal)) => cal.clone(),
                Some(Err(m)) => return Err(ErrorAccion::Argumentos(format!("calendario no válido: {m}"))),
                None => return Err(ErrorAccion::Sistema("calendario no observado".into())),
            };
            let timer = hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
            let igual = timer.calendario.as_deref().is_some_and(|t| t == c || t == cal.normalizado);
            if igual && timer.activo.as_deref() == Some("active") {
                return Err(ErrorAccion::NoAplica(format!(
                    "la rutina '{}' ya está programada con OnCalendar={c}",
                    a.slug
                )));
            }
            let ahora = match (&timer.calendario, timer.activo.as_deref()) {
                (Some(t), Some(act)) => format!("Ahora está programada con OnCalendar={t} ({act}); se sustituye."),
                (Some(t), None) => format!("Ahora está programada con OnCalendar={t}; se sustituye."),
                (None, _) => "Ahora no tiene horario.".to_string(),
            };
            let proximas = if cal.proximas.is_empty() {
                "systemd no prevé ninguna ejecución".to_string()
            } else {
                format!("próximas ejecuciones: {}", cal.proximas.join("; "))
            };
            (
                json!({ "slug": a.slug, "calendario": c }),
                format!(
                    "Programaría la rutina '{}' (versión {version}) con OnCalendar={c} (para systemd: {}); \
                     {proximas}. Escribe {} en el directorio de unidades de systemd --user, hace \
                     daemon-reload y lo activa (enable y restart). A cada hora, ea-rutina@{}.service solo \
                     crea el trabajo (`ea rutina lanzar {}`), con la última versión registrada; además se \
                     dispara una vez 10 min después de arrancar la sesión, como la plantilla. {ahora}",
                    a.slug,
                    cal.normalizado,
                    unidad_timer(&a.slug),
                    a.slug,
                    a.slug
                ),
                json!({ "calendario_actual": timer.calendario, "fichero": timer.fichero }),
            )
        }
        "ea_desprogramar_rutina" => {
            let a: PorSlug = parsear(args)?;
            comprobar_slug(&a.slug)?;
            // No exige que esté registrada: también quita el timer de una
            // rutina retirada.
            let timer = hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
            if !timer.programado() {
                return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no tiene timer programado", a.slug)));
            }
            let horario = timer.calendario.as_deref().map(|c| format!(" (OnCalendar={c})")).unwrap_or_default();
            (
                json!({ "slug": a.slug }),
                format!(
                    "Desprogramaría la rutina '{}'{horario}: `systemctl --user disable --now {}`, borra el \
                     fichero del timer y daemon-reload. La rutina sigue registrada y se puede lanzar a mano \
                     o volver a programar.",
                    a.slug,
                    unidad_timer(&a.slug)
                ),
                json!({ "calendario_actual": timer.calendario, "fichero": timer.fichero }),
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
        "ea_activar_herramienta" | "ea_desactivar_herramienta" => {
            let a: PorElemento = parsear(args)?;
            a.comprobar()?;
            let activar = nombre == "ea_activar_herramienta";
            let cambio = consulta.biblioteca_cambio(&a.elemento, activar).map_err(ErrorAccion::NoAplica)?;
            // El cambio entero es la precondición: al aceptar se recalcula y debe ser idéntico
            // (incluida la huella del fichero que se editaría).
            let precondicion = json!({ "cambio": cambio });
            (json!({ "elemento": a.elemento }), format!("{} Es reversible: {} lo deshace.", cambio.describir(), if activar { "desactivarlo" } else { "activarlo" }), precondicion)
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
    // Primero la validación completa de la simulación (la
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
pub(crate) fn aplicar_en_store(
    store: &mut Store,
    actor: &str,
    canal: Canal,
    nombre: &str,
    args: &Value,
) -> Result<Option<Aplicada>, ErrorAccion> {
    match nombre {
        "ea_encolar_rutina" => {
            let a: EncolarRutina = parsear(args)?;
            let bus = bus_de(store, actor)?;
            let id = bus.lanzar_rutina(&a.slug, &a.pares()).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
            let _ = canal;
            Ok(Some(Aplicada {
                texto: format!("Rutina '{}' lanzada: trabajo {id} en la cola del agente local.", a.slug),
                trabajo: Some(id.to_string()),
                datos: json!({ "trabajo": id, "rutina": a.slug }),
            }))
        }
        "ea_reiniciar_servicio" | "ea_eliminar_rutina" | "ea_promover_nota" | "ea_registrar_rutina" => Ok(None),
        "ea_programar_rutina" | "ea_desprogramar_rutina" | "ea_lanzar_sesion" => Ok(None),
        "ea_retirar_rutina" | "ea_restaurar_rutina" | "ea_limpiar_worktree" => Ok(None),
        "ea_activar_herramienta" | "ea_desactivar_herramienta" => Ok(None),
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}

/// Aplica la parte de la acción que actúa sobre el sistema operativo.
pub(crate) async fn aplicar_en_sistema<S: Sistema>(
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
                trabajo: None,
                datos: json!({ "origen": plan.origen, "destino": plan.destino }),
            })
        }
        "ea_reiniciar_servicio" => {
            let a: PorServicio = parsear(args)?;
            sistema.servicio_reiniciar(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
            let ahora = sistema.servicio_estado(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("{}.service reiniciado (ahora: {}).", a.servicio, ahora.activo),
                trabajo: None,
                datos: json!({ "servicio": a.servicio, "activo": ahora.activo }),
            })
        }
        "ea_registrar_rutina" => {
            let a: RegistrarRutina = parsear(args)?;
            let (slug, version) = sistema.rutina_registrar(store, &a.toml, actor).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("Rutina '{slug}' registrada en la versión {version}."),
                trabajo: None,
                datos: json!({ "rutina": slug, "version": version }),
            })
        }
        "ea_lanzar_sesion" => {
            let a: LanzarSesion = parsear(args)?;
            a.comprobar()?;
            let plan = sistema.sesion_plan(&a.agente, &a.ids(), a.directorio()).map_err(ErrorAccion::NoAplica)?;
            sistema.sesion_lanzar(&plan).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!(
                    "Sesión de {} abierta en una terminal con {} herramientas de la biblioteca ({}).",
                    plan.agente,
                    plan.herramientas.len(),
                    plan.id
                ),
                trabajo: None,
                // Sin secretos: ids y argv (rutas de ficheros 0600, nunca su contenido).
                datos: json!({
                    "sesion": plan.id, "agente": plan.agente, "herramientas": plan.herramientas,
                    "argv": plan.argv_terminal(),
                }),
            })
        }
        "ea_programar_rutina" => {
            let a: ProgramarRutina = parsear(args)?;
            let c = a.comprobar()?.to_string();
            sistema.timer_programar(&a.slug, &c).await.map_err(ErrorAccion::Sistema)?;
            let ahora = sistema.timer_estado(&a.slug).await.map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!(
                    "Rutina '{}' programada con OnCalendar={c} ({}).",
                    a.slug,
                    ahora.activo.as_deref().unwrap_or("estado desconocido")
                ),
                trabajo: None,
                datos: json!({ "rutina": a.slug, "calendario": c, "activo": ahora.activo }),
            })
        }
        "ea_desprogramar_rutina" => {
            let a: PorSlug = parsear(args)?;
            let habia = sistema.timer_desprogramar(&a.slug).await.map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: if habia {
                    format!("Timer de la rutina '{}' desactivado y borrado.", a.slug)
                } else {
                    format!("La rutina '{}' no tenía timer.", a.slug)
                },
                trabajo: None,
                datos: json!({ "rutina": a.slug, "desprogramada": habia }),
            })
        }
        "ea_eliminar_rutina" => {
            let a: PorSlug = parsear(args)?;
            sistema.rutina_eliminar(&a.slug).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("Copia retirada de la rutina '{}' borrada para siempre.", a.slug),
                trabajo: None,
                datos: json!({ "rutina": a.slug }),
            })
        }
        "ea_retirar_rutina" => {
            let a: PorSlug = parsear(args)?;
            // El timer primero: si systemd falla, la rutina sigue como estaba
            // (activa y con su horario) en vez de quedar retirada con un timer
            // que la intenta lanzar.
            let habia = sistema.timer_desprogramar(&a.slug).await.map_err(|e| {
                ErrorAccion::Sistema(format!("no se pudo quitar el timer; la rutina no se ha retirado: {e}"))
            })?;
            sistema.rutina_retirar(&a.slug).map_err(|e| {
                ErrorAccion::Sistema(if habia { format!("timer quitado, pero la rutina no se pudo retirar: {e}") } else { e })
            })?;
            Ok(Aplicada {
                texto: format!(
                    "Rutina '{}' retirada (su TOML está en la papelera){}.",
                    a.slug,
                    if habia { " y su timer desprogramado" } else { "" }
                ),
                trabajo: None,
                datos: json!({ "rutina": a.slug, "desprogramada": habia }),
            })
        }
        "ea_activar_herramienta" | "ea_desactivar_herramienta" => {
            let a: PorElemento = parsear(args)?;
            let activar = nombre == "ea_activar_herramienta";
            // Lo que aceptó el usuario es el cambio simulado; la biblioteca lo revalida
            // contra el estado de ahora y rechaza el que no coincida con lo que habría hecho.
            let cambio: Cambio = serde_json::from_value(precondicion["cambio"].clone())
                .map_err(|_| ErrorAccion::NoAplica("la propuesta no lleva el cambio simulado".into()))?;
            if cambio.id != a.elemento || cambio.activar != activar {
                return Err(ErrorAccion::NoAplica("el cambio simulado no es el de esta herramienta y este elemento".into()));
            }
            let h = sistema.biblioteca_aplicar(&cambio, canal).await.map_err(ErrorAccion::Sistema)?;
            let datos = json!({
                "elemento": h.id, "activar": h.activar, "mecanismo": h.mecanismo, "objetivo": h.objetivo,
                "antes": h.antes, "despues": h.despues, "copia": h.copia,
            });
            // Un evento propio con el valor anterior (art. VI): el de la acción lo lleva también en `resultado`.
            let _ = store.append_event(NewEvent {
                actor: actor.to_string(),
                action: "biblioteca_cambio".to_string(),
                outcome: Some(EventOutcome::Ok),
                detail: Some(datos.to_string()),
                ..Default::default()
            });
            Ok(Aplicada {
                texto: format!("{} {}: {}.", if activar { "Activado" } else { "Desactivado" }, h.id, h.texto),
                trabajo: None,
                datos,
            })
        }
        "ea_limpiar_worktree" => {
            let a: PorTrabajo = parsear(args)?;
            let ruta = precondicion["ruta"].as_str().unwrap_or_default().to_string();
            let habia = sistema.worktree_limpiar(&a.trabajo).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: if habia {
                    format!("Worktree del trabajo {} quitado; la rama ea/{} sigue en el repo.", a.trabajo, a.trabajo)
                } else {
                    format!("El trabajo {} ya no tenía worktree.", a.trabajo)
                },
                trabajo: Some(a.trabajo.clone()),
                datos: json!({ "trabajo": a.trabajo, "ruta": ruta, "rama": format!("ea/{}", a.trabajo), "quitado": habia }),
            })
        }
        "ea_restaurar_rutina" => {
            let a: PorSlug = parsear(args)?;
            sistema.rutina_restaurar(&a.slug).map_err(ErrorAccion::Sistema)?;
            Ok(Aplicada {
                texto: format!("Rutina '{}' restaurada (sin timer: prográmala si lo quieres).", a.slug),
                trabajo: None,
                datos: json!({ "rutina": a.slug }),
            })
        }
        otro => Err(ErrorAccion::NoSupervisada(otro.to_string())),
    }
}
