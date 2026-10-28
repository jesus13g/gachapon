//! Fija lo que el ejecutor deja en disco y en SQLite en cada trayectoria de
//! un trabajo: los ficheros del trabajo (orden, estado, historial, informe y
//! los artefactos: salida y skills del agente, instantáneas de árbol, fases
//! informadas y paquete de corrección), sus filas de fase y su diario, las
//! colas y el ocupante del semáforo. Con bus, base y repos de `tempfile`, sin
//! sandbox y con un doble del agente (para el tope de tiempo, un binario
//! falso con el adaptador de Codex). Las fechas, los ids generados, los pids,
//! las duraciones, los commits y la carpeta temporal se normalizan. Lo que se
//! escribe con `json!` (las líneas del historial, el diario y el informe de
//! cada fase en SQLite) va con las claves ordenadas: su orden depende de si
//! serde_json se compila con `preserve_order` (en el workspace, sí; con `-p`,
//! no). Lo demás, byte a byte.
//!
//! El resultado se compara con `fijado/trabajos.txt`. Solo si el cambio es a
//! propósito, `EA_FIJAR=1 cargo test -p ea-orchestrator --test fijado` lo
//! reescribe. Solo en Unix: el doble y el binario falso son guiones de `sh`.
#![cfg(unix)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::Mutex;
use std::time::Duration;

use ea_bus::{Bus, CandadoWorktree, Evento, Fase, Miembro, NuevoEquipo, Ocupante, Orden, Rol, RolEquipo, Topes};
use ea_orchestrator::config::ConfigOrquestador;
use ea_orchestrator::ejecutor::{ejecutar_intento, ejecutar_uno, Ejecutor, SalidaAgente};
use ea_store::Store;

/// Qué hace el doble, además de su guion, la primera vez que lo llaman.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Al {
    Nada,
    /// Cancela el trabajo desde fuera (como la web o la CLI).
    Cancelar,
    /// Pide la parada del orquestador.
    Apagar,
}

/// Doble del agente: corre `guion` en el worktree, anota el ocupante del
/// semáforo tal como está mientras corre y sale con `codigo`.
struct Doble {
    guion: &'static str,
    codigo: i32,
    al: Al,
    raiz: PathBuf,
    id: String,
    llamadas: AtomicUsize,
    apagar: AtomicBool,
    ocupantes: Mutex<Vec<String>>,
}

impl Doble {
    fn nuevo(bus: &Bus, id: &str, guion: &'static str) -> Self {
        Self {
            guion,
            codigo: 0,
            al: Al::Nada,
            raiz: bus.raiz().to_path_buf(),
            id: id.into(),
            llamadas: AtomicUsize::new(0),
            apagar: AtomicBool::new(false),
            ocupantes: Mutex::new(vec![]),
        }
    }
}

impl Ejecutor for Doble {
    fn ejecutar(&self, _: &str, _: &str, wt: &Path, _: &Topes, _: &ConfigOrquestador, cancelado: &dyn Fn() -> bool) -> SalidaAgente {
        let n = self.llamadas.fetch_add(1, SeqCst);
        for e in fs::read_dir(self.raiz.join("semaforos")).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "ocupante") {
                self.ocupantes.lock().unwrap().push(fs::read_to_string(e.path()).unwrap());
            }
        }
        assert!(Command::new("sh").arg("-c").arg(self.guion).current_dir(wt).status().unwrap().success());
        if n == 0 {
            match self.al {
                Al::Nada => {}
                Al::Cancelar => {
                    Bus::abrir_en(&self.raiz).unwrap().aplicar(&self.id, Evento::Cancelar).unwrap();
                }
                Al::Apagar => self.apagar.store(true, SeqCst),
            }
        }
        let cancelado = self.al == Al::Apagar && cancelado();
        SalidaAgente { codigo: Some(self.codigo), bruta: r#"[{"type":"result","result":"hecho"}]"#.into(), resumen: "hecho".into(), cancelado }
    }

    /// Como Qwen Code: 55 es «topes agotados».
    fn codigo_topes(&self) -> Option<i32> {
        Some(55)
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let s = Command::new("git").args(["-c", "user.email=a@b", "-c", "user.name=x", "-C"]).arg(dir).args(args).output().unwrap();
    assert!(s.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&s.stderr));
    String::from_utf8_lossy(&s.stdout).trim().to_string()
}

