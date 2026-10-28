//! `ea biblioteca`: el inventario de herramientas de los agentes (spec 018).
//! Listar, ver y el uso solo leen. Activar, desactivar y abrir una sesión
//! crean una propuesta por el flujo de `ea-acciones` (el usuario las acepta
//! con `ea propuestas decidir`, o aquí mismo con `--esperar` y una terminal
//! interactiva). `categoria` escribe `biblioteca.categorias` por la vía
//! normal de los ajustes, con su regla de edición.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use comfy_table::Table;
use ea_acciones::{aceptar_propuesta, observar, proponer};
use ea_ajustes::Fuentes;
use ea_biblioteca::uso::{calcular_uso, FuentesUso, InformeUso, Uso};
use ea_biblioteca::{Activado, Biblioteca, Elemento, Filtro, Inventario, Mecanismo, Raices, Tipo};
use ea_store::Store;
use serde_json::{json, Value};

use super::{bloquear, confirmar_en_terminal, limpio, sistema};

const ACTOR: &str = ea_ajustes::ACTOR_USUARIO;
const CLAVE_CATEGORIAS: &str = "biblioteca.categorias";

fn cargar() -> (Raices, Inventario) {
    let raices = Raices::desde_entorno();
    let inventario = Biblioteca::cargar(&raices, &ea_ajustes::cargar());
    (raices, inventario)
}

fn uso_de(raices: &Raices, inv: &Inventario) -> InformeUso {
    calcular_uso(raices, inv, &FuentesUso::desde_raices(raices))
}

fn texto_mecanismo(m: Mecanismo) -> &'static str {
    match m {
        Mecanismo::ComandoOficial => "comando oficial",
        Mecanismo::Campo => "campo enabled",
        Mecanismo::Mover => "mover fichero",
        Mecanismo::Ajustes => "ajustes.toml",
        Mecanismo::SoloSesion => "solo en sesión",
        Mecanismo::No => "no se puede",
    }
}

fn texto_estado(a: Activado) -> &'static str {
    match a {
        Activado::Si => "sí",
        Activado::No => "no",
        Activado::Desconocido => "?",
    }
}

fn elemento_json(e: &Elemento, uso: Option<&Uso>) -> Value {
    let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
    v["uso"] = uso.map_or(json!({ "total": 0, "ultimo": null, "por": [] }), |u| json!(u));
    v
}

