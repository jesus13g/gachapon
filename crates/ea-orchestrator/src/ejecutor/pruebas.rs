//! Lo que comparten las pruebas del ejecutor: dobles del agente, repos y
//! órdenes de prueba, y lo que debe quedar tras cortar un trabajo.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ea_bus::{Bus, CandadoWorktree, EstadoTrabajo, Evento, Modo, Orden, Rol, Topes};

use super::{Ejecutor, SalidaAgente};
use crate::config::ConfigOrquestador;

/// Doble: ejecuta un script de shell en el worktree en lugar de Qwen.
pub(super) struct Doble {
    pub(super) script: &'static str,
    pub(super) codigo: i32,
    pub(super) visto: Mutex<Vec<String>>,
    pub(super) sistemas: Mutex<Vec<String>>,
    pub(super) topes: Mutex<Vec<Topes>>,
}

impl Ejecutor for Doble {
    fn ejecutar(&self, prompt: &str, sistema: &str, wt: &Path, topes: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
        self.visto.lock().unwrap().push(prompt.to_string());
        self.topes.lock().unwrap().push(*topes);
        self.sistemas.lock().unwrap().push(sistema.to_string());
        let _ = Command::new("sh").arg("-c").arg(self.script).current_dir(wt).status();
        SalidaAgente { codigo: Some(self.codigo), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
    }
    /// Como Qwen Code: 55 es «topes agotados».
    fn codigo_topes(&self) -> Option<i32> {
        Some(55)
    }
}

pub(super) fn doble(script: &'static str, codigo: i32) -> Doble {
    Doble { script, codigo, visto: Mutex::new(vec![]), sistemas: Mutex::new(vec![]), topes: Mutex::new(vec![]) }
}

pub(super) fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for a in [&["init", "-q"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
        Command::new("git").args(a).current_dir(d.path()).status().unwrap();
    }
    std::fs::write(d.path().join("a.txt"), "a").unwrap();
    for a in [&["add", "."][..], &["commit", "-qm", "inicial"]] {
        Command::new("git").args(a).current_dir(d.path()).status().unwrap();
    }
    d
}

pub(super) fn orden(wt: &Path, criterio: &str) -> Orden {
    Orden { notas_revision: vec!["usa mayúsculas".into()], ..Orden::mision("prueba", wt.to_string_lossy(), "crea ok.txt", criterio, "test") }
}

pub(super) fn inf_obra_completa(bus: &Bus, id: &str) -> bool {
    bus.leer_informe(id).unwrap().unwrap().obra_completa
}

pub(super) fn con_store(e: &Path) -> Bus {
    let s = ea_store::Store::open_at(&e.join("ea.db")).unwrap();
    Bus::abrir_en(&e.join("estado")).unwrap().con_store(s)
}

pub(super) fn fases3(wt: &Path, revisar_cada: Option<u32>) -> Orden {
    let fase = |n: u32| ea_bus::Fase {
        numero: n,
        titulo: format!("f{n}"),
        instrucciones: format!("crea el fichero f{n}"),
        entregable: format!("f{n}"),
        criterio: format!("test -f f{n}"),
        skills: vec![],
    };
    Orden {
        modo: Modo::Obra,
        criterio_exito: None,
        fases: vec![fase(1), fase(2), fase(3)],
        revisar_cada,
        topes: Topes { iteraciones: 2, ..Topes::default() },
        ..orden(wt, "")
    }
}

/// Crea el siguiente `fN` que falte.
pub(super) const SIGUIENTE: &str = "for f in f1 f2 f3; do [ -f $f ] || { echo x > $f; break; }; done";

pub(super) fn rutina_orden(wt: &Path, criterio: &str) -> Orden {
    Orden {
        modo: Modo::Rutina,
        titulo: "Rutina prueba".into(),
        rutina: Some(ea_bus::RutinaRef { slug: "prueba".into(), version: 1 }),
        notas_revision: vec![],
        skills: vec![],
        topes: Topes { iteraciones: 1, ..Topes::default() },
        ..orden(wt, criterio)
    }
}

/// Sin sandbox: el criterio corre tal cual (los dobles no lo usan). El
/// criterio aislado tiene sus pruebas (`tests/paralelo.rs`).
pub(super) fn config() -> ConfigOrquestador {
    ConfigOrquestador { avisos_escritorio: false, sandbox: false, ..Default::default() }
}

pub(super) fn config_con_skills(dir: &Path) -> ConfigOrquestador {
    std::fs::write(dir.join("rust.md"), "---\ndescripcion: Rust\n---\nUsa cargo test.\n").unwrap();
    std::fs::write(dir.join("tests.md"), "---\ndescripcion: tests\n---\nPrueba primero.\n").unwrap();
    let mut c = config();
    c.skills.dir = dir.to_string_lossy().into();
    c
}

/// Doble que cuenta sus llamadas y, en la primera, cancela el trabajo
/// desde fuera (con otro `Bus`, como la web o la CLI) antes de volver.
pub(super) struct Cancelador {
    pub(super) raiz: std::path::PathBuf,
    pub(super) llamadas: std::sync::atomic::AtomicUsize,
}

impl Ejecutor for Cancelador {
    fn ejecutar(&self, _: &str, _: &str, _: &Path, _: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
        if self.llamadas.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            let bus = Bus::abrir_en(&self.raiz).unwrap();
            let id = bus.listar_trabajos().unwrap().pop().unwrap();
            bus.aplicar(&id, Evento::Cancelar).unwrap();
        }
        SalidaAgente { codigo: Some(0), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
    }
}

pub(super) fn cancelador(bus: &Bus) -> Cancelador {
    Cancelador { raiz: bus.raiz().to_path_buf(), llamadas: Default::default() }
}

/// Detalles (JSON) de los `bus_cancelado_en_curso` del diario.
pub(super) fn cancelaciones_en_curso(e: &Path) -> Vec<serde_json::Value> {
    ea_store::Store::open_at(&e.join("ea.db"))
        .unwrap()
        .list_events(&Default::default())
        .unwrap()
        .into_iter()
        .filter(|ev| ev.action == "bus_cancelado_en_curso")
        .filter_map(|ev| serde_json::from_str(ev.detail.as_deref()?).ok())
        .collect()
}

/// Lo que debe quedar tras cortar un trabajo cancelado: cancelado, sin
/// informe, en ninguna cola y con el worktree libre.
pub(super) fn queda_cancelado_y_suelto(bus: &Bus, id: &str, wt: &Path) {
    assert_eq!(bus.leer_estado(id).unwrap().estado, EstadoTrabajo::Cancelado);
    assert!(bus.leer_informe(id).unwrap().is_none());
    assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
    assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt).is_none(), "el semáforo sigue cogido");
}

