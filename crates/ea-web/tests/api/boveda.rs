//! La bóveda desde la web: listar, buscar, leer y promover, el editor
//! (árbol, notas, carpetas y papelera) y el índice.

use crate::comun::*;

#[tokio::test]
async fn la_boveda_se_lista_busca_lee_y_promueve_desde_la_web() {
    let f = montar();
    let ruta = nota(&f, "modelo-local", "Receta de lentejas", "Lentejas con chorizo y pimentón.");
    nota(&f, "claude-code", "Otra cosa", "Nada que ver.");

    let (s, v) = get(&f, "/api/boveda").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["notas"].as_array().unwrap().len(), 2);
    let (_, v) = get(&f, "/api/boveda?q=lentejas&region=modelo-local").await;
    assert_eq!(v["notas"].as_array().unwrap().len(), 1);
    assert!(v["notas"][0]["fragmento"].as_str().unwrap().contains("«"));
    // Una consulta a medio escribir (solo signos) no es un error.
    let (s, v) = get(&f, "/api/boveda?q=%22%20(").await;
    assert_eq!((s, v["notas"].as_array().unwrap().len()), (StatusCode::OK, 0));
    let (s, _) = get(&f, "/api/boveda?region=inventada").await;
    assert_ne!(s, StatusCode::OK);

    let (s, v) = get(&f, &format!("/api/boveda/nota?ruta={ruta}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cabecera"]["autor"], "modelo-local");
    assert!(v["contenido"].as_str().unwrap().contains("chorizo"));
    // Fuera de la bóveda, no.
    for mala in ["../etc/passwd", "/etc/passwd", "modelo-local/../../x.md"] {
        let (s, _) = get(&f, &format!("/api/boveda/nota?ruta={mala}")).await;
        assert_ne!(s, StatusCode::OK, "{mala}");
    }

    // Promoción del usuario desde la web: copia y deja el evento.
    let (s, v) = post(&f, "/api/boveda/promover", json!({ "ruta": ruta })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = get(&f, "/api/boveda?region=compartida").await;
    assert_eq!(v["notas"].as_array().unwrap().len(), 1);
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    let acciones: Vec<_> = d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect();
    assert!(acciones.contains(&"boveda_promocion".to_string()) && acciones.contains(&"accion_ejecutada".to_string()));
    // Ya está: no se promueve dos veces; y lo que no es de un agente, tampoco.
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": ruta })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": "compartida/receta-de-lentejas.md" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn aceptar_en_la_web_la_propuesta_de_promocion_de_un_agente_la_ejecuta() {
    let f = montar();
    let ruta = nota(&f, "claude-code", "Hallazgo", "Texto del hallazgo.");
    {
        let store = f.state.store.lock().await;
        let c = ea_acciones::SoloConsulta::usuario(f.state.config.rutinas_dir.clone(), f.state.boveda.clone(), f.state.config.estado_dir.clone());
        let args = json!({ "ruta": ruta });
        ea_acciones::proponer(&store, &c, &Default::default(), "claude-code", None, "ea_promover_nota", &args)
            .unwrap();
    }
    assert!(!f.state.boveda.raiz().join("compartida").exists());
    let (_, ps) = get(&f, "/api/propuestas?estado=pendiente").await;
    let id = ps[0]["id"].as_i64().unwrap();
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["ejecutada"]["estado"], "ejecutada", "{p}");
    assert!(f.state.boveda.raiz().join("compartida/hallazgo.md").exists());
}

