//! Tests herméticos del catálogo reducido y sus schemas — no dependen de
//! `llama-server` ni de `ea-mcp`, solo comprueban que lo declarado en
//! `catalog.rs` tiene la forma que el resto del flujo asume.

use ea_proxy::catalog;

#[test]
fn el_catalogo_es_solo_lectura_de_las_cuatro_herramientas_esperadas() {
    let nombres = catalog::tool_names();
    assert_eq!(
        nombres,
        vec!["ea_status", "ea_listar_tareas", "ea_ver_tarea", "ea_listar_diario"],
        "el catálogo reducido debe ser exactamente estas cuatro herramientas de solo \
         lectura (ea_crear_tarea queda fuera a propósito en esta fase)"
    );
}

#[test]
fn cada_herramienta_del_catalogo_se_encuentra_por_nombre() {
    for nombre in catalog::tool_names() {
        assert!(
            catalog::find(nombre).is_some(),
            "'{nombre}' está en tool_names() pero find() no la encuentra"
        );
    }
    assert!(catalog::find("no_existe").is_none());
}

#[test]
fn los_schemas_de_argumentos_son_objetos_estrictos_validos() {
    for tool in catalog::CATALOGO {
        let schema = (tool.args_schema)();
        assert_eq!(schema["type"], "object", "{}: type debe ser 'object'", tool.name);
        assert_eq!(
            schema["additionalProperties"], false,
            "{}: additionalProperties debe ser false (modo estricto)",
            tool.name
        );
        let props = schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{}: properties debe ser un objeto", tool.name));
        let required = schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{}: required debe ser un array", tool.name));
        assert_eq!(
            props.len(),
            required.len(),
            "{}: todo campo de properties debe estar en required (modo estricto de \
             generación acotada por forma)",
            tool.name
        );
    }
}

#[test]
fn el_schema_de_estado_incluye_los_cinco_valores_del_dominio_y_null() {
    let schema = (catalog::find("ea_listar_tareas").unwrap().args_schema)();
    let enum_estado = schema["properties"]["estado"]["enum"]
        .as_array()
        .expect("estado debe declarar un enum");
    let valores: Vec<&str> = enum_estado.iter().filter_map(|v| v.as_str()).collect();
    for esperado in ["pendiente", "en_progreso", "completada", "fallida", "cancelada"] {
        assert!(
            valores.contains(&esperado),
            "falta '{esperado}' en el enum de estado: {valores:?}"
        );
    }
    assert!(
        enum_estado.contains(&serde_json::Value::Null),
        "el enum de estado debe admitir null explícitamente (campo opcional)"
    );
}

#[test]
fn el_schema_de_eleccion_de_herramienta_no_admite_null() {
    let schema = catalog::schema_elegir_herramienta();
    let enum_herramienta = schema["properties"]["herramienta"]["enum"]
        .as_array()
        .expect("herramienta debe declarar un enum");
    assert!(
        !enum_herramienta.contains(&serde_json::Value::Null),
        "el paso de elegir herramienta no debe admitir null: el paso de clasificación \
         previo ya decidió que hace falta una (mezclar ambos sesga al modelo, validado \
         empíricamente)"
    );
}
