//! Abrir en una terminal (solo órdenes del catálogo) y el plan de los
//! servicios.

use crate::comun::*;

#[tokio::test]
async fn abrir_en_terminal_solo_acepta_comandos_del_catalogo() {
    let f = montar();
    let reversible = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    // Solo se borra para siempre una rutina retirada (en la papelera).
    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let id = proponer_accion(&f, "ea_eliminar_rutina", json!({ "slug": "revision-diaria" })).await;

    // Texto libre, comandos, campos de más o lo que se decide desde la web: 400.
    for cuerpo in [
        json!("ea propuestas decidir 1 --aceptar"),
        json!({ "comando": "rm -rf ~" }),
        json!({ "propuesta": id, "confirmo": id }),
        json!({ "propuesta": format!("{id}; rm -rf ~") }),
        json!({ "propuesta": reversible }),
        json!({ "herramienta": "rm", "argumentos": {} }),
        json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "../x" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "x", "otro": "y" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": "slug=x" }),
    ] {
        let (s, _) = post(&f, "/api/terminal/abrir", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    }
    let (s, _) = enviar(&f, Method::POST, "/api/terminal/abrir", "localhost:8092", None, true).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "cuerpo vacío");
    let (s, _) = post(&f, "/api/terminal/abrir", json!({ "propuesta": 9999 })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Sin la clave de sesión no se abre nada.
    let cuerpo = json!({ "propuesta": id });
    let (s, _) = enviar_con_clave(&f, Method::POST, "/api/terminal/abrir", "localhost:8092", Some(cuerpo), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(lanzadas(&f).is_empty(), "nada de lo anterior abre una ventana");

    // Lo que sí: el comando de la CLI como argv, sin shell ni frase.
    let (s, r) = post(&f, "/api/terminal/abrir", json!({ "propuesta": id })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["comando"], format!("ea propuestas decidir {id} --aceptar"));
    let args = json!({ "slug": "revision-diaria" });
    let (s, _) = post(&f, "/api/terminal/abrir", json!({ "herramienta": "ea_eliminar_rutina", "argumentos": args })).await;
    assert_eq!(s, StatusCode::OK);
    let ea = f.state.config.ea.as_ref().unwrap().display().to_string();
    let id_txt = id.to_string();
    let esperado: Vec<Vec<String>> = [
        vec![&ea, "propuestas", "decidir", &id_txt, "--aceptar", "--esperar"],
        vec![&ea, "acciones", "ejecutar", "ea_eliminar_rutina", "--args", r#"{"slug":"revision-diaria"}"#, "--esperar"],
    ]
    .into_iter()
    .map(|v| v.into_iter().map(String::from).collect())
    .collect();
    assert_eq!(lanzadas(&f), esperado);
    // Abrir no decide nada: la propuesta sigue pendiente y la rutina, en su sitio.
    let (_, p) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(p.as_array().unwrap().len(), 2);
    assert!(ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap().exists());
    let (_, d) = get(&f, "/api/diario?accion=accion_terminal").await;
    assert_eq!(d.as_array().unwrap().len(), 2);
    assert_eq!(d[0]["actor"], "usuario");
}

/// Spec 020, H4 (issue #5): el plan de `ea servicios ver` por la API (solo
/// lectura, sin `PATH` ni secretos, sin consultar al gestor en las pruebas).
#[tokio::test]
async fn el_plan_de_servicios_dice_gestor_unidades_y_binarios() {
    let f = montar();
    let (s, p) = get(&f, "/api/servicios/plan").await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(["systemd", "launchd", "schtasks"].contains(&p["backend"].as_str().unwrap()), "{p}");
    assert!(!p["gestor"].as_str().unwrap().is_empty());
    assert!(!p["binarios"].as_str().unwrap().is_empty());
    let nombres: Vec<&str> = p["servicios"].as_array().unwrap().iter().map(|x| x["nombre"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["ea-mcp", "ea-orchestrator", "ea-web"]);
    for x in p["servicios"].as_array().unwrap() {
        assert!(x["unidad"].as_str().unwrap().contains(x["nombre"].as_str().unwrap()));
        assert!(x["binario"].as_str().unwrap().contains(x["nombre"].as_str().unwrap()));
        // Sin consultar al gestor (pruebas): ni instalado ni activo se saben.
        assert!(x["instalado"].is_null(), "{x}");
        assert_eq!(x["activo"], "desconocido");
    }
    // Cada binario que no está junto a ea-web se anuncia como «falta»; el plan lleva pasos.
    assert!(!p["pasos"].as_array().unwrap().is_empty());
    assert!(p["faltan"].is_array());
    // Nada del entorno: ni el valor de PATH ni claves.
    let texto = p.to_string();
    if let Ok(path) = std::env::var("PATH") {
        assert!(!texto.contains(&path), "el plan no debe llevar el PATH");
    }
    assert!(p["path_fijo"].is_boolean());
}

/// Spec 020, H4 (RF-018): instalar y desinstalar los servicios se abren en la
/// terminal (con `--esperar`), nunca se hacen desde la web; el resto, 400.
#[tokio::test]
async fn abrir_en_terminal_instalar_y_desinstalar_servicios() {
    let f = montar();
    for cuerpo in [
        json!({ "servicios": "reiniciar" }),
        json!({ "servicios": "instalar; rm -rf ~" }),
        json!({ "servicios": 1 }),
        json!({ "servicios": "instalar", "confirmo": "s" }),
        json!({ "servicios": ["instalar"] }),
    ] {
        let (s, _) = post(&f, "/api/terminal/abrir", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    }
    let (s, _) = enviar_con_clave(&f, Method::POST, "/api/terminal/abrir", "localhost:8092", Some(json!({ "servicios": "instalar" })), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(lanzadas(&f).is_empty(), "nada de lo anterior abre una ventana");

    for (accion, comando) in [("instalar", "ea servicios instalar"), ("desinstalar", "ea servicios desinstalar")] {
        let (s, r) = post(&f, "/api/terminal/abrir", json!({ "servicios": accion })).await;
        assert_eq!(s, StatusCode::OK, "{r}");
        assert_eq!(r["comando"], comando);
    }
    let ea = f.state.config.ea.as_ref().unwrap().display().to_string();
    let esperado: Vec<Vec<String>> = [vec![&ea, "servicios", "instalar", "--esperar"], vec![&ea, "servicios", "desinstalar", "--esperar"]]
        .into_iter()
        .map(|v| v.into_iter().map(String::from).collect())
        .collect();
    assert_eq!(lanzadas(&f), esperado);
    let (_, d) = get(&f, "/api/diario?accion=accion_terminal").await;
    assert_eq!(d.as_array().unwrap().len(), 2);
    assert_eq!(d[0]["actor"], "usuario");
}
