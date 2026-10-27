//! Notas del usuario en el diario (`usuario_nota`): dejar constancia de algo
//! o corregir un evento. El diario es append-only, así que corregir es añadir
//! una nota que apunta al evento corregido (`detail.corrige`); nunca se toca
//! la fila original.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use crate::error::{StoreError, StoreResult};
use crate::models::{Event, NewEvent};
use crate::repository::events;

/// La acción de las notas del usuario (familia `usuario_`).
pub const ACCION_NOTA: &str = "usuario_nota";
/// Actor de las notas: solo las escribe el usuario (CLI o ea-web con clave).
pub const ACTOR_NOTA: &str = "usuario";
/// Tope del texto, en caracteres.
pub const MAX_TEXTO_NOTA: usize = 10_000;

/// Una nota por escribir, tal como la pide el usuario.
#[derive(Debug, Clone, Default)]
pub struct NuevaNota {
    pub texto: String,
    /// Id del evento que esta nota corrige.
    pub corrige: Option<i64>,
    /// Trabajo del bus al que se asocia (va también en la columna `trabajo`).
    pub trabajo: Option<String>,
}

/// Valida la nota y la añade al diario. Si algo no cuadra no escribe nada.
pub fn anotar(conn: &Connection, nota: NuevaNota) -> StoreResult<Event> {
    let texto = nota.texto.trim();
    if texto.is_empty() {
        return Err(StoreError::NotaInvalida("el texto de la nota está vacío".into()));
    }
    let n = texto.chars().count();
    if n > MAX_TEXTO_NOTA {
        return Err(StoreError::NotaInvalida(format!(
            "el texto de la nota tiene {n} caracteres; el máximo es {MAX_TEXTO_NOTA}"
        )));
    }
    let trabajo = nota.trabajo.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if let Some(t) = &trabajo
        && (t.len() > 128 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        return Err(StoreError::NotaInvalida(format!(
            "id de trabajo inválido: «{t}» (solo letras, cifras y `-`)"
        )));
    }
    if let Some(id) = nota.corrige {
        match events::get_event(conn, id) {
            Ok(_) => {}
            Err(StoreError::EventNotFound(_)) => {
                return Err(StoreError::NotaInvalida(format!(
                    "el evento #{id} que se quiere corregir no existe"
                )));
            }
            Err(e) => return Err(e),
        }
    }

    let mut detalle = serde_json::Map::new();
    detalle.insert("texto".into(), texto.into());
    if let Some(id) = nota.corrige {
        detalle.insert("corrige".into(), id.into());
    }
    if let Some(t) = &trabajo {
        detalle.insert("trabajo".into(), t.clone().into());
    }
    events::append_event(
        conn,
        NewEvent {
            actor: ACTOR_NOTA.into(),
            action: ACCION_NOTA.into(),
            trabajo,
            detail: Some(serde_json::Value::Object(detalle).to_string()),
            ..Default::default()
        },
    )
}

/// Para cada id de `ids`, las notas que lo corrigen (ids ascendentes). Los
/// ids sin correcciones no aparecen.
pub fn correcciones(conn: &Connection, ids: &[i64]) -> StoreResult<BTreeMap<i64, Vec<i64>>> {
    let mut mapa = BTreeMap::new();
    if ids.is_empty() {
        return Ok(mapa);
    }
    let buscados: BTreeSet<i64> = ids.iter().copied().collect();
    let mut stmt = conn.prepare(
        "SELECT id, json_extract(detail, '$.corrige') FROM events
         WHERE action = ?1 AND json_valid(detail)
           AND json_type(detail, '$.corrige') = 'integer'
         ORDER BY id",
    )?;
    let filas = stmt.query_map(params![ACCION_NOTA], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for fila in filas {
        let (nota, corregido) = fila?;
        if buscados.contains(&corregido) {
            mapa.entry(corregido).or_insert_with(Vec::new).push(nota);
        }
    }
    Ok(mapa)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn store() -> Store {
        Store::open_at(std::path::Path::new(":memory:")).unwrap()
    }

    fn evento(s: &Store) -> Event {
        s.append_event(NewEvent {
            actor: "orquestador".into(),
            action: "bus_cerrar".into(),
            trabajo: None,
            outcome: None,
            duration_ms: None,
            detail: Some("{\"x\":1}".into()),
        })
        .unwrap()
    }

    #[test]
    fn una_correccion_apunta_al_evento_y_no_lo_toca() {
        let s = store();
        let original = evento(&s);
        let nota = s
            .anotar_diario(NuevaNota { texto: "  era otro  ".into(), corrige: Some(original.id), trabajo: Some("t-1".into()) })
            .unwrap();
        assert_eq!(nota.actor, "usuario");
        assert_eq!(nota.action, ACCION_NOTA);
        assert_eq!(nota.trabajo.as_deref(), Some("t-1"));
        let d: serde_json::Value = serde_json::from_str(nota.detail.as_deref().unwrap()).unwrap();
        assert_eq!(d["texto"], "era otro");
        assert_eq!(d["corrige"], original.id);
        assert_eq!(d["trabajo"], "t-1");
        assert_eq!(s.get_event(original.id).unwrap().detail, original.detail);
        let c = s.correcciones_diario(&[original.id, nota.id]).unwrap();
        assert_eq!(c.get(&original.id), Some(&vec![nota.id]));
        assert!(!c.contains_key(&nota.id));
    }

    #[test]
    fn rechaza_sin_escribir_lo_invalido() {
        let s = store();
        let malas = [
            NuevaNota { texto: "   ".into(), ..Default::default() },
            NuevaNota { texto: "x".repeat(MAX_TEXTO_NOTA + 1), ..Default::default() },
            NuevaNota { texto: "x".into(), corrige: Some(999), ..Default::default() },
            NuevaNota { texto: "x".into(), trabajo: Some("../x".into()), ..Default::default() },
        ];
        for m in malas {
            assert!(matches!(s.anotar_diario(m), Err(StoreError::NotaInvalida(_))));
        }
        assert!(s.list_events(&Default::default()).unwrap().is_empty());
        // Justo en el tope (en caracteres, no en bytes) sí vale.
        s.anotar_diario(NuevaNota { texto: "ñ".repeat(MAX_TEXTO_NOTA), ..Default::default() }).unwrap();
    }
}