/// Un repo con un commit; apunta el commit para normalizarlo.
fn repo(d: &Path, nombre: &str, commits: &mut Vec<String>) -> PathBuf {
    let r = d.join(nombre);
    fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    fs::write(r.join("a.txt"), "a\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "inicial"]);
    commits.push(git(&r, &["rev-parse", "HEAD"]));
    r
}

fn mision(id: &str, wt: &Path, criterio: &str, iteraciones: u32) -> Orden {
    let mut o = Orden::mision(format!("misión {id}"), wt.to_string_lossy(), "crea ok.txt", criterio, "claude-code");
    o.id = id.into();
    o.notas_revision = vec!["usa minúsculas".into()];
    o.topes.iteraciones = iteraciones;
    o
}

fn obra(id: &str, wt: &Path, revisar_cada: Option<u32>) -> Orden {
    let fase = |n: u32| Fase {
        numero: n,
        titulo: format!("f{n}"),
        instrucciones: format!("crea el fichero f{n}"),
        entregable: format!("f{n}"),
        criterio: format!("test -f f{n}"),
        skills: if n == 2 { vec!["tests".into()] } else { vec![] },
    };
    let mut o = Orden::obra(format!("obra {id}"), wt.to_string_lossy(), "tres ficheros", vec![fase(1), fase(2), fase(3)], revisar_cada, "claude-code");
    o.id = id.into();
    o.skills = vec!["rust".into()];
    o.topes.iteraciones = 2;
    o
}

/// Crea el siguiente `fN` que falte.
const SIGUIENTE: &str = "for f in f1 f2 f3; do [ -f $f ] || { echo x > $f; break; }; done";

/// `AAAA-MM-DDThh:mm:ss`, con fracción y zona opcionales: los bytes que ocupa.
fn fecha(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let c = |i: usize, ch: u8| b.get(i) == Some(&ch);
    let forma = (0..4).all(d) && c(4, b'-') && d(5) && d(6) && c(7, b'-') && d(8) && d(9) && c(10, b'T');
    if !(forma && d(11) && d(12) && c(13, b':') && d(14) && d(15) && c(16, b':') && d(17) && d(18)) {
        return None;
    }
    let mut n = 19;
    if c(n, b'.') {
        n += 1;
        while d(n) {
            n += 1;
        }
    }
    if c(n, b'Z') {
        n += 1;
    } else if (c(n, b'+') || c(n, b'-')) && d(n + 1) && d(n + 2) && c(n + 3, b':') && d(n + 4) && d(n + 5) {
        n += 6;
    }
    Some(n)
}

/// Un id del bus (`AAAAMMDDThhmmss-hhhhhhhh`): los bytes que ocupa.
fn id_generado(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let ok = b.len() >= 24
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'-'
        && b[16..24].iter().all(u8::is_ascii_hexdigit);
    ok.then_some(24)
}

/// Sin fechas, ids generados, pids, duraciones, commits, el hash del
/// semáforo ni la carpeta temporal.
fn normalizar(texto: &str, tmp: &str, commits: &[String]) -> String {
    let mut texto = texto.replace(tmp, "<TMP>");
    for c in commits {
        texto = texto.replace(c.as_str(), "<COMMIT>");
    }
    let mut out = String::new();
    let mut resto = texto.as_str();
    while let Some(ch) = resto.chars().next() {
        if let Some(n) = fecha(resto) {
            out.push_str("<FECHA>");
            resto = &resto[n..];
            continue;
        }
        if let Some(n) = id_generado(resto) {
            out.push_str("<ID>");
            resto = &resto[n..];
            continue;
        }
        let mut hecho = false;
        for (prefijo, marca) in [("\"pid\":", "<PID>"), ("\"duracion_s\": ", "<S>"), ("worktree-", "<H>")] {
            if let Some(r) = resto.strip_prefix(prefijo) {
                let n = if marca == "<H>" { r.bytes().take_while(u8::is_ascii_hexdigit).count() } else { r.bytes().take_while(u8::is_ascii_digit).count() };
                if n > 0 {
                    out.push_str(prefijo);
                    out.push_str(marca);
                    resto = &r[n..];
                    hecho = true;
                    break;
                }
            }
        }
        if hecho {
            continue;
        }
        out.push(ch);
        resto = &resto[ch.len_utf8()..];
    }
    out
}

/// Un JSON con las claves ordenadas, sin espacios; lo que no es JSON, tal cual.
fn canonico(texto: &str) -> String {
    fn escribir(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut claves: Vec<&String> = m.keys().collect();
                claves.sort();
                out.push('{');
                for (i, k) in claves.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    escribir(&m[k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    escribir(x, out);
                }
                out.push(']');
            }
            otro => out.push_str(&otro.to_string()),
        }
    }
    match serde_json::from_str::<serde_json::Value>(texto) {
        Ok(v) => {
            let mut out = String::new();
            escribir(&v, &mut out);
            out
        }
        Err(_) => texto.to_string(),
    }
}

