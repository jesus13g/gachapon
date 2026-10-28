//! Trabajadores en paralelo (spec 019, F3): varios ejecutores a la vez, cada
//! uno con su cola, su worktree y su presencia. Todo con un doble del agente
//! y directorios temporales; sin esperas largas (canales y sondeo de 10 ms).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ea_ajustes::{Ajustes, Fuentes};
use ea_bus::{
    Bus, EstadoTrabajo, Miembro, Modo, NuevoEquipo, Orden, Presencia, ResultadoInforme, Rol, RolEquipo, Topes,
};
use ea_orchestrator::config::ConfigOrquestador;
use ea_orchestrator::ejecutor::{correr_criterio, ejecutar_uno, Ejecutor, SalidaAgente};
use ea_orchestrator::sandbox::Sandbox;
use ea_orchestrator::trabajadores::{arrancar, ejecutores_despiertos, Contexto, Trabajadores};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const LIMITE: Duration = Duration::from_secs(20);

fn esperar_que(mut cond: impl FnMut() -> bool, que: &str) {
    let fin = Instant::now() + LIMITE;
    while !cond() {
        assert!(Instant::now() < fin, "no ocurrió a tiempo: {que}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let s = Command::new("git")
        .args(["-c", "user.email=a@b", "-c", "user.name=x", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(s.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&s.stderr));
    String::from_utf8_lossy(&s.stdout).trim().to_string()
}

fn repo(d: &Path, nombre: &str) -> PathBuf {
    let r = d.join(nombre);
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    std::fs::write(r.join("a.txt"), "a").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "inicial"]);
    r
}

fn orden(wt: &Path) -> Orden {
    Orden {
        id: String::new(),
        modo: Modo::Mision,
        titulo: "prueba".into(),
        worktree: wt.to_string_lossy().into(),
        objetivo: "crea ok.txt".into(),
        criterio_exito: Some("test -f ok.txt".into()),
        fases: vec![],
        revisar_cada: None,
        topes: Topes::default(),
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        equipo: None,
        asignado_a: None,
        revisor: None,
        worktree_propio: false,
        creada_en: String::new(),
        creada_por: "claude-code".into(),
    }
}

fn m(agente: &str, rol: RolEquipo) -> Miembro {
    Miembro { agente: agente.into(), rol }
}

/// Lo que hace el doble cuando lo llaman, además de crear `ok.txt`.
type Espera = Box<dyn Fn(&Path) + Send + Sync>;

/// Doble del agente: anota quién (el id del ejecutor de la config) y dónde
/// (el cwd) lo llamaron, cuántos corren a la vez, y crea `ok.txt`.
struct Doble {
    llamadas: Mutex<Vec<(String, PathBuf)>>,
    dentro: AtomicUsize,
    maximo: AtomicUsize,
    espera: Espera,
}

impl Doble {
    fn nuevo(espera: Espera) -> Arc<Self> {
        Arc::new(Self { llamadas: Mutex::new(vec![]), dentro: AtomicUsize::new(0), maximo: AtomicUsize::new(0), espera })
    }
    fn simple() -> Arc<Self> {
        Self::nuevo(Box::new(|_| {}))
    }
    /// Espera a que `n` llamadas coincidan dentro a la vez (o falla a los 20 s).
    fn encuentro(n: usize) -> Arc<Self> {
        let llegadas = Arc::new(AtomicUsize::new(0));
        Self::nuevo(Box::new(move |_| {
            llegadas.fetch_add(1, Ordering::SeqCst);
            esperar_que(|| llegadas.load(Ordering::SeqCst) >= n, "las llamadas no coincidieron");
        }))
    }
    /// El primero que llega avisa por `dentro` y se queda hasta que llegue `soltar`.
    fn retenido() -> (Arc<Self>, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (dentro_tx, dentro_rx) = mpsc::channel::<()>();
        let (soltar_tx, soltar_rx) = mpsc::channel::<()>();
        let dentro_tx = Mutex::new(dentro_tx);
        let soltar_rx = Mutex::new(soltar_rx);
        let primera = AtomicBool::new(true);
        let doble = Self::nuevo(Box::new(move |_| {
            if primera.swap(false, Ordering::SeqCst) {
                dentro_tx.lock().unwrap().send(()).unwrap();
                soltar_rx.lock().unwrap().recv_timeout(LIMITE).unwrap();
            }
        }));
        (doble, dentro_rx, soltar_tx)
    }
    fn maximo(&self) -> usize {
        self.maximo.load(Ordering::SeqCst)
    }
    fn llamadas(&self) -> Vec<(String, PathBuf)> {
        self.llamadas.lock().unwrap().clone()
    }
}

struct Adaptador(Arc<Doble>);

impl Ejecutor for Adaptador {
    fn ejecutar(&self, _: &str, _: &str, wt: &Path, _: &Topes, config: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
        let d = &self.0;
        let ahora = d.dentro.fetch_add(1, Ordering::SeqCst) + 1;
        d.maximo.fetch_max(ahora, Ordering::SeqCst);
        d.llamadas.lock().unwrap().push((config.ejecutor.id.clone(), wt.to_path_buf()));
        (d.espera)(wt);
        std::fs::write(wt.join("ok.txt"), "hola").unwrap();
        d.dentro.fetch_sub(1, Ordering::SeqCst);
        SalidaAgente { codigo: Some(0), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
    }
}

struct Entorno {
    d: tempfile::TempDir,
    ajustes: Ajustes,
    bus: Bus,
    apagar: Arc<AtomicBool>,
}

impl Entorno {
    fn nuevo(paralelo: u32) -> Self {
        let d = tempfile::tempdir().unwrap();
        let mut ajustes = Ajustes::por_defecto(&Fuentes::aislada(&d.path().join("ajustes")));
        ajustes.orquestador.paralelo = paralelo;
        ajustes.agente.avisos_escritorio = false;
        ajustes.agente.sandbox = false;
        let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
        Self { d, ajustes, bus, apagar: Arc::new(AtomicBool::new(false)) }
    }

    fn raiz(&self) -> PathBuf {
        self.bus.raiz().to_path_buf()
    }

    /// Despierta un equipo con qwen-code y codex de ejecutores.
    fn equipo(&self) -> String {
        let nuevo = NuevoEquipo {
            titulo: "equipo".into(),
            objetivo: "probar".into(),
            worktree_base: self.d.path().to_string_lossy().into(),
            miembros: vec![m("claude-code", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor), m("codex", RolEquipo::Ejecutor)],
            creado_por: "ea-cli".into(),
        };
        // Se despierta con holgura; el tope de los ajustes del orquestador puede
        // bajarse después (lo que prueban algunas pruebas).
        let mut holgado = self.ajustes.clone();
        holgado.orquestador.paralelo = 8;
        self.bus.crear_equipo(nuevo, &holgado).unwrap().id
    }

    fn contexto(&self, doble: &Arc<Doble>) -> Contexto {
        let raiz = self.raiz();
        let ajustes = self.ajustes.clone();
        let doble = Arc::clone(doble);
        Contexto {
            abrir_bus: Arc::new(move || Bus::abrir_en(&raiz).ok()),
            cargar_ajustes: Arc::new(move || ajustes.clone()),
            fabricar: Arc::new(move |_| Box::new(Adaptador(Arc::clone(&doble)))),
            sondeo: Duration::from_millis(10),
            espera_candado: Duration::from_millis(60),
            latido: Duration::from_millis(20),
            apagar: Arc::clone(&self.apagar),
        }
    }

    fn estado(&self, id: &str) -> EstadoTrabajo {
        self.bus.leer_estado(id).unwrap().estado
    }

    fn en_revision(&self, ids: &[&String]) -> bool {
        ids.iter().all(|i| self.estado(i) == EstadoTrabajo::EnRevision)
    }
}

impl Drop for Entorno {
    fn drop(&mut self) {
        self.apagar.store(true, Ordering::SeqCst);
    }
}

#[test]
fn los_ejecutores_despiertos_salen_de_los_equipos_con_el_tope_y_sin_equipos_es_el_del_rol() {
    let e = Entorno::nuevo(1);
    let agentes = |a: &Ajustes| -> Vec<String> {
        ejecutores_despiertos(&e.bus.listar_equipos().unwrap_or_default(), a).into_iter().map(|d| d.agente).collect()
    };
    assert_eq!(agentes(&e.ajustes), ["qwen-code"], "sin equipos: roles.ejecutor");
    let id = e.equipo();
    assert_eq!(agentes(&e.ajustes), ["qwen-code"], "paralelo = 1 recorta al primero");
    let mut a2 = e.ajustes.clone();
    a2.orquestador.paralelo = 2;
    let v = ejecutores_despiertos(&e.bus.listar_equipos().unwrap(), &a2);
    assert_eq!(v.iter().map(|d| d.agente.as_str()).collect::<Vec<_>>(), ["qwen-code", "codex"]);
    assert!(v.iter().all(|d| d.equipo.as_deref() == Some(id.as_str())));
    e.bus.dormir_equipo(&id).unwrap();
    assert_eq!(agentes(&a2), ["qwen-code"], "dormido: otra vez solo el del rol");
}

#[test]
fn dos_trabajos_en_worktrees_distintos_corren_a_la_vez() {
    let e = Entorno::nuevo(2);
    let eq = e.equipo();
    let a = repo(e.d.path(), "a");
    let b = repo(e.d.path(), "b");
    let ia = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), ..orden(&a) }).unwrap();
    let ib = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), ..orden(&b) }).unwrap();
    let doble = Doble::encuentro(2);
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    assert_eq!(t.activos(), ["codex", "qwen-code"]);
    esperar_que(|| e.en_revision(&[&ia, &ib]), "ambos trabajos entregados");
    assert_eq!(doble.maximo(), 2, "corrieron solapados");
    let quien: Vec<String> = doble.llamadas().into_iter().map(|(q, _)| q).collect();
    assert!(quien.contains(&"qwen-code".into()) && quien.contains(&"codex".into()), "{quien:?}");
    t.detener();
}

