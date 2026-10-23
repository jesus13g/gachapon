//! Un `sh` para los criterios de éxito y otros comandos escritos en shell.

use std::path::PathBuf;
use std::process::Command;

/// Ruta de un `sh` POSIX. En Unix, `/bin/sh`. En Windows, el `sh.exe` del
/// `PATH` o, si no está, el de Git for Windows deducido de `git.exe`
/// (`<git>/../../bin/sh.exe` y `<git>/../../usr/bin/sh.exe`).
pub fn sh() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let p = PathBuf::from("/bin/sh");
        p.is_file().then_some(p)
    }
    #[cfg(not(unix))]
    {
        use crate::rutas::buscar_en_path;
        if let Some(p) = buscar_en_path("sh") {
            return Some(p);
        }
        let git = buscar_en_path("git")?;
        // `…/Git/cmd/git.exe` o `…/Git/mingw64/bin/git.exe`: se sube al raíz de Git.
        let mut padres = git.ancestors().skip(1);
        for _ in 0..4 {
            let Some(base) = padres.next() else { break };
            for rel in ["bin/sh.exe", "usr/bin/sh.exe"] {
                let c = base.join(rel);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
        None
    }
}

/// `sh -c <script>` listo para configurar y lanzar.
pub fn orden_sh(script: &str) -> Option<Command> {
    let mut c = Command::new(sh()?);
    c.arg("-c").arg(script);
    Some(c)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sh_ejecuta_un_script() {
        let Some(mut c) = super::orden_sh("echo hola") else { return };
        let s = c.output().unwrap();
        assert!(s.status.success());
        assert_eq!(String::from_utf8_lossy(&s.stdout).trim(), "hola");
    }
}
