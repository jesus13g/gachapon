//! `ea rutina ver --version N` de extremo a extremo, con el binario real y
//! todo el estado en directorios temporales: también el HOME y los XDG, y un
//! `systemctl` de mentira delante en el PATH (la próxima ejecución de
//! `ea rutina listar` y `ver` nunca pregunta al systemd real).

use std::process::Output;

/// Un `systemctl` que solo falla (como si no hubiera timer), delante en el PATH.
#[cfg(unix)]
fn sin_systemd(dir: &std::path::Path) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("systemctl"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(bin.join("systemctl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap()
}

/// Fuera de Linux no hay systemd: el PATH de siempre.
#[cfg(not(unix))]
fn sin_systemd(_dir: &std::path::Path) -> std::ffi::OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

fn ea(dir: &std::path::Path, args: &[&str]) -> Output {
    ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), dir).args(args).env("PATH", sin_systemd(dir)).output().expect("lanzar ea")
}

/// La rutina de estas pruebas; su worktree, el repo común (U-05: la raíz de
/// un repo con commits).
fn rutina() -> String {
    format!(
        "[rutina]\nslug = \"informe\"\nversion = 1\nworktree = \"{}\"\ncriterio_exito = \"true\"\n\n[[pasos]]\ntexto = \"primer texto\"\n",
        ea_pruebas::repo_comun()
    )
}