#[test]
fn dos_trabajos_en_el_mismo_worktree_no_corren_a_la_vez() {
    let e = Entorno::nuevo(2);
    let eq = e.equipo();
    let r = repo(e.d.path(), "r");
    let i1 = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), ..orden(&r) }).unwrap();
    let i2 = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), ..orden(&r) }).unwrap();
    let (doble, dentro, soltar) = Doble::retenido();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    dentro.recv_timeout(LIMITE).unwrap();
    // El otro trabajador se topa con el semáforo y devuelve el trabajo a la cola.
    esperar_que(
        || e.bus.listar_cola(Rol::Ejecutor).unwrap().iter().any(|x| x.motivo == "worktree ocupado"),
        "el trabajo vuelve a la cola de rol",
    );
    assert_eq!(doble.llamadas().len(), 1, "solo uno ha llegado al agente");
    soltar.send(()).unwrap();
    esperar_que(|| e.en_revision(&[&i1, &i2]), "ambos trabajos entregados, uno tras otro");
    assert_eq!(doble.maximo(), 1, "nunca dos a la vez en el mismo worktree");
    t.detener();
}

#[test]
fn un_trabajo_con_worktree_propio_corre_en_el_estado_y_su_criterio_e_informe_salen_ahi() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let id = e.bus.crear_trabajo(Orden { worktree_propio: true, ..orden(&r) }).unwrap();
    let doble = Doble::simple();
    let cfg = ConfigOrquestador::para_agente(&e.ajustes, "qwen-code").unwrap();
    assert_eq!(ejecutar_uno(&e.bus, &cfg, &Adaptador(Arc::clone(&doble))), Some(id.clone()));
    let propio = e.raiz().join("worktrees").join(&id);
    assert_eq!(doble.llamadas(), [("qwen-code".to_string(), propio.clone())]);
    assert!(propio.join("ok.txt").exists() && !r.join("ok.txt").exists(), "el repo original no se toca");
    let inf = e.bus.leer_informe(&id).unwrap().unwrap();
    assert_eq!(inf.resultado, ResultadoInforme::Terminado, "el criterio corrió en el worktree propio: {}", inf.evidencia);
    assert!(inf.diff_stat.unwrap().contains("ok.txt"));
    assert_eq!(inf.commit_inicio.as_deref(), Some(git(&r, &["rev-parse", "HEAD"]).as_str()));
    assert_eq!(git(&propio, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("ea/{id}"));
    // El semáforo fue el del worktree propio, y ya está libre.
    assert!(ea_bus::listar_semaforos_en(&e.raiz()).is_empty());
}

