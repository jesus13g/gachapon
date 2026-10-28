use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use comfy_table::Table;
use ea_acciones::Temporizadores;
use ea_store::Store;

use super::Contexto;

fn rutinas_dir(ctx: &mut Contexto) -> PathBuf {
    ctx.ajustes().orquestador.rutinas_dir.clone()
}

/// Cómo se llama el programador de este sistema, para las cabeceras.
fn programador() -> &'static str {
    match ea_acciones::Backend::de(ea_plataforma::Sistema::actual()) {
        ea_acciones::Backend::Systemd => "systemd",
        ea_acciones::Backend::Launchd => "launchd",
        ea_acciones::Backend::Schtasks => "Programador de tareas",
    }
}

/// Próxima ejecución programada, si la rutina tiene timer. Linux: la línea de
/// `systemctl --user list-timers`; macOS y Windows: la calculada a partir del
/// calendario programado. Mejor esfuerzo: la CLI no habla con ningún agente
/// (§12.2), pero preguntarle al gestor de servicios del sistema por su propio
/// estado no lo es — y si no está disponible o el timer no está instalado,
/// se omite sin error (§12.4: degradación limpia).
fn proxima_ejecucion(t: &Temporizadores, slug: &str) -> Option<String> {
    super::bloquear(t.proxima_legible(slug))
}

pub fn listar(ctx: &mut Contexto, retiradas: bool) -> Result<()> {
    let (dir, t) = (rutinas_dir(ctx), ctx.temporizadores().clone());
    let rutinas = ea_bus::rutinas_por_ubicacion(ctx.store()?.listar_rutinas()?, &dir, retiradas);
    if rutinas.is_empty() {
        if retiradas {
            println!("No hay rutinas retiradas.");
        } else {
            println!("No hay rutinas en uso (ea rutina registrar <fichero.toml>; las retiradas: ea rutina listar --retiradas).");
        }
        return Ok(());
    }
    let mut ultimas = ultimas_ejecuciones(&rutinas);
    let mut table = Table::new();
    if retiradas {
        table.set_header(vec!["Rutina", "Versión", "Dónde", "Última ejecución"]);
        for (r, u) in rutinas {
            let donde = match u {
                ea_bus::Ubicacion::Retirada => "papelera (ea rutina restaurar)",
                _ => "sin fichero (registra una versión nueva)",
            };
            let ultima =
                ultimas.remove(&r.slug).map(|(id, e)| format!("{} ({}, {id})", e.actualizado_en, e.estado.as_str())).unwrap_or_else(|| "nunca".to_string());
            table.add_row(vec![r.slug, r.version.to_string(), donde.to_string(), ultima]);
        }
        println!("{table}");
        return Ok(());
    }
    let proxima_cabecera = format!("Próxima ({})", programador());
    table.set_header(vec!["Rutina", "Versión", "Última ejecución", proxima_cabecera.as_str()]);
    for (r, _) in rutinas {
        let ultima =
            ultimas.remove(&r.slug).map(|(id, e)| format!("{} ({}, {id})", e.actualizado_en, e.estado.as_str())).unwrap_or_else(|| "nunca".to_string());
        let proxima = proxima_ejecucion(&t, &r.slug).unwrap_or_else(|| "—".to_string());
        table.add_row(vec![r.slug, r.version.to_string(), ultima, proxima]);
    }
    println!("{table}");
    Ok(())
}

/// `ea rutina ver <slug> --version N`: una versión registrada concreta y la
/// lista de todas. Solo lectura.
pub fn ver_version(store: &Store, slug: &str, n: i64) -> Result<()> {
    let versiones = store.listar_versiones_rutina(slug)?;
    if versiones.is_empty() {
        anyhow::bail!("la rutina '{slug}' no está registrada (ea rutina registrar <fichero>)");
    }
    let fila = store.obtener_rutina(slug, n)?.ok_or_else(|| {
        let hay: Vec<String> = versiones.iter().map(|v| format!("v{}", v.version)).collect();
        anyhow::anyhow!("la rutina '{slug}' no tiene versión {n} (registradas: {})", hay.join(", "))
    })?;
    println!("Rutina '{slug}', versión {} (registrada {} por {}):\n{}", fila.version, fila.creada_en, fila.creada_por, fila.definicion);
    println!("Versiones registradas:");
    for v in versiones {
        println!("  v{:<4} {}  {}", v.version, v.creada_en, v.creada_por);
    }
    Ok(())
}