/// Espera (como mucho `max`) a que exista un fichero.
pub(super) fn esperar_fichero(p: &Path, max: Duration) -> bool {
    let limite = Instant::now() + max;
    while Instant::now() < limite {
        if p.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Cancela el trabajo `id` desde otro hilo en cuanto aparezca `marca`.
pub(super) fn cancelar_al_ver(raiz: &Path, id: &str, marca: std::path::PathBuf) -> std::thread::JoinHandle<bool> {
    let (raiz, id) = (raiz.to_path_buf(), id.to_string());
    std::thread::spawn(move || {
        if !esperar_fichero(&marca, Duration::from_secs(20)) {
            return false;
        }
        Bus::abrir_en(&raiz).unwrap().aplicar(&id, Evento::Cancelar).is_ok()
    })
}

/// Doble que hace de señal de parada a mitad de la corrida: enciende
/// `apagar` y apunta si el ejecutor se lo hace ver por `cancelado`.
pub(super) struct Apagador {
    pub(super) apagar: std::sync::atomic::AtomicBool,
    pub(super) lo_vio: std::sync::atomic::AtomicBool,
}

impl Ejecutor for Apagador {
    fn ejecutar(&self, _: &str, _: &str, _: &Path, _: &Topes, _: &ConfigOrquestador, cancelado: &dyn Fn() -> bool) -> SalidaAgente {
        use std::sync::atomic::Ordering::SeqCst;
        self.apagar.store(true, SeqCst);
        self.lo_vio.store(cancelado(), SeqCst);
        SalidaAgente { codigo: None, bruta: "[]".into(), resumen: String::new(), cancelado: self.lo_vio.load(SeqCst) }
    }
}

/// Lo que ha visto la presencia de un trabajador, en orden.
pub(super) type Vistas = Arc<Mutex<Vec<String>>>;

/// Las llamadas a `al_cambiar` (la presencia del trabajador) y, en cada
/// `None`, si el semáforo del worktree ya estaba libre.
pub(super) fn presencia(raiz: &Path, wt: &Path) -> (impl Fn(Option<&str>), Vistas) {
    let vistas = Arc::new(Mutex::new(Vec::new()));
    let (v, raiz, wt) = (Arc::clone(&vistas), raiz.to_path_buf(), wt.to_path_buf());
    let f = move |t: Option<&str>| {
        let libre = CandadoWorktree::ocupante_en(&raiz, &wt).is_none();
        v.lock().unwrap().push(match t {
            Some(id) => format!("en {id}"),
            None => format!("libre (semáforo suelto: {libre})"),
        });
    };
    (f, vistas)
}
