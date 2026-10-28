//! `ea trabajo` con el binario real y todo el estado en directorios
//! temporales: `crear` valida la orden igual que la web, por las dos formas
//! (`--orden <fichero>` y las opciones sueltas), y lo que escriben los
//! agentes sale a la terminal sin caracteres de control (también en `ea
//! diario` y `ea propuestas`).

use std::path::Path;
use std::process::Output;

use ea_pruebas::repo_git;

fn ea(dir: &Path, cwd: &Path, args: &[&str]) -> Output {
    ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), dir).args(args).current_dir(cwd).output().expect("lanzar ea")
}

/// Escribe una orden de misión (o de obra con `fases`) para `worktree` y la
/// crea con `ea trabajo crear --orden`.
fn crear_con_orden(d: &Path, worktree: &str, fases: Option<&[u32]>) -> Output {
    let mut o = serde_json::json!({
        "id": "", "modo": "mision", "titulo": "t", "worktree": worktree, "objetivo": "o",
        "criterio_exito": "true", "creada_en": "", "creada_por": "planificador",
    });
    if let Some(numeros) = fases {
        o["modo"] = "obra".into();
        o["criterio_exito"] = serde_json::Value::Null;
        o["fases"] = numeros
            .iter()
            .map(|n| serde_json::json!({ "numero": n, "titulo": "f", "instrucciones": "i", "entregable": "e", "criterio": "true" }))
            .collect();
    }
    let f = d.join("orden.json");
    std::fs::write(&f, o.to_string()).unwrap();
    ea(d, d, &["trabajo", "crear", "--orden", f.to_str().unwrap()])
}

fn falla_con(o: &Output, texto: &str) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains(texto), "se esperaba un fallo con {texto:?}: {err}");
}

fn sin_trabajos(d: &Path) {
    let n = std::fs::read_dir(d.join("estado/trabajos")).map(|r| r.count()).unwrap_or(0);
    assert_eq!(n, 0, "no debía crearse ningún trabajo");
}

/// B-14: `--orden` (el camino de `/mision` y `/obra`) rechaza al crear lo que
/// la web ya rechazaba y el ejecutor acababa bloqueando o ejecutando en otro
/// sitio: un worktree relativo («.» sería el directorio del servicio, `$HOME`),
/// uno que no es un repo y un repo sin commits. Con los mismos mensajes que la web.
#[test]
fn crear_con_orden_rechaza_un_worktree_relativo_sin_repo_o_sin_commits() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    falla_con(&crear_con_orden(d, ".", None), "'worktree' debe ser una ruta absoluta");
    falla_con(&crear_con_orden(d, "repo/sub", None), "'worktree' debe ser una ruta absoluta");
    falla_con(&crear_con_orden(d, d.join("no-existe").to_str().unwrap(), None), "no existe");
    let sin_repo = d.join("sin-repo");
    std::fs::create_dir_all(&sin_repo).unwrap();
    falla_con(&crear_con_orden(d, sin_repo.to_str().unwrap(), None), "no está dentro de un repositorio git");
    let vacio = d.join("vacio");
    repo_git(&vacio, false);
    falla_con(&crear_con_orden(d, vacio.to_str().unwrap(), None), "el repo no tiene ningún commit");
    sin_trabajos(d);
}

/// U-03: ni el directorio personal (aquí el `HOME` temporal que pone
/// `ea_pruebas::cli`) ni un ancestro suyo ni `/` valen como worktree, aunque
/// sean un repo con commits; tampoco un subdirectorio del repo, por las dos
/// formas de `ea trabajo crear`.
#[test]
fn crear_rechaza_el_home_sus_ancestros_y_un_subdirectorio_del_repo() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let home = d.join("home");
    repo_git(&home, true);
    repo_git(d, true);
    let personal = "es tu directorio personal o lo contiene";
    falla_con(&crear_con_orden(d, home.to_str().unwrap(), None), personal);
    falla_con(&crear_con_orden(d, d.to_str().unwrap(), None), personal);
    falla_con(&crear_con_orden(d, "/", None), personal);
    let corta = ["trabajo", "crear", "--titulo", "t", "--worktree", ".", "--objetivo", "o", "--criterio", "true"];
    falla_con(&ea(d, &home, &corta), personal);

    let wt = d.join("proyecto");
    repo_git(&wt, true);
    std::fs::create_dir_all(wt.join("src")).unwrap();
    falla_con(&crear_con_orden(d, wt.join("src").to_str().unwrap(), None), "no es la raíz de su repositorio");
    falla_con(&ea(d, &wt.join("src"), &corta), "no es la raíz de su repositorio");
    sin_trabajos(d);
    let o = crear_con_orden(d, wt.to_str().unwrap(), None);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// B-06 por la CLI: una obra con las fases mal numeradas no se crea.