#[test]
fn dos_trabajos_con_worktree_propio_del_mismo_repo_corren_a_la_vez() {
    let e = Entorno::nuevo(2);
    let eq = e.equipo();
    let r = repo(e.d.path(), "r");
    let i1 = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), worktree_propio: true, ..orden(&r) }).unwrap();
    let i2 = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), worktree_propio: true, ..orden(&r) }).unwrap();
    let doble = Doble::encuentro(2);
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    esperar_que(|| e.en_revision(&[&i1, &i2]), "ambos trabajos entregados");
    assert_eq!(doble.maximo(), 2);
    for i in [&i1, &i2] {
        assert_eq!(e.bus.leer_informe(i).unwrap().unwrap().resultado, ResultadoInforme::Terminado);
        assert!(e.raiz().join("worktrees").join(i).join("ok.txt").exists());
    }
    assert!(!r.join("ok.txt").exists());
    t.detener();
}

#[test]
fn worktree_propio_sobre_algo_que_no_es_un_repo_deja_el_trabajo_bloqueado_con_el_motivo() {
    let e = Entorno::nuevo(1);
    let no_repo = e.d.path().join("sin-git");
    std::fs::create_dir(&no_repo).unwrap();
    let id = e.bus.crear_trabajo(Orden { worktree_propio: true, ..orden(&no_repo) }).unwrap();
    let doble = Doble::simple();
    let cfg = ConfigOrquestador::para_agente(&e.ajustes, "qwen-code").unwrap();
    assert_eq!(ejecutar_uno(&e.bus, &cfg, &Adaptador(Arc::clone(&doble))), Some(id.clone()));
    assert!(doble.llamadas().is_empty(), "el agente no llegó a lanzarse");
    assert_eq!(e.estado(&id), EstadoTrabajo::EnRevision);
    let inf = e.bus.leer_informe(&id).unwrap().unwrap();
    assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
    assert!(inf.motivo.unwrap().contains("no es un repositorio git"), "con el motivo");
}

