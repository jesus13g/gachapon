//! La lógica de cada herramienta mutante supervisada: validar argumentos,
//! simular (describir sin hacer), revalidar contra la precondición y aplicar.
//! Es la "capa interna" de §7.3; el catálogo y las definiciones MCP solo
//! nombran estas herramientas.
//!
//! Cada herramienta es un caso de [`Supervisada`] y un módulo con sus
//! argumentos (`Args`), su validación sin mirar el mundo, su simulación y su
//! aplicación, que solo actúa a través de un [`Sistema`]. Las funciones de
//! aquí despachan con un `match` exhaustivo; revalidar es volver a simular y
//! comparar la precondición, igual para todas.

use std::path::Path;

use ea_bus::slug_valido;
use ea_store::Store;
use serde::Deserialize;
use serde_json::Value;

use crate::catalogo::{self, Canal, Herramienta, Riesgo};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, EstadoServicio, Sistema};
use crate::temporizador::{Calendario, EstadoTimer};

mod biblioteca;
mod desprogramar_rutina;
mod encolar_rutina;
mod lanzar_sesion;
mod limpiar_worktree;
mod papelera_rutina;
mod programar_rutina;
mod promover_nota;
mod registrar_rutina;
mod reiniciar_servicio;

use Supervisada::*;

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

/// Las herramientas mutantes supervisadas: las entradas supervisadas del
/// catálogo (lo vigila una prueba), que sigue siendo la única tabla de riesgo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervisada {
    EncolarRutina,
    RegistrarRutina,
    ReiniciarServicio,
    ProgramarRutina,
    DesprogramarRutina,
    RetirarRutina,
    RestaurarRutina,
    LanzarSesion,
    LimpiarWorktree,
    EliminarRutina,
    PromoverNota,
    ActivarHerramienta,
    DesactivarHerramienta,
}

impl Supervisada {
    /// Todas, en el orden del catálogo.
    pub const TODAS: [Supervisada; 13] = [
        EncolarRutina, RegistrarRutina, ReiniciarServicio, ProgramarRutina, DesprogramarRutina, RetirarRutina,
        RestaurarRutina, LanzarSesion, LimpiarWorktree, EliminarRutina, PromoverNota, ActivarHerramienta,
        DesactivarHerramienta,
    ];

    /// El nombre del catálogo, que es el de MCP, el diario y las propuestas.
    pub fn nombre(self) -> &'static str {
        match self {
            EncolarRutina => "ea_encolar_rutina",
            RegistrarRutina => "ea_registrar_rutina",
            ReiniciarServicio => "ea_reiniciar_servicio",
            ProgramarRutina => "ea_programar_rutina",
            DesprogramarRutina => "ea_desprogramar_rutina",
            RetirarRutina => "ea_retirar_rutina",
            RestaurarRutina => "ea_restaurar_rutina",
            LanzarSesion => "ea_lanzar_sesion",
            LimpiarWorktree => "ea_limpiar_worktree",
            EliminarRutina => "ea_eliminar_rutina",
            PromoverNota => "ea_promover_nota",
            ActivarHerramienta => "ea_activar_herramienta",
            DesactivarHerramienta => "ea_desactivar_herramienta",
        }
    }

    pub fn desde(nombre: &str) -> Option<Self> {
        Self::TODAS.into_iter().find(|s| s.nombre() == nombre)
    }

    /// Su entrada del catálogo: riesgo y a quién se proyecta.
    pub fn ficha(self) -> &'static Herramienta {
        catalogo::buscar(self.nombre()).expect("toda herramienta supervisada está en el catálogo")
    }

    /// Los argumentos que acepta, en el orden en que conviene pedirlos. Deben
    /// casar con su `Args` (lo vigila una prueba).
    pub fn campos(self) -> &'static [Campo] {
        match self {
            EncolarRutina | DesprogramarRutina | RetirarRutina | RestaurarRutina | EliminarRutina => CAMPOS_SLUG,
            RegistrarRutina => registrar_rutina::CAMPOS,
            ReiniciarServicio => reiniciar_servicio::CAMPOS,
            ProgramarRutina => programar_rutina::CAMPOS,
            LanzarSesion => lanzar_sesion::CAMPOS,
            LimpiarWorktree => limpiar_worktree::CAMPOS,
            PromoverNota => promover_nota::CAMPOS,
            ActivarHerramienta | DesactivarHerramienta => biblioteca::CAMPOS,
        }
    }
}