#[test]
fn crear_con_orden_rechaza_las_fases_mal_numeradas() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let wt = d.join("repo");
    repo_git(&wt, true);
    let wt = wt.to_str().unwrap();
    falla_con(&crear_con_orden(d, wt, Some(&[10, 20, 30])), "las fases deben numerarse 1, 2, …, N");
    falla_con(&crear_con_orden(d, wt, Some(&[1, 1, 2])), "las fases deben numerarse 1, 2, …, N");
    sin_trabajos(d);
    let o = crear_con_orden(d, wt, Some(&[1, 2, 3]));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// Una orden válida se crea con el worktree en su forma canónica (la que usan
/// el semáforo y el ejecutor), y la forma corta sigue aceptando `--worktree .`
/// desde el repo, como documenta la skill.
#[test]
fn el_worktree_queda_canonico_por_las_dos_formas() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let wt = d.join("repo");
    repo_git(&wt, true);
    let canon = std::fs::canonicalize(&wt).unwrap().to_string_lossy().into_owned();
    let ver = |id: &[u8]| {
        let id = String::from_utf8_lossy(id).trim().to_string();
        let v: serde_json::Value = serde_json::from_slice(&ea(d, d, &["trabajo", "ver", &id, "--json"]).stdout).unwrap();
        v["orden"]["worktree"].as_str().unwrap().to_string()
    };
    std::fs::create_dir_all(wt.join("sub")).unwrap();
    let o = crear_con_orden(d, wt.join("sub").join("..").to_str().unwrap(), None);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(ver(&o.stdout), canon);
    let corta = ["trabajo", "crear", "--titulo", "t", "--worktree", ".", "--objetivo", "o", "--criterio", "true"];
    let o = ea(d, &wt, &corta);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(ver(&o.stdout), canon);
    // Y la forma corta rechaza lo mismo que `--orden`.
    let sin_repo = d.join("sin-repo");
    std::fs::create_dir_all(&sin_repo).unwrap();
    falla_con(&ea(d, &sin_repo, &corta), "no está dentro de un repositorio git");
}

/// U-06: sin terminal, quien lanza `ea` es un agente con Bash (el
/// planificador), y lo que hace en el bus queda firmado con su rol en disco,
/// el mismo actor que le da el MCP; ya no como `ea-cli`.
#[test]
fn sin_terminal_la_cli_firma_en_el_diario_como_el_planificador() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let wt = d.join("repo");
    repo_git(&wt, true);
    let o = crear_con_orden(d, wt.to_str().unwrap(), None);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let o = ea(d, d, &["trabajo", "cancelar", &id]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let store = ea_store::Store::open_at(&d.join("ea.db")).unwrap();
    let eventos = store.list_events_by_trabajo(&id).unwrap();
    assert!(eventos.len() >= 2, "{eventos:?}");
    for e in &eventos {
        assert_eq!(e.actor, ea_ajustes::Rol::Planificador.as_str(), "{e:?}");
    }
}

/// Lo que puede dejar un agente en su texto o en la salida de su criterio: un
/// OSC 52 (escribe en el portapapeles de la terminal), un borrado de pantalla,
/// un color, el CSI de 8 bits y un retorno de carro (para pisar la línea).
const SECUENCIAS: &str = "\x1b]52;c;ZWNobyBob2xh\x07\x1b[2J\x1b[31m\u{9b}0m\r";

/// La salida de tabla no lleva ningún carácter de control salvo `\n` y `\t`.
fn sin_control(args: &[&str], salida: &[u8]) -> String {
    let s = String::from_utf8(salida.to_vec()).unwrap();
    assert!(!s.contains(|c: char| c.is_control() && !matches!(c, '\n' | '\t')), "{args:?} pasa secuencias a la terminal: {s:?}");
    s
}