#[allow(clippy::too_many_arguments)]
pub fn listar(
    agente: Option<String>,
    tipo: Option<String>,
    categoria: Option<String>,
    activados: bool,
    desactivados: bool,
    ids: bool,
    json: bool,
) -> Result<()> {
    let tipo = match tipo {
        Some(t) => Some(Tipo::desde(&t).with_context(|| {
            format!("tipo desconocido {t:?} (uno de: {})", Tipo::TODOS.map(|t| t.as_str()).join(", "))
        })?),
        None => None,
    };
    let activado = match (activados, desactivados) {
        (true, false) => Some(Activado::Si),
        (false, true) => Some(Activado::No),
        _ => None,
    };
    let filtro = Filtro { agente, tipo, categoria, activado };
    let (raices, inv) = cargar();
    let uso = uso_de(&raices, &inv);
    let elementos = inv.filtrar(&filtro);

    if json {
        let v = json!({
            "elementos": elementos.iter().map(|e| elemento_json(e, uso.buscar(&e.id))).collect::<Vec<_>>(),
            "avisos": inv.avisos,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if elementos.is_empty() {
        println!("No hay elementos que coincidan con el filtro.");
    }
    let mut grupos: BTreeMap<&str, BTreeMap<Tipo, Vec<&Elemento>>> = BTreeMap::new();
    for e in &elementos {
        grupos.entry(e.categoria.as_str()).or_default().entry(e.tipo).or_default().push(e);
    }
    for (categoria, por_tipo) in &grupos {
        println!("\n== {categoria}");
        for (tipo, lista) in por_tipo {
            let mut t = Table::new();
            t.set_header(vec![if ids { "Id" } else { "Nombre" }, "Agente", "Ámbito", "Activado", "Cómo se cambia", "Usos"]);
            for e in lista {
                t.add_row(vec![
                    limpio(if ids { &e.id } else { &e.nombre }),
                    e.agente.clone(),
                    e.ambito.texto(),
                    texto_estado(e.activado).to_string(),
                    texto_mecanismo(e.mecanismo).to_string(),
                    uso.buscar(&e.id).map_or("0".to_string(), |u| u.total.to_string()),
                ]);
            }
            println!("{tipo} ({})\n{t}", lista.len());
        }
    }
    if !inv.avisos.is_empty() {
        println!("\nAvisos del inventario:");
        for a in &inv.avisos {
            println!("  ! {}", limpio(a));
        }
    }
    Ok(())
}

pub fn ver(id: &str, json: bool) -> Result<()> {
    let (raices, inv) = cargar();
    let e = inv.buscar(id).with_context(|| format!("no existe el elemento {id:?} (mira `ea biblioteca listar --ids`)"))?;
    let informe = uso_de(&raices, &inv);
    let uso = informe.buscar(id);
    if json {
        println!("{}", serde_json::to_string_pretty(&elemento_json(e, uso))?);
        return Ok(());
    }
    println!("{}", e.id);
    println!("  Nombre:       {} ({})", e.nombre, e.tipo);
    println!("  Agente:       {}", e.agente);
    println!("  Ámbito:       {}", e.ambito);
    println!("  Categoría:    {}", e.categoria);
    println!("  Origen:       {}", limpio(&e.ruta.display().to_string()));
    if !e.descripcion.is_empty() {
        println!("  Descripción:  {}", limpio(&e.descripcion));
    }
    println!("  Activado:     {}", texto_estado(e.activado));
    println!("  Cómo se cambia: {}", texto_mecanismo(e.mecanismo));
    if let Some(n) = &e.nota {
        println!("  Nota:         {}", limpio(n));
    }
    if !e.version_formato.is_empty() {
        println!("  Formato:      versión {}", e.version_formato);
    }
    if let Some(d) = &e.def_mcp {
        println!("  Servidor MCP: {}", d.transporte);
        if let Some(c) = &d.comando {
            println!("    comando:    {} {}", limpio(c), limpio(&d.args.join(" ")));
        }
        if let Some(u) = &d.url {
            println!("    url:        {}", limpio(u));
        }
        if !d.env.is_empty() {
            println!("    variables:  {} (solo los nombres)", d.env.join(", "));
        }
        if !d.headers.is_empty() {
            println!("    cabeceras:  {} (solo los nombres)", d.headers.join(", "));
        }
    }
    match uso {
        Some(u) => {
            println!("  Uso:          {} en total, último {}", u.total, u.ultimo.as_deref().unwrap_or("sin fecha"));
            for a in &u.por {
                println!("    {:<14} {:>5}  {}", a.actor, a.usos, a.ultimo.as_deref().unwrap_or(""));
            }
        }
        None => println!("  Uso:          sin uso registrado"),
    }
    Ok(())
}

pub fn uso(id: Option<String>, json: bool) -> Result<()> {
    let (raices, inv) = cargar();
    let informe = uso_de(&raices, &inv);
    if let Some(id) = id {
        inv.buscar(&id).with_context(|| format!("no existe el elemento {id:?}"))?;
        let u = informe.buscar(&id);
        if json {
            let v = u.map_or(json!({ "id": id, "total": 0, "ultimo": null, "por": [] }), |u| json!(u));
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            match u {
                None => println!("{id}: sin uso registrado."),
                Some(u) => {
                    println!("{id}: {} usos, último {}", u.total, u.ultimo.as_deref().unwrap_or("sin fecha"));
                    for a in &u.por {
                        println!("  {:<14} {:>5}  {}", a.actor, a.usos, a.ultimo.as_deref().unwrap_or(""));
                    }
                }
            }
        }
        return Ok(());
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&informe)?);
        return Ok(());
    }
    let por_actor = |p: &[ea_biblioteca::uso::UsoActor]| p.iter().map(|a| format!("{} {}", a.actor, a.usos)).collect::<Vec<_>>().join(", ");
    if informe.usos.is_empty() {
        println!("Ningún elemento del inventario tiene uso registrado.");
    } else {
        let mut t = Table::new();
        t.set_header(vec!["Id", "Usos", "Último", "Por actor"]);
        let mut usos: Vec<&Uso> = informe.usos.iter().collect();
        usos.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.id.cmp(&b.id)));
        for u in usos {
            t.add_row(vec![u.id.clone(), u.total.to_string(), u.ultimo.clone().unwrap_or_default(), por_actor(&u.por)]);
        }
        println!("{t}");
    }
    if !informe.sin_elemento.is_empty() {
        let mut t = Table::new();
        t.set_header(vec!["Agente", "Tipo", "Nombre", "Usos", "Último", "Por actor"]);
        for s in &informe.sin_elemento {
            t.add_row(vec![
                s.agente.clone(),
                s.tipo.to_string(),
                limpio(&s.nombre),
                s.total.to_string(),
                s.ultimo.clone().unwrap_or_default(),
                por_actor(&s.por),
            ]);
        }
        println!("\nSin elemento en el inventario (integrados, de claude.ai…)\n{t}");
    }
    if !informe.actividad.is_empty() {
        let mut t = Table::new();
        t.set_header(vec!["Agente", "Última actividad", "Fuente"]);
        for a in &informe.actividad {
            t.add_row(vec![a.agente.clone(), a.ultima.clone().unwrap_or_else(|| "sin datos".into()), a.fuente.clone()]);
        }
        println!("\nActividad por agente\n{t}");
    }
    if !informe.avisos.is_empty() {
        println!("\nAvisos:");
        for a in &informe.avisos {
            println!("  ! {}", limpio(a));
        }
    }
    Ok(())
}

