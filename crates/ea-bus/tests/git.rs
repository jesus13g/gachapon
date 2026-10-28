//! `ea_bus::git` con git de verdad en directorios temporales: el texto de los
//! diffs es el de siempre (el de `git` a secas más los ficheros nuevos), y
//! nada de lo que el repo configure (fsmonitor, `diff.external`, `textconv`)
//! se ejecuta al pedir diffs o instantáneas.

use std::path::{Path, PathBuf};
use std::process::Command;

use ea_bus::git::{self, FicheroNuevo, TOPE_LECTURA};
use ea_bus::{EXCLUIR_DIFF, ErrorBus};

/// `git` sin nada más, como lo lanzaría el usuario a mano. Devuelve stdout tal cual.
fn git_a_secas(wt: &Path, args: &[&str], indice: Option<&Path>) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(wt).args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"]);
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

struct Repo {
    d: tempfile::TempDir,
    wt: PathBuf,
    inicio: String,
}

/// Un repo con un commit (que lleva `base`, p. ej. un `.gitattributes`) y,
/// encima, un cambio y ficheros nuevos de todo tipo: texto con y sin salto
/// final, binario, nombre con tilde y con espacio, y `__pycache__`.
fn repo(base: &[(&str, &str)]) -> Repo {
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().join("repo");
    std::fs::create_dir_all(&wt).unwrap();
    git_a_secas(&wt, &["init", "-q"], None);
    git_a_secas(&wt, &["config", "core.autocrlf", "false"], None);
    git_a_secas(&wt, &["config", "core.quotePath", "true"], None);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    for (f, c) in base {
        std::fs::write(wt.join(f), c).unwrap();
    }
    git_a_secas(&wt, &["add", "-A"], None);
    git_a_secas(&wt, &["commit", "-q", "-m", "inicio"], None);
    let inicio = git_a_secas(&wt, &["rev-parse", "HEAD"], None).trim().to_string();
    std::fs::write(wt.join("a.txt"), "a\nb\n").unwrap();
    std::fs::write(wt.join("nuevo.txt"), "hola\n").unwrap();
    std::fs::write(wt.join("sin-salto.txt"), "x").unwrap();
    std::fs::write(wt.join("datos.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    std::fs::write(wt.join("canción.txt"), "ñ\n").unwrap();
    std::fs::create_dir_all(wt.join("sub dir")).unwrap();
    std::fs::write(wt.join("sub dir/espacio.txt"), "e\n").unwrap();
    std::fs::create_dir_all(wt.join("__pycache__")).unwrap();
    std::fs::write(wt.join("__pycache__/m.pyc"), "pyc").unwrap();
    Repo { d, wt, inicio }
}

/// Los bloques de los ficheros nuevos tal como los pone el panel tras el diff.
const NUEVOS_PANEL: &str = "\n=== fichero nuevo sin seguimiento: canción.txt ===\nñ\n\
\n=== fichero nuevo sin seguimiento: datos.bin (binario o ilegible) ===\n\
\n=== fichero nuevo sin seguimiento: nuevo.txt ===\nhola\n\
\n=== fichero nuevo sin seguimiento: sin-salto.txt ===\nx\
\n=== fichero nuevo sin seguimiento: sub dir/espacio.txt ===\ne\n";

#[test]
fn el_diff_del_trabajo_es_el_de_git_mas_los_ficheros_nuevos() {
    let r = repo(&[]);
    let a_secas = git_a_secas(&r.wt, &["diff", &r.inicio, "--", ".", EXCLUIR_DIFF], None);
    assert!(a_secas.contains("+b\n"), "{a_secas}");
    assert_eq!(git::diff_desde(&r.wt, &r.inicio, TOPE_LECTURA).unwrap(), a_secas);
    assert_eq!(git::diff_trabajo(&r.wt, &r.inicio, TOPE_LECTURA).unwrap(), format!("{a_secas}{NUEVOS_PANEL}"));

    let nuevos = git::ficheros_nuevos(&r.wt, TOPE_LECTURA).unwrap();
    let rutas: Vec<&str> = nuevos.iter().map(|f| f.ruta.as_str()).collect();
    assert_eq!(rutas, ["canción.txt", "datos.bin", "nuevo.txt", "sin-salto.txt", "sub dir/espacio.txt"]);
    assert_eq!(nuevos[1], FicheroNuevo { ruta: "datos.bin".into(), texto: None });
    assert_eq!(nuevos[3].bloque(), "\n=== fichero nuevo sin seguimiento: sin-salto.txt ===\nx");
    assert_eq!(nuevos[1].bloque(), "\n=== fichero nuevo sin seguimiento: datos.bin (binario o ilegible) ===");

    // El del informe: el `--stat` de git y los nuevos como los cita git.
    let stat = git_a_secas(&r.wt, &["diff", "--stat", &r.inicio], None).trim().to_string();
    let esperado = format!("{stat}\nnuevos: \"canci\\303\\263n.txt\", datos.bin, nuevo.txt, sin-salto.txt, sub dir/espacio.txt");
    assert_eq!(git::diff_stat_desde(&r.wt, &r.inicio), esperado);
}

#[test]
fn la_instantanea_no_toca_el_indice_y_su_diff_es_el_de_git() {
    let r = repo(&[]);
    let dir = r.d.path().join("trabajo");
    std::fs::create_dir_all(&dir).unwrap();
    let arbol = git::instantanea(&r.wt, &dir).unwrap();
    let idx = r.d.path().join("indice");
    git_a_secas(&r.wt, &["add", "-A"], Some(&idx));
    assert_eq!(arbol, git_a_secas(&r.wt, &["write-tree"], Some(&idx)).trim());
    assert!(!dir.join(".indice-tmp").exists(), "el índice temporal se borra");
    assert_eq!(git_a_secas(&r.wt, &["diff", "--cached", "--name-only"], None), "", "el índice del usuario sigue intacto");

    let base = format!("{}^{{tree}}", r.inicio);
    for stat in [false, true] {
        let mut args = vec!["diff"];
        if stat {
            args.push("--stat");
        }
        args.extend([base.as_str(), &arbol, "--", ".", EXCLUIR_DIFF]);
        let a_secas = git_a_secas(&r.wt, &args, None);
        assert!(a_secas.contains("nuevo.txt") && !a_secas.contains("m.pyc"), "{a_secas}");
        assert_eq!(git::diff_entre(&r.wt, &base, &arbol, stat, TOPE_LECTURA).unwrap(), a_secas, "stat = {stat}");
    }
}

#[test]
fn repo_commits_y_errores() {
    let r = repo(&[]);
    assert_eq!(git::commit_actual(&r.wt).as_deref(), Some(r.inicio.as_str()));
    assert!(git::es_repo(&r.wt, TOPE_LECTURA).unwrap() && git::tiene_commits(&r.wt, TOPE_LECTURA).unwrap());
    let fuera = tempfile::tempdir().unwrap();
    assert!(!git::es_repo(fuera.path(), TOPE_LECTURA).unwrap());
    assert!(!git::tiene_commits(fuera.path(), TOPE_LECTURA).unwrap());
    assert_eq!(git::commit_actual(fuera.path()), None);
    git_a_secas(fuera.path(), &["init", "-q"], None);
    assert!(git::es_repo(fuera.path(), TOPE_LECTURA).unwrap() && !git::tiene_commits(fuera.path(), TOPE_LECTURA).unwrap());

    // Lo que dice git llega en el error, con la orden.
    let e = git::diff_desde(&r.wt, "0123456789abcdef", TOPE_LECTURA).unwrap_err();
    assert!(matches!(&e, ErrorBus::Git { orden, detalle } if orden.starts_with("diff ") && !detalle.is_empty()), "{e}");
    assert!(e.to_string().starts_with("git diff "), "{e}");
    // Una revisión que parece una opción no llega a git.
    let salida = r.d.path().join("escrito");
    let opcion = format!("--output={}", salida.display());
    assert!(matches!(git::diff_desde(&r.wt, &opcion, TOPE_LECTURA), Err(ErrorBus::Argumentos(_))));
    assert!(matches!(git::diff_entre(&r.wt, "HEAD", &opcion, true, TOPE_LECTURA), Err(ErrorBus::Argumentos(_))));
    assert!(!salida.exists());
}

/// Un guion que deja la marca `nombre` en `dir` y hace lo mínimo de su papel.
#[cfg(unix)]
fn guion(dir: &Path, nombre: &str, cuerpo: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let g = dir.join(format!("{nombre}.sh"));
    std::fs::write(&g, format!("#!/bin/sh\ntouch '{}'\n{cuerpo}\n", dir.join(format!("marca-{nombre}")).display())).unwrap();
    std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    g
}

/// Planta en `.git/config` `clave = guion` y llama a todo lo del módulo que
/// el sistema usa fuera del sandbox. Devuelve qué llamadas dejaron marca.
#[cfg(unix)]
fn marcas_al_llamar(clave: &str, nombre: &str, cuerpo: &str, base: &[(&str, &str)]) -> Vec<&'static str> {
    let r = repo(base);
    let guiones = r.d.path().join("guiones");
    std::fs::create_dir_all(&guiones).unwrap();
    let dir = r.d.path().join("trabajo");
    std::fs::create_dir_all(&dir).unwrap();
    let arbol = git::instantanea(&r.wt, &dir).unwrap();
    let g = guion(&guiones, nombre, cuerpo);
    git_a_secas(&r.wt, &["config", clave, &g.to_string_lossy()], None);
    let marca = guiones.join(format!("marca-{nombre}"));
    let base = format!("{}^{{tree}}", r.inicio);
    let llamadas: [(&'static str, &dyn Fn()); 9] = [
        ("diff_trabajo", &|| drop(git::diff_trabajo(&r.wt, &r.inicio, TOPE_LECTURA))),
        ("diff_desde", &|| drop(git::diff_desde(&r.wt, &r.inicio, TOPE_LECTURA))),
        ("diff_entre", &|| drop(git::diff_entre(&r.wt, &base, &arbol, false, TOPE_LECTURA))),
        ("diff_entre --stat", &|| drop(git::diff_entre(&r.wt, &base, &arbol, true, TOPE_LECTURA))),
        ("diff_stat_desde", &|| drop(git::diff_stat_desde(&r.wt, &r.inicio))),
        ("instantanea", &|| drop(git::instantanea(&r.wt, &dir))),
        ("ficheros_nuevos", &|| drop(git::ficheros_nuevos(&r.wt, TOPE_LECTURA))),
        ("es_repo", &|| drop(git::es_repo(&r.wt, TOPE_LECTURA))),
        ("commit_actual", &|| drop(git::commit_actual(&r.wt))),
    ];
    let mut con_marca = vec![];
    for (que, llamar) in llamadas {
        llamar();
        if std::fs::remove_file(&marca).is_ok() {
            con_marca.push(que);
        }
    }
    con_marca
}

#[cfg(unix)]
#[test]
fn el_fsmonitor_del_repo_no_se_ejecuta() {
    let m = marcas_al_llamar("core.fsmonitor", "fsmonitor", "exit 1", &[]);
    assert!(m.is_empty(), "se ejecutó el fsmonitor del repo en: {m:?}");
}

#[cfg(unix)]
#[test]
fn el_diff_externo_del_repo_no_se_ejecuta() {
    let m = marcas_al_llamar("diff.external", "externo", "exit 0", &[]);
    assert!(m.is_empty(), "se ejecutó diff.external en: {m:?}");
}

#[cfg(unix)]
#[test]
fn el_textconv_del_repo_no_se_ejecuta() {
    let m = marcas_al_llamar("diff.x.textconv", "textconv", "cat \"$1\"", &[(".gitattributes", "* diff=x\n")]);
    assert!(m.is_empty(), "se ejecutó el textconv del repo en: {m:?}");
}

/// Un git que no termina se mata al vencer su tope (aquí, un filtro `clean`
/// que tarda: los filtros no se tocan, git-lfs los necesita).
#[cfg(unix)]
#[test]
fn un_git_que_no_termina_se_corta_en_su_tope() {
    use std::time::Duration;
    let r = repo(&[(".gitattributes", "*.txt filter=lento\n")]);
    let guiones = r.d.path().join("guiones");
    std::fs::create_dir_all(&guiones).unwrap();
    let g = guion(&guiones, "lento", "sleep 30; cat");
    git_a_secas(&r.wt, &["config", "filter.lento.clean", &g.to_string_lossy()], None);
    let t = std::time::Instant::now();
    let e = git::git_con_indice(&r.wt, ["add", "-A"], &r.d.path().join("idx"), Duration::from_secs(1)).unwrap_err();
    assert!(matches!(e, ErrorBus::GitSinRespuesta(1)), "{e}");
    assert!(t.elapsed() < Duration::from_secs(20), "tardó {:?}", t.elapsed());
}

/// U-03: el worktree de una orden es la raíz de su árbol de trabajo. Un
/// subdirectorio del repo se rechaza (no tiene un `.git` que el sandbox
/// pueda dejar de solo lectura); la raíz y un `git worktree add` valen, y lo
/// que se devuelve es la ruta canónica.
#[test]
fn el_worktree_tiene_que_ser_la_raiz_de_su_repo() {
    let r = repo(&[]);
    let raiz = ea_plataforma::rutas::canonica(&r.wt).unwrap();
    let validar = |p: &Path| ea_bus::validar_worktree(p.to_str().unwrap(), TOPE_LECTURA);
    assert_eq!(validar(&r.wt).unwrap(), raiz.to_str().unwrap());

    let sub = r.wt.join("sub dir");
    let e = validar(&sub).unwrap_err();
    assert!(matches!(&e, ErrorBus::Argumentos(m) if m.contains("no es la raíz de su repositorio") && m.contains(raiz.to_str().unwrap())), "{e}");

    let enlazado = r.d.path().join("enlazado");
    git_a_secas(&r.wt, &["worktree", "add", "-q", "--detach", enlazado.to_str().unwrap()], None);
    let canon = ea_plataforma::rutas::canonica(&enlazado).unwrap();
    assert_eq!(validar(&enlazado).unwrap(), canon.to_str().unwrap());
    std::fs::create_dir(canon.join("otro")).unwrap();
    let e = validar(&canon.join("otro")).unwrap_err();
    assert!(e.to_string().contains("no es la raíz de su repositorio"), "tampoco un subdirectorio del worktree enlazado: {e}");
}
