//! Utilidades de las pruebas de integración del workspace: lo que antes
//! copiaba cada `tests/*.rs`. Escribir ficheros, repos git, una base con
//! rutinas, órdenes, un bus con cuatro trabajos conocidos, un `systemctl`
//! falso, el binario `ea` aislado ([`cli`]), el HOME pequeño de la
//! biblioteca ([`biblioteca`]) y, con la *feature* `mcp`, el servidor y el
//! cliente MCP de prueba.
//!
//! Solo como `[dev-dependencies]` y solo desde `tests/`: una prueba
//! `#[cfg(test)]` de un crate del que este depende vería dos copias de sus
//! tipos. Nada de aquí toca el estado real: todo va bajo el directorio que
//! se le da (un `tempfile`).

pub mod biblioteca;
pub mod cli;
#[cfg(feature = "mcp")]
pub mod mcp;

use std::path::{Path, PathBuf};
use std::process::Command;

use ea_bus::{Bus, Evento, Fase, Orden};
use ea_store::Store;

/// Escribe `texto` en `ruta`, creando antes sus directorios.
pub fn escribe(ruta: impl AsRef<Path>, texto: &str) {
    let ruta = ruta.as_ref();
    std::fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    std::fs::write(ruta, texto).unwrap();
}

/// Falla si `texto` lleva alguno de los `secretos`.
pub fn sin_secretos(texto: &str, secretos: &[&str]) {
    for s in secretos {
        assert!(!texto.contains(s), "se coló {s}: {texto}");
    }
}

/// `git -C dir args` con un autor de prueba y sin firmar; falla si git
/// falla. Devuelve la salida sin los blancos de los extremos.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let s = Command::new("git").args(["-c", "user.email=a@b", "-c", "user.name=x", "-c", "commit.gpgsign=false", "-C"]).arg(dir).args(args).output().unwrap();
    assert!(s.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&s.stderr));
    String::from_utf8_lossy(&s.stdout).trim().to_string()
}

/// Un repo git en `dir` (rama `main`) con `a.txt`; con `commit`, guardado ya
/// en un primer commit (el bus no acepta un worktree sin commits). Devuelve
/// `dir`.
pub fn repo_git(dir: &Path, commit: bool) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    if commit {
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "inicial"]);
    }
    dir.to_path_buf()
}

/// Un repo git con un commit, el mismo para todo el proceso de pruebas: el
/// worktree de las rutinas de prueba, que tiene que ser la raíz de un repo y
/// no `$HOME` (U-05). En forma canónica y con `/`, para poder escribirlo en
/// un TOML. Vive en un directorio temporal que no se borra (es un `static`),
/// y ninguna prueba escribe en él.
pub fn repo_comun() -> &'static str {
    static REPO: std::sync::OnceLock<(tempfile::TempDir, String)> = std::sync::OnceLock::new();
    &REPO
        .get_or_init(|| {
            let d = tempfile::tempdir().unwrap();
            let repo = std::fs::canonicalize(repo_git(&d.path().join("repo"), true)).unwrap();
            let texto = repo.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
            (d, texto)
        })
        .1
}

/// Una base nueva (`t.db` en un directorio temporal, que se devuelve con
/// ella) con `rutinas` ya registradas (ver [`guardar_rutinas`]).
pub fn store(rutinas: &[(&str, u32, &str)]) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_at(&dir.path().join("t.db")).unwrap();
    guardar_rutinas(&s, rutinas);
    (s, dir)
}

/// Registra en `store` cada rutina (slug, versión y definición), creada por
/// `test` el 1 de enero de 2026.
pub fn guardar_rutinas(store: &Store, rutinas: &[(&str, u32, &str)]) {
    for (slug, version, definicion) in rutinas {
        store
            .guardar_rutina(&ea_store::FilaRutina {
                slug: slug.to_string(),
                version: i64::from(*version),
                definicion: definicion.to_string(),
                creada_en: "2026-01-01T00:00:00Z".into(),
                creada_por: "test".into(),
            })
            .unwrap();
    }
}

