//! El bucle del ejecutor: toma un trabajo de la cola del ejecutor
//! (`modelo-local`), lo pasa headless al agente que ocupa ese rol (Qwen
//! Code, Codex u OpenCode: ver [`crate::agentes`]) dentro del worktree,
//! comprueba el criterio de éxito POR SU CUENTA (el agente puede decir
//! «success» sin haber hecho nada) y entrega el informe a la revisión.
//!
//! - Misión: si el criterio falla y quedan iteraciones, repite la llamada en
//!   el mismo worktree con la evidencia del fallo; solo entrega al pasar, al
//!   agotarlas o cuando el agente dice que agotó sus topes.
//! - Obra: recorre las fases con ese mismo bucle, guarda una instantánea del
//!   árbol tras cada una y se detiene para revisión cada `revisar_cada`.
//! - Rutina: si pasa, se cierra sola; si falla, deja un paquete de
//!   corrección en espera hasta que el usuario lo confirme.
//!
//! Seguridad: con `sandbox = true` (por defecto) el agente se lanza aislado
//! (spec 020: `bwrap` en Linux, `sandbox-exec` en macOS; en Windows el
//! ejecutor no arranca, ver [`crate::sandbox`]). En Linux: sistema de ficheros de solo lectura salvo el worktree
//! (su `.git`, también de solo lectura), `/tmp` propio y sus directorios de estado; `$XDG_RUNTIME_DIR` tapado, para que no
//! lea la clave del panel web; la red queda abierta para llegar a
//! llama-server. Con `sandbox = false` Qwen y OpenCode corren con aprobación
//! automática y pueden escribir y ejecutar donde escriba el usuario. Además
//! limitan los topes de la orden (recortados al techo de los ajustes,
//! `agente.techo_topes`), el aislamiento por worktree y, en cada adaptador,
//! la exclusión de las herramientas web.
//!
//! El agente de cada trabajador se rehace en cada ciclo ([`fabricar`]) y queda anotado en el
//! trabajo al tomarlo (`ejecutado_por`); un agente que no se puede lanzar
//! deja el trabajo bloqueado con el motivo.
//!
//! Si el agente va aislado, el criterio corre en su mismo sandbox (solo
//! escribe en el worktree, su `/tmp` y `agente.criterio.escribibles`, y no ve
//! el directorio de ejecución): ver [`correr_criterio`].
//!
//! Cancelación cooperativa: el estado se relee antes de cada iteración y de
//! cada fase, y mientras corren el agente o el criterio se sondea cada
//! `SONDEO_CANCELACION` (ver [`ea_plataforma::proceso::correr`]). Los dos van
//! en su propio grupo de procesos (Job Object en Windows): si el trabajo pasa
//! a `cancelado` o vence su tope de tiempo, el árbol recibe `SIGTERM` y, tras
//! `MARGEN_TERM`, se mata siempre; al acabar la corrida muere también lo que
//! hubieran dejado de fondo.
//! Tras una cancelación no se corre el criterio (o se corta) ni se entrega
//! informe; lo que el agente ya escribió en el worktree se queda como está.
//! La parada del orquestador (`apagando`) corta igual, pero no anota nada: el
//! trabajo se queda `ejecutando` y [`Bus::recuperar`] lo da por interrumpido
//! al volver.
//!
//! Cada intento ([`ejecutar_intento`]) es una tubería de pasos, cada uno en
//! su módulo: `reclamo` (la entrada, la orden y sus topes), `ocupacion` (el
//! worktree, su semáforo y la presencia, que se sueltan solos al salir),
//! `mision` (el bucle de iteraciones) u `obra` (sus fases), con el
//! `criterio` que corre el sistema, y la entrega con su `aviso`.

mod aviso;
mod criterio;
mod mision;
mod obra;
mod ocupacion;
#[cfg(test)]
mod pruebas;
mod reclamo;

use std::path::Path;
use std::time::{Duration, Instant};

use ea_bus::{Artefactos, Bus, Entrega, Informe, Modo, Orden, ResultadoInforme, git};
use ea_plataforma::proceso::Corrida;

use crate::config::ConfigOrquestador;
use ocupacion::Ocupacion;

pub use crate::agentes::{Ejecutor, QwenCode, fabricar};
pub use criterio::correr_criterio;