/// La herramienta supervisada de ese nombre: `Desconocida` si no está en el
/// catálogo y `NoSupervisada` si es de ejecución directa.
pub fn supervisada(nombre: &str) -> Result<Supervisada, ErrorAccion> {
    Supervisada::desde(nombre).ok_or_else(|| match catalogo::buscar(nombre) {
        Some(_) => ErrorAccion::NoSupervisada(nombre.to_string()),
        None => ErrorAccion::Desconocida(nombre.to_string()),
    })
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

/// Con qué se simula: la base de datos y el sistema, solo para mirar, y lo
/// que se observó antes.
struct ParaSimular<'a, C> {
    store: &'a Store,
    consulta: &'a C,
    hechos: &'a Hechos,
}

/// Con qué se aplica: el sistema, la base de datos, quién confirmó y por qué
/// canal, y la precondición que se simuló.
struct ParaAplicar<'a, S> {
    sistema: &'a S,
    store: &'a mut Store,
    actor: &'a str,
    canal: Canal,
    precondicion: &'a Value,
}

/// Lo que dice una simulación: los argumentos normalizados, el texto y la
/// precondición.
type Descripcion = (Value, String, Value);

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

/// Los argumentos que acepta una herramienta supervisada; ninguno si no lo es.
pub fn campos(nombre: &str) -> &'static [Campo] {
    Supervisada::desde(nombre).map_or(&[], Supervisada::campos)
}

/// Los argumentos de las herramientas que solo nombran una rutina.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PorSlug {
    slug: String,
}

