//! El worktree de un trabajo: prepararlo (el worktree propio, spec 019,
//! RF-007) y ocuparlo con el semáforo exclusivo y la presencia del
//! trabajador ([`Ocupacion`]), que se sueltan solos al salir, pase lo que
//! pase.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ea_bus::{Bus, CandadoWorktree, Ocupante, Orden, Resultado, Rol};

use crate::config::ConfigOrquestador;

/// `git worktree add` toca el `.git` común del repo: de dos en dos, no a la vez.
static PREPARAR_WORKTREE: Mutex<()> = Mutex::new(());

/// Donde trabaja la orden: el suyo o, con `worktree_propio`, el que
/// [`Bus::preparar_worktree`] crea o reutiliza, de uno en uno.
pub(super) fn preparar_worktree(bus: &Bus, id: &str, orden: &Orden) -> Resultado<PathBuf> {
    let _a_solas = PREPARAR_WORKTREE.lock().unwrap_or_else(|e| e.into_inner());
    bus.preparar_worktree(id, orden)
}

/// Devuelve a su cola un trabajo reclamado que no se llegó a tomar
/// ([`Bus::devolver`]); un fallo queda en el registro.
pub(super) fn devolver(bus: &Bus, id: &str, motivo: &str) {
    if let Err(e) = bus.devolver(id, motivo) {
        tracing::error!(%id, error = %e, "no se pudo devolver el trabajo a su cola");
    }
}

/// Lo que avisa a la presencia del trabajador en qué trabajo está.
type AlCambiar<'a> = &'a dyn Fn(Option<&str>);

/// El worktree ocupado por un trabajo: el semáforo exclusivo y, desde que el
/// trabajo empieza de verdad ([`Ocupacion::anunciar`]), la presencia del
/// trabajador en él. Al soltarse, también si algo falla a mitad, suelta
/// primero el semáforo y después la presencia (`al_cambiar(None)`).
pub(super) struct Ocupacion<'a> {
    candado: Option<CandadoWorktree>,
    al_cambiar: Option<AlCambiar<'a>>,
}

impl<'a> Ocupacion<'a> {
    /// Toma el semáforo de `wt` para el trabajo `id`, esperando como mucho
    /// `espera` (RF-009: no esperar bloqueado).
    pub(super) fn tomar(bus: &Bus, config: &ConfigOrquestador, id: &str, wt: &Path, espera: Duration) -> Resultado<Self> {
        let ocupante = Ocupante { agente: config.ejecutor.id.clone(), trabajo: id.to_string(), pid: 0, desde: String::new(), rol: Some(Rol::Ejecutor) };
        let candado = CandadoWorktree::adquirir_en(bus.raiz(), wt, ocupante, espera)?;
        Ok(Self { candado: Some(candado), al_cambiar: None })
    }

    /// El trabajo empieza: la presencia lo dice (`al_cambiar(Some(id))`)
    /// hasta que se suelte la ocupación.
    pub(super) fn anunciar(mut self, id: &str, al_cambiar: AlCambiar<'a>) -> Self {
        al_cambiar(Some(id));
        self.al_cambiar = Some(al_cambiar);
        self
    }

    /// Suelta el worktree y devuelve a su cola el trabajo, que no se tomó.
    pub(super) fn devolver(self, bus: &Bus, id: &str, motivo: &str) {
        drop(self);
        devolver(bus, id, motivo);
    }
}