#[test]
fn un_trabajo_asignado_lo_toma_el_trabajador_de_ese_agente() {
    let e = Entorno::nuevo(2);
    let eq = e.equipo();
    let r1 = repo(e.d.path(), "r1");
    let r2 = repo(e.d.path(), "r2");
    let para_codex = Orden { equipo: Some(eq), asignado_a: Some("codex".into()), ..orden(&r1) };
    let ic = e.bus.crear_trabajo(para_codex).unwrap();
    let ig = e.bus.crear_trabajo(orden(&r2)).unwrap();
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap().len(), 1);
    let doble = Doble::simple();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    esperar_que(|| e.en_revision(&[&ic, &ig]), "ambos entregados");
    t.detener();
    let por = |id: &str| e.bus.leer_informe(id).unwrap().unwrap().ejecutado_por.unwrap().agente;
    assert_eq!(por(&ic), "codex", "el asignado lo hizo codex, que es quien aparece como ejecutor");
    assert!(!doble.llamadas().iter().any(|(q, wt)| q == "qwen-code" && wt == &r1), "qwen-code no tocó el asignado");
}

#[test]
fn un_semaforo_ocupado_devuelve_el_asignado_a_la_cola_de_su_agente() {
    let e = Entorno::nuevo(1);
    let eq = e.equipo();
    let r = repo(e.d.path(), "r");
    let id = e.bus.crear_trabajo(Orden { equipo: Some(eq), asignado_a: Some("qwen-code".into()), ..orden(&r) }).unwrap();
    let ocupante = ea_bus::Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: None };
    let _c = ea_bus::CandadoWorktree::adquirir_en(&e.raiz(), &r, ocupante, Duration::from_secs(1)).unwrap();
    let cfg = ConfigOrquestador::para_agente(&e.ajustes, "qwen-code").unwrap();
    let doble = Doble::simple();
    assert_eq!(ejecutar_uno(&e.bus, &cfg, &Adaptador(doble)), Some(id.clone()));
    assert_eq!(e.estado(&id), EstadoTrabajo::EnCola);
    assert_eq!(e.bus.listar_cola_agente("qwen-code").unwrap()[0].trabajo, id, "a SU cola");
    assert!(e.bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
}

