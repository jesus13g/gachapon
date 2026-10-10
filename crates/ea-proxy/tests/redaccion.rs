//! El paso de redacción: lo que se le pide al modelo para que cuente el
//! resultado de una herramienta en vez de devolverlo crudo.

use ea_proxy::catalog;
use ea_proxy::decide::{instruccion_extraccion, mensajes_redaccion, recortar};
use ea_proxy::llama_client::ChatMessage;
use serde_json::json;

#[test]
fn la_redaccion_lleva_el_resultado_y_conserva_la_conversacion() {
    let conversacion = [ChatMessage::user("¿Qué tareas tengo pendientes?")];
    let mensajes = mensajes_redaccion(&conversacion, "ea_listar_tareas", &json!({"tareas": []}));

    assert_eq!(mensajes.len(), 2);
    assert_eq!(mensajes[0].role, "system");
    assert!(mensajes[0].content.contains("ea_listar_tareas"));
    assert!(mensajes[0].content.contains(r#"{"tareas":[]}"#));
    assert!(mensajes[0].content.contains("sin JSON"));
    assert!(!mensajes[0].content.contains("recortado"));
    assert_eq!(mensajes[1].content, "¿Qué tareas tengo pendientes?");
}

#[test]
fn un_resultado_enorme_se_recorta_y_se_avisa() {
    let enorme = json!({"eventos": "ñ".repeat(20_000)});
    let mensajes = mensajes_redaccion(&[ChatMessage::user("diario")], "ea_listar_diario", &enorme);

    assert!(mensajes[0].content.chars().count() < 7_000);
    assert!(mensajes[0].content.contains("recortado"));
}

#[test]
fn recortar_no_parte_caracteres_multibyte() {
    assert_eq!(recortar("añoñ", 2), ("añ", true));
    assert_eq!(recortar("año", 3), ("año", false));
}

#[test]
fn la_extraccion_lleva_la_fecha_de_hoy_y_prohibe_inventar() {
    let tool = catalog::find("ea_listar_diario").expect("herramienta del catálogo");
    let instruccion = instruccion_extraccion(tool, "2026-10-10");

    assert!(instruccion.contains("Hoy es 2026-10-10"));
    assert!(instruccion.contains("null"));
    assert!(instruccion.contains(tool.args_help));
}