impl Drop for Ocupacion<'_> {
    fn drop(&mut self) {
        drop(self.candado.take());
        if let Some(al_cambiar) = self.al_cambiar {
            al_cambiar(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::ejecutor::pruebas::*;
    use crate::ejecutor::*;
    use ea_bus::{Bus, CandadoWorktree, Cola, EstadoTrabajo, Evento, Ocupante, Orden, ResultadoInforme, Rol};

    #[test]
    fn candado_ocupado_vuelve_a_encolar() {
        // La espera real es de 60 s; la prueba usa una corta.
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: Some(Rol::Planificador) };
        let _c = CandadoWorktree::adquirir_en(e.path(), wt.path(), ocupante, Duration::from_secs(1)).unwrap();
        assert_eq!(ejecutar_uno_con_espera(&bus, &config(), &doble("true", 0), Duration::from_millis(200)), Some(id.clone()));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
        assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].motivo, "worktree ocupado");
    }

    /// B-12: si el equipo se duerme entre el reclamo y la vuelta por «worktree
    /// ocupado», el trabajo va a la cola del rol y no a la del agente, que ya
    /// nadie atiende.
    #[test]
    fn ocupado_con_el_equipo_dormido_entre_medias_vuelve_a_la_cola_del_rol() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let mut a = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(&e.path().join("config")));
        a.orquestador.paralelo = 4;
        let miembros = vec![
            ea_bus::Miembro { agente: "claude-code".into(), rol: ea_bus::RolEquipo::Planificador },
            ea_bus::Miembro { agente: "codex".into(), rol: ea_bus::RolEquipo::Ejecutor },
        ];
        let nuevo = ea_bus::NuevoEquipo {
            titulo: "t".into(),
            objetivo: "o".into(),
            worktree_base: wt.path().to_string_lossy().into(),
            miembros,
            creado_por: "ea-cli".into(),
        };
        let eq = bus.crear_equipo(nuevo, &a).unwrap();
        let o = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), ..orden(wt.path(), "true") };
        let id = bus.crear_trabajo(o).unwrap();
        let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: Some(Rol::Planificador) };
        let _c = CandadoWorktree::adquirir_en(e.path(), wt.path(), ocupante, Duration::from_secs(1)).unwrap();
        // En cuanto el trabajador reclama la entrada, el usuario duerme el equipo.
        let dormir = {
            let (raiz, eq) = (e.path().to_path_buf(), eq.id.clone());
            std::thread::spawn(move || {
                let bus = Bus::abrir_en(&raiz).unwrap();
                let limite = Instant::now() + Duration::from_secs(10);
                while !bus.listar_cola(Cola::Agente("codex")).unwrap().is_empty() && Instant::now() < limite {
                    std::thread::sleep(Duration::from_millis(5));
                }
                bus.dormir_equipo(&eq).unwrap();
            })
        };
        let mut cfg = config();
        cfg.ejecutor.id = "codex".into();
        assert_eq!(ejecutar_intento(&bus, &cfg, &doble("true", 0), Duration::from_secs(1), &|_| {}, &|_| true, &|| false), Intento::Ocupado(id.clone()));
        dormir.join().unwrap();
        assert!(bus.listar_cola(Cola::Agente("codex")).unwrap().is_empty(), "el trabajo quedó en la cola de un agente dormido");
        let cola = bus.listar_cola(Rol::Ejecutor).unwrap();
        assert_eq!((cola.len(), cola[0].trabajo.as_str(), cola[0].motivo.as_str()), (1, id.as_str(), "worktree ocupado"));
    }

    /// Con la parada ya pedida, lo que se acaba de reclamar no se empieza:
    /// vuelve a su cola sin llamar al agente.
    #[test]
    fn con_la_parada_pedida_el_trabajo_reclamado_vuelve_a_su_cola() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ag = doble("true", 0);
        assert_eq!(ejecutar_intento(&bus, &config(), &ag, ESPERA_CANDADO, &|_| {}, &|_| true, &|| true), Intento::Nada);
        assert!(ag.visto.lock().unwrap().is_empty());
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
        assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].trabajo, id);
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none());
    }

    /// Si el trabajo se cancela mientras el ejecutor espera el semáforo, no
    /// se toma: el semáforo se suelta, sin presencia ni informe.
    #[test]
    fn cancelado_mientras_espera_el_semaforo_no_se_toma_y_lo_suelta() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: Some(Rol::Planificador) };
        let candado = CandadoWorktree::adquirir_en(e.path(), wt.path(), ocupante, Duration::from_secs(1)).unwrap();
        let (raiz, wt2) = (e.path().to_path_buf(), wt.path().to_path_buf());
        let hilo = std::thread::spawn(move || {
            let bus = Bus::abrir_en(&raiz).unwrap();
            let ag = doble("true", 0);
            let (al_cambiar, vistas) = presencia(&raiz, &wt2);
            let r = ejecutar_intento(&bus, &config(), &ag, Duration::from_secs(20), &al_cambiar, &|_| true, &|| false);
            (r, ag.visto.lock().unwrap().len(), vistas.lock().unwrap().clone())
        });
        // En cuanto la entrada está reclamada, se cancela y se suelta el semáforo.
        let limite = Instant::now() + Duration::from_secs(10);
        while !bus.listar_cola(Rol::Ejecutor).unwrap().is_empty() && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(5));
        }
        bus.aplicar(&id, Evento::Cancelar).unwrap();
        drop(candado);
        let (r, llamadas, vistas) = hilo.join().unwrap();
        assert_eq!((r, llamadas, vistas), (Intento::Hecho(id.clone()), 0, vec![]));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Cancelado);
        assert!(bus.leer_informe(&id).unwrap().is_none());
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none(), "el semáforo sigue cogido");
    }

    /// En toda salida tras empezar (entrega, ejecutor no disponible,
    /// cancelación, parada) se suelta primero el semáforo y luego la
    /// presencia; si no llegó a empezar, la presencia no se toca.
    #[test]
    fn al_salir_se_suelta_el_semaforo_y_despues_la_presencia() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let empezado = |id: &str| vec![format!("en {id}"), "libre (semáforo suelto: true)".to_string()];
        let intento = |ag: &dyn Ejecutor, cfg: &ConfigOrquestador, apagando: &dyn Fn() -> bool| {
            let (al_cambiar, vistas) = presencia(bus.raiz(), wt.path());
            let r = ejecutar_intento(&bus, cfg, ag, ESPERA_CANDADO, &al_cambiar, &|_| true, apagando);
            assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt.path()).is_none(), "el semáforo sigue cogido");
            (r, vistas.lock().unwrap().clone())
        };
        // Entregado.
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        assert_eq!(intento(&doble("true", 0), &config(), &|| false), (Intento::Hecho(id.clone()), empezado(&id)));
        // Ejecutor no disponible.
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let mut roto = config();
        roto.ejecutor.bin = "/no/existe/qwen".into();
        assert_eq!(intento(&QwenCode, &roto, &|| false), (Intento::Hecho(id.clone()), empezado(&id)));
        assert_eq!(bus.leer_informe(&id).unwrap().unwrap().resultado, ResultadoInforme::Bloqueado);
        // Cancelado en curso.
        let id = bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        assert_eq!(intento(&cancelador(&bus), &config(), &|| false), (Intento::Hecho(id.clone()), empezado(&id)));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Cancelado);
        // Parada en curso.
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ag = Apagador { apagar: Default::default(), lo_vio: Default::default() };
        let apagando = || ag.apagar.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(intento(&ag, &config(), &apagando), (Intento::Hecho(id.clone()), empezado(&id)));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Ejecutando);
        bus.aplicar(&id, Evento::Cancelar).unwrap();
        // Parada antes de empezar: vuelve a su cola y la presencia no se toca.
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        assert_eq!(intento(&doble("true", 0), &config(), &|| true), (Intento::Nada, vec![]));
        assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].trabajo, id);
    }
}