#[test]
fn sin_equipos_y_con_paralelo_uno_hay_un_solo_trabajador_con_el_ejecutor_del_rol() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let ids: Vec<String> = (0..2).map(|_| e.bus.crear_trabajo(orden(&r)).unwrap()).collect();
    let doble = Doble::simple();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    t.reconciliar();
    assert_eq!(t.activos(), ["qwen-code"]);
    esperar_que(|| ids.iter().all(|i| e.estado(i) == EstadoTrabajo::EnRevision), "ambos entregados");
    assert_eq!(doble.maximo(), 1);
    let p = e.bus.presencia_de("qwen-code").unwrap().unwrap();
    assert_eq!((p.rol, p.equipo), (RolEquipo::Ejecutor, None), "el ejecutor por defecto también anota presencia, sin equipo");
    t.detener();
}

#[test]
fn la_presencia_se_anota_late_con_el_trabajo_y_se_quita() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let id = e.bus.crear_trabajo(orden(&r)).unwrap();
    let (doble, dentro, soltar) = Doble::retenido();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    dentro.recv_timeout(LIMITE).unwrap();
    let trabajo_de = |bus: &Bus| bus.presencia_de("qwen-code").unwrap().and_then(|p| p.trabajo);
    esperar_que(|| trabajo_de(&e.bus) == Some(id.clone()), "el trabajo actual en la presencia");
    // Un trabajo largo no deja caducar la presencia: el hilo del latido sigue latiendo.
    let mut vieja: Presencia = e.bus.presencia_de("qwen-code").unwrap().unwrap();
    vieja.latido = "2000-01-01T00:00:00Z".into();
    e.bus.anotar_presencia(&vieja).unwrap();
    esperar_que(
        || {
            let p = e.bus.presencia_de("qwen-code").unwrap().unwrap();
            OffsetDateTime::parse(&p.latido, &Rfc3339).unwrap().year() > 2000 && p.trabajo.as_deref() == Some(id.as_str())
        },
        "latido durante el trabajo",
    );
    assert!(e.bus.presencias().iter().any(|(p, viva)| p.agente == "qwen-code" && *viva));
    soltar.send(()).unwrap();
    esperar_que(|| e.estado(&id) == EstadoTrabajo::EnRevision, "entregado");
    esperar_que(|| trabajo_de(&e.bus).is_none(), "entre trabajos, sin trabajo");
    assert!(e.bus.presencia_de("qwen-code").unwrap().is_some(), "sigue despierto");
    t.detener();
    assert!(e.bus.presencia_de("qwen-code").unwrap().is_none(), "se quita al irse");
}

#[test]
fn dormir_el_equipo_deja_sin_tomar_mas_y_quita_la_presencia_de_sus_trabajadores() {
    let e = Entorno::nuevo(2);
    let eq = e.equipo();
    let doble = Doble::simple();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    assert_eq!(t.activos(), ["codex", "qwen-code"]);
    esperar_que(|| e.bus.presencia_de("codex").unwrap().is_some(), "presencia de codex");
    assert_eq!(e.bus.presencia_de("codex").unwrap().unwrap().equipo.as_deref(), Some(eq.as_str()));
    e.bus.dormir_equipo(&eq).unwrap();
    // Codex se va; qwen-code (el de `roles.ejecutor`) sigue, ya sin equipo.
    esperar_que(|| e.bus.presencia_de("codex").unwrap().is_none(), "codex se va");
    let r = repo(e.d.path(), "r");
    let id = e.bus.crear_trabajo(orden(&r)).unwrap();
    esperar_que(
        || {
            t.reconciliar();
            e.estado(&id) == EstadoTrabajo::EnRevision
        },
        "qwen-code, por rol, toma el trabajo",
    );
    assert!(!doble.llamadas().iter().any(|(q, _)| q == "codex"));
    assert_eq!(t.activos(), ["qwen-code"]);
    t.detener();
}