/// La misión de prueba en `wt`: «prueba», «haz algo», criterio `true`, de
/// `claude-code`.
pub fn orden(wt: &Path) -> Orden {
    Orden::mision("prueba", wt.to_string_lossy(), "haz algo", "true", "claude-code")
}

/// Cuatro trabajos que aparecen en un orden distinto del de los estados: una
/// misión cerrada, otra en cola, una obra en curso y una misión bloqueada en
/// revisión. Ids y fecha de creación fijos; devuelve los ids.
pub fn sembrar_trabajos(bus: &Bus, wt: &str) -> [String; 4] {
    let ids = ["20260101T000001-00000001", "20260101T000002-00000002", "20260101T000003-00000003", "20260101T000004-00000004"].map(String::from);
    let fase =
        |n: u32| Fase { numero: n, titulo: format!("fase {n}"), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
    for (i, id) in ids.iter().enumerate() {
        let base = if i == 2 {
            Orden::obra("una obra", wt, "o", vec![fase(1), fase(2)], None, "prueba")
        } else {
            Orden::mision(format!("misión {}", i + 1), wt, "o", "true", "prueba")
        };
        bus.crear_trabajo(Orden { id: id.clone(), creada_en: "2026-01-01T00:00:00Z".into(), ..base }).unwrap();
    }
    for id in [&ids[0], &ids[2], &ids[3]] {
        bus.retirar_de_colas(id).unwrap();
        bus.aplicar(id, Evento::Tomar).unwrap();
    }
    let informe = |v: serde_json::Value| serde_json::from_value(v).unwrap();
    bus.aplicar(&ids[0], Evento::Entregar(informe(serde_json::json!({ "resultado": "terminado", "duracion_s": 12 })))).unwrap();
    bus.aplicar(&ids[0], Evento::Cerrar).unwrap();
    bus.aplicar(&ids[3], Evento::Entregar(informe(serde_json::json!({ "resultado": "bloqueado", "motivo": "no pasa", "duracion_s": 30 })))).unwrap();
    ids
}

/// Un `systemctl` y un `systemd-analyze` falsos (guiones de `sh` en `dir`;
/// solo sirven donde hay `sh`): apuntan sus argumentos en
/// `<dir>/ordenes.log`, uno por línea y `--` al final, y responden lo justo
/// para las pruebas. `is-active` mira si la unidad está en `<dir>/unidades`
/// (pásale ese directorio a `Temporizadores::systemd`) y un calendario con
/// «malo» no vale. Devuelve los argv de los dos y el registro.
pub fn systemd_falso(dir: &Path) -> (Vec<String>, Vec<String>, PathBuf) {
    let log = dir.join("ordenes.log");
    let sc = dir.join("systemctl.sh");
    std::fs::write(
        &sc,
        format!(
            "printf '%s\\n' systemctl \"$@\" -- >> '{log}'\n\
             if [ \"$2\" = is-active ]; then\n\
               if [ -f '{dir}/unidades/'\"$3\" ]; then echo active; else echo inactive; exit 3; fi\n\
             fi\n",
            log = log.display(),
            dir = dir.display()
        ),
    )
    .unwrap();
    let sa = dir.join("analyze.sh");
    std::fs::write(
        &sa,
        format!(
            "printf '%s\\n' analyze \"$@\" -- >> '{log}'\n\
             case \"$4\" in *malo*) echo \"Failed to parse calendar specification '$4'\" >&2; exit 1;; esac\n\
             echo \"Normalized form: $4\"\n\
             echo '    Next elapse: Thu 2026-10-22 08:00:00 CEST'\n\
             echo '   Iteration #2: Fri 2026-10-23 08:00:00 CEST'\n\
             echo '   Iteration #3: Sat 2026-10-24 08:00:00 CEST'\n",
            log = log.display()
        ),
    )
    .unwrap();
    let sh = |p: &Path| vec!["sh".to_string(), p.display().to_string()];
    (sh(&sc), sh(&sa), log)
}