/// Lo que se espera al semáforo de un worktree antes de devolver el trabajo
/// a su cola y seguir con otro (RF-009: no esperar bloqueado).
pub(crate) const ESPERA_CANDADO: Duration = Duration::from_secs(2);

/// Lo que devuelve el agente tras una ejecución.
#[derive(Debug, Clone, Default)]
pub struct SalidaAgente {
    /// Código de salida (`None` si murió por señal, por tiempo, o no arrancó).
    pub codigo: Option<i32>,
    /// Salida en bruto (para `salida-agente.json`).
    pub bruta: String,
    /// Texto final del agente (resumen), si se pudo extraer.
    pub resumen: String,
    /// `true` si se cortó porque el trabajo se canceló mientras corría.
    pub cancelado: bool,
}

/// Deja en el registro que el proceso no se pudo agrupar: al vencer el tope
/// (o cancelar) solo habrá muerto el líder.
pub(crate) fn avisar_sin_arbol(c: &Corrida) {
    if let Some(e) = &c.sin_arbol {
        tracing::warn!(error = %e, "no se pudo agrupar el proceso del agente: al vencer el tope solo morirá el líder");
    }
}

fn bloqueado(motivo: &str, evidencia: String) -> Informe {
    Informe { evidencia, motivo: Some(motivo.into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) }
}

/// Qué pasó en un intento de tomar y ejecutar un trabajo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intento {
    /// No había nada que tomar (o la cola falló).
    Nada,
    /// Se procesó (o se descartó una entrada ilegible): el id.
    Hecho(String),
    /// El semáforo del worktree estaba ocupado: el trabajo volvió a su cola.
    Ocupado(String),
}

impl Intento {
    pub fn id(self) -> Option<String> {
        match self {
            Intento::Nada => None,
            Intento::Hecho(id) | Intento::Ocupado(id) => Some(id),
        }
    }
}

/// Toma y ejecuta un trabajo para el ejecutor de `config.ejecutor`: primero de
/// la cola de ese agente y luego de la de rol `modelo-local`. Devuelve su id,
/// o `None` si no había nada (o la cola falló).
pub fn ejecutar_uno(bus: &Bus, config: &ConfigOrquestador, agente: &dyn Ejecutor) -> Option<String> {
    ejecutar_uno_con_espera(bus, config, agente, ESPERA_CANDADO)
}

fn ejecutar_uno_con_espera(bus: &Bus, config: &ConfigOrquestador, agente: &dyn Ejecutor, espera: Duration) -> Option<String> {
    ejecutar_intento(bus, config, agente, espera, &|_| {}, &|_| true, &|| false).id()
}

/// Lo que comparten los pasos de un trabajo ya tomado.
struct Contexto<'a> {
    bus: &'a Bus,
    config: &'a ConfigOrquestador,
    agente: &'a dyn Ejecutor,
    id: &'a str,
    orden: &'a Orden,
    artefactos: &'a Artefactos,
    /// Donde se trabaja: el de la orden o su worktree propio.
    wt: &'a Path,
    apagando: &'a dyn Fn() -> bool,
}

impl Contexto<'_> {
    /// Hay que dejar el trabajo: se canceló o el orquestador se para.
    fn cortar(&self) -> bool {
        self.bus.esta_cancelado(self.id) || (self.apagando)()
    }
}