/// Crea la propuesta como el usuario y enseña la simulación. Con `esperar` y
/// una terminal interactiva real, pide confirmación y la acepta y ejecuta en
/// el acto (como `ea propuestas decidir --aceptar`); sin terminal, solo
/// propone: un agente que lance `ea` no puede confirmar nada.
fn proponer_y_quiza_aceptar(store: &mut Store, herramienta: &str, args: &Value, esperar: bool) -> Result<()> {
    let sis = sistema();
    let hechos = bloquear(observar(&sis, herramienta, args))?;
    let (p, nueva) = proponer(store, &sis, &hechos, ACTOR, None, herramienta, args)?;
    let accion = p.accion.as_ref().context("la propuesta no tiene acción")?;
    println!(
        "Propuesta #{} {} ({}, riesgo {}):",
        p.id,
        if nueva { "creada" } else { "ya estaba pendiente" },
        accion.herramienta,
        accion.riesgo
    );
    println!("  {}", accion.simulacion);
    if !esperar {
        println!("No se ha hecho nada. Para aplicarla: `ea propuestas decidir {} --aceptar`.", p.id);
        return Ok(());
    }
    let Ok(canal) = ea_acciones::terminal_interactiva() else {
        println!("Sin terminal interactiva no se acepta: la propuesta #{} queda pendiente.", p.id);
        return Ok(());
    };
    if !confirmar_en_terminal(&accion.herramienta, &accion.riesgo, &accion.argumentos, &accion.simulacion, &p.id.to_string())? {
        println!("Cancelado: la propuesta #{} sigue pendiente y no se ha hecho nada.", p.id);
        return Ok(());
    }
    let p = bloquear(aceptar_propuesta(store, &sis, p.id, ACTOR, canal))?;
    println!("Propuesta #{} marcada como '{}'", p.id, p.estado.as_str());
    if let Some(e) = &p.ejecucion {
        println!("  Ejecución: {} — {}", e.estado.as_str(), e.detalle.clone().unwrap_or_default());
    }
    Ok(())
}