/// Los ficheros de `dir`, recursivos y por orden, con su contenido. El
/// orden no depende del hash de cada semáforo (sale de la carpeta temporal).
fn volcar(dir: &Path, base: &Path, saltar: Option<&Path>, out: &mut String) {
    let mut bloques: Vec<(String, String)> = Vec::new();
    for p in fs::read_dir(dir).unwrap().flatten().map(|e| e.path()) {
        if Some(p.as_path()) == saltar {
            continue;
        }
        let rel = p.strip_prefix(base).unwrap().display().to_string();
        let mut bloque = String::new();
        if p.is_dir() {
            writeln!(bloque, "-- {rel}/").unwrap();
            volcar(&p, base, saltar, &mut bloque);
        } else {
            let b = fs::read(&p).unwrap();
            let mut texto = String::from_utf8_lossy(&b).into_owned();
            if rel.ends_with(".jsonl") {
                texto = texto.lines().map(|l| canonico(l) + "\n").collect();
            }
            if b.is_empty() {
                writeln!(bloque, "-- {rel} (vacío)").unwrap();
            } else {
                writeln!(bloque, "-- {rel}\n{texto}\n-- fin").unwrap();
            }
        }
        let clave = match rel.find("worktree-") {
            Some(i) => format!("{}worktree-{}", &rel[..i], rel[i + 9..].trim_start_matches(|c: char| c.is_ascii_hexdigit())),
            None => rel,
        };
        bloques.push((clave, bloque));
    }
    bloques.sort();
    for (_, b) in bloques {
        out.push_str(&b);
    }
}

/// Todo lo de un trabajo: sus ficheros, sus filas de fase y su diario.
fn rastro(bus: &Bus, db: &Path, id: &str, que: &str, out: &mut String) {
    writeln!(out, "==== {id}: {que}").unwrap();
    let dir = bus.raiz().join("trabajos").join(id);
    volcar(&dir, &dir, None, out);
    let s = Store::open_at(db).unwrap();
    for f in s.listar_fases(id).unwrap() {
        writeln!(out, "-- fase {} {} {} {:?}", f.numero, f.estado, canonico(&f.informe.unwrap_or_default()), f.cerrado_en).unwrap();
    }
    let mut eventos = s.list_events_by_trabajo(id).unwrap();
    eventos.sort_by_key(|e| e.id);
    for e in eventos {
        let outcome = e.outcome.map(|o| o.as_str()).unwrap_or("-");
        writeln!(out, "-- diario {} {} {outcome} {}", e.actor, e.action, canonico(&e.detail.unwrap_or_default())).unwrap();
    }
}

