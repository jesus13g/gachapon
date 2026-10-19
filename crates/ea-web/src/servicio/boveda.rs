//! La bóveda de notas: listar, buscar, leer y promover; y el editor del
//! usuario (árbol, leer entera, guardar, crear, renombrar, papelera, carpetas
//! y mostrar en el explorador).

use ea_acciones::Canal;
use serde_json::json;

use super::{sistema, Resultado, ACTOR};
use crate::vistas::{
    ArbolBoveda, CabeceraVista, EnPapelera, Hecho, ListadoBoveda, NotaFichero, NotaResumen, NotaVista, Promocion,
    RenombradoBoveda,
};
use crate::AppState;

/// Cotas de la interfaz del usuario: más holgadas que las de los agentes,
/// pero acotadas (la página no pinta cientos de notas).
const LIMITES_WEB: ea_boveda::Limites = ea_boveda::Limites { max_resultados: 50, ..ea_boveda::Limites::AMPLIOS };

/// Con `consulta`, búsqueda de texto completo; sin ella, las notas más recientes.
pub async fn listar(
    s: &AppState,
    consulta: Option<&str>,
    region: Option<&str>,
    limite: Option<usize>,
) -> Resultado<ListadoBoveda> {
    let mut store = s.store.lock().await;
    let region = region.filter(|r| !r.is_empty());
    let filas = match consulta.map(str::trim).filter(|t| !t.is_empty()) {
        // Una consulta sin términos buscables (solo signos) no es un error aquí:
        // se escribe letra a letra en el cuadro de filtro.
        Some(texto) => match s.boveda.buscar(&mut store, texto, region, limite, &LIMITES_WEB) {
            Err(ea_boveda::ErrorBoveda::Argumentos(_)) => Vec::new(),
            otro => otro?,
        },
        None => s.boveda.listar(&mut store, region, limite, &LIMITES_WEB)?,
    };
    Ok(ListadoBoveda {
        raiz: s.boveda.raiz().display().to_string(),
        notas: filas
            .into_iter()
            .map(|r| NotaResumen {
                ruta: r.ruta,
                region: r.region,
                autor: r.autor,
                titulo: r.titulo,
                actualizada: r.actualizada,
                promocion: r.promocion,
                fragmento: r.fragmento,
            })
            .collect(),
    })
}

pub fn leer(s: &AppState, ruta: &str, desde: usize) -> Resultado<NotaVista> {
    let n = s.boveda.leer(ruta, desde, &LIMITES_WEB)?;
    Ok(NotaVista {
        ruta: n.ruta,
        region: n.region,
        cabecera: n.cabecera.map(|c| CabeceraVista {
            titulo: c.titulo,
            autor: c.autor,
            creada: c.creada,
            actualizada: c.actualizada,
            origen: c.origen,
            promocion: c.promocion,
            tags: c.tags,
            promovida_de: c.promovida_de,
            promovida_a: c.promovida_a,
        }),
        aviso_cabecera: n.aviso_cabecera,
        contenido: n.contenido,
        total_caracteres: n.total_caracteres,
        truncado: n.truncado,
        siguiente_desde: n.siguiente_desde,
    })
}

/// Promoción directa del usuario desde la página (el gesto es la
/// confirmación; la web la pide con un segundo clic). Misma herramienta, mismo
/// flujo y mismo rastro en el diario que aceptar la propuesta de un agente.
pub async fn promover(s: &AppState, ruta: &str, destino: Option<&str>) -> Resultado<Promocion> {
    let sis = sistema(s);
    let mut args = json!({ "ruta": ruta });
    if let Some(d) = destino.filter(|d| !d.trim().is_empty()) {
        args["destino"] = json!(d);
    }
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar_directo(&mut store, &sis, ACTOR, Canal::Web, "ea_promover_nota", &args).await?;
    Ok(Promocion { texto: r.texto, datos: r.datos })
}

// ------------------------------------------------- editor (gestos del usuario)
//
// Lo que el usuario hace a mano desde el editor de la bóveda. Nada de esto
// está en `ea-mcp` ni en `ea-acciones`: ningún agente llega aquí. Cada
// operación deja su evento en el diario (lo escribe `ea-boveda`).

