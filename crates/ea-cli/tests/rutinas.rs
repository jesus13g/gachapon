//! `ea rutina ver --version N` de extremo a extremo, con el binario real y
//! todo el estado en directorios temporales: también el HOME y los XDG, y un
//! `systemctl` de mentira delante en el PATH (la próxima ejecución de
//! `ea rutina listar` y `ver` nunca pregunta al systemd real).

use std::process::Output;

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

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

const RUTINA: &str = concat!("[rutina]\nslug = \"informe\"\nversion = 1\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"true\"\n\n[[pasos]]\ntexto = \"primer texto\"\n");

#[test]
fn ver_una_version_concreta_de_una_rutina() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, RUTINA).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::write(&f, RUTINA.replace("version = 1", "version = 2").replace("primer", "segundo")).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

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
    std::fs::write(&f, RUTINA).unwrap();
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
    std::fs::write(&f, RUTINA).unwrap();
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
    for (slug, texto) in [("informe", RUTINA.to_string()), ("otra", RUTINA.replace("\"informe\"", "\"otra\""))] {
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
