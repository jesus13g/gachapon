//! Las instantáneas de árbol que la obra guarda al cerrar cada fase (el
//! `arbol` del informe de la fase en `trabajo_fases`) y el rango de `git
//! diff` que sale de ellas. Lógica pura, común a `ea trabajo diff --fase N`
//! y a `GET /api/trabajos/<id>/diff?fase=N`.

use ea_store::FilaFase;

use crate::error::{ErrorBus, Resultado};

/// Lo que se excluye de todos los diffs de un trabajo.
pub const EXCLUIR_DIFF: &str = ":(exclude,glob)**/__pycache__/**";

/// El árbol guardado al cerrar la fase `numero`, si lo hay.
pub fn arbol_de_fase(fases: &[FilaFase], numero: u32) -> Option<String> {
    let f = fases.iter().find(|f| f.numero == i64::from(numero))?;
    let v: serde_json::Value = serde_json::from_str(f.informe.as_deref()?).ok()?;
    v.get("arbol")?.as_str().filter(|a| es_objeto(a)).map(String::from)
}

/// Un id de objeto de git (hexadecimal): lo que va a `git diff` como
/// argumento nunca puede empezar por `-` ni ser otra cosa.
fn es_objeto(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// El `diff_stat` guardado al cerrar la fase, si lo hay.
pub fn diff_stat_de_fase(f: &FilaFase) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(f.informe.as_deref()?).ok()?;
    v.get("diff_stat")?.as_str().map(String::from)
}

/// `(base, fin)` para `git diff base fin` de la fase `n`: de la instantánea
/// de la fase anterior (o de `commit_inicio^{tree}` para la primera) a la
/// suya. Sin instantánea, [`ErrorBus::SinInstantanea`] con el motivo.
pub fn rango_diff_fase(commit_inicio: Option<&str>, fases: &[FilaFase], n: u32) -> Resultado<(String, String)> {
    if n == 0 {
        return Err(ErrorBus::Argumentos("las fases empiezan en 1".into()));
    }
    let fin = arbol_de_fase(fases, n).ok_or_else(|| {
        ErrorBus::SinInstantanea(format!(
            "no hay instantánea de árbol de la fase {n} (la obra no la guardó o el store no la tiene)"
        ))
    })?;
    let base = if n == 1 {
        let inicio = commit_inicio.filter(|c| es_objeto(c)).ok_or_else(|| {
            ErrorBus::SinInstantanea("el trabajo no tiene informe con commit_inicio: no hay base para la fase 1".into())
        })?;
        format!("{inicio}^{{tree}}")
    } else {
        arbol_de_fase(fases, n - 1)
            .ok_or_else(|| ErrorBus::SinInstantanea(format!("no hay instantánea de árbol de la fase {}", n - 1)))?
    };
    Ok((base, fin))
}

/// El texto final del agente en su salida en bruto (`salida-qwen.json`:
/// el último mensaje `result`); vacío si no se puede extraer.
pub fn resumen_salida_agente(bruta: &str) -> String {
    serde_json::from_str::<serde_json::Value>(bruta)
        .ok()
        .and_then(|v| {
            v.as_array()?
                .iter()
                .rev()
                .find(|m| m["type"] == "result")
                .and_then(|m| m["result"].as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fase(n: i64, informe: Option<&str>) -> FilaFase {
        FilaFase {
            trabajo_id: "t".into(),
            numero: n,
            titulo: format!("f{n}"),
            estado: "hecha".into(),
            informe: informe.map(String::from),
            cerrado_en: None,
        }
    }

    #[test]
    fn el_rango_va_de_la_fase_anterior_a_la_pedida() {
        let fs = vec![fase(1, Some(r#"{"arbol":"a1a1","diff_stat":"x"}"#)), fase(2, Some(r#"{"arbol":"a2a2"}"#)), fase(3, None), fase(4, Some(r#"{"arbol":"--output=/tmp/x"}"#))];
        assert_eq!(rango_diff_fase(Some("c0c0"), &fs, 1).unwrap(), ("c0c0^{tree}".into(), "a1a1".into()));
        assert_eq!(rango_diff_fase(None, &fs, 2).unwrap(), ("a1a1".into(), "a2a2".into()));
        assert!(matches!(rango_diff_fase(Some("c0c0"), &fs, 3), Err(ErrorBus::SinInstantanea(m)) if m.contains("fase 3")));
        assert!(matches!(rango_diff_fase(None, &fs, 1), Err(ErrorBus::SinInstantanea(m)) if m.contains("commit_inicio")));
        assert!(matches!(rango_diff_fase(Some("c0c0"), &fs, 0), Err(ErrorBus::Argumentos(_))));
        assert!(matches!(rango_diff_fase(Some("c0c0"), &fs, 4), Err(ErrorBus::SinInstantanea(_))), "un árbol que no es un id no llega a git");
        assert!(matches!(rango_diff_fase(Some("-x"), &fs, 1), Err(ErrorBus::SinInstantanea(_))));
        assert_eq!(diff_stat_de_fase(&fs[0]).as_deref(), Some("x"));
    }

    #[test]
    fn el_resumen_es_el_ultimo_result() {
        let b = r#"[{"type":"assistant"},{"type":"result","result":"uno"},{"type":"result","result":"dos"}]"#;
        assert_eq!(resumen_salida_agente(b), "dos");
        assert_eq!(resumen_salida_agente("no es json"), "");
    }
}
