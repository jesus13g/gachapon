//! Cambiar los ajustes como gesto del usuario (`ea ajustes establecer`, sus
//! atajos y el panel): escribir el lote en `ajustes.toml`
//! (`ea_ajustes::establecer`), apuntarlo en el diario y aplicar los efectos que
//! no son solo el fichero. Hoy uno: `escritorio.al_iniciar` escribe o quita el
//! autoarranque de `ea-escritorio` (`ea-plataforma`). Escribir el fichero
//! sigue sin tocar nada fuera de él.

use std::path::{Path, PathBuf};

use ea_ajustes::{Aplicado, Canal, ErrorAjustes, Fuentes, establecer};
use ea_store::{EventOutcome, NewEvent, Store, StoreError};
use serde_json::{Value, json};

/// Clave del ajuste de autoarranque.
pub const CLAVE_AL_INICIAR: &str = "escritorio.al_iniciar";

/// Quien cambia los ajustes desde cualquier canal: el usuario.
const ACTOR: &str = ea_ajustes::ACTOR_USUARIO;

/// Lo que deja un cambio ya escrito.
#[derive(Debug)]
pub struct Cambio {
    pub aplicados: Vec<Aplicado>,
    /// Un aviso por cada efecto que falló (el ajuste queda guardado igualmente;
    /// el efecto se reintenta al volver a guardarlo).
    pub avisos: Vec<String>,
    /// Por qué no quedó en el diario, si no quedó (mejor esfuerzo: el cambio
    /// ya está escrito).
    pub sin_diario: Option<StoreError>,
}

/// Escribe el lote por `canal`, lo apunta en el diario (`ajustes_cambiados`)
/// con la base que da `diario` y aplica sus efectos. Solo falla si no se
/// escribe; el diario y los efectos van aparte, en [`Cambio`].
pub fn cambiar(
    f: &Fuentes,
    canal: Canal,
    version: Option<&str>,
    lote: &[(String, Option<Value>)],
    diario: impl FnOnce() -> Result<Store, StoreError>,
) -> Result<Cambio, ErrorAjustes> {
    let aplicados = establecer(f, canal, version, lote)?;
    // Antes que los efectos: si algo se corta, el diario ya lo tiene.
    let sin_diario = diario().and_then(|s| apuntar(&s, canal, f, &aplicados)).err();
    let avisos = aplicar_efectos(&aplicados);
    Ok(Cambio { aplicados, avisos, sin_diario })
}

/// El evento `ajustes_cambiados` de un lote ya escrito: el canal, el fichero y
/// cada clave con su valor de antes y de después. Para quien escribe el lote
/// sin [`cambiar`] (el instalador aplica el autoarranque con su binario).
pub fn apuntar(store: &Store, canal: Canal, f: &Fuentes, aplicados: &[Aplicado]) -> Result<(), StoreError> {
    let cambios: Vec<Value> = aplicados.iter().map(|a| json!({ "clave": a.clave, "antes": a.antes, "despues": a.despues })).collect();
    let detalle = json!({ "canal": canal.nombre(), "fichero": f.fichero().display().to_string(), "cambios": cambios });
    store.append_event(NewEvent {
        actor: ACTOR.into(),
        action: "ajustes_cambiados".into(),
        outcome: Some(EventOutcome::Ok),
        detail: Some(detalle.to_string()),
        ..Default::default()
    })?;
    Ok(())
}

/// El binario `ea-escritorio`: junto al ejecutable actual (los binarios del
/// ecosistema viven juntos) y, si no, en el `PATH`.
fn binario_escritorio() -> Option<PathBuf> {
    ea_plataforma::rutas::binario_hermano("ea-escritorio").or_else(|| ea_plataforma::rutas::buscar_en_path("ea-escritorio"))
}

/// Aplica a la plataforma el nuevo valor de `escritorio.al_iniciar`.
/// `binario` solo hace falta para activarlo.
pub fn aplicar_al_iniciar(activar: bool, binario: Option<&Path>) -> Result<(), String> {
    let destino = match (activar, binario) {
        (true, None) => return Err("no se encuentra el binario ea-escritorio (compílalo e instálalo junto a ea)".into()),
        (_, Some(b)) => b,
        (false, None) => Path::new("ea-escritorio"),
    };
    ea_plataforma::escritorio::autoarranque(activar, destino).map_err(|e| format!("autoarranque de ea-escritorio: {e}"))
}

/// Los efectos de un lote ya escrito: un aviso por cada uno que falló.
fn aplicar_efectos(aplicados: &[Aplicado]) -> Vec<String> {
    let mut avisos = Vec::new();
    for a in aplicados.iter().filter(|a| a.clave == CLAVE_AL_INICIAR) {
        // Quitar la clave deja el valor por defecto: apagado.
        let activar = matches!(a.despues, Value::Bool(true));
        if let Err(e) = aplicar_al_iniciar(activar, binario_escritorio().as_deref()) {
            avisos.push(e);
        }
    }
    avisos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activar_sin_binario_falla_y_desactivar_no_lo_necesita() {
        assert!(aplicar_al_iniciar(true, None).unwrap_err().contains("ea-escritorio"));
    }

    #[test]
    fn el_cambio_queda_en_el_diario_con_el_mismo_detalle() {
        let d = tempfile::tempdir().unwrap();
        let f = Fuentes::aislada(d.path());
        let base = d.path().join("diario.db");
        let lote = [("web.sondeo_ms".to_string(), Some(json!(5000)))];
        let c = cambiar(&f, Canal::Terminal, None, &lote, || Store::open_at(&base)).unwrap();
        assert!(c.avisos.is_empty() && c.sin_diario.is_none());
        assert_eq!((c.aplicados[0].antes.clone(), c.aplicados[0].despues.clone()), (json!(3000), json!(5000)));
        let store = Store::open_at(&base).unwrap();
        let ev = store.list_events(&ea_store::EventFilter::default()).unwrap().into_iter().find(|e| e.action == "ajustes_cambiados").unwrap();
        assert_eq!(ev.actor, "usuario");
        let detalle = format!(
            r#"{{"canal":"terminal","fichero":{},"cambios":[{{"clave":"web.sondeo_ms","antes":3000,"despues":5000}}]}}"#,
            json!(f.fichero().display().to_string())
        );
        assert_eq!(ev.detail.as_deref(), Some(detalle.as_str()));
        // Sin diario, el cambio queda escrito igual y se dice por qué.
        let lote = [("web.sondeo_ms".to_string(), Some(json!(6000)))];
        let c = cambiar(&f, Canal::Terminal, None, &lote, || Err(StoreError::EsquemaAntiguo)).unwrap();
        assert!(c.sin_diario.is_some());
        assert_eq!(c.aplicados[0].despues, json!(6000));
    }
}
