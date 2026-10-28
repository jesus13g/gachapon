//! Las instantáneas de árbol que la obra guarda al cerrar cada fase (el
//! `arbol` del informe de la fase en `trabajo_fases`) y el rango de `git
//! diff` que sale de ellas. Lógica pura, común a `ea trabajo diff --fase N`
//! y a `GET /api/trabajos/<id>/diff?fase=N`.

use std::path::Path;

use ea_store::FilaFase;
use serde::Serialize;

use crate::error::{ErrorBus, Resultado};
use crate::tipos::EjecutadoPor;

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

/// La salida en bruto del agente en el directorio del trabajo (spec 017,
/// RF-010): `salida-agente.json`, o `salida-qwen.json` en los trabajos de
/// antes.
pub const SALIDA_AGENTE: &str = "salida-agente.json";
pub const SALIDA_ANTIGUA: &str = "salida-qwen.json";
/// Las skills que se le pasaron (`skills-qwen.md` en los trabajos de antes).
pub const SKILLS_AGENTE: &str = "skills-agente.md";

/// Lee la salida en bruto de un trabajo, con el nombre nuevo o el antiguo.
pub fn leer_salida_agente(dir_trabajo: &Path) -> Option<String> {
    [SALIDA_AGENTE, SALIDA_ANTIGUA].iter().find_map(|n| std::fs::read_to_string(dir_trabajo.join(n)).ok())
}

/// El adaptador de cada agente que puede ejecutar (spec 017): cada CLI tiene
/// sus argumentos y su formato de salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TipoAgente {
    QwenCode,
    Codex,
    OpenCode,
}

impl TipoAgente {
    /// El tipo de un agente del catálogo que admite el rol de ejecutor.
    pub fn desde(id: &str) -> Option<Self> {
        match id {
            "qwen-code" => Some(Self::QwenCode),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::QwenCode => "qwen-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

/// El texto final del agente en su salida en bruto; vacío si no se puede
/// extraer. Cada CLI tiene su formato:
///
/// - Qwen Code (`--output-format json`): un array; el último `result`.
/// - Codex (`exec --json`): JSONL; el último `item.completed` con un
///   `agent_message`.
/// - OpenCode (`run --format json`): JSONL; la última parte `text`.
///
/// Si el formato cambia con una versión, el resumen queda vacío y el
/// criterio sigue decidiendo.
pub fn resumen_salida_agente(tipo: TipoAgente, bruta: &str) -> String {
    let texto = |v: &serde_json::Value| v.as_str().map(str::to_string);
    match tipo {
        TipoAgente::QwenCode => serde_json::from_str::<serde_json::Value>(bruta).ok().and_then(|v| {
            v.as_array()?.iter().rev().find(|m| m["type"] == "result").and_then(|m| texto(&m["result"]))
        }),
        TipoAgente::Codex => lineas_json(bruta).rev().find_map(|m| {
            (m["type"] == "item.completed" && m["item"]["type"] == "agent_message").then(|| texto(&m["item"]["text"])).flatten()
        }),
        TipoAgente::OpenCode => lineas_json(bruta).rev().find_map(|m| {
            (m["type"] == "text").then(|| texto(&m["part"]["text"]).or_else(|| texto(&m["text"]))).flatten()
        }),
    }
    .unwrap_or_default()
}

fn lineas_json(bruta: &str) -> impl DoubleEndedIterator<Item = serde_json::Value> + '_ {
    bruta.lines().filter_map(|l| serde_json::from_str(l.trim()).ok())
}

/// El tipo del agente que ejecutó un trabajo según su `ejecutado_por`; los
/// trabajos de antes (sin él) son de Qwen Code.
pub fn tipo_ejecutor(por: Option<&EjecutadoPor>) -> TipoAgente {
    por.and_then(|p| TipoAgente::desde(&p.agente)).unwrap_or(TipoAgente::QwenCode)
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
    fn el_resumen_depende_del_agente() {
        let b = r#"[{"type":"assistant"},{"type":"result","result":"uno"},{"type":"result","result":"dos"}]"#;
        assert_eq!(resumen_salida_agente(TipoAgente::QwenCode, b), "dos");
        let codex = concat!(
            r#"{"type":"thread.started"}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"uno"}}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"ls"}}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"hecho"}}"#, "\n",
            r#"{"type":"turn.completed"}"#, "\n",
        );
        assert_eq!(resumen_salida_agente(TipoAgente::Codex, codex), "hecho");
        let opencode = concat!(
            r#"{"type":"step_start"}"#, "\n",
            r#"{"type":"text","part":{"type":"text","text":"listo"}}"#, "\n",
            r#"{"type":"step_finish"}"#, "\n",
        );
        assert_eq!(resumen_salida_agente(TipoAgente::OpenCode, opencode), "listo");
        for t in [TipoAgente::QwenCode, TipoAgente::Codex, TipoAgente::OpenCode] {
            assert_eq!(resumen_salida_agente(t, "no es json"), "", "un formato desconocido da un resumen vacío");
        }
        assert_eq!(resumen_salida_agente(TipoAgente::Codex, b), "", "cada agente con su formato");
    }

    #[test]
    fn la_salida_se_lee_con_el_nombre_nuevo_o_el_antiguo() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(leer_salida_agente(d.path()), None);
        std::fs::write(d.path().join(SALIDA_ANTIGUA), "vieja").unwrap();
        assert_eq!(leer_salida_agente(d.path()).as_deref(), Some("vieja"));
        std::fs::write(d.path().join(SALIDA_AGENTE), "nueva").unwrap();
        assert_eq!(leer_salida_agente(d.path()).as_deref(), Some("nueva"));
        assert_eq!(tipo_ejecutor(None), TipoAgente::QwenCode, "un trabajo de antes es de Qwen");
        let por = EjecutadoPor { agente: "codex".into(), modelo: "m".into() };
        assert_eq!(tipo_ejecutor(Some(&por)), TipoAgente::Codex);
    }
}