/// Como [`ejecutar_uno`], pero dice si se topó con el semáforo y avisa de en
/// qué trabajo está (`al_cambiar(Some(id))` al empezar de verdad,
/// `al_cambiar(None)` al terminar) para la presencia del trabajador.
/// `admite` filtra lo que se toma de la cola de rol (la propia del agente se
/// toma entera): ver [`Bus::tomar_para_con`]. `apagando` dice si el
/// orquestador se está parando: el agente y el criterio lo miran como una
/// cancelación, pero el trabajo cortado así no se anota como cancelado ni se
/// entrega (sigue `ejecutando` hasta que [`Bus::recuperar`] lo marque).
pub fn ejecutar_intento(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn Ejecutor,
    espera: Duration,
    al_cambiar: &dyn Fn(Option<&str>),
    admite: &dyn Fn(&Orden) -> bool,
    apagando: &dyn Fn() -> bool,
) -> Intento {
    let reclamo::Reclamo { id, orden, artefactos } = match reclamo::reclamar(bus, config, admite) {
        Ok(r) => r,
        Err(intento) => return intento,
    };
    // Worktree propio (spec 019, RF-007): antes del semáforo, que es el suyo.
    // Si no se puede preparar (no es un repo git…), el trabajo queda
    // bloqueado con el motivo.
    let wt = match ocupacion::preparar_worktree(bus, &id, &orden) {
        Ok(wt) => wt,
        Err(e) => {
            if let Err(e2) = bus.tomar_con(&id, config.ejecutor.ejecutado_por()) {
                tracing::warn!(%id, error = %e2, "no se pudo pasar a ejecutando");
                return Intento::Hecho(id);
            }
            let motivo = e.to_string();
            tracing::warn!(%id, %motivo, "trabajo bloqueado");
            entregar(bus, config, &id, &orden, bloqueado(&motivo, orden.worktree.clone()));
            return Intento::Hecho(id);
        }
    };
    // Semáforo antes de pasar a Ejecutando: si está ocupado, el trabajo sigue
    // EnCola y vuelve a SU cola (la del agente al que se asignó, si su equipo
    // sigue despierto: `Bus::devolver`).
    let ocupacion = match Ocupacion::tomar(bus, config, &id, &wt, espera) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(%id, error = %e, "worktree ocupado; se vuelve a encolar");
            ocupacion::devolver(bus, &id, "worktree ocupado");
            return Intento::Ocupado(id);
        }
    };
    // La parada llegó antes de empezar: no se toma y vuelve a su cola.
    if apagando() {
        ocupacion.devolver(bus, &id, "parada del orquestador");
        return Intento::Nada;
    }
    let fase_actual = match bus.tomar_con(&id, config.ejecutor.ejecutado_por()) {
        Ok(e) => e.fase_actual,
        Err(e) => {
            tracing::warn!(%id, error = %e, "no se pudo pasar a ejecutando");
            return Intento::Hecho(id);
        }
    };
    let inicio = Instant::now();
    // Desde aquí, salga por donde salga, se suelta el semáforo y luego la presencia.
    let _ocupacion = ocupacion.anunciar(&id, al_cambiar);

    // Un ejecutor que no se puede lanzar deja el trabajo bloqueado con el
    // motivo, sin gastar iteraciones (spec 017).
    if let Err(m) = agente.disponible(config) {
        let motivo = format!("el ejecutor {} no está disponible: {m}", config.ejecutor.id);
        tracing::warn!(%id, %motivo, "trabajo bloqueado");
        entregar(bus, config, &id, &orden, bloqueado(&motivo, String::new()));
        return Intento::Hecho(id);
    }

    // El semáforo se mantiene durante todo el bucle de iteraciones y fases.
    let ctx = Contexto { bus, config, agente, id: &id, orden: &orden, artefactos: &artefactos, wt: &wt, apagando };
    match trabajar(&ctx, fase_actual, inicio) {
        Ok(informe) => entregar(bus, config, &id, &orden, informe),
        // Cortado por la parada y no cancelado: ni informe ni cancelación;
        // sigue `ejecutando` y `recuperar` lo entrega como «interrumpido».
        Err(_) if apagando() && !bus.esta_cancelado(&id) => {
            tracing::warn!(%id, "parada del orquestador: el trabajo se queda en ejecución y se dará por interrumpido al volver");
        }
        Err(c) => cancelado_en_curso(bus, &id, c.fase),
    }
    Intento::Hecho(id)
}

/// Hubo que dejar el trabajo mientras se ejecutaba (se canceló o el
/// orquestador se para): no hay informe que entregar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cancelado {
    /// En una obra, la fase en que se cortó.
    fase: Option<u32>,
}

/// El trabajo en su worktree: una obra por fases ([`obra`]) o, si no, el
/// bucle de una misión o una rutina ([`mision`]). Sin worktree o sin repo
/// git, bloqueado. El informe lleva lo que duró desde `inicio`.
fn trabajar(ctx: &Contexto, fase_actual: Option<u32>, inicio: Instant) -> Result<Informe, Cancelado> {
    let mut informe = if !ctx.wt.is_dir() {
        bloqueado("el worktree no existe", ctx.wt.display().to_string())
    } else if let Some(commit_inicio) = git::commit_actual(ctx.wt) {
        if ctx.orden.modo == Modo::Obra && !ctx.orden.fases.is_empty() {
            obra::trabajar(ctx, fase_actual, commit_inicio)?
        } else {
            mision::trabajar(ctx, commit_inicio)?
        }
    } else {
        bloqueado("el worktree no es un repo git", String::new())
    };
    informe.duracion_s = inicio.elapsed().as_secs();
    Ok(informe)
}