#[test]
fn ver_una_version_concreta_de_una_rutina() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, rutina()).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // La v2, como la dejaría aceptar `ea_registrar_rutina` (sin terminal, la
    // CLI no la registra).
    let store = ea_store::Store::open_at(&d.path().join("ea.db")).unwrap();
    ea_pruebas::guardar_rutinas(&store, &[("informe", 2, &rutina().replace("version = 1", "version = 2").replace("primer", "segundo"))]);

    let o = ea(d.path(), &["rutina", "ver", "informe", "--version", "1"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("versión 1") && out.contains("primer texto") && !out.contains("segundo texto"), "{out}");
    let (v2, v1) = (out.find("  v2 ").expect("v2 en la lista"), out.find("  v1 ").expect("v1 en la lista"));
    assert!(v2 < v1, "la más reciente primero: {out}");

    let o = ea(d.path(), &["rutina", "ver", "informe", "--version", "7"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no tiene versión 7"));
}

/// Una rutina retirada (su TOML en la papelera de `rutinas_dir`) no sale en
/// `ea rutina listar` salvo con `--retiradas`, y `ea rutina lanzar` (lo que
/// ejecuta su timer) la rechaza. Retirar sin terminal no hace nada.
#[test]
fn una_rutina_retirada_no_se_lista_ni_se_lanza_desde_la_cli() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, rutina()).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let rutinas = d.path().join("config/rutinas");
    assert!(rutinas.join("informe.toml").exists());

    // Sin TTY no se confirma: el fichero sigue en su sitio.
    let o = ea(d.path(), &["rutina", "retirar", "informe"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("terminal"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(rutinas.join("informe.toml").exists());

    // Retirada como lo haría `ea_retirar_rutina`.
    std::fs::create_dir_all(rutinas.join(".papelera")).unwrap();
    std::fs::rename(rutinas.join("informe.toml"), rutinas.join(".papelera/informe.toml")).unwrap();
    let o = ea(d.path(), &["rutina", "listar"]);
    assert!(!String::from_utf8_lossy(&o.stdout).contains("informe"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = ea(d.path(), &["rutina", "listar", "--retiradas"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("informe") && out.contains("papelera"), "{out}");
    let o = ea(d.path(), &["rutina", "lanzar", "informe"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("está retirada"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!d.path().join("estado/trabajos").exists() || std::fs::read_dir(d.path().join("estado/trabajos")).unwrap().next().is_none());
}

/// B-07: `ea rutina lanzar` (lo que ejecuta su timer) no apila una segunda
/// ejecución mientras la anterior sigue en cola: sale con error y el mismo
/// texto que la web, sin crear otro trabajo.
#[test]
fn lanzar_dos_veces_seguidas_la_misma_rutina_falla_la_segunda() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, rutina()).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = ea(d.path(), &["rutina", "lanzar", "informe"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = ea(d.path(), &["rutina", "lanzar", "informe"]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("la rutina 'informe' ya tiene una ejecución en curso"), "{err}");
    assert_eq!(std::fs::read_dir(d.path().join("estado/trabajos")).unwrap().count(), 1);
}

/// `ea rutina listar` cruza cada rutina con su última ejecución en el bus:
/// la lanzada muestra su trabajo y su estado; la que no se lanzó, «nunca».
#[test]
fn listar_muestra_la_ultima_ejecucion_de_cada_rutina() {
    let d = tempfile::tempdir().unwrap();
    for (slug, texto) in [("informe", rutina()), ("otra", rutina().replace("\"informe\"", "\"otra\""))] {
        let f = d.path().join(format!("{slug}.toml"));
        std::fs::write(&f, texto).unwrap();
        let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let o = ea(d.path(), &["rutina", "lanzar", "informe"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let id = String::from_utf8_lossy(&o.stdout).trim().to_string();

    let o = ea(d.path(), &["rutina", "listar"]);
    let out = String::from_utf8_lossy(&o.stdout);
    let fila = |slug: &str| out.lines().find(|l| l.contains(&format!(" {slug} "))).unwrap_or_else(|| panic!("{slug}: {out}")).to_string();
    assert!(fila("informe").contains(&format!("(en_cola, {id})")), "{out}");
    assert!(fila("otra").contains("nunca"), "{out}");
}

/// U-05: la v1 de una rutina se registra directamente y deja
/// `rutina_registrada` en el diario. Una versión nueva de una que ya existe
/// es `ea_registrar_rutina`, que se confirma en una terminal: sin ella no se
/// registra nada y el mensaje dice por qué y qué hacer.
#[test]
fn registrar_la_v1_es_directa_y_una_version_nueva_exige_terminal() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, rutina()).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let store = ea_store::Store::open_at(&d.path().join("ea.db")).unwrap();
    let registros = || store.list_events(&Default::default()).unwrap().into_iter().filter(|e| e.action == "rutina_registrada").collect::<Vec<_>>();
    let r = registros();
    assert_eq!(r.len(), 1, "{r:?}");
    // Sin terminal la firma el planificador (U-06).
    assert_eq!(r[0].actor, "claude-code");
    let fichero = d.path().join("config/rutinas/informe.toml");
    let antes = std::fs::read_to_string(&fichero).unwrap();

    std::fs::write(&f, rutina().replace("version = 1", "version = 2")).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(err.contains("sin TTY no se confirma") && err.contains("ya está registrada (versión 1)"), "{err}");
    assert!(err.contains("ea_registrar_rutina"), "{err}");
    assert_eq!(store.ultima_rutina("informe").unwrap().unwrap().version, 1);
    assert_eq!(std::fs::read_to_string(&fichero).unwrap(), antes);
    assert_eq!(registros().len(), 1);

    // Lo que no vale se dice antes que lo de la terminal: la misma versión.
    std::fs::write(&f, rutina()).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("la nueva debe ser mayor"));
}

/// U-05: el worktree de una rutina tiene que ser la raíz de un repo con
/// commits también al registrarla desde la CLI.
#[test]
fn registrar_rechaza_un_worktree_que_no_es_un_repo() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    let sin_repo = d.path().join("sin-repo");
    std::fs::create_dir_all(&sin_repo).unwrap();
    std::fs::write(&f, rutina().replace(ea_pruebas::repo_comun(), &sin_repo.to_string_lossy().replace('\\', "/"))).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no está dentro de un repositorio git"));
    assert!(!d.path().join("config/rutinas/informe.toml").exists());
}

/// U-05: con terminal (una pty de `script`, como la de una persona), la
/// versión nueva pasa por `ea_registrar_rutina`: simula con los cambios,
/// pregunta y, con «s», la registra con `rutina_registrada` (firmada por el
/// usuario) y `accion_ejecutada`. Solo en Linux y con `script` (util-linux).
#[cfg(target_os = "linux")]
#[test]
fn con_terminal_una_version_nueva_se_simula_se_confirma_y_deja_evento() {
    use std::io::Write;
    if std::process::Command::new("script").arg("--version").output().is_err() {
        eprintln!("aviso: no hay `script`; se salta la prueba con terminal");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, rutina()).unwrap();
    assert!(ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]).status.success());
    std::fs::write(&f, rutina().replace("version = 1", "version = 2").replace("primer", "segundo")).unwrap();
    let orden = format!("{} rutina registrar {}", env!("CARGO_BIN_EXE_ea"), f.display());
    let mut hijo = ea_pruebas::cli::ea("script", d.path())
        .args(["-qec", &orden, "/dev/null"])
        .env("PATH", sin_systemd(d.path()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("lanzar script");
    hijo.stdin.take().unwrap().write_all(b"s\n").unwrap();
    let o = hijo.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("ea_registrar_rutina") && out.contains("Cambios frente a la versión 1"), "{out}");
    assert!(out.contains("Rutina 'informe' registrada en la versión 2"), "{out}");
    let store = ea_store::Store::open_at(&d.path().join("ea.db")).unwrap();
    let fila = store.ultima_rutina("informe").unwrap().unwrap();
    assert_eq!((fila.version, fila.creada_por.as_str()), (2, "usuario"));
    assert!(std::fs::read_to_string(d.path().join("config/rutinas/informe.toml")).unwrap().contains("segundo texto"));
    let eventos = store.list_events(&Default::default()).unwrap();
    let registro = eventos.iter().find(|e| e.action == "rutina_registrada" && e.detail.as_deref().is_some_and(|x| x.contains("\"version\":2")));
    assert_eq!(registro.map(|e| e.actor.as_str()), Some("usuario"), "{eventos:?}");
    assert!(eventos.iter().any(|e| e.action == "accion_ejecutada"), "{eventos:?}");
}
