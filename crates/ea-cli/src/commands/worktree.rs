//! `ea worktree`: los worktrees propios de los trabajos (spec 019, anexo A).
//! Listar solo lee. Limpiar es `ea_limpiar_worktree` con su simulación y su
//! confirmación en una terminal real (como `ea rutina retirar`): quita el
//! directorio con `git worktree remove` sin `--force` y conserva la rama
//! `ea/<id>`. Fusionar la rama o descartar cambios es del usuario, con git.

use anyhow::Result;
use comfy_table::Table;
use ea_bus::{Bus, InfoWorktree};
use ea_store::Store;

pub fn listar(json: bool) -> Result<()> {
    let v = Bus::abrir()?.listar_worktrees()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if v.is_empty() {
        println!("(ningún worktree propio vivo)");
        return Ok(());
    }
    let mut t = Table::new();
    t.set_header(vec!["Trabajo", "Rama", "Estado del trabajo", "Worktree", "Ruta"]);
    for w in &v {
        t.add_row(vec![
            w.trabajo.clone(),
            w.rama.clone(),
            w.estado_trabajo.unwrap_or("sin trabajo").to_string(),
            estado_texto(w).to_string(),
            w.ruta.clone(),
        ]);
    }
    println!("{t}");
    println!("Quitar uno: ea worktree limpiar <trabajo> (solo cerrado o cancelado y limpio; la rama se conserva).");
    Ok(())
}

fn estado_texto(w: &InfoWorktree) -> &'static str {
    if w.ocupado {
        return "en uso";
    }
    match w.limpio {
        Some(true) => "limpio",
        Some(false) => "con cambios",
        None => "desconocido",
    }
}

pub fn limpiar(store: &mut Store, id: &str) -> Result<()> {
    super::acciones::ejecutar(store, "ea_limpiar_worktree", &serde_json::json!({ "trabajo": id }).to_string())
}