/// `sh -c guion` dentro del sandbox del agente en `wt`; `None` si aquí no
/// hay bwrap o no puede crear espacios de nombres (la prueba se omite).
fn en_bwrap(wt: &Path, guion: &str) -> Option<std::process::Output> {
    if Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("sin bwrap: prueba omitida");
        return None;
    }
    let args = ["-c".to_string(), guion.to_string()];
    let s = Sandbox::Bwrap.envolver("sh", &args, wt, &[], None).unwrap().orden.output().unwrap();
    if !s.status.success() && String::from_utf8_lossy(&s.stderr).contains("namespace") {
        eprintln!("bwrap sin espacios de nombres aquí: prueba omitida");
        return None;
    }
    Some(s)
}

#[test]
fn bwrap_deja_escribible_el_worktree_propio_y_nada_mas() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let ruta = e.bus.preparar_worktree("t1", &Orden { worktree_propio: true, ..orden(&r) }).unwrap();
    let antes = std::fs::read_to_string(ruta.join(".git")).unwrap();
    // El fichero `.git` del worktree propio (a dónde apunta) tampoco se reescribe.
    let guion = format!(
        "echo x > '{w}/dentro.txt' && ! (echo x > '{r}/fuera.txt') 2>/dev/null && ! (echo x > '{g}/worktrees/t1/HEAD') 2>/dev/null \
         && ! (printf 'gitdir: /tmp/x' > '{w}/.git') 2>/dev/null && ! rm -f '{w}/.git' 2>/dev/null",
        w = ruta.display(),
        r = r.display(),
        g = r.join(".git").display()
    );
    let Some(s) = en_bwrap(&ruta, &guion) else { return };
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    assert!(ruta.join("dentro.txt").exists() && !r.join("fuera.txt").exists());
    assert_eq!(std::fs::read_to_string(ruta.join(".git")).unwrap(), antes);
    // La captura del sistema, fuera del sandbox, ve lo que el agente escribió.
    assert_eq!(git(&ruta, &["ls-files", "--others", "--exclude-standard"]), "dentro.txt");
}

#[test]
fn bwrap_deja_el_git_de_un_repo_normal_de_solo_lectura() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), "r");
    let config = std::fs::read_to_string(r.join(".git/config")).unwrap();
    // Ni la configuración, ni los hooks, ni quitar o mover `.git`; el resto
    // del worktree, sí.
    let guion = "echo x > dentro.txt && ! (echo x >> .git/config) 2>/dev/null && ! (echo x > .git/hooks/pre-commit) 2>/dev/null \
                 && ! mv .git otro 2>/dev/null && ! rm -rf .git 2>/dev/null && mkdir sub && echo y > sub/f.txt";
    let Some(s) = en_bwrap(&r, guion) else { return };
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    assert_eq!(std::fs::read_to_string(r.join(".git/config")).unwrap(), config);
    assert!(!r.join(".git/hooks/pre-commit").exists() && r.join(".git/HEAD").exists());
    assert!(r.join("dentro.txt").exists() && r.join("sub/f.txt").exists());
}

#[test]
fn el_criterio_en_bwrap_no_lee_el_directorio_de_ejecucion() {
    if Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("sin bwrap: prueba omitida");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), "r");
    let ejecucion = d.path().join("ejecucion");
    std::fs::create_dir(&ejecucion).unwrap();
    std::fs::write(ejecucion.join("marca"), "secreto").unwrap();
    let criterio = |guion: &str| {
        let mut c = Command::new("sh");
        c.args(["-c", guion]).current_dir(&r);
        c
    };
    let leer = format!("cat '{}'", ejecucion.join("marca").display());
    // Fuera, se lee.
    let fuera = criterio(&leer).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&fuera.stdout), "secreto");
    let dentro = Sandbox::Bwrap.envolver_criterio(criterio(&leer), &r, Some(&ejecucion)).unwrap().output().unwrap();
    if !dentro.status.success() && String::from_utf8_lossy(&dentro.stderr).contains("namespace") {
        eprintln!("bwrap sin espacios de nombres aquí: prueba omitida");
        return;
    }
    assert!(!dentro.status.success() && dentro.stdout.is_empty(), "el criterio leyó el directorio de ejecución");
    // Lo demás, como fuera: escribe en el worktree.
    let ok = Sandbox::Bwrap.envolver_criterio(criterio("echo hecho > ok.txt && test -f ok.txt"), &r, Some(&ejecucion)).unwrap().output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    assert!(r.join("ok.txt").exists());
}