/// El trabajo se canceló mientras corría y el ejecutor dejó de trabajar en
/// él (`fase`: la de la obra en que se cortó). Lo deja en el historial y en
/// el diario; el estado ya es `cancelado` y el worktree queda como esté.
fn cancelado_en_curso(bus: &Bus, id: &str, fase: Option<u32>) {
    let iteracion = bus.leer_estado(id).map(|e| e.iteracion).unwrap_or(0);
    tracing::info!(%id, iteracion, ?fase, "trabajo cancelado en curso; se suelta el worktree");
    if let Err(e) = bus.anotar_cancelacion_en_curso(id, iteracion, fase) {
        tracing::warn!(%id, error = %e, "no se pudo anotar la cancelación en curso");
    }
}

/// Entrega el informe con [`Bus::entregar`], que decide según el modo (una
/// rutina que pasa se cierra sola; una que falla queda en espera con su
/// paquete de corrección), y avisa en el escritorio.
fn entregar(bus: &Bus, config: &ConfigOrquestador, id: &str, orden: &Orden, informe: Informe) {
    let motivo = informe.motivo.clone();
    let slug = orden.rutina.as_ref().map(|r| r.slug.as_str()).unwrap_or("?");
    match bus.entregar(id, informe) {
        Ok(Entrega::EnRevision) => aviso::avisar(config, &orden.titulo, &motivo.unwrap_or_else(|| "terminado".into()), id),
        Ok(Entrega::EnEspera) => aviso::avisar(config, &format!("rutina {slug} falló"), "paquete en espera", id),
        Ok(Entrega::Cerrado) => aviso::avisar(config, &format!("rutina {slug}"), "terminada", id),
        // Cancelado después de la última comprobación (p. ej. mientras corría
        // el criterio): no es un fallo, el trabajo se queda cancelado.
        Err(_) if bus.esta_cancelado(id) => cancelado_en_curso(bus, id, None),
        Err(e) => tracing::error!(%id, error = %e, "no se pudo entregar el informe"),
    }
}

#[cfg(test)]
mod tests {
    use super::pruebas::*;
    use super::*;
    #[cfg(target_os = "linux")]
    use crate::sandbox::Sandbox;
    use ea_bus::{CandadoWorktree, EstadoTrabajo, Rol, Topes};