#[tokio::test]
async fn reindexar_la_boveda_reconstruye_el_indice_y_deja_evento() {
    let f = montar();
    nota(&f, "claude-code", "Una", "uno");
    let (_, v) = get(&f, "/api/boveda/arbol").await; // pone el índice al día
    assert_eq!(v["notas"].as_array().unwrap().len(), 1);
    let (s, v) = get(&f, "/api/boveda/indice").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["desfasado"], false);

    // Una nota creada a mano en el disco y otra borrada: el índice se desfasa.
    let raiz = f.state.boveda.raiz().to_path_buf();
    std::fs::create_dir_all(raiz.join("compartida")).unwrap();
    std::fs::write(raiz.join("compartida/externa.md"), "# Externa\n\ntexto").unwrap();
    std::fs::remove_file(raiz.join("claude-code/una.md")).unwrap();
    let (_, v) = get(&f, "/api/boveda/indice").await;
    assert_eq!((v["desfasado"].clone(), v["nuevas"].clone(), v["eliminadas"].clone()), (json!(true), json!(1), json!(1)));

    let (s, v) = post(&f, "/api/boveda/reindexar", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["nuevas"].clone(), v["eliminadas"].clone()), (json!(1), json!(1)));
    let (_, v) = get(&f, "/api/boveda/indice").await;
    assert_eq!(v["desfasado"], false);
    let (_, d) = get(&f, "/api/diario?q=boveda_reindexado").await;
    assert!(d.to_string().contains("boveda_reindexado"), "{d}");
}

// ------------------------------------------------------- editor de la bóveda

#[tokio::test]
async fn el_arbol_de_la_boveda_lleva_regiones_carpetas_y_notas() {
    let f = montar();
    nota(&f, "claude-code", "Una", "uno");
    std::fs::create_dir_all(f.state.boveda.raiz().join("modelo-local/vacia")).unwrap();
    let (s, v) = get(&f, "/api/boveda/arbol").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["regiones"], json!(["modelo-local", "claude-code", "compartida"]));
    assert_eq!(v["notas"][0]["ruta"], "claude-code/una.md");
    let carpetas: Vec<_> = v["carpetas"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(carpetas.contains(&"modelo-local/vacia") && carpetas.contains(&"claude-code"));
    assert_eq!(v["en_papelera"], 0);
}