/// B-01: el criterio que corre el ejecutor, con el agente en bwrap, no lee
/// la clave del panel (`<ejecucion>/ea-web/clave`) y escribe en el worktree.
#[test]
fn el_criterio_del_ejecutor_en_bwrap_no_lee_la_clave_del_panel_y_escribe_en_el_worktree() {
    if Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("sin bwrap: prueba omitida");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), "r");
    let ejecucion = d.path().join("ejecucion");
    let clave = ea_ajustes::fichero_clave_web(&ejecucion);
    std::fs::create_dir_all(clave.parent().unwrap()).unwrap();
    std::fs::write(&clave, "secreto").unwrap();
    let criterio = |sandbox: &Sandbox, guion: &str| correr_criterio(sandbox, &r, guion, Some(&ejecucion), &|| false).unwrap();
    let leer = format!("cat '{}'", clave.display());
    // Sin sandbox, tal cual: se lee.
    assert_eq!(criterio(&Sandbox::Ninguno, &leer), (true, "secreto".to_string()));
    let (pasa, evidencia) = criterio(&Sandbox::Bwrap, &leer);
    if !pasa && evidencia.contains("namespace") {
        eprintln!("bwrap sin espacios de nombres aquí: prueba omitida");
        return;
    }
    assert!(!pasa && !evidencia.contains("secreto"), "el criterio leyó la clave del panel: {evidencia}");
    let (pasa, evidencia) = criterio(&Sandbox::Bwrap, "echo hecho > ok.txt && test -f ok.txt");
    assert!(pasa, "{evidencia}");
    assert_eq!(std::fs::read_to_string(r.join("ok.txt")).unwrap(), "hecho\n");
}

/// F-14: con un orquestador en marcha, un segundo no arranca y no toca nada:
/// ni da por interrumpido lo que el primero ejecuta ni devuelve a la cola lo
/// que acaba de reclamar. Cuando el primero se va, el siguiente sí recupera.
#[test]
fn una_segunda_instancia_no_arranca_ni_toca_lo_que_lleva_la_primera() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let primera = arrancar(&e.bus).unwrap();
    let por = ConfigOrquestador::para_agente(&e.ajustes, "qwen-code").unwrap().ejecutor.ejecutado_por();
    // La primera ejecuta `a` y acaba de reclamar `b` (aún en cola, sin entrada).
    let a = e.bus.crear_trabajo(orden(&r)).unwrap();
    assert_eq!(e.bus.tomar_para_con("qwen-code", &|_| true).unwrap(), Some(a.clone()));
    e.bus.tomar_con(&a, por).unwrap();
    let b = e.bus.crear_trabajo(orden(&r)).unwrap();
    assert_eq!(e.bus.tomar_para_con("qwen-code", &|_| true).unwrap(), Some(b.clone()));

    let segunda = arrancar(&Bus::abrir_en(&e.raiz()).unwrap());
    assert_eq!(e.estado(&a), EstadoTrabajo::Ejecutando);
    assert!(e.bus.leer_informe(&a).unwrap().is_none(), "la segunda dio por interrumpido lo que ejecuta la primera");
    assert!(e.bus.listar_cola(Rol::Ejecutor).unwrap().is_empty(), "la segunda devolvió a la cola lo reclamado");
    let error = segunda.unwrap_err();
    assert!(error.contains("ya hay un ea-orchestrator en marcha"), "{error}");

    drop(primera);
    let _tercera = arrancar(&e.bus).unwrap();
    assert_eq!(e.bus.leer_informe(&a).unwrap().unwrap().motivo.as_deref(), Some("interrumpido"));
    assert_eq!(e.bus.listar_cola(Rol::Ejecutor).unwrap().iter().map(|x| x.trabajo.clone()).collect::<Vec<_>>(), [b]);
}

