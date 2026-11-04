//! Trabajadores del ejecutor en paralelo (spec 019, RF-008 a RF-010).
//!
//! Hay un trabajador (un hilo) por agente ejecutor despierto: siempre el
//! agente de `roles.ejecutor` (como en la 017) y, además, los miembros
//! `ejecutor` de los equipos despiertos. Nunca hay más de
//! `orquestador.paralelo` a la vez ni dos del mismo agente. De la cola de rol
//! cada uno toma solo lo que le toca ([`le_toca`]).
//!
//! Cada trabajador, en su bucle: comprueba que su agente sigue despierto
//! (si no, termina el trabajo que tenga entre manos y se va), rehace su
//! configuración desde los ajustes (`ConfigOrquestador::para_agente`) y su
//! adaptador, y toma un trabajo de su cola y luego de la de rol
//! (`ejecutar_intento`: worktree propio, semáforo exclusivo, ejecución y
//! entrega). Un semáforo ocupado devuelve el trabajo a su cola y el
//! trabajador espera un sondeo antes de probar otra vez.
//!
//! Presencia (`agentes/<id>/presencia.json`): se anota al arrancar el
//! trabajador, late cada [`Contexto::latido`] (también durante un trabajo
//! largo: el hilo de [`Latido`]) con el trabajo actual y se quita al irse.
//!
//! Instancia única ([`Instancia`], [`arrancar`]) y parada limpia
//! ([`Trabajadores::detener`]): un solo orquestador por raíz del bus, y al
//! pararse corta lo que corre sin darlo por terminado ni por cancelado.

use std::collections::HashMap;
use std::fs::{OpenOptions, TryLockError};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ea_ajustes::Ajustes;
use ea_bus::{Bus, Equipo, EstadoEquipo, INTERVALO_LATIDO_S, Latido, Presencia, RolEquipo};

use crate::config::{ConfigOrquestador, PerfilEjecutor};
use crate::ejecutor::{Ejecutor, Intento, ejecutar_intento, fabricar};

/// Un agente ejecutor que debe tener trabajador, y el equipo que lo despertó.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Despierto {
    pub agente: String,
    pub equipo: Option<String>,
}

/// Los ejecutores despiertos ahora, en orden estable y recortados al tope
/// `orquestador.paralelo` (al menos 1). El agente de `roles.ejecutor` va
/// siempre el primero (atiende los trabajos sin equipo; si además es ejecutor
/// de un equipo despierto, es el mismo trabajador); después, los ejecutores
/// de los equipos despiertos.
pub fn ejecutores_despiertos(equipos: &[Equipo], a: &Ajustes) -> Vec<Despierto> {
    let rol = a.agente_de(ea_ajustes::Rol::Ejecutor).0.to_string();
    let mut v: Vec<Despierto> = vec![Despierto { agente: rol, equipo: None }];
    for e in equipos.iter().filter(|e| e.estado == EstadoEquipo::Despierto) {
        for m in e.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor) {
            match v.iter_mut().find(|d| d.agente == m.agente) {
                Some(d) => {
                    if d.equipo.is_none() {
                        d.equipo = Some(e.id.clone());
                    }
                }
                None => v.push(Despierto { agente: m.agente.clone(), equipo: Some(e.id.clone()) }),
            }
        }
    }
    v.truncate((a.orquestador.paralelo as usize).max(1));
    v
}

/// Si una orden de la cola de rol le toca a `agente` (spec 019, casos límite):
/// las de su equipo despierto y, si es el agente de `roles.ejecutor`, las que
/// no tienen equipo o son de un equipo dormido o inexistente.
pub fn le_toca(orden: &ea_bus::Orden, agente: &str, es_del_rol: bool, equipos: &[Equipo]) -> bool {
    let Some(id) = orden.equipo.as_deref() else { return es_del_rol };
    match equipos.iter().find(|e| e.id == id && e.estado == EstadoEquipo::Despierto) {
        Some(e) => e.miembros.iter().any(|m| m.agente == agente && m.rol == RolEquipo::Ejecutor),
        None => es_del_rol,
    }
}

type AbrirBus = Arc<dyn Fn() -> Option<Bus> + Send + Sync>;
type CargarAjustes = Arc<dyn Fn() -> Ajustes + Send + Sync>;
type Fabricar = Arc<dyn Fn(&PerfilEjecutor) -> Box<dyn Ejecutor + Send + Sync> + Send + Sync>;

