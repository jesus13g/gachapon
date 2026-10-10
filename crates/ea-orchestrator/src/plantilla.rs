//! Sustitución de plantilla mínima para textos y argumentos de una rutina:
//! dos marcadores literales, sin motor de expresiones (§9.2: los pasos son
//! fijos, no generados — no hace falta más). `{ultima_ejecucion}` es la
//! ventana real transcurrida desde la última ejecución con éxito (§10.2);
//! `{salida_previa}` es el resultado JSON del paso anterior.

pub struct Contexto<'a> {
    pub ultima_ejecucion: &'a str,
    pub salida_previa: &'a str,
}

pub fn renderizar(texto: &str, ctx: &Contexto) -> String {
    texto
        .replace("{ultima_ejecucion}", ctx.ultima_ejecucion)
        .replace("{salida_previa}", ctx.salida_previa)
}

/// Igual que [`renderizar`] pero recorriendo un `serde_json::Value`
/// completo (los `argumentos` de un paso `accion`), sustituyendo solo en
/// las hojas de tipo cadena.
pub fn renderizar_valor(valor: &serde_json::Value, ctx: &Contexto) -> serde_json::Value {
    match valor {
        serde_json::Value::String(s) => serde_json::Value::String(renderizar(s, ctx)),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(|v| renderizar_valor(v, ctx)).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), renderizar_valor(v, ctx)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sustituye_ambos_marcadores_en_texto() {
        let ctx = Contexto {
            ultima_ejecucion: "2026-09-13T00:00:00Z",
            salida_previa: r#"{"eventos":[]}"#,
        };
        let out = renderizar("desde {ultima_ejecucion}: {salida_previa}", &ctx);
        assert_eq!(out, r#"desde 2026-09-13T00:00:00Z: {"eventos":[]}"#);
    }

    #[test]
    fn sustituye_dentro_de_objetos_json_anidados() {
        let ctx = Contexto {
            ultima_ejecucion: "2026-09-13T00:00:00Z",
            salida_previa: "",
        };
        let valor = serde_json::json!({ "desde": "{ultima_ejecucion}", "limite": 50 });
        let out = renderizar_valor(&valor, &ctx);
        assert_eq!(out["desde"], "2026-09-13T00:00:00Z");
        assert_eq!(out["limite"], 50);
    }
}