#[test]
fn una_entrada_rancia_se_descarta_sin_worktree_ni_cambio_de_estado() {
    let e = Entorno::nuevo(1);
    let r = repo(e.d.path(), "r");
    let id = e.bus.crear_trabajo(Orden { worktree_propio: true, ..orden(&r) }).unwrap();
    e.bus.aplicar(&id, ea_bus::Evento::Cancelar).unwrap();
    e.bus.encolar(Rol::Ejecutor, &id, "rancia").unwrap();
    let doble = Doble::simple();
    let cfg = ConfigOrquestador::para_agente(&e.ajustes, "qwen-code").unwrap();
    assert_eq!(ejecutar_uno(&e.bus, &cfg, &Adaptador(Arc::clone(&doble))), Some(id.clone()));
    assert!(doble.llamadas().is_empty());
    assert_eq!(e.estado(&id), EstadoTrabajo::Cancelado);
    assert!(!e.raiz().join("worktrees").join(&id).exists(), "no se creó el worktree");
    assert!(e.bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
}

#[test]
fn roles_ejecutor_sigue_mandando_en_los_trabajos_sin_equipo() {
    let e = Entorno::nuevo(2);
    // Equipo despierto solo con codex de ejecutor; `roles.ejecutor` es qwen-code.
    let nuevo = NuevoEquipo {
        titulo: "equipo".into(),
        objetivo: "probar".into(),
        worktree_base: e.d.path().to_string_lossy().into(),
        miembros: vec![m("claude-code", RolEquipo::Planificador), m("codex", RolEquipo::Ejecutor)],
        creado_por: "ea-cli".into(),
    };
    let eq = e.bus.crear_equipo(nuevo, &e.ajustes).unwrap().id;
    let r1 = repo(e.d.path(), "r1");
    let r2 = repo(e.d.path(), "r2");
    let libre = e.bus.crear_trabajo(orden(&r1)).unwrap();
    let del_equipo = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), ..orden(&r2) }).unwrap();
    let doble = Doble::simple();
    let mut t = Trabajadores::new(e.contexto(&doble));
    t.reconciliar();
    assert_eq!(t.activos(), ["codex", "qwen-code"], "el del rol tiene trabajador aunque haya equipo");
    esperar_que(|| e.en_revision(&[&libre, &del_equipo]), "ambos entregados");
    let quien = |wt: &PathBuf| doble.llamadas().into_iter().find(|(_, w)| w == wt).map(|(q, _)| q);
    assert_eq!(quien(&r1).as_deref(), Some("qwen-code"), "sin equipo: roles.ejecutor");
    assert_eq!(quien(&r2).as_deref(), Some("codex"), "del equipo: su ejecutor");
    // Lo que se devuelve a la cola al dormir lo hace el de `roles.ejecutor`.
    // Sin trabajadores entre medias: con el de codex vivo podría tomar el
    // trabajo asignado antes de que el equipo se duerma (y la prueba sería
    // una carrera, sobre todo en máquinas lentas).
    t.detener();
    let r3 = repo(e.d.path(), "r3");
    let asignado = e.bus.crear_trabajo(Orden { equipo: Some(eq.clone()), asignado_a: Some("codex".into()), ..orden(&r3) }).unwrap();
    e.bus.dormir_equipo(&eq).unwrap();
    e.apagar.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut t = Trabajadores::new(e.contexto(&doble));
    esperar_que(
        || {
            t.reconciliar();
            e.estado(&asignado) == EstadoTrabajo::EnRevision
        },
        "el trabajo devuelto lo hace roles.ejecutor",
    );
    assert_eq!(quien(&r3).as_deref(), Some("qwen-code"));
    t.detener();
}

#[test]
fn con_paralelo_1_un_equipo_solo_cabe_si_su_ejecutor_es_el_de_roles() {
    let e = Entorno::nuevo(1);
    let nuevo = |ej: &str| NuevoEquipo {
        titulo: "equipo".into(),
        objetivo: "probar".into(),
        worktree_base: e.d.path().to_string_lossy().into(),
        miembros: vec![m("claude-code", RolEquipo::Planificador), m(ej, RolEquipo::Ejecutor)],
        creado_por: "ea-cli".into(),
    };
    let err = e.bus.crear_equipo(nuevo("codex"), &e.ajustes).unwrap_err().to_string();
    assert!(err.contains("roles.ejecutor") && err.contains("orquestador.paralelo"), "{err}");
    e.bus.crear_equipo(nuevo("qwen-code"), &e.ajustes).unwrap();
}