/// Lo que comparten los trabajadores. Todo es inyectable para probar con un
/// doble del agente y sin esperas reales.
#[derive(Clone)]
pub struct Contexto {
    pub abrir_bus: AbrirBus,
    pub cargar_ajustes: CargarAjustes,
    pub fabricar: Fabricar,
    /// Espera de un trabajador sin trabajo (o con el worktree ocupado).
    pub sondeo: Duration,
    /// Cuánto espera un trabajador al semáforo antes de devolver el trabajo.
    pub espera_candado: Duration,
    /// Cada cuánto late la presencia.
    pub latido: Duration,
    /// Cuando vale `true`, los trabajadores cortan lo que tienen (el trabajo
    /// se queda `ejecutando`, para [`Bus::recuperar`]) y salen.
    pub apagar: Arc<AtomicBool>,
}

impl Contexto {
    /// El de producción: ajustes del sistema, adaptadores reales.
    pub fn sistema(abrir_bus: impl Fn() -> Option<Bus> + Send + Sync + 'static, sondeo: Duration) -> Self {
        Self {
            abrir_bus: Arc::new(abrir_bus),
            cargar_ajustes: Arc::new(ea_ajustes::cargar),
            fabricar: Arc::new(fabricar),
            sondeo,
            espera_candado: crate::ejecutor::ESPERA_CANDADO,
            latido: Duration::from_secs(INTERVALO_LATIDO_S),
            apagar: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// El supervisor: mantiene un trabajador por ejecutor despierto.
pub struct Trabajadores {
    ctx: Arc<Contexto>,
    activos: HashMap<String, JoinHandle<()>>,
}

impl Trabajadores {
    pub fn new(ctx: Contexto) -> Self {
        Self { ctx: Arc::new(ctx), activos: HashMap::new() }
    }

    /// Agentes con trabajador vivo (incluidos los que están acabando su
    /// último trabajo antes de irse).
    pub fn activos(&self) -> Vec<String> {
        let mut v: Vec<String> = self.activos.keys().cloned().collect();
        v.sort();
        v
    }

    /// Recoge a los que terminaron y lanza a los ejecutores despiertos que
    /// aún no tienen trabajador, sin pasar del tope. Los que dejaron de estar
    /// despiertos se van solos al acabar su trabajo en curso; mientras
    /// tanto cuentan para el tope.
    pub fn reconciliar(&mut self) {
        let acabados: Vec<String> = self.activos.iter().filter(|(_, h)| h.is_finished()).map(|(a, _)| a.clone()).collect();
        for a in acabados {
            if let Some(h) = self.activos.remove(&a)
                && h.join().is_err()
            {
                tracing::error!(agente = %a, "el trabajador terminó con un pánico");
            }
        }
        let ajustes = (self.ctx.cargar_ajustes)();
        let Some(bus) = (self.ctx.abrir_bus)() else { return };
        let equipos = bus.listar_equipos().unwrap_or_default();
        let tope = (ajustes.orquestador.paralelo as usize).max(1);
        for d in ejecutores_despiertos(&equipos, &ajustes) {
            if self.activos.contains_key(&d.agente) {
                continue;
            }
            if self.activos.len() >= tope {
                break;
            }
            tracing::info!(agente = %d.agente, equipo = ?d.equipo, "trabajador arrancado");
            let ctx = Arc::clone(&self.ctx);
            let agente = d.agente.clone();
            let h = std::thread::Builder::new()
                .name(format!("trabajador-{agente}"))
                .spawn(move || trabajador(&ctx, &d.agente, d.equipo.as_deref()))
                .expect("no se pudo lanzar el hilo del trabajador");
            self.activos.insert(agente, h);
        }
    }

    /// Pide a todos que terminen y espera a que lo hagan, como mucho
    /// [`ESPERA_PARADA`]. El agente y el criterio en curso ven `apagar` en su
    /// siguiente sondeo y mueren con su árbol; un trabajador que no acaba a
    /// tiempo se abandona (el proceso sale igual).
    pub fn detener(&mut self) {
        self.ctx.apagar.store(true, Ordering::SeqCst);
        let limite = Instant::now() + ESPERA_PARADA;
        while self.activos.values().any(|h| !h.is_finished()) && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(25));
        }
        for (agente, h) in self.activos.drain() {
            if !h.is_finished() {
                tracing::warn!(%agente, "el trabajador no terminó a tiempo; se abandona");
            } else if h.join().is_err() {
                tracing::error!(%agente, "el trabajador terminó con un pánico");
            }
        }
    }
}

/// Lo que [`Trabajadores::detener`] espera, como mucho, a los trabajadores.
pub const ESPERA_PARADA: Duration = Duration::from_secs(30);

/// El candado de instancia, en la raíz del bus.
pub const FICHERO_INSTANCIA: &str = "orquestador.lock";

/// Instancia única del orquestador: `flock` exclusivo sobre
/// `<raiz del bus>/orquestador.lock` mientras viva el valor (si el proceso
/// muere, lo suelta el sistema). No entra en el orden de candados del bus
/// (`Bus::bloquear_estado`): solo lo pide el orquestador, una vez al
/// arrancar, sin esperar y sin otro en la mano, y todos los del bus se toman
/// después, dentro de él.
#[derive(Debug)]
pub struct Instancia {
    _candado: std::fs::File,
}

impl Instancia {
    /// Toma el candado; `Err` si lo tiene otro proceso (o no se puede abrir).
    pub fn tomar(raiz: &Path) -> Result<Self, String> {
        let ruta = raiz.join(FICHERO_INSTANCIA);
        let f = OpenOptions::new().create(true).truncate(false).write(true).open(&ruta).map_err(|e| format!("no se pudo abrir {}: {e}", ruta.display()))?;
        match f.try_lock() {
            Ok(()) => Ok(Self { _candado: f }),
            Err(TryLockError::WouldBlock) => Err(format!("ya hay un ea-orchestrator en marcha (tiene {})", ruta.display())),
            Err(TryLockError::Error(e)) => Err(format!("no se pudo tomar {}: {e}", ruta.display())),
        }
    }
}

/// El arranque del orquestador: toma la [`Instancia`] y, solo con ella en la
/// mano, recupera lo que dejó a medias la anterior ([`Bus::recuperar`]). Sin
/// ella no toca nada: una segunda instancia devolvería a la cola lo que la
/// primera acaba de reclamar y daría por interrumpido lo que está ejecutando.
pub fn arrancar(bus: &Bus) -> Result<Instancia, String> {
    let instancia = Instancia::tomar(bus.raiz())?;
    match bus.recuperar() {
        Ok(v) if !v.is_empty() => tracing::warn!(trabajos = ?v, "trabajos recuperados al arrancar"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "no se pudo recuperar el bus"),
    }
    Ok(instancia)
}

/// Anota la presencia de un trabajador, que late mientras viva el valor. Sin
/// ella el trabajador trabaja igual.
fn anotar(bus: &Bus, agente: &str, equipo: Option<&str>, cada: Duration) -> Option<Latido> {
    let p = Presencia::nueva(agente, RolEquipo::Ejecutor, equipo, std::process::id());
    Latido::anotar(bus, &p, cada, |_| true).map_err(|e| tracing::warn!(%agente, error = %e, "no se pudo anotar la presencia")).ok()
}

/// Duerme `total` en rebanadas, saliendo antes si se pide apagar.
fn esperar(total: Duration, apagar: &AtomicBool) {
    let fin = Instant::now() + total;
    while !apagar.load(Ordering::SeqCst) {
        let resto = fin.saturating_duration_since(Instant::now());
        if resto.is_zero() {
            break;
        }
        std::thread::sleep(resto.min(Duration::from_millis(25)));
    }
}

/// El bucle de un trabajador. Sale cuando su agente deja de estar despierto
/// (tras acabar el trabajo en curso) o cuando se pide apagar.
fn trabajador(ctx: &Contexto, agente: &str, equipo: Option<&str>) {
    let Some(bus0) = (ctx.abrir_bus)() else {
        tracing::error!(%agente, "el trabajador no pudo abrir el bus");
        return;
    };
    let presente = anotar(&bus0, agente, equipo, ctx.latido);
    drop(bus0);
    while !ctx.apagar.load(Ordering::SeqCst) {
        let ajustes = (ctx.cargar_ajustes)();
        let Some(bus) = (ctx.abrir_bus)() else {
            esperar(ctx.sondeo, &ctx.apagar);
            continue;
        };
        let equipos = bus.listar_equipos().unwrap_or_default();
        if !ejecutores_despiertos(&equipos, &ajustes).iter().any(|d| d.agente == agente) {
            tracing::info!(%agente, "el agente ya no está despierto; el trabajador se va");
            break;
        }
        let Some(config) = ConfigOrquestador::para_agente(&ajustes, agente) else {
            tracing::warn!(%agente, "sin perfil de ejecutor local para el agente; se reintenta");
            esperar(ctx.sondeo, &ctx.apagar);
            continue;
        };
        let adaptador = (ctx.fabricar)(&config.ejecutor);
        let al_cambiar = |t: Option<&str>| {
            if let Some(p) = &presente {
                p.fijar(t);
            }
        };
        let es_del_rol = ajustes.agente_de(ea_ajustes::Rol::Ejecutor).0 == agente;
        let admite = |o: &ea_bus::Orden| le_toca(o, agente, es_del_rol, &equipos);
        let apagando = || ctx.apagar.load(Ordering::SeqCst);
        match ejecutar_intento(&bus, &config, adaptador.as_ref(), ctx.espera_candado, &al_cambiar, &admite, &apagando) {
            Intento::Hecho(id) => tracing::info!(%agente, %id, "trabajo procesado"),
            Intento::Ocupado(_) | Intento::Nada => esperar(ctx.sondeo, &ctx.apagar),
        }
    }
}