pub fn cambiar(store: &mut Store, id: &str, activar: bool, esperar: bool) -> Result<()> {
    let herramienta = if activar { "ea_activar_herramienta" } else { "ea_desactivar_herramienta" };
    proponer_y_quiza_aceptar(store, herramienta, &json!({ "elemento": id }), esperar)
}

pub fn sesion(store: &mut Store, agente: &str, ids: &[String], directorio: Option<String>, esperar: bool) -> Result<()> {
    // Una ruta relativa se entiende desde donde se lanza `ea`.
    let directorio = match directorio {
        Some(d) => Some(
            ea_plataforma::rutas::canonica(&d)
                .with_context(|| format!("el directorio {d:?} no existe"))?
                .display()
                .to_string(),
        ),
        None => None,
    };
    // El plan (sin secretos) se enseña antes de proponer, y un plan imposible
    // no deja propuesta.
    let plan = ea_acciones::sesion::plan_real(agente, ids, directorio.as_deref()).map_err(anyhow::Error::msg)?;
    println!("Sesión de {} con {} herramienta(s): {}", plan.agente, plan.herramientas.len(), plan.herramientas.join(", "));
    println!("  Comando: {}", plan.comando());
    println!("  Directorio efímero: {}", plan.directorio.display());
    for f in &plan.ficheros {
        println!("  Fichero {:o}: {}", f.modo, limpio(&f.ruta.display().to_string()));
    }
    for e in &plan.enlaces {
        println!("  Enlace: {} -> {}", e.ruta.display(), e.destino.display());
    }
    let mut args = json!({ "agente": agente, "herramientas": ids });
    if let Some(d) = directorio {
        args["directorio"] = Value::String(d);
    }
    proponer_y_quiza_aceptar(store, "ea_lanzar_sesion", &args, esperar)
}

/// Atajo de `ea ajustes establecer biblioteca.categorias=…` para un solo
/// elemento: lee el mapa, cambia o quita la entrada y lo escribe por la vía
/// de los ajustes (que exige terminal interactiva y valida la clave).
pub fn categoria(id: &str, categoria: Option<&str>, quitar: bool) -> Result<()> {
    ea_acciones::terminal_interactiva()?;
    let (_, inv) = cargar();
    if !quitar && inv.buscar(id).is_none() {
        bail!("no existe el elemento {id:?} (mira `ea biblioteca listar --ids`)");
    }
    let f = Fuentes::sistema();
    let mut mapa: BTreeMap<String, String> = ea_ajustes::cargar_de(&f).ajustes.biblioteca.categorias;
    match (quitar, categoria) {
        (true, None) => {
            if mapa.remove(id).is_none() {
                println!("{id} no tiene categoría propia: no hay nada que quitar.");
                return Ok(());
            }
        }
        (false, Some(c)) => {
            let c = c.trim();
            if c.is_empty() || c.chars().any(char::is_control) {
                bail!("la categoría no puede estar vacía ni llevar caracteres de control");
            }
            mapa.insert(id.to_string(), c.to_string());
        }
        _ => bail!("indica una categoría o --quitar"),
    }
    let valor = if mapa.is_empty() { None } else { Some(serde_json::to_value(&mapa)?) };
    super::ajustes::cambiar(&f, &[(CLAVE_CATEGORIAS.to_string(), valor)])?;
    match categoria {
        Some(c) => println!("{id}: categoría «{c}»."),
        None => println!("{id}: vuelve a su categoría por defecto (el origen)."),
    }
    Ok(())
}