/// S-19: `ea trabajo ver` y `listar` no reenvían a la terminal las secuencias
/// que un agente deja en el título, el criterio, el motivo, el aviso, las fases
/// o la evidencia; el texto visible queda, y `--json` lo da entero.
#[test]
fn ver_y_listar_no_pasan_a_la_terminal_las_secuencias_de_los_agentes() {
    use ea_bus::{Bus, FaseInforme, Informe, ResultadoInforme};
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let wt = d.join("repo");
    repo_git(&wt, true);
    let bus = Bus::abrir_en(&d.join("estado")).unwrap();
    let orden: ea_bus::Orden = serde_json::from_value(serde_json::json!({
        "id": "", "modo": "mision", "titulo": format!("titulo{SECUENCIAS}"), "worktree": wt.to_string_lossy(),
        "objetivo": "o", "criterio_exito": format!("true # {SECUENCIAS}"), "creada_en": "", "creada_por": "planificador",
    }))
    .unwrap();
    let id = bus.crear_trabajo(orden).unwrap();
    let evidencia = format!("salida{SECUENCIAS}\nfin\tok");
    let fase = FaseInforme { numero: 1, titulo: format!("fase{SECUENCIAS}"), resultado: ea_bus::EstadoFase::Hecha, diff_stat: String::new(), ejecutado_por: None };
    bus.escribir_informe(
        &id,
        &Informe {
            evidencia: evidencia.clone(),
            motivo: Some(format!("motivo{SECUENCIAS}")),
            aviso: Some(format!("aviso{SECUENCIAS}")),
            diff_stat: Some(format!("a.txt{SECUENCIAS}")),
            fases: vec![fase],
            ..Informe::nuevo(ResultadoInforme::Bloqueado)
        },
    )
    .unwrap();

    let args = ["trabajo", "ver", id.as_str()];
    let o = ea(d, d, &args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let s = sin_control(&args, &o.stdout);
    for visible in ["titulo]52;c;ZWNobyBob2xh[2J[31m0m", "criterio  true # ]52;c;", "motivo]52;", "aviso]52;", "fase]52;", "a.txt]52;"] {
        assert!(s.contains(visible), "falta {visible:?} en {s}");
    }
    assert!(s.contains("salida]52;c;ZWNobyBob2xh[2J[31m0m\nfin\tok"), "los saltos y tabuladores se quedan: {s}");

    let args = ["trabajo", "listar"];
    let o = ea(d, d, &args);
    assert!(sin_control(&args, &o.stdout).contains("titulo]52;"));

    let o = ea(d, d, &["trabajo", "ver", &id, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["informe"]["evidencia"], evidencia, "con --json el texto va entero (escapado por JSON)");
}

/// S-19 en el diario (el `trabajo` de un evento de `ea-mcp` es el que pidió el
/// agente) y en las propuestas (descripción y proponente).
#[test]
fn el_diario_y_las_propuestas_tampoco_pasan_secuencias() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let store = ea_store::Store::open_at(&d.join("ea.db")).unwrap();
    let e = store
        .append_event(ea_store::NewEvent {
            actor: "planificador".into(),
            action: "mcp_ver_trabajo".into(),
            trabajo: Some(format!("trabajo{SECUENCIAS}")),
            outcome: Some(ea_store::EventOutcome::Ok),
            duration_ms: None,
            detail: None,
        })
        .unwrap();
    let p = store
        .crear_propuesta(ea_store::NuevaPropuesta {
            proponente: format!("claude-code{SECUENCIAS}"),
            descripcion: format!("descripcion{SECUENCIAS}"),
            accion: None,
        })
        .unwrap();
    drop(store);
    let (e, p) = (e.id.to_string(), p.id.to_string());
    let casos: [(&[&str], &str); 4] = [
        (&["diario", "ver", &e], "trabajo]52;"),
        (&["diario", "listar"], "trabajo]52;"),
        (&["propuestas", "ver", &p], "descripcion]52;"),
        (&["propuestas", "listar"], "descripcion]52;"),
    ];
    for (args, visible) in casos {
        let o = ea(d, d, args);
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(sin_control(args, &o.stdout).contains(visible), "{args:?}");
    }
}