#[test]
fn cada_trayectoria_deja_en_disco_lo_mismo_que_siempre() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let mut commits = Vec::new();
    let db = d.join("ea.db");
    let skills = d.join("skills");
    fs::create_dir_all(&skills).unwrap();
    fs::write(skills.join("rust.md"), "---\ndescripcion: Rust\n---\nUsa cargo test.\n").unwrap();
    fs::write(skills.join("tests.md"), "---\ndescripcion: tests\n---\nPrueba primero.\n").unwrap();
    let mut cfg = ConfigOrquestador { avisos_escritorio: false, sandbox: false, ..Default::default() };
    cfg.skills.dir = skills.to_string_lossy().into();
    let bus = Bus::abrir_en(&d.join("estado")).unwrap().con_store(Store::open_at(&db).unwrap()).con_skills(cfg.skills.clone());
    let mut out = String::new();

    // Misión que pasa a la primera, con skills.
    let wt = repo(d, "r01", &mut commits);
    let o = Orden { skills: vec!["rust".into()], ..mision("t01-feliz", &wt, "test -f ok.txt", 5) };
    let id = bus.crear_trabajo(o).unwrap();
    let ag = Doble::nuevo(&bus, &id, "echo hola > ok.txt");
    assert_eq!(ejecutar_uno(&bus, &cfg, &ag), Some(id.clone()));
    rastro(&bus, &db, &id, "misión que pasa", &mut out);
    for o in ag.ocupantes.lock().unwrap().iter() {
        writeln!(out, "-- ocupante del semáforo mientras corre\n{o}").unwrap();
    }

    // Misión que pasa en la segunda iteración.
    let wt = repo(d, "r02", &mut commits);
    let id = bus.crear_trabajo(mision("t02-reintento", &wt, "test -f ok.txt || { echo FALTA-OK; exit 1; }", 3)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble::nuevo(&bus, &id, "if [ -f marca ]; then echo hola > ok.txt; else touch marca; fi"));
    rastro(&bus, &db, &id, "misión que pasa en la segunda iteración", &mut out);

    // Misión cuyo criterio falla siempre.
    let wt = repo(d, "r03", &mut commits);
    let id = bus.crear_trabajo(mision("t03-falla", &wt, "echo roto; exit 1", 2)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble::nuevo(&bus, &id, "true"));
    rastro(&bus, &db, &id, "misión cuyo criterio falla", &mut out);

    // El agente dice que agotó sus topes.
    let wt = repo(d, "r04", &mut commits);
    let id = bus.crear_trabajo(mision("t04-topes", &wt, "false", 3)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble { codigo: 55, ..Doble::nuevo(&bus, &id, "true") });
    rastro(&bus, &db, &id, "topes agotados", &mut out);

    // Cancelada mientras corría el agente.
    let wt = repo(d, "r05", &mut commits);
    let id = bus.crear_trabajo(mision("t05-cancelada", &wt, "touch criterio-corrio; false", 3)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble { al: Al::Cancelar, ..Doble::nuevo(&bus, &id, "true") });
    rastro(&bus, &db, &id, "cancelada en curso", &mut out);

    // La parada del orquestador mientras corría el agente, y el arranque siguiente.
    let wt = repo(d, "r06", &mut commits);
    let id = bus.crear_trabajo(mision("t06-parada", &wt, "touch criterio-corrio; true", 3)).unwrap();
    let ag = Doble { al: Al::Apagar, ..Doble::nuevo(&bus, &id, "true") };
    ejecutar_intento(&bus, &cfg, &ag, Duration::from_secs(2), &|_| {}, &|_| true, &|| ag.apagar.load(SeqCst));
    rastro(&bus, &db, &id, "parada en curso", &mut out);
    assert_eq!(bus.recuperar().unwrap(), vec![id.clone()]);
    rastro(&bus, &db, &id, "recuperada al arrancar", &mut out);

    // Tope de tiempo: el adaptador de Codex con un binario que no acaba.
    let wt = repo(d, "r07", &mut commits);
    let bin = d.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let lento = bin.join("codex-lento");
    fs::write(&lento, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&lento, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut o = mision("t07-tiempo", &wt, "test -f ok.txt", 1);
    o.topes.tiempo_s = 1;
    let id = bus.crear_trabajo(o).unwrap();
    let mut codex = cfg.clone();
    codex.ejecutor.id = "codex".into();
    codex.ejecutor.tipo = ea_bus::TipoAgente::Codex;
    codex.ejecutor.bin = lento.to_string_lossy().into();
    ejecutar_uno(&bus, &codex, &ea_orchestrator::agentes::Codex);
    rastro(&bus, &db, &id, "tope de tiempo del agente", &mut out);

    // Obra con revisión cada dos fases, y su continuación.
    let wt = repo(d, "r08", &mut commits);
    let id = bus.crear_trabajo(obra("t08-obra", &wt, Some(2))).unwrap();
    let ag = Doble::nuevo(&bus, &id, SIGUIENTE);
    ejecutar_uno(&bus, &cfg, &ag);
    rastro(&bus, &db, &id, "obra en revisión intermedia", &mut out);
    bus.reformular(&id, "", None).unwrap();
    ejecutar_uno(&bus, &cfg, &ag);
    rastro(&bus, &db, &id, "obra completa", &mut out);

    // Obra que se bloquea en la fase 2.
    let wt = repo(d, "r09", &mut commits);
    let id = bus.crear_trabajo(obra("t09-obra-bloqueada", &wt, None)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble::nuevo(&bus, &id, "[ -f f1 ] || echo x > f1"));
    rastro(&bus, &db, &id, "obra bloqueada", &mut out);

    // Rutinas: una que falla (paquete y confirmación) y otra que pasa.
    let rutinas = d.join("rutinas");
    for (slug, wt_nombre, criterio, id) in [("fallida", "r10", "false", "t10-rutina-falla"), ("buena", "r11", "test -f ok.txt", "t11-rutina-pasa")] {
        let wt = repo(d, wt_nombre, &mut commits);
        let toml = format!(
            "[rutina]\nslug = \"{slug}\"\nversion = 1\nworktree = '{}'\ncriterio_exito = \"{criterio}\"\n[topes]\niteraciones = 2\n[[pasos]]\ntexto = \"crea ok.txt\"\n",
            wt.display()
        );
        bus.registrar_rutina(&toml, &rutinas).unwrap();
        let o = Orden { id: id.into(), ..bus.rutina(slug).unwrap().a_orden(&[]).unwrap() };
        let id = bus.crear_trabajo(o).unwrap();
        ejecutar_uno(&bus, &cfg, &Doble::nuevo(&bus, &id, "echo hola > ok.txt"));
        if slug == "fallida" {
            bus.confirmar(&id, "revisa la ruta").unwrap();
        }
        rastro(&bus, &db, &id, &format!("rutina {slug}"), &mut out);
    }

    // Sin repo git.
    let sin_git = d.join("r12");
    fs::create_dir_all(&sin_git).unwrap();
    let id = bus.crear_trabajo(mision("t12-sin-git", &sin_git, "true", 1)).unwrap();
    ejecutar_uno(&bus, &cfg, &Doble::nuevo(&bus, &id, "true"));
    rastro(&bus, &db, &id, "sin repo git", &mut out);

    // Un ejecutor que no se puede lanzar.
    let wt = repo(d, "r13", &mut commits);
    let id = bus.crear_trabajo(mision("t13-no-disponible", &wt, "true", 1)).unwrap();
    let mut roto = codex.clone();
    roto.ejecutor.bin = "/no/existe/codex".into();
    ejecutar_uno(&bus, &roto, &ea_orchestrator::agentes::Codex);
    rastro(&bus, &db, &id, "ejecutor no disponible", &mut out);

    // Trabajo de equipo asignado a codex, con revisor.
    let wt = repo(d, "r14", &mut commits);
    let mut a = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(&d.join("config")));
    a.orquestador.paralelo = 4;
    let miembros = vec![
        Miembro { agente: "claude-code".into(), rol: RolEquipo::Planificador },
        Miembro { agente: "codex".into(), rol: RolEquipo::Ejecutor },
        Miembro { agente: "antigravity".into(), rol: RolEquipo::Revisor },
    ];
    let nuevo = NuevoEquipo { titulo: "equipo".into(), objetivo: "o".into(), worktree_base: wt.to_string_lossy().into(), miembros, creado_por: "usuario".into() };
    let eq = bus.crear_equipo(nuevo, &a).unwrap();
    let o = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), revisor: Some("antigravity".into()), ..mision("t14-equipo", &wt, "test -f ok.txt", 1) };
    let id = bus.crear_trabajo(o).unwrap();
    let mut de_codex = cfg.clone();
    de_codex.ejecutor.id = "codex".into();
    assert_eq!(ejecutar_uno(&bus, &de_codex, &Doble::nuevo(&bus, &id, "echo hola > ok.txt")), Some(id.clone()));
    rastro(&bus, &db, &id, "trabajo de equipo con revisor", &mut out);

    // Con el semáforo cogido por otro: vuelve a su cola.
    let wt = repo(d, "r15", &mut commits);
    let id = bus.crear_trabajo(mision("t15-ocupado", &wt, "true", 1)).unwrap();
    let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: Some(Rol::Planificador) };
    let candado = CandadoWorktree::adquirir_en(bus.raiz(), &wt, ocupante, Duration::from_secs(1)).unwrap();
    let r = ejecutar_intento(&bus, &cfg, &Doble::nuevo(&bus, &id, "true"), Duration::from_millis(100), &|_| {}, &|_| true, &|| false);
    assert_eq!(r.id(), Some(id.clone()));
    rastro(&bus, &db, &id, "worktree ocupado", &mut out);
    writeln!(out, "==== semáforos mientras otro lo tiene").unwrap();
    let raiz = bus.raiz();
    volcar(&raiz.join("semaforos"), raiz, None, &mut out);
    drop(candado);

    // Y el resto del bus: colas, semáforos y equipos.
    writeln!(out, "==== bus").unwrap();
    volcar(raiz, raiz, Some(&raiz.join("trabajos")), &mut out);

    let texto = normalizar(&out, &d.display().to_string(), &commits);
    let fichero = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fijado/trabajos.txt");
    if std::env::var_os("EA_FIJAR").is_some() {
        fs::create_dir_all(fichero.parent().unwrap()).unwrap();
        fs::write(&fichero, &texto).unwrap();
    }
    let esperado = fs::read_to_string(&fichero).unwrap_or_default();
    if let Some((n, (a, e))) = texto.lines().zip(esperado.lines()).enumerate().find(|(_, (a, e))| a != e) {
        panic!("la línea {} cambió:\n  antes: {e}\n  ahora: {a}", n + 1);
    }
    assert_eq!(texto.lines().count(), esperado.lines().count(), "cambió el número de líneas");
}