fn parsear<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, ErrorAccion> {
    serde_json::from_value(args.clone()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
}

/// Huella estable (FNV-1a, la de `ea-plataforma`) del contenido de una
/// rutina, para detectar que cambió desde la simulación.
fn huella(texto: &str) -> String {
    ea_plataforma::fs::huella(texto.as_bytes())
}

/// El bus de la raíz `estado` ([`Consulta::estado_dir`]), con historial.
pub(crate) fn bus_de(estado: &Path, store: &Store, actor: &str) -> Result<ea_bus::Bus, ErrorAccion> {
    let bus = ea_bus::Bus::abrir_en(estado).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
    Ok(bus.con_store(store.duplicar()?).con_actor(actor))
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

/// El timer de la rutina de `args`, si su slug es válido (programar,
/// desprogramar y retirar, que también lo quita).
async fn observar_timer<C: Consulta>(consulta: &C, args: &Value) -> Result<Option<EstadoTimer>, ErrorAccion> {
    match args["slug"].as_str().filter(|s| slug_valido(s)) {
        Some(slug) => Ok(Some(consulta.timer_estado(slug).await.map_err(ErrorAccion::Sistema)?)),
        None => Ok(None),
    }
}

/// Validación estructural de unos argumentos, sin mirar el estado del
/// mundo: la herramienta existe y es supervisada, la forma es la esperada y
/// los valores están en su dominio. La usa la carga de rutinas, para
/// rechazar al cargar una rutina mal escrita (no a las 3 de la mañana).
pub fn validar_argumentos(nombre: &str, args: &Value) -> Result<(), ErrorAccion> {
    match supervisada(nombre)? {
        EncolarRutina => encolar_rutina::validar(&parsear(args)?),
        RegistrarRutina => registrar_rutina::validar(&parsear(args)?),
        ReiniciarServicio => reiniciar_servicio::validar(&parsear(args)?),
        ProgramarRutina => programar_rutina::validar(&parsear(args)?),
        DesprogramarRutina | RetirarRutina | RestaurarRutina | EliminarRutina => {
            comprobar_slug(&parsear::<PorSlug>(args)?.slug)
        }
        LanzarSesion => lanzar_sesion::validar(&parsear(args)?),
        LimpiarWorktree => limpiar_worktree::validar(&parsear(args)?),
        PromoverNota => promover_nota::validar(&parsear(args)?),
        ActivarHerramienta | DesactivarHerramienta => biblioteca::validar(&parsear(args)?),
    }
}

/// Consulta al sistema lo que la simulación de `nombre` necesita. No valida:
/// con argumentos que no sirven no pregunta y la simulación dirá por qué.
pub async fn observar<C: Consulta>(consulta: &C, nombre: &str, args: &Value) -> Result<Hechos, ErrorAccion> {
    let mut h = Hechos::default();
    match Supervisada::desde(nombre) {
        Some(ReiniciarServicio) => h.servicio = reiniciar_servicio::observar(consulta, args).await?,
        Some(ProgramarRutina) => {
            h.timer = observar_timer(consulta, args).await?;
            h.calendario = programar_rutina::observar(consulta, args).await?;
        }
        Some(DesprogramarRutina | RetirarRutina) => h.timer = observar_timer(consulta, args).await?,
        Some(
            EncolarRutina | RegistrarRutina | RestaurarRutina | LanzarSesion | LimpiarWorktree | EliminarRutina
            | PromoverNota | ActivarHerramienta | DesactivarHerramienta,
        )
        | None => {}
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
    let s = supervisada(nombre)?;
    let p = ParaSimular { store, consulta, hechos };
    let (argumentos, texto, precondicion) = match s {
        EncolarRutina => encolar_rutina::simular(&p, parsear(args)?),
        RegistrarRutina => registrar_rutina::simular(&p, parsear(args)?),
        ReiniciarServicio => reiniciar_servicio::simular(&p, parsear(args)?),
        ProgramarRutina => programar_rutina::simular(&p, parsear(args)?),
        DesprogramarRutina => desprogramar_rutina::simular(&p, parsear(args)?),
        RetirarRutina => papelera_rutina::simular_retirar(&p, parsear(args)?),
        RestaurarRutina => papelera_rutina::simular_restaurar(&p, parsear(args)?),
        EliminarRutina => papelera_rutina::simular_eliminar(&p, parsear(args)?),
        LanzarSesion => lanzar_sesion::simular(&p, parsear(args)?),
        LimpiarWorktree => limpiar_worktree::simular(&p, parsear(args)?),
        PromoverNota => promover_nota::simular(&p, parsear(args)?),
        ActivarHerramienta => biblioteca::simular(&p, parsear(args)?, true),
        DesactivarHerramienta => biblioteca::simular(&p, parsear(args)?, false),
    }?;
    let h = s.ficha();
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

/// Aplica la acción ya revalidada. Todo lo que muta pasa por `sistema`.
pub(crate) async fn aplicar<S: Sistema>(
    s: Supervisada,
    sistema: &S,
    store: &mut Store,
    actor: &str,
    canal: Canal,
    args: &Value,
    precondicion: &Value,
) -> Result<Aplicada, ErrorAccion> {
    let p = ParaAplicar { sistema, store, actor, canal, precondicion };
    match s {
        EncolarRutina => encolar_rutina::aplicar(p, parsear(args)?),
        RegistrarRutina => registrar_rutina::aplicar(p, parsear(args)?),
        ReiniciarServicio => reiniciar_servicio::aplicar(p, parsear(args)?).await,
        ProgramarRutina => programar_rutina::aplicar(p, parsear(args)?).await,
        DesprogramarRutina => desprogramar_rutina::aplicar(p, parsear(args)?).await,
        RetirarRutina => papelera_rutina::aplicar_retirar(p, parsear(args)?).await,
        RestaurarRutina => papelera_rutina::aplicar_restaurar(p, parsear(args)?),
        EliminarRutina => papelera_rutina::aplicar_eliminar(p, parsear(args)?),
        LanzarSesion => lanzar_sesion::aplicar(p, parsear(args)?),
        LimpiarWorktree => limpiar_worktree::aplicar(p, parsear(args)?),
        PromoverNota => promover_nota::aplicar(p, parsear(args)?),
        ActivarHerramienta => biblioteca::aplicar(p, parsear(args)?, true).await,
        DesactivarHerramienta => biblioteca::aplicar(p, parsear(args)?, false).await,
    }
}
