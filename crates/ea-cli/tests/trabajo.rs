//! `ea trabajo crear` con el binario real y todo el estado en directorios
//! temporales: valida la orden igual que la web, por las dos formas
//! (`--orden <fichero>` y las opciones sueltas).

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn ea(dir: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ea"))
        .args(args)
        .current_dir(cwd)
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .stdin(Stdio::null())
        .output()
        .expect("lanzar ea")
}

fn git(wt: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(wt)
        .args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// Un repo git en `wt`, con un commit si `commit`.
fn repo(wt: &Path, commit: bool) {
    std::fs::create_dir_all(wt).unwrap();
    git(wt, &["init", "-q"]);
    if commit {
        std::fs::write(wt.join("a.txt"), "a\n").unwrap();
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-q", "-m", "inicio"]);
    }
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
    repo(&vacio, false);
    falla_con(&crear_con_orden(d, vacio.to_str().unwrap(), None), "el repo no tiene ningún commit");
    sin_trabajos(d);
}

/// B-06 por la CLI: una obra con las fases mal numeradas no se crea.
#[test]
fn crear_con_orden_rechaza_las_fases_mal_numeradas() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    let wt = d.join("repo");
    repo(&wt, true);
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
    repo(&wt, true);
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
