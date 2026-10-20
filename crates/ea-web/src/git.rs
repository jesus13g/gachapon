//! `git` sobre el worktree de un trabajo, con tiempo límite.

use std::time::Duration;

use crate::servicio::{ErrorWeb, Resultado};

const LIMITE: Duration = Duration::from_secs(10);

async fn git(wt: &str, args: &[&str]) -> Resultado<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(wt).args(args).kill_on_drop(true);
    let o = tokio::time::timeout(LIMITE, cmd.output())
        .await
        .map_err(|_| ErrorWeb::Tiempo("git no respondió".into()))?
        .map_err(|e| ErrorWeb::Interno(e.to_string()))?;
    if !o.status.success() {
        return Err(ErrorWeb::Interno(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Lo cambiado desde `inicio` más los ficheros nuevos sin seguimiento,
/// recortado a `max` bytes. Devuelve el texto y si se recortó.
pub async fn diff_desde(wt: &str, inicio: &str, max: usize) -> Resultado<(String, bool)> {
    let mut texto = git(wt, &["diff", inicio, "--", ".", ":(exclude,glob)**/__pycache__/**"]).await?;
    let nuevos = git(wt, &["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    for f in nuevos.split('\0').filter(|f| !f.is_empty() && !f.contains("__pycache__")) {
        match std::fs::read(std::path::Path::new(wt).join(f)) {
            Ok(b) if std::str::from_utf8(&b).is_ok() => {
                texto.push_str(&format!("\n=== fichero nuevo sin seguimiento: {f} ===\n{}", String::from_utf8_lossy(&b)))
            }
            _ => texto.push_str(&format!("\n=== fichero nuevo sin seguimiento: {f} (binario o ilegible) ===\n")),
        }
    }
    let truncado = texto.len() > max;
    if truncado {
        let mut fin = max;
        while !texto.is_char_boundary(fin) {
            fin -= 1;
        }
        texto.truncate(fin);
    }
    Ok((texto, truncado))
}

/// Si `wt` está dentro de un árbol de trabajo de git. Sin shell y sin
/// fsmonitor (que podría lanzar un programa del repo).
pub async fn es_repo(wt: &str) -> Resultado<bool> {
    match git(wt, &["-c", "core.fsmonitor=false", "rev-parse", "--is-inside-work-tree"]).await {
        Ok(salida) => Ok(salida.trim() == "true"),
        Err(ErrorWeb::Tiempo(m)) => Err(ErrorWeb::Tiempo(m)),
        Err(_) => Ok(false),
    }
}

/// Si el repo de `wt` tiene HEAD apuntando a un commit. El ejecutor arranca
/// con `rev-parse HEAD` (es el `commit_inicio`) y bloquea un repo sin
/// commits; esto lo detecta antes de encolar (D11). Sin fsmonitor.
pub async fn tiene_commits(wt: &str) -> Resultado<bool> {
    match git(wt, &["-c", "core.fsmonitor=false", "rev-parse", "--verify", "-q", "HEAD^{commit}"]).await {
        Ok(salida) => Ok(!salida.trim().is_empty()),
        Err(ErrorWeb::Tiempo(m)) => Err(ErrorWeb::Tiempo(m)),
        Err(_) => Ok(false),
    }
}