#[tokio::test]
async fn la_nota_se_lee_entera_y_se_guarda_con_su_version() {
    let f = montar();
    let ruta = nota(&f, "modelo-local", "Receta", "Lentejas.");
    let (s, v) = get(&f, &format!("/api/boveda/fichero?ruta={ruta}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let contenido = v["contenido"].as_str().unwrap().to_string();
    assert!(contenido.starts_with("---\n") && contenido.contains("Lentejas."));
    assert!(std::path::Path::new(v["absoluta"].as_str().unwrap()).ends_with("boveda/modelo-local/receta.md"));
    let version = v["version"].as_str().unwrap().to_string();

    let nuevo = contenido.replace("Lentejas.", "Garbanzos.");
    let (s, g) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": nuevo, "version": version })).await;
    assert_eq!(s, StatusCode::OK, "{g}");
    assert!(std::fs::read_to_string(f.state.boveda.raiz().join(&ruta)).unwrap().contains("Garbanzos."));
    let (_, b) = get(&f, "/api/boveda?q=garbanzos").await;
    assert_eq!(b["notas"].as_array().unwrap().len(), 1);
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    assert!(acciones_del_diario(&d).contains(&"boveda_edicion".to_string()));

    // Con la versión vieja: 409 y el disco no cambia.
    let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": "otra", "version": version })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(std::fs::read_to_string(f.state.boveda.raiz().join(&ruta)).unwrap().contains("Garbanzos."));
    // Cambiar la firma: 400.
    let v2 = g["version"].as_str().unwrap();
    let firma = g["contenido"].as_str().unwrap().replace("autor: modelo-local", "autor: usuario");
    let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": firma, "version": v2 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, "/api/boveda/fichero?ruta=compartida/no-esta.md").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn crear_solo_en_la_compartida() {
    let f = montar();
    let (s, v) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/ideas/nueva" })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["ruta"], "compartida/ideas/nueva.md");
    assert_eq!(v["cabecera"]["autor"], "usuario");
    let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/ideas/nueva.md" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": "claude-code/mia.md" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(!f.state.boveda.raiz().join("claude-code/mia.md").exists());
    let (s, _) = post(&f, "/api/boveda/carpeta", json!({ "ruta": "claude-code/carpeta" })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(f.state.boveda.raiz().join("claude-code/carpeta").is_dir());
}

#[tokio::test]
async fn renombrar_en_la_boveda_reescribe_los_enlaces() {
    let f = montar();
    let (_, x) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/x", "contenido": "soy x" })).await;
    post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/otra", "contenido": "[[x]] y [x](x.md)" })).await;
    let (s, r) = post(
        &f,
        "/api/boveda/renombrar",
        json!({ "ruta": "compartida/x.md", "nuevo": "compartida/sub/y", "version": x["version"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["nota"]["ruta"], "compartida/sub/y.md");
    assert_eq!(r["actualizadas"], json!(["compartida/otra.md"]));
    let otra = std::fs::read_to_string(f.state.boveda.raiz().join("compartida/otra.md")).unwrap();
    assert!(otra.contains("[[y]] y [x](sub/y.md)"), "{otra}");
    // No pisa otra nota ni cambia de región.
    let v = r["nota"]["version"].clone();
    let (s, _) = post(&f, "/api/boveda/renombrar", json!({ "ruta": "compartida/sub/y.md", "nuevo": "compartida/otra", "version": v })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/renombrar", json!({ "ruta": "compartida/sub/y.md", "nuevo": "claude-code/y", "version": v })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn eliminar_manda_a_la_papelera_y_se_restaura() {
    let f = montar();
    let ruta = nota(&f, "claude-code", "Borrable", "contenido");
    let (_, n) = get(&f, &format!("/api/boveda/fichero?ruta={ruta}")).await;
    let (s, p) = post(&f, "/api/boveda/eliminar", json!({ "ruta": ruta, "version": n["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(!f.state.boveda.raiz().join(&ruta).exists());
    let (_, a) = get(&f, "/api/boveda/arbol").await;
    assert_eq!((a["notas"].as_array().unwrap().len(), a["en_papelera"].as_u64()), (0, Some(1)));
    let (_, l) = get(&f, "/api/boveda/papelera").await;
    assert_eq!(l[0]["ruta"], ruta.as_str());
    let (s, r) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": p["id"] })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(r["contenido"].as_str().unwrap().contains("contenido"));
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    let acc = acciones_del_diario(&d);
    assert!(acc.contains(&"boveda_papelera".to_string()) && acc.contains(&"boveda_restauracion".to_string()));
    let (s, _) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": p["id"] })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_editor_de_la_boveda_rechaza_rutas_maliciosas_y_exige_json() {
    let f = montar();
    nota(&f, "claude-code", "Algo", "x");
    for mala in ["../etc/passwd", "/etc/passwd", "claude-code/../../x.md", ".papelera/x.md", "claude-code/.x.md"] {
        let (s, _) = get(&f, &format!("/api/boveda/fichero?ruta={mala}")).await;
        assert_ne!(s, StatusCode::OK, "{mala}");
        let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": mala, "contenido": "x", "version": "0" })).await;
        assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::FORBIDDEN, "{mala}: {s}");
        let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": mala })).await;
        assert!(s.is_client_error(), "{mala}: {s}");
    }
    let (s, _) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": "../../etc" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = enviar(&f, Method::POST, "/api/boveda/crear", "localhost:8092", Some(json!({ "ruta": "compartida/x" })), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!f.state.boveda.raiz().join("compartida/x.md").exists());
    let (s, _) = post(&f, "/api/boveda/mostrar", json!({ "ruta": "compartida/no-esta.md" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// --- fase 2 del editor de la bóveda: carpetas y vaciar la papelera (spec 015) ---

#[tokio::test]
async fn renombrar_y_borrar_carpetas_de_la_boveda() {
    let f = montar();
    post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/a/x", "contenido": "soy x" })).await;
    post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/a/sub/y", "contenido": "soy y" })).await;
    post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/otra", "contenido": "[[x]] [y](a/sub/y.md)" })).await;
    let (s, r) = post(&f, "/api/boveda/carpeta/renombrar", json!({ "ruta": "compartida/a", "nuevo": "compartida/b" })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["carpeta"], "compartida/b");
    assert_eq!(r["movidas"].as_array().unwrap().len(), 2);
    assert_eq!(r["actualizadas"], json!(["compartida/otra.md"]));
    let otra = std::fs::read_to_string(f.state.boveda.raiz().join("compartida/otra.md")).unwrap();
    assert!(otra.contains("[y](b/sub/y.md)"), "{otra}");
    assert!(f.state.boveda.raiz().join("compartida/b/x.md").exists() && !f.state.boveda.raiz().join("compartida/a").exists());
    // Una región no se renombra; otra región o un destino ocupado, tampoco.
    let (s, _) = post(&f, "/api/boveda/carpeta/renombrar", json!({ "ruta": "compartida", "nuevo": "compartida/z" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/boveda/carpeta/renombrar", json!({ "ruta": "compartida/b", "nuevo": "claude-code/b" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    post(&f, "/api/boveda/carpeta", json!({ "ruta": "compartida/libre" })).await;
    let (s, _) = post(&f, "/api/boveda/carpeta/renombrar", json!({ "ruta": "compartida/b", "nuevo": "compartida/libre" })).await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Borrar es mandar a la papelera, y restaurar devuelve todo.
    let (s, p) = post(&f, "/api/boveda/carpeta/eliminar", json!({ "ruta": "compartida/b" })).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["vacia"], false);
    assert_eq!(p["notas"].as_array().unwrap().len(), 2);
    assert!(!f.state.boveda.raiz().join("compartida/b").exists());
    let (_, l) = get(&f, "/api/boveda/papelera").await;
    assert_eq!(l.as_array().unwrap().len(), 2);
    let (s, r) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": p["id"] })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(f.state.boveda.raiz().join("compartida/b/sub/y.md").exists());
    let (_, a) = get(&f, "/api/boveda/arbol").await;
    assert_eq!(a["en_papelera"], 0);
    // Una carpeta vacía se quita sin papelera; una región, no.
    let (s, p) = post(&f, "/api/boveda/carpeta/eliminar", json!({ "ruta": "compartida/libre" })).await;
    assert_eq!((s, p["vacia"].clone()), (StatusCode::OK, json!(true)));
    let (s, _) = post(&f, "/api/boveda/carpeta/eliminar", json!({ "ruta": "compartida" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/boveda/carpeta/eliminar", json!({ "ruta": "compartida/no-esta" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, d) = get(&f, "/api/diario?limite=100").await;
    let acc = acciones_del_diario(&d);
    for e in ["boveda_carpeta_renombrada", "boveda_papelera_carpeta", "boveda_carpeta_eliminada", "boveda_restauracion"] {
        assert!(acc.contains(&e.to_string()), "{e}: {acc:?}");
    }
}

#[tokio::test]
async fn vaciar_la_papelera_de_la_boveda_no_se_hace_desde_la_web() {
    let f = montar();
    let ruta = nota(&f, "claude-code", "Borrable", "contenido");
    let (_, n) = get(&f, &format!("/api/boveda/fichero?ruta={ruta}")).await;
    post(&f, "/api/boveda/eliminar", json!({ "ruta": ruta, "version": n["version"] })).await;
    // No hay ruta que borre la papelera.
    for ruta in ["/api/boveda/papelera/vaciar", "/api/boveda/vaciar-papelera", "/api/boveda/papelera/eliminar"] {
        let (s, _) = post(&f, ruta, json!({})).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{ruta}");
    }
    let (s, _) = enviar(&f, Method::DELETE, "/api/boveda/papelera", "localhost:8092", None, true).await;
    assert!(s == StatusCode::METHOD_NOT_ALLOWED || s == StatusCode::NOT_FOUND, "{s}");
    let (_, l) = get(&f, "/api/boveda/papelera").await;
    assert_eq!(l.as_array().unwrap().len(), 1, "la papelera sigue como estaba");
    // Lo único que hace la web es abrir la terminal con el comando de `ea`.
    for malo in [json!({ "boveda": "vaciar" }), json!({ "boveda": "vaciar-papelera", "confirmo": "vaciar-papelera" })] {
        let (s, _) = post(&f, "/api/terminal/abrir", malo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{malo}");
    }
    assert!(lanzadas(&f).is_empty());
    let (s, r) = post(&f, "/api/terminal/abrir", json!({ "boveda": "vaciar-papelera" })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["comando"], "ea boveda vaciar-papelera");
    let l = lanzadas(&f);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0][l[0].len() - 3..], ["boveda", "vaciar-papelera", "--esperar"]);
    let (_, l) = get(&f, "/api/boveda/papelera").await;
    assert_eq!(l.as_array().unwrap().len(), 1, "abrir la terminal no borra nada");
}
