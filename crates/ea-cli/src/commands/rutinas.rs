use std::path::PathBuf;

use anyhow::Result;
use comfy_table::Table;
use ea_store::Store;


fn rutinas_dir() -> PathBuf {
    ea_ajustes::cargar().orquestador.rutinas_dir
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
fn proxima_ejecucion(slug: &str) -> Option<String> {
    super::bloquear(ea_acciones::Temporizadores::usuario().proxima_legible(slug))
}

/// Las rutinas registradas cruzadas con su fichero en `rutinas_dir`
/// ([`ea_bus::ubicacion_rutina`]): las activas o, con `retiradas`, las que
/// están en la papelera o sin fichero.
pub fn filtrar_por_ubicacion(
    filas: Vec<ea_store::FilaRutina>,
    dir: &std::path::Path,
    retiradas: bool,
) -> Vec<(ea_store::FilaRutina, ea_bus::Ubicacion)> {
    filas
        .into_iter()
        .filter_map(|f| {
            let u = ea_bus::ubicacion_rutina(dir, &f.slug).unwrap_or(ea_bus::Ubicacion::SinFichero);
            ((u == ea_bus::Ubicacion::Activa) != retiradas).then_some((f, u))
        })
        .collect()
}

pub fn listar(store: &Store, retiradas: bool) -> Result<()> {
    let rutinas = filtrar_por_ubicacion(store.listar_rutinas()?, &rutinas_dir(), retiradas);
    if rutinas.is_empty() {
        if retiradas {
            println!("No hay rutinas retiradas.");
        } else {
            println!("No hay rutinas en uso (ea rutina registrar <fichero.toml>; las retiradas: ea rutina listar --retiradas).");
        }
        return Ok(());
    }
    let mut table = Table::new();
    if retiradas {
        table.set_header(vec!["Rutina", "Versión", "Dónde", "Última ejecución"]);
        for (r, u) in rutinas {
            let donde = match u {
                ea_bus::Ubicacion::Retirada => "papelera (ea rutina restaurar)",
                _ => "sin fichero (registra una versión nueva)",
            };
            let ultima = ultimo_trabajo(&r.slug)
                .map(|(id, e)| format!("{} ({}, {id})", e.actualizado_en, e.estado.as_str()))
                .unwrap_or_else(|| "nunca".to_string());
            table.add_row(vec![r.slug, r.version.to_string(), donde.to_string(), ultima]);
        }
        println!("{table}");
        return Ok(());
    }
    let proxima_cabecera = format!("Próxima ({})", programador());
    table.set_header(vec!["Rutina", "Versión", "Última ejecución", proxima_cabecera.as_str()]);
    for (r, _) in rutinas {
        let ultima = ultimo_trabajo(&r.slug)
            .map(|(id, e)| format!("{} ({}, {id})", e.actualizado_en, e.estado.as_str()))
            .unwrap_or_else(|| "nunca".to_string());
        let proxima = proxima_ejecucion(&r.slug).unwrap_or_else(|| "—".to_string());
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

pub fn ver(store: &Store, slug: &str) -> Result<()> {
    let fila = store
        .ultima_rutina(slug)?
        .ok_or_else(|| anyhow::anyhow!("la rutina '{slug}' no está registrada (ea rutina registrar <fichero>)"))?;
    println!("Rutina '{slug}', versión {} (registrada {} por {}):\n{}", fila.version, fila.creada_en, fila.creada_por, fila.definicion);
    if let Some(proxima) = proxima_ejecucion(slug) {
        println!("  {:<24}{proxima}", format!("Próxima ({}):", programador()));
    }

    let historial = super::trabajo::bus()?.trabajos_de_rutina(slug).unwrap_or_default();
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

/// Trabajo más reciente de una rutina en el bus (cualquiera que sea su estado).
pub fn ultimo_trabajo(slug: &str) -> Option<(String, ea_bus::Estado)> {
    super::trabajo::bus().ok()?.trabajos_de_rutina(slug).ok()?.into_iter().next()
}

pub fn registrar(fichero: &std::path::Path) -> Result<()> {
    let texto = std::fs::read_to_string(fichero)
        .map_err(|e| anyhow::anyhow!("no se pudo leer {}: {e}", fichero.display()))?;
    let bus = super::trabajo::bus()?;
    let r = bus.registrar_rutina(&texto, &rutinas_dir())?;
    println!("rutina '{}' registrada en la versión {}", r.rutina.slug, r.rutina.version);
    Ok(())
}

/// Lo que invoca el `.service` plantilla de un disparador temporal: crea el
/// trabajo en el bus y vuelve; el orquestador lo ejecuta cuando puede.
pub fn lanzar(slug: &str, entradas: &[String]) -> Result<()> {
    let pares = entradas
        .iter()
        .map(|e| {
            e.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| anyhow::anyhow!("entrada inválida {e:?} (clave=valor)"))
        })
        .collect::<Result<Vec<_>>>()?;
    // Una rutina retirada o sin fichero no se lanza, tampoco desde su timer.
    let id = super::trabajo::bus()?.lanzar_rutina_en(&rutinas_dir(), slug, &pares)?;
    println!("{id}");
    Ok(())
}

/// `ea rutina retirar <slug>`: `ea_retirar_rutina` con su simulación y
/// confirmación en una terminal interactiva, como `ea acciones ejecutar`.
pub fn retirar(store: &mut Store, slug: &str) -> Result<()> {
    super::acciones::ejecutar(store, "ea_retirar_rutina", &serde_json::json!({ "slug": slug }).to_string())
}

/// `ea rutina restaurar <slug>`: `ea_restaurar_rutina`, igual.
pub fn restaurar(store: &mut Store, slug: &str) -> Result<()> {
    super::acciones::ejecutar(store, "ea_restaurar_rutina", &serde_json::json!({ "slug": slug }).to_string())
}
