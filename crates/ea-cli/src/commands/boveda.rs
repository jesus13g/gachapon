//! `ea boveda …`: ver, buscar y leer la bóveda, y promover notas a la región
//! compartida. Promover es siempre del usuario (decisión 9): pide terminal
//! interactiva, muestra la simulación y exige confirmación, igual que aceptar
//! la propuesta de un agente.

use anyhow::{Context, Result};
use comfy_table::Table;
use ea_boveda::{ruta::REGIONES, Boveda, Limites};
use ea_store::{ResultadoBusqueda, Store};
use serde_json::json;

fn tabla(r: &[ResultadoBusqueda], con_fragmento: bool) {
    if r.is_empty() {
        println!("No hay notas que coincidan.");
        return;
    }
    let mut t = Table::new();
    let mut cab = vec!["Ruta", "Título", "Autor", "Actualizada", "Promoción"];
    if con_fragmento {
        cab.push("Fragmento");
    }
    t.set_header(cab);
    for x in r {
        let mut fila = vec![
            x.ruta.clone(),
            x.titulo.clone(),
            x.autor.clone().unwrap_or_else(|| "-".into()),
            x.actualizada.clone().unwrap_or_else(|| "-".into()),
            x.promocion.clone().unwrap_or_else(|| "-".into()),
        ];
        if con_fragmento {
            fila.push(x.fragmento.replace('\n', " "));
        }
        t.add_row(fila);
    }
    println!("{t}");
}

pub fn estado(store: &mut Store) -> Result<()> {
    let b = Boveda::configurada();
    println!("Raíz de la bóveda: {}", b.raiz().display());
    println!("  (EA_BOVEDA_DIR o ~/.config/ecosistema-asistente/boveda.toml la cambian)");
    let r = b.sincronizar(store, false)?;
    if r.indexadas + r.actualizadas + r.eliminadas > 0 {
        println!(
            "  Índice al día: {} nuevas, {} actualizadas, {} eliminadas",
            r.indexadas, r.actualizadas, r.eliminadas
        );
    }
    let cuentas = store.boveda_contar_por_region()?;
    for region in REGIONES {
        let n = cuentas.iter().find(|(r, _)| r == region).map(|(_, n)| *n).unwrap_or(0);
        println!("  {region:<13} {n} notas");
    }
    Ok(())
}

pub fn listar(store: &mut Store, region: Option<String>, limite: usize) -> Result<()> {
    let b = Boveda::configurada();
    let lim = Limites { max_resultados: limite.max(1), ..Limites::AMPLIOS };
    tabla(&b.listar(store, region.as_deref(), Some(limite), &lim)?, false);
    Ok(())
}

pub fn buscar(store: &mut Store, consulta: &str, region: Option<String>, limite: usize) -> Result<()> {
    let b = Boveda::configurada();
    let lim = Limites { max_resultados: limite.max(1), ..Limites::AMPLIOS };
    tabla(&b.buscar(store, consulta, region.as_deref(), Some(limite), &lim)?, true);
    Ok(())
}

pub fn leer(ruta: &str, desde: usize) -> Result<()> {
    let n = Boveda::configurada().leer(ruta, desde, &Limites::AMPLIOS)?;
    println!("Nota {} (región {})", n.ruta, n.region);
    match &n.cabecera {
        Some(c) => {
            println!("  Título:      {}", c.titulo);
            println!("  Autor:       {}", c.autor);
            println!("  Creada:      {}", c.creada);
            println!("  Actualizada: {}", c.actualizada);
            println!("  Origen:      {}", c.origen);
            println!("  Promoción:   {}", c.promocion);
            if !c.tags.is_empty() {
                println!("  Etiquetas:   {}", c.tags.join(", "));
            }
            if let Some(de) = &c.promovida_de {
                println!("  Promovida de {de} por {}", c.promovida_por.clone().unwrap_or_default());
            }
            if let Some(a) = &c.promovida_a {
                println!("  Promovida a  {a}");
            }
        }
        None => println!("  (sin cabecera del contrato: {})", n.aviso_cabecera.clone().unwrap_or_default()),
    }
    println!("\n{}", n.contenido);
    if let Some(sig) = n.siguiente_desde {
        println!("\n… quedan {} caracteres; sigue con --desde {sig}", n.total_caracteres - sig);
    }
    Ok(())
}

pub fn reindexar(store: &mut Store) -> Result<()> {
    let r = Boveda::configurada().reindexar(store, super::actor())?;
    println!(
        "Índice reconstruido: {} nuevas, {} actualizadas, {} eliminadas, {} omitidas por tamaño",
        r.indexadas, r.actualizadas, r.eliminadas, r.omitidas
    );
    Ok(())
}

/// Promoción directa del usuario: la misma herramienta y el mismo flujo de
/// confirmación que `ea acciones ejecutar ea_promover_nota`.
pub fn promover(ctx: &mut super::Contexto, ruta: &str, destino: Option<String>) -> Result<()> {
    let mut args = json!({ "ruta": ruta });
    if let Some(d) = destino {
        args["destino"] = json!(d);
    }
    super::acciones::ejecutar(ctx, "ea_promover_nota", &args.to_string()).context("no se pudo promover")
}

/// Vacía la papelera de la bóveda: borra para siempre lo que hay en ella.
/// Es irreversible (art. IV.2), así que solo se hace desde aquí: terminal
/// interactiva real (un agente que lance `ea` sin TTY no puede), lo que se
/// borraría a la vista y la frase `confirmo vaciar-papelera`.
pub fn vaciar_papelera(store: &mut Store) -> Result<()> {
    let b = Boveda::configurada();
    ea_acciones::terminal_interactiva()?;
    let r = b.simular_vaciar_papelera()?;
    if r.entradas == 0 {
        println!("La papelera de la bóveda está vacía: no hay nada que borrar.");
        return Ok(());
    }
    let simulacion = format!(
        "borraría PARA SIEMPRE {} entradas de la papelera ({} notas, {} bytes) en {}/.papelera; no se pueden recuperar",
        r.entradas,
        r.notas,
        r.bytes,
        b.raiz().display()
    );
    let args = json!({});
    if !super::confirmar_en_terminal("vaciar la papelera de la bóveda", "irreversible", &args, &simulacion, "vaciar-papelera")? {
        println!("Cancelado: la papelera sigue como estaba y no se ha borrado nada.");
        return Ok(());
    }
    let hecho = b.vaciar_papelera(store, ea_ajustes::ACTOR_USUARIO)?;
    println!(
        "Papelera vaciada: {} entradas ({} notas, {} bytes) borradas para siempre.",
        hecho.entradas, hecho.notas, hecho.bytes
    );
    Ok(())
}