    /// T-007 de la spec 017: una misión trivial con el agente de verdad,
    /// dentro de bwrap y contra el endpoint local de los ajustes por defecto.
    /// `EA_AGENTE_REAL=qwen-code|codex|opencode cargo test -p ea-orchestrator
    /// -- --ignored --nocapture agente_real` (`EA_AGENTE_BIN` cambia el binario
    /// `EA_AGENTE_OBJETIVO` la tarea y `EA_AGENTE_BASE_URL` el endpoint).
    #[test]
    #[ignore = "lanza un agente real contra llama-server"]
    fn mision_trivial_con_el_agente_real() {
        let id = std::env::var("EA_AGENTE_REAL").unwrap_or_else(|_| "opencode".into());
        let tipo = ea_bus::TipoAgente::desde(&id).expect("qwen-code, codex u opencode");
        let mut cfg = ConfigOrquestador { sandbox: true, ..config() };
        cfg.ejecutor.id = id.clone();
        cfg.ejecutor.tipo = tipo;
        cfg.ejecutor.bin = std::env::var("EA_AGENTE_BIN").unwrap_or_else(|_| if id == "qwen-code" { "qwen".into() } else { id.clone() });
        if let Ok(url) = std::env::var("EA_AGENTE_BASE_URL") {
            cfg.ejecutor.modelo.base_url = url;
        }
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "test -f ok.txt");
        o.topes = Topes { tiempo_s: 300, iteraciones: 1, ..Topes::default() };
        o.notas_revision.clear(); // «usa mayúsculas» haría crear OK.TXT
        // `EA_AGENTE_OBJETIVO` cambia la tarea (p. ej. «lista tus herramientas»).
        if let Ok(obj) = std::env::var("EA_AGENTE_OBJETIVO") {
            o.objetivo = format!("{obj} Después crea ok.txt.");
        }
        let t = bus.crear_trabajo(o).unwrap();
        let agente = fabricar(&cfg.ejecutor);
        let inicio = Instant::now();
        ejecutar_uno(&bus, &cfg, agente.as_ref());
        let inf = bus.leer_informe(&t).unwrap().unwrap();
        let bruta = std::fs::read_to_string(e.path().join("estado/trabajos").join(&t).join(ea_bus::SALIDA_AGENTE)).unwrap_or_default();
        println!(
            "{id}: {:?} en {:?}\nmotivo: {:?}\nevidencia:\n{}\n--- salida ({} bytes), final:\n{}",
            inf.resultado,
            inicio.elapsed(),
            inf.motivo,
            inf.evidencia,
            bruta.len(),
            &bruta[bruta.len().saturating_sub(1500)..]
        );
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert_eq!(inf.ejecutado_por.unwrap().agente, id);
        assert!(inf.evidencia.contains("resumen del agente"), "el resumen se extrajo de la salida");
    }

    /// Doble que, mientras ejecuta, cambia `roles.ejecutor` en los ajustes
    /// (como haría el usuario desde la web) y luego hace el trabajo.
    struct CambiaEjecutor {
        ajustes: std::path::PathBuf,
    }

    impl Ejecutor for CambiaEjecutor {
        fn ejecutar(&self, _: &str, _: &str, wt: &Path, _: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
            std::fs::create_dir_all(self.ajustes.parent().unwrap()).unwrap();
            std::fs::write(&self.ajustes, "[roles]\nejecutor = \"opencode\"\n").unwrap();
            std::fs::write(wt.join("ok.txt"), "").unwrap();
            SalidaAgente { codigo: Some(0), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
        }
    }

    #[test]
    fn cambiar_de_ejecutor_en_caliente_no_toca_el_trabajo_en_curso() {
        // Lo que hace `main` en cada ciclo, sobre unos ajustes aislados.
        let d = tempfile::tempdir().unwrap();
        let f = ea_ajustes::Fuentes::aislada(d.path());
        let ciclo = |bus: &Bus, agente: &dyn Ejecutor| {
            let cfg = ConfigOrquestador { avisos_escritorio: false, ..ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes) };
            ejecutar_uno(bus, &cfg, agente)
        };
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let a = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let b = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let cambia = CambiaEjecutor { ajustes: f.dir_config.join("ajustes.toml") };
        assert_eq!(ciclo(&bus, &cambia), Some(a.clone()));
        assert_eq!(ciclo(&bus, &doble("true", 0)), Some(b.clone()));
        let quien = |id: &str| bus.leer_informe(id).unwrap().unwrap().ejecutado_por.unwrap().agente;
        assert_eq!(quien(&a), "qwen-code", "el trabajo en curso termina con el ejecutor con que se tomó");
        assert_eq!(quien(&b), "opencode", "el siguiente sale con el nuevo");
        assert_eq!(bus.leer_estado(&b).unwrap().ejecutado_por.unwrap().modelo, "qwen3-coder-30b");
        // Y en el ciclo siguiente se fabrica el adaptador del nuevo ejecutor.
        let cfg = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
        assert_eq!(cfg.ejecutor.tipo, ea_bus::TipoAgente::OpenCode);
        assert!(fabricar(&cfg.ejecutor).codigo_topes().is_none(), "no es el de Qwen");
    }

    #[test]
    fn sin_repo_git_se_entrega_bloqueado() {
        let wt = tempfile::tempdir().unwrap();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert_eq!(inf.motivo.as_deref(), Some("el worktree no es un repo git"));
    }

    #[test]
    fn rutina_que_pasa_se_cierra_sola_y_no_va_a_ninguna_cola() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(rutina_orden(wt.path(), "test -f ok.txt")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("echo hola > ok.txt", 0));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Cerrado);
        assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
        assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
        assert!(!e.path().join("trabajos").join(&id).join("paquete-correccion.md").exists());
    }

    #[test]
    fn rutina_que_falla_queda_en_espera_con_paquete_y_confirmar_la_encola() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let toml = format!(
            "[rutina]\nslug = \"prueba\"\nversion = 1\nworktree = '{}'\ncriterio_exito = \"test -f ok.txt\"\n[[pasos]]\ntexto = \"crea ok.txt\"\n",
            wt.path().display()
        );
        bus.registrar_rutina(&toml, &e.path().join("rutinas")).unwrap();
        let id = bus.lanzar_rutina("prueba", &[]).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnEspera);
        assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("slug = \"prueba\"") && p.contains("test -f ok.txt") && p.contains("iteraciones agotadas"));
        bus.confirmar(&id, "revisa la ruta").unwrap();
        let c = bus.listar_cola(Rol::Planificador).unwrap();
        assert_eq!((c.len(), c[0].trabajo.as_str(), c[0].motivo.as_str()), (1, id.as_str(), "corrección de rutina"));
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("revisa la ruta"));
    }

    /// Historial (`evento:estado`) y diario (`acción:estado`) de un trabajo,
    /// en el orden en que se escribieron.
    fn rastro(e: &Path, id: &str) -> (Vec<String>, Vec<String>) {
        let h = std::fs::read_to_string(e.join("estado/trabajos").join(id).join("historial.jsonl")).unwrap();
        let historial = h
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .map(|v| format!("{}:{}", v["evento"].as_str().unwrap(), v["estado"].as_str().unwrap()))
            .collect();
        let mut eventos = ea_store::Store::open_at(&e.join("ea.db")).unwrap().list_events_by_trabajo(id).unwrap();
        eventos.sort_by_key(|ev| ev.id);
        let diario = eventos
            .into_iter()
            .map(|ev| {
                let d: serde_json::Value = ev.detail.as_deref().and_then(|d| serde_json::from_str(d).ok()).unwrap_or_default();
                format!("{}:{}", ev.action, d["estado"].as_str().unwrap_or("-"))
            })
            .collect();
        (historial, diario)
    }

    /// La entrega de cada modo deja la misma secuencia de transiciones en el
    /// historial y de eventos en el diario (fijada antes de mover la política
    /// de las rutinas al bus).
    #[test]
    fn la_entrega_de_cada_modo_deja_la_misma_secuencia_en_historial_y_diario() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let ejecutar = |o: Orden, script: &'static str| {
            let id = bus.crear_trabajo(o).unwrap();
            assert_eq!(ejecutar_uno(&bus, &config(), &doble(script, 0)), Some(id.clone()));
            id
        };
        let revision =
            (s(&["crear:en_cola", "tomar:ejecutando", "entregar:en_revision"]), s(&["bus_crear:-", "bus_tomar:ejecutando", "bus_entregar:en_revision"]));
        // Misión que pasa y que no pasa; obra a medias: a revisión.
        let mut no_pasa = orden(wt.path(), "false");
        no_pasa.topes.iteraciones = 1;
        for (o, script) in [(orden(wt.path(), "test -f ok.txt"), "echo hola > ok.txt"), (no_pasa, "true"), (fases3(wt.path(), Some(1)), SIGUIENTE)] {
            let id = ejecutar(o, script);
            assert_eq!(rastro(e.path(), &id), revision, "{id}");
        }
        // Rutina que pasa: se cierra sola.
        let id = ejecutar(rutina_orden(wt.path(), "true"), "true");
        assert_eq!(
            rastro(e.path(), &id),
            (
                s(&["crear:en_cola", "tomar:ejecutando", "entregar:en_revision", "cerrar:cerrado"]),
                s(&["bus_crear:-", "bus_tomar:ejecutando", "bus_entregar:en_revision", "bus_cerrar:cerrado"]),
            )
        );
        // Rutina que falla: en espera con su paquete.
        let id = ejecutar(rutina_orden(wt.path(), "false"), "true");
        assert_eq!(
            rastro(e.path(), &id),
            (
                s(&["crear:en_cola", "tomar:ejecutando", "entregar:en_revision", "poner_en_espera:en_espera"]),
                s(&["bus_crear:-", "bus_tomar:ejecutando", "bus_entregar:en_revision", "bus_poner_en_espera:en_espera"]),
            )
        );
        assert!(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md").is_file());
        // Sin repo git: bloqueado, por el mismo camino.
        let sin_git = tempfile::tempdir().unwrap();
        let id = ejecutar(orden(sin_git.path(), "true"), "true");
        assert_eq!(rastro(e.path(), &id), revision);
    }

    /// B-11: la parada del orquestador corta al agente (lo ve como una
    /// cancelación), pero el trabajo sigue `ejecutando`, sin informe ni
    /// `cancelado_en_curso`, y con el worktree libre; al volver, `recuperar`
    /// lo entrega como «interrumpido».
    #[test]
    fn apagar_durante_el_agente_deja_el_trabajo_ejecutando_para_recuperar() {
        use std::sync::atomic::Ordering::SeqCst;
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch criterio-corrio; true");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let ag = Apagador { apagar: Default::default(), lo_vio: Default::default() };
        let apagando = || ag.apagar.load(SeqCst);
        let r = ejecutar_intento(&bus, &config(), &ag, ESPERA_CANDADO, &|_| {}, &|_| true, &apagando);
        assert_eq!(r, Intento::Hecho(id.clone()));
        assert!(ag.lo_vio.load(SeqCst), "el agente no vio la parada");
        assert!(!wt.path().join("criterio-corrio").exists());
        let estado = bus.leer_estado(&id).unwrap();
        assert_eq!((estado.estado, estado.iteracion), (EstadoTrabajo::Ejecutando, 1));
        assert!(bus.leer_informe(&id).unwrap().is_none());
        assert!(cancelaciones_en_curso(e.path()).is_empty());
        let historial = std::fs::read_to_string(bus.raiz().join("trabajos").join(&id).join("historial.jsonl")).unwrap();
        assert!(!historial.contains("cancelado_en_curso"), "{historial}");
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none(), "el semáforo sigue cogido");
        // Al volver a arrancar.
        assert_eq!(bus.recuperar().unwrap(), vec![id.clone()]);
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnRevision);
        assert_eq!(bus.leer_informe(&id).unwrap().unwrap().motivo.as_deref(), Some("interrumpido"));
    }

    #[cfg(unix)]
    #[test]
    fn cancelar_durante_la_ejecucion_mata_al_agente() {
        use std::os::unix::fs::PermissionsExt;
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        // Un «qwen» que deja un nieto (`sleep` largo) en su mismo grupo y lo
        // espera: si solo muriera el hijo, el nieto seguiría vivo.
        let bin = tempfile::tempdir().unwrap();
        let qwen = bin.path().join("qwen-falso");
        std::fs::write(&qwen, "#!/bin/sh\nsleep 600 &\necho $! > pid-nieto\nwait\n").unwrap();
        std::fs::set_permissions(&qwen, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cfg = ConfigOrquestador { sandbox: false, ..config() };
        cfg.ejecutor.bin = qwen.to_string_lossy().into();
        let mut o = orden(wt.path(), "touch criterio-corrio; false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let hilo = cancelar_al_ver(bus.raiz(), &id, wt.path().join("pid-nieto"));
        let t = Instant::now();
        ejecutar_uno(&bus, &cfg, &QwenCode);
        assert!(hilo.join().unwrap(), "no se pudo cancelar");
        // Sondeo (2 s) + margen del TERM (5 s), con holgura.
        assert!(t.elapsed() < Duration::from_secs(15), "tardó {:?}", t.elapsed());
        let nieto: u32 = std::fs::read_to_string(wt.path().join("pid-nieto")).unwrap().trim().parse().unwrap();
        let vivo = ea_plataforma::proceso::vive;
        let limite = Instant::now() + Duration::from_secs(3);
        while vivo(nieto) && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!vivo(nieto), "el nieto {nieto} sigue vivo");
        assert!(!wt.path().join("criterio-corrio").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);
    }

    /// Doble que corre `guion` con `sh` dentro de bwrap de verdad, con el tope
    /// de tiempo de la orden y la cancelación del ejecutor (lo que hace cada
    /// adaptador, sin tocar los directorios de estado del HOME).
    #[cfg(target_os = "linux")]
    struct EnBwrap {
        guion: String,
        apagar: std::sync::atomic::AtomicBool,
    }

    #[cfg(target_os = "linux")]
    impl Ejecutor for EnBwrap {
        fn ejecutar(&self, _: &str, _: &str, wt: &Path, topes: &Topes, _: &ConfigOrquestador, cancelado: &dyn Fn() -> bool) -> SalidaAgente {
            let args = ["-c".to_string(), self.guion.clone()];
            let e = Sandbox::Bwrap.envolver("sh", &args, wt, &[], None).unwrap();
            let c = ea_plataforma::proceso::correr(e.orden, Duration::from_secs(topes.tiempo_s), cancelado);
            SalidaAgente { codigo: c.codigo, bruta: c.salida, resumen: String::new(), cancelado: c.cancelado }
        }
    }

    /// Los procesos vivos con `marca` en su línea de órdenes (pid, estado y orden).
    #[cfg(target_os = "linux")]
    fn vivos_con(marca: &str) -> Vec<String> {
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let orden = String::from_utf8_lossy(&std::fs::read(e.path().join("cmdline")).ok()?).replace('\0', " ");
                let estado = std::fs::read_to_string(e.path().join("stat")).ok()?;
                orden.contains(marca).then(|| format!("{} [{}] {orden}", e.file_name().to_string_lossy(), estado.split(' ').nth(2).unwrap_or("?")))
            })
            .collect()
    }

    /// ¿Puede bwrap crear sus espacios de nombres aquí?
    #[cfg(target_os = "linux")]
    fn bwrap_disponible(wt: &Path) -> bool {
        let Ok(e) = Sandbox::Bwrap.envolver("true", &[], wt, &[], None) else { return false };
        let mut c = e.orden;
        c.output().is_ok_and(|s| s.status.success()) || {
            eprintln!("bwrap no funciona aquí: prueba omitida");
            false
        }
    }

    /// Con el agente y el criterio dentro de bwrap: la cancelación y la
    /// parada matan todo el árbol del agente (también el nieto que deja de
    /// fondo) y sueltan el semáforo; el tope de tiempo lo corta y el criterio
    /// (aislado) decide. Lo que queda en el bus, como sin sandbox.
    #[cfg(target_os = "linux")]
    #[test]
    fn en_bwrap_la_cancelacion_el_tope_y_la_parada_matan_el_arbol_y_sueltan_el_semaforo() {
        use std::sync::atomic::Ordering::SeqCst;
        let wt = repo();
        if !bwrap_disponible(wt.path()) {
            return;
        }
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let cfg = ConfigOrquestador { sandbox: true, ..config() };
        let esperar_muerte = |marca: &str| {
            let limite = Instant::now() + Duration::from_secs(15);
            while !vivos_con(marca).is_empty() && Instant::now() < limite {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert_eq!(vivos_con(marca), Vec::<String>::new(), "sigue vivo un proceso con {marca}");
        };

        // Duraciones de `sleep` propias de este proceso: las marcas de sus árboles.
        let marca = |n: u32| format!("sleep {}", 100_000 + std::process::id() % 100_000 * 10 + n);
        let en_bwrap = |guion: String| EnBwrap { guion, apagar: Default::default() };

        // Cancelación mientras corre.
        let mut o = orden(wt.path(), "touch criterio-corrio; true");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let hilo = cancelar_al_ver(bus.raiz(), &id, wt.path().join("nieto-1"));
        let t = Instant::now();
        ejecutar_uno(&bus, &cfg, &en_bwrap(format!("{} & echo $! > nieto-1; wait", marca(1))));
        assert!(hilo.join().unwrap(), "no se pudo cancelar");
        assert!(t.elapsed() < Duration::from_secs(15), "tardó {:?}", t.elapsed());
        esperar_muerte(&marca(1));
        assert!(!wt.path().join("criterio-corrio").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);

        // Tope de tiempo: lo corta y el criterio, aislado, decide.
        let mut o = orden(wt.path(), "test -f ok-2.txt");
        (o.topes.iteraciones, o.topes.tiempo_s) = (1, 1);
        let id = bus.crear_trabajo(o).unwrap();
        ejecutar_uno(&bus, &cfg, &en_bwrap(format!("{}; touch ok-2.txt", marca(2))));
        esperar_muerte(&marca(2));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("iteraciones agotadas")));
        assert!(inf.evidencia.contains("código None"), "{}", inf.evidencia);
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none());

        // Parada mientras corre: el trabajo se queda en ejecución para recuperar.
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ag = en_bwrap(format!("{} & echo $! > nieto-3; wait", marca(3)));
        let nieto = wt.path().join("nieto-3");
        let apagando = || {
            if nieto.exists() {
                ag.apagar.store(true, SeqCst);
            }
            ag.apagar.load(SeqCst)
        };
        ejecutar_intento(&bus, &cfg, &ag, ESPERA_CANDADO, &|_| {}, &|_| true, &apagando);
        esperar_muerte(&marca(3));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Ejecutando);
        assert!(bus.leer_informe(&id).unwrap().is_none());
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none());
    }
}