fn cabecera_vista(c: ea_boveda::Cabecera) -> CabeceraVista {
    CabeceraVista {
        titulo: c.titulo,
        autor: c.autor,
        creada: c.creada,
        actualizada: c.actualizada,
        origen: c.origen,
        promocion: c.promocion,
        tags: c.tags,
        promovida_de: c.promovida_de,
        promovida_a: c.promovida_a,
    }
}

fn fichero_vista(s: &AppState, n: ea_boveda::NotaCompleta) -> NotaFichero {
    let raiz = std::fs::canonicalize(s.boveda.raiz()).unwrap_or_else(|_| s.boveda.raiz().to_path_buf());
    NotaFichero {
        absoluta: raiz.join(&n.ruta).display().to_string(),
        ruta: n.ruta,
        region: n.region,
        contenido: n.contenido,
        version: n.version,
        cabecera: n.cabecera.map(cabecera_vista),
        aviso_cabecera: n.aviso_cabecera,
        bytes: n.bytes,
        modificada: n.modificada,
    }
}

fn papelera_vista(p: ea_boveda::EnPapelera) -> EnPapelera {
    EnPapelera { id: p.id, ruta: p.ruta, borrada: p.borrada, bytes: p.bytes }
}

pub async fn arbol(s: &AppState) -> Resultado<ArbolBoveda> {
    let mut store = s.store.lock().await;
    let a = s.boveda.arbol(&mut store)?;
    Ok(ArbolBoveda {
        raiz: s.boveda.raiz().display().to_string(),
        regiones: ea_boveda::ruta::REGIONES.to_vec(),
        carpetas: a.carpetas,
        notas: a
            .notas
            .into_iter()
            .map(|r| NotaResumen {
                ruta: r.ruta,
                region: r.region,
                autor: r.autor,
                titulo: r.titulo,
                actualizada: r.actualizada,
                promocion: r.promocion,
                fragmento: r.fragmento,
            })
            .collect(),
        en_papelera: a.en_papelera,
    })
}

pub fn fichero(s: &AppState, ruta: &str) -> Resultado<NotaFichero> {
    Ok(fichero_vista(s, s.boveda.leer_completa(ruta)?))
}

pub async fn guardar(s: &AppState, ruta: &str, contenido: &str, version: &str) -> Resultado<NotaFichero> {
    let mut store = s.store.lock().await;
    Ok(fichero_vista(s, s.boveda.guardar_usuario(&mut store, ruta, contenido, version, ACTOR)?))
}

pub async fn crear(s: &AppState, ruta: &str, contenido: &str) -> Resultado<NotaFichero> {
    let mut store = s.store.lock().await;
    Ok(fichero_vista(s, s.boveda.crear_usuario(&mut store, ruta, contenido, ACTOR)?))
}

pub async fn renombrar(s: &AppState, ruta: &str, nuevo: &str, version: &str) -> Resultado<RenombradoBoveda> {
    let mut store = s.store.lock().await;
    let r = s.boveda.renombrar(&mut store, ruta, nuevo, version, ACTOR)?;
    Ok(RenombradoBoveda { nota: fichero_vista(s, r.nota), actualizadas: r.actualizadas, ambiguas: r.ambiguas })
}

/// Borrar es mandar a la papelera: reversible (se restaura).
pub async fn eliminar(s: &AppState, ruta: &str, version: &str) -> Resultado<EnPapelera> {
    let mut store = s.store.lock().await;
    Ok(papelera_vista(s.boveda.a_papelera(&mut store, ruta, version, ACTOR)?))
}

pub async fn carpeta(s: &AppState, ruta: &str) -> Resultado<Hecho> {
    let mut store = s.store.lock().await;
    s.boveda.crear_carpeta(&mut store, ruta, ACTOR)?;
    Ok(Hecho { ok: true })
}

pub fn papelera(s: &AppState) -> Resultado<Vec<EnPapelera>> {
    Ok(s.boveda.papelera()?.into_iter().map(papelera_vista).collect())
}

pub async fn restaurar(s: &AppState, id: &str) -> Resultado<NotaFichero> {
    let mut store = s.store.lock().await;
    Ok(fichero_vista(s, s.boveda.restaurar(&mut store, id, ACTOR)?))
}

pub async fn mostrar(s: &AppState, ruta: &str) -> Resultado<Hecho> {
    let p = s.boveda.ruta_absoluta(ruta)?;
    super::explorador::mostrar(&p).await
}