pub fn ver(ctx: &mut Contexto, slug: &str) -> Result<()> {
    let t = ctx.temporizadores().clone();
    let fila = ctx.store()?.ultima_rutina(slug)?.ok_or_else(|| anyhow::anyhow!("la rutina '{slug}' no está registrada (ea rutina registrar <fichero>)"))?;
    println!("Rutina '{slug}', versión {} (registrada {} por {}):\n{}", fila.version, fila.creada_en, fila.creada_por, fila.definicion);
    if let Some(proxima) = proxima_ejecucion(&t, slug) {
        println!("  {:<24}{proxima}", format!("Próxima ({}):", programador()));
    }

    let historial = ea_bus::Bus::abrir()?.trabajos_de_rutina(slug).unwrap_or_default();
    if historial.is_empty() {
        println!("\nSin ejecuciones todavía.");
        return Ok(());
    }

    println!("\nÚltimos trabajos de la rutina:");
    let mut table = Table::new();
    table.set_header(vec!["Trabajo", "Estado", "Actualizado", "Detalle"]);
    for (id, e) in historial.into_iter().take(20) {
        table.add_row(vec![id, e.estado.as_str().to_string(), e.actualizado_en, e.detalle.unwrap_or_default()]);
    }
    println!("{table}");
    Ok(())
}

/// El trabajo más reciente de cada una de `rutinas` en el bus (cualquiera
/// que sea su estado), en una sola pasada; sin bus, ninguno.
pub fn ultimas_ejecuciones(rutinas: &[(ea_store::FilaRutina, ea_bus::Ubicacion)]) -> HashMap<String, (String, ea_bus::Estado)> {
    let slugs = rutinas.iter().map(|(r, _)| r.slug.clone()).collect();
    ea_bus::Bus::abrir().ok().and_then(|b| b.ultimas_ejecuciones(&slugs).ok()).unwrap_or_default()
}

/// `ea rutina registrar <fichero>` (U-05). Primero lo que comprueba todo
/// registro ([`ea_bus::comprobar_registro`]). Una rutina nueva (su v1) se
/// registra directamente; una versión nueva de una que ya existe es
/// `ea_registrar_rutina`, con su simulación (los cambios frente a la
/// registrada) y la confirmación en una terminal interactiva, como `ea
/// acciones ejecutar`. Sin terminal no se registra: el planificador la
/// propone por MCP y la aceptas tú. Por cualquier camino queda
/// `rutina_registrada` en el diario.
pub fn registrar(ctx: &mut Contexto, fichero: &std::path::Path) -> Result<()> {
    let texto = std::fs::read_to_string(fichero).map_err(|e| anyhow::anyhow!("no se pudo leer {}: {e}", fichero.display()))?;
    let (r, anterior) = ea_bus::comprobar_registro(ctx.store()?, &texto)?;
    if let Some(ult) = anterior {
        if let Err(e) = ea_acciones::terminal_interactiva() {
            anyhow::bail!(
                "{e}: la rutina '{}' ya está registrada (versión {}) y la {} es una versión nueva. Regístrala desde una \
                 terminal (ea rutina registrar, que la simula y te pide confirmar) o que el planificador la proponga \
                 con ea_registrar_rutina por MCP y la aceptes con ea propuestas decidir",
                r.rutina.slug,
                ult.version,
                r.rutina.version
            );
        }
        return super::acciones::ejecutar(ctx, "ea_registrar_rutina", &serde_json::json!({ "toml": texto }).to_string());
    }
    let dir = rutinas_dir(ctx);
    let r = ctx.bus()?.registrar_rutina(&texto, &dir)?;
    println!("rutina '{}' registrada en la versión {}", r.rutina.slug, r.rutina.version);
    Ok(())
}

/// Lo que invoca el `.service` plantilla de un disparador temporal: crea el
/// trabajo en el bus y vuelve; el orquestador lo ejecuta cuando puede.
pub fn lanzar(ctx: &mut Contexto, slug: &str, entradas: &[String]) -> Result<()> {
    let pares = entradas
        .iter()
        .map(|e| e.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).ok_or_else(|| anyhow::anyhow!("entrada inválida {e:?} (clave=valor)")))
        .collect::<Result<Vec<_>>>()?;
    // Una rutina retirada o sin fichero no se lanza, tampoco desde su timer.
    let dir = rutinas_dir(ctx);
    let id = ctx.bus()?.lanzar_rutina_en(&dir, slug, &pares)?;
    println!("{id}");
    Ok(())
}

/// `ea rutina retirar <slug>`: `ea_retirar_rutina` con su simulación y
/// confirmación en una terminal interactiva, como `ea acciones ejecutar`.
pub fn retirar(ctx: &mut Contexto, slug: &str) -> Result<()> {
    super::acciones::ejecutar(ctx, "ea_retirar_rutina", &serde_json::json!({ "slug": slug }).to_string())
}

/// `ea rutina restaurar <slug>`: `ea_restaurar_rutina`, igual.
pub fn restaurar(ctx: &mut Contexto, slug: &str) -> Result<()> {
    super::acciones::ejecutar(ctx, "ea_restaurar_rutina", &serde_json::json!({ "slug": slug }).to_string())
}
