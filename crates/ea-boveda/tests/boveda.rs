//! Pruebas del escritor único, las regiones, el contrato de cabecera, la
//! promoción y la búsqueda acotada. Todo sobre directorios y bases temporales.

use std::sync::{Arc, Barrier};
use std::time::Duration;

use ea_boveda::cabecera::{interpretar, Cabeceras};
use ea_boveda::ruta::validar_ruta;
use ea_boveda::{Boveda, ErrorBoveda, Limites, NuevaNota};
use ea_store::{EventFilter, Store};
use tempfile::TempDir;

struct Entorno {
    dir: TempDir,
    boveda: Boveda,
    db: std::path::PathBuf,
}

fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let boveda = Boveda::en(dir.path().join("boveda"));
    Entorno { dir, boveda, db }
}

impl Entorno {
    fn store(&self) -> Store {
        Store::open_at(&self.db).unwrap()
    }
    fn escribir(&self, autor: &str, titulo: &str, cuerpo: &str) -> ea_boveda::NotaEscrita {
        let mut s = self.store();
        self.boveda
            .escribir_nota(
                &mut s,
                autor,
                &NuevaNota { titulo, cuerpo, ruta: None, tags: &[], origen: "manual", sobrescribir: false },
                &Limites::AMPLIOS,
            )
            .unwrap()
    }
    fn eventos(&self, accion: &str) -> Vec<ea_store::Event> {
        self.store()
            .list_events(&EventFilter { limit: Some(1000), ..Default::default() })
            .unwrap()
            .into_iter()
            .filter(|e| e.action == accion)
            .collect()
    }
}

fn nueva<'a>(titulo: &'a str, cuerpo: &'a str, ruta: Option<&'a str>) -> NuevaNota<'a> {
    NuevaNota { titulo, cuerpo, ruta, tags: &[], origen: "manual", sobrescribir: false }
}

// --- rutas ----------------------------------------------------------------

#[test]
fn rechaza_rutas_que_salen_de_la_boveda() {
    for mala in [
        "",
        "/etc/passwd",
        "~/x.md",
        "modelo-local/../compartida/x.md",
        "modelo-local/../../x.md",
        "../x.md",
        "modelo-local/./x.md",
        "modelo-local//x.md",
        "modelo-local/x.md/",
        "modelo-local/.oculta/x.md",
        "modelo-local\\x.md",
        "otra/x.md",
        "x.md",
        "modelo-local/x.txt",
        "modelo-local/x\0.md",
    ] {
        assert!(validar_ruta(mala).is_err(), "debería rechazar {mala:?}");
    }
    assert!(validar_ruta("modelo-local/ideas/rust.md").is_ok());

    // Y la escritura, que antepone la región, tampoco se deja desviar.
    let e = entorno();
    let mut s = e.store();
    for mala in ["../compartida/x.md", "/tmp/x.md", "a/../../x.md", ".oculta/x.md"] {
        let r = e.boveda.escribir_nota(&mut s, "modelo-local", &nueva("T", "c", Some(mala)), &Limites::AMPLIOS);
        assert!(matches!(r, Err(ErrorBoveda::Ruta(_))), "{mala}: {r:?}");
    }
    assert!(e.boveda.leer("../etc/passwd", 0, &Limites::AMPLIOS).is_err());
    assert!(!e.dir.path().join("compartida").exists());
}

#[test]
fn rechaza_enlaces_simbolicos_que_apuntan_fuera() {
    let e = entorno();
    let fuera = e.dir.path().join("fuera");
    std::fs::create_dir_all(&fuera).unwrap();
    std::fs::write(fuera.join("secreto.md"), "secreto").unwrap();
    let region = e.boveda.raiz().join("modelo-local");
    std::fs::create_dir_all(&region).unwrap();
    // Un directorio y un fichero enlazados hacia fuera de la bóveda.
    std::os::unix::fs::symlink(&fuera, region.join("salida")).unwrap();
    std::os::unix::fs::symlink(fuera.join("secreto.md"), region.join("atajo.md")).unwrap();
    // Y un enlace hacia otra región de la propia bóveda.
    std::fs::create_dir_all(e.boveda.raiz().join("compartida")).unwrap();
    std::os::unix::fs::symlink(e.boveda.raiz().join("compartida"), region.join("a-compartida")).unwrap();

    let mut s = e.store();
    for ruta in ["salida/x.md", "atajo.md", "a-compartida/x.md"] {
        let r = e.boveda.escribir_nota(
            &mut s,
            "modelo-local",
            &NuevaNota { sobrescribir: true, ..nueva("T", "contenido", Some(ruta)) },
            &Limites::AMPLIOS,
        );
        assert!(matches!(r, Err(ErrorBoveda::FueraDeLaBoveda(_))), "{ruta}: {r:?}");
    }
    assert_eq!(std::fs::read_to_string(fuera.join("secreto.md")).unwrap(), "secreto");
    assert!(!fuera.join("x.md").exists());
    assert!(!e.boveda.raiz().join("compartida/x.md").exists());
    // Tampoco se lee a través de ellos.
    for ruta in ["modelo-local/salida/secreto.md", "modelo-local/atajo.md"] {
        assert!(
            matches!(e.boveda.leer(ruta, 0, &Limites::AMPLIOS), Err(ErrorBoveda::FueraDeLaBoveda(_))),
            "{ruta}"
        );
    }
}

// --- regiones y agentes ----------------------------------------------------

#[test]
fn un_agente_solo_escribe_en_su_region_y_no_en_la_compartida() {
    let e = entorno();
    let n = e.escribir("modelo-local", "Mi idea", "algo");
    assert_eq!(n.ruta, "modelo-local/mi-idea.md");
    let m = e.escribir("claude-code", "Mi idea", "otra cosa");
    assert_eq!(m.ruta, "claude-code/mi-idea.md");

    let mut s = e.store();
    // Quien no es un agente no escribe por esta vía (el orquestador es de solo lectura).
    for quien in ["orquestador", "usuario", "compartida", "otro"] {
        let r = e.boveda.escribir_nota(&mut s, quien, &nueva("T", "c", None), &Limites::AMPLIOS);
        assert!(matches!(r, Err(ErrorBoveda::Prohibida(_))), "{quien}: {r:?}");
    }
    // No existe forma de nombrar la región compartida: un prefijo "compartida/" queda DENTRO de la propia región.
    let r = e
        .boveda
        .escribir_nota(&mut s, "modelo-local", &nueva("T", "c", Some("compartida/x.md")), &Limites::AMPLIOS)
        .unwrap();
    assert_eq!(r.ruta, "modelo-local/compartida/x.md");
    assert!(!e.boveda.raiz().join("compartida/x.md").exists());
    // Y la región compartida sigue vacía.
    assert!(!e.boveda.raiz().join("compartida").exists());
}

#[test]
fn un_agente_no_puede_promover() {
    let e = entorno();
    let n = e.escribir("modelo-local", "Hallazgo", "texto del hallazgo");
    let plan = e.boveda.planear_promocion(&n.ruta, None).unwrap();
    let mut s = e.store();
    for quien in ["modelo-local", "claude-code", "orquestador", ""] {
        let r = e.boveda.promover(&mut s, &n.ruta, None, &plan.huella, quien, "test");
        assert!(matches!(r, Err(ErrorBoveda::Prohibida(_))), "{quien:?}: {r:?}");
    }
    assert!(!e.boveda.raiz().join("compartida").exists());
    assert!(e.eventos("boveda_promocion").is_empty());
}

// --- contrato de cabecera ---------------------------------------------------

#[test]
fn la_nota_escrita_cumple_el_contrato_de_cabecera() {
    let e = entorno();
    let mut s = e.store();
    let tags = vec!["servicios".to_string(), "mcp".to_string()];
    let n = e
        .boveda
        .escribir_nota(
            &mut s,
            "claude-code",
            &NuevaNota { tags: &tags, origen: "rutina:revision-diaria", ..nueva("Cómo «reiniciar» ea-mcp: \"así\"", "cuerpo\ncon varias líneas", None) },
            &Limites::AMPLIOS,
        )
        .unwrap();
    let texto = std::fs::read_to_string(e.boveda.raiz().join(&n.ruta)).unwrap();
    assert!(texto.starts_with("---\ncontrato: 1\n"));
    let (c, cuerpo) = interpretar(&texto);
    let Cabeceras::Valida(c) = c else { panic!("cabecera no válida: {c:?}") };
    assert_eq!(c.titulo, "Cómo «reiniciar» ea-mcp: \"así\"");
    assert_eq!((c.autor.as_str(), c.region.as_str()), ("claude-code", "claude-code"));
    assert_eq!(c.origen, "rutina:revision-diaria");
    assert_eq!(c.promocion, "ninguna");
    assert_eq!(c.tags, tags);
    assert_eq!(cuerpo, "cuerpo\ncon varias líneas\n");
    // Reescribir la cabecera da el mismo texto: el contrato es estable.
    assert_eq!(c.con_cuerpo(cuerpo), texto);
}

#[test]
fn la_escritura_valida_la_cabecera() {
    let e = entorno();
    let mut s = e.store();
    let mut mal = |n: NuevaNota<'_>| e.boveda.escribir_nota(&mut s, "modelo-local", &n, &Limites::AMPLIOS);
    assert!(matches!(mal(nueva("", "c", None)), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(mal(nueva("dos\nlíneas", "c", None)), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(mal(nueva(&"x".repeat(201), "c", None)), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(mal(NuevaNota { origen: "trabajo:a/b", ..nueva("T", "c", None) }), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(mal(NuevaNota { origen: "yo", ..nueva("T", "c", None) }), Err(ErrorBoveda::Cabecera(_))));
    let tags = vec!["con espacios".to_string()];
    assert!(matches!(mal(NuevaNota { tags: &tags, ..nueva("T", "c", None) }), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(mal(nueva("T", "   ", None)), Err(ErrorBoveda::Argumentos(_))));
    assert!(matches!(mal(nueva("T", &"x".repeat(70_000), None)), Err(ErrorBoveda::Argumentos(_))));

    // Cabeceras ajenas o rotas se detectan al leer, sin romper.
    for (texto, esperado) in [
        ("sin cabecera\n", "ausente"),
        ("---\ntitulo: x\n---\ncuerpo\n", "invalida"),
        ("---\ncontrato: 2\n---\n", "invalida"),
    ] {
        let (c, _) = interpretar(texto);
        match (esperado, c) {
            ("ausente", Cabeceras::Ausente) | ("invalida", Cabeceras::Invalida(_)) => {}
            (_, otra) => panic!("{texto:?} -> {otra:?}"),
        }
    }
    // Una región que no es de su autor viola el contrato.
    let t = "---\ncontrato: 1\ntitulo: \"x\"\nautor: claude-code\nregion: modelo-local\ncreada: 2026-01-01T00:00:00Z\nactualizada: 2026-01-01T00:00:00Z\norigen: \"manual\"\npromocion: ninguna\n---\n";
    assert!(matches!(interpretar(t).0, Cabeceras::Invalida(_)));
}

#[test]
fn crear_no_pisa_y_sobrescribir_conserva_la_fecha_de_creacion() {
    let e = entorno();
    let mut s = e.store();
    let a = e.boveda.escribir_nota(&mut s, "modelo-local", &nueva("T", "uno", Some("x.md")), &Limites::AMPLIOS).unwrap();
    assert!(a.creada);
    let r = e.boveda.escribir_nota(&mut s, "modelo-local", &nueva("T", "dos", Some("x.md")), &Limites::AMPLIOS);
    assert!(matches!(r, Err(ErrorBoveda::YaExiste(_))));
    let antes = e.boveda.leer(&a.ruta, 0, &Limites::AMPLIOS).unwrap().cabecera.unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let b = e
        .boveda
        .escribir_nota(&mut s, "modelo-local", &NuevaNota { sobrescribir: true, ..nueva("T", "dos", Some("x.md")) }, &Limites::AMPLIOS)
        .unwrap();
    assert!(!b.creada);
    let despues = e.boveda.leer(&a.ruta, 0, &Limites::AMPLIOS).unwrap();
    assert_eq!(despues.contenido, "dos\n");
    let c = despues.cabecera.unwrap();
    assert_eq!(c.creada, antes.creada);
    assert_ne!(c.actualizada, antes.actualizada);
    // Sin ruta, dos notas con el mismo título no se pisan.
    let n1 = e.escribir("claude-code", "Igual", "1");
    let n2 = e.escribir("claude-code", "Igual", "2");
    assert_ne!(n1.ruta, n2.ruta);
    assert_eq!(n2.ruta, "claude-code/igual-2.md");
}

// --- serialización ---------------------------------------------------------

#[test]
fn las_escrituras_concurrentes_se_serializan() {
    let e = entorno();
    const HILOS: usize = 8;
    const POR_HILO: usize = 6;
    let barrera = Arc::new(Barrier::new(HILOS));
    let mut hilos = Vec::new();
    for h in 0..HILOS {
        let boveda = e.boveda.clone();
        let db = e.db.clone();
        let barrera = barrera.clone();
        hilos.push(std::thread::spawn(move || {
            // Cada hilo tiene su propia conexión, como procesos distintos.
            let mut s = Store::open_at(&db).unwrap();
            barrera.wait();
            for i in 0..POR_HILO {
                let autor = if h % 2 == 0 { "modelo-local" } else { "claude-code" };
                let cuerpo = format!("hilo {h} nota {i}\n").repeat(200);
                boveda
                    .escribir_nota(&mut s, autor, &nueva(&format!("Nota {h}-{i}"), &cuerpo, None), &Limites::AMPLIOS)
                    .unwrap();
                // Todos pelean además por una misma nota compartida: exactamente uno la crea.
                let _ = boveda.escribir_nota(
                    &mut s,
                    "modelo-local",
                    &nueva("Disputada", &format!("de {h}"), Some("disputada.md")),
                    &Limites::AMPLIOS,
                );
            }
        }));
    }
    for h in hilos {
        h.join().unwrap();
    }

    let escrituras = e.eventos("boveda_escritura");
    // 48 notas + exactamente 1 creación de la disputada.
    assert_eq!(escrituras.len(), HILOS * POR_HILO + 1);
    // Todas las notas están enteras: cabecera válida y cuerpo completo.
    let mut n = 0;
    for region in ["modelo-local", "claude-code"] {
        for f in std::fs::read_dir(e.boveda.raiz().join(region)).unwrap() {
            let f = f.unwrap();
            let nombre = f.file_name().to_string_lossy().into_owned();
            assert!(!nombre.starts_with('.'), "queda un temporal: {nombre}");
            let texto = std::fs::read_to_string(f.path()).unwrap();
            let (c, cuerpo) = interpretar(&texto);
            assert!(matches!(c, Cabeceras::Valida(_)), "{nombre}");
            if nombre != "disputada.md" {
                assert_eq!(cuerpo.lines().count(), 200, "{nombre}");
            }
            n += 1;
        }
    }
    assert_eq!(n, HILOS * POR_HILO + 1);
    // Los eventos están en el orden en que se escribió: ids estrictamente crecientes.
    assert!(escrituras.windows(2).all(|w| w[0].id > w[1].id), "list_events devuelve del más reciente al más antiguo");
}

#[test]
fn un_escritor_ocupado_bloquea_a_los_demas_y_los_temporales_no_quedan() {
    let e = entorno();
    e.escribir("modelo-local", "Primera", "x");
    // Otro "proceso" (otra descripción de fichero) retiene el candado.
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(e.boveda.raiz().join(".ea-escritor.lock"))
        .unwrap();
    f.lock().unwrap();

    let rapida = e.boveda.clone().con_espera(Duration::from_millis(150));
    let mut s = e.store();
    let r = rapida.escribir_nota(&mut s, "modelo-local", &nueva("Segunda", "y", None), &Limites::AMPLIOS);
    assert!(matches!(r, Err(ErrorBoveda::Ocupada)), "{r:?}");
    assert!(!e.boveda.raiz().join("modelo-local/segunda.md").exists());

    // Al soltarlo, escribe.
    drop(f);
    rapida.escribir_nota(&mut s, "modelo-local", &nueva("Segunda", "y", None), &Limites::AMPLIOS).unwrap();
    let ocultos: Vec<_> = std::fs::read_dir(e.boveda.raiz().join("modelo-local"))
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(ocultos.is_empty());
}

// --- eventos ----------------------------------------------------------------

#[test]
fn cada_escritura_deja_su_evento_en_el_diario() {
    let e = entorno();
    let n = e.escribir("modelo-local", "Con evento", "hola");
    let ev = e.eventos("boveda_escritura");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].actor, "modelo-local");
    let d: serde_json::Value = serde_json::from_str(ev[0].detail.as_ref().unwrap()).unwrap();
    assert_eq!(d["ruta"], n.ruta);
    assert_eq!(d["modo"], "creada");
    assert_eq!(d["huella"], n.huella);
    // Un intento rechazado no escribe ni deja evento de escritura.
    let mut s = e.store();
    let _ = e.boveda.escribir_nota(&mut s, "orquestador", &nueva("T", "c", None), &Limites::AMPLIOS);
    assert_eq!(e.eventos("boveda_escritura").len(), 1);
}

// --- promoción ---------------------------------------------------------------

#[test]
fn la_promocion_copia_la_nota_a_la_compartida_y_deja_evento() {
    let e = entorno();
    let tags = vec!["util".to_string()];
    let mut s = e.store();
    let n = e
        .boveda
        .escribir_nota(
            &mut s,
            "claude-code",
            &NuevaNota { tags: &tags, origen: "trabajo:m-7", ..nueva("Hallazgo", "El puerto es 8765.", Some("servicios/mcp.md")) },
            &Limites::AMPLIOS,
        )
        .unwrap();
    let plan = e.boveda.planear_promocion(&n.ruta, None).unwrap();
    assert_eq!(plan.destino, "compartida/servicios/mcp.md");
    assert!(!e.boveda.raiz().join("compartida").exists(), "planear no escribe");

    e.boveda.promover(&mut s, &n.ruta, None, &plan.huella, "usuario", "terminal").unwrap();

    // La copia: región compartida, mismo autor y cuerpo, rastro de la promoción.
    let copia = e.boveda.leer("compartida/servicios/mcp.md", 0, &Limites::AMPLIOS).unwrap();
    let c = copia.cabecera.unwrap();
    assert_eq!((c.region.as_str(), c.autor.as_str(), c.promocion.as_str()), ("compartida", "claude-code", "promovida"));
    assert_eq!(c.promovida_de.as_deref(), Some("claude-code/servicios/mcp.md"));
    assert_eq!(c.promovida_por.as_deref(), Some("usuario"));
    assert_eq!(c.origen, "trabajo:m-7");
    assert_eq!(copia.contenido, "El puerto es 8765.\n");
    // El original se conserva, marcado.
    let orig = e.boveda.leer(&n.ruta, 0, &Limites::AMPLIOS).unwrap();
    assert_eq!(orig.contenido, "El puerto es 8765.\n");
    let o = orig.cabecera.unwrap();
    assert_eq!((o.promocion.as_str(), o.promovida_a.as_deref()), ("promovida", Some("compartida/servicios/mcp.md")));
    // Diario.
    let ev = e.eventos("boveda_promocion");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].actor, "usuario");
    let d: serde_json::Value = serde_json::from_str(ev[0].detail.as_ref().unwrap()).unwrap();
    assert_eq!((d["origen"].as_str(), d["destino"].as_str()), (Some("claude-code/servicios/mcp.md"), Some("compartida/servicios/mcp.md")));
    assert_eq!(d["canal"], "terminal");
    // Y la búsqueda ve ambas, la copia en la región compartida.
    let r = e.boveda.buscar(&mut s, "puerto 8765", Some("compartida"), None, &Limites::AMPLIOS).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].ruta, "compartida/servicios/mcp.md");
    // No se promueve dos veces sobre el mismo destino.
    assert!(matches!(e.boveda.planear_promocion(&n.ruta, None), Err(ErrorBoveda::YaExiste(_))));
}

#[test]
fn la_promocion_falla_si_la_nota_cambio_desde_la_simulacion() {
    let e = entorno();
    let mut s = e.store();
    let n = e.escribir("modelo-local", "Cambiante", "versión uno");
    let plan = e.boveda.planear_promocion(&n.ruta, None).unwrap();
    e.boveda
        .escribir_nota(
            &mut s,
            "modelo-local",
            &NuevaNota { sobrescribir: true, ..nueva("Cambiante", "versión dos", Some("cambiante.md")) },
            &Limites::AMPLIOS,
        )
        .unwrap();
    let r = e.boveda.promover(&mut s, &n.ruta, None, &plan.huella, "usuario", "web");
    assert!(matches!(r, Err(ErrorBoveda::Cambio(_))), "{r:?}");
    assert!(!e.boveda.raiz().join("compartida/cambiante.md").exists());
    assert!(e.eventos("boveda_promocion").is_empty());
}

#[test]
fn solo_se_promueven_notas_de_la_region_de_un_agente() {
    let e = entorno();
    assert!(e.boveda.planear_promocion("compartida/x.md", None).is_err());
    assert!(e.boveda.planear_promocion("modelo-local/no-existe.md", None).is_err());
    assert!(e.boveda.planear_promocion("../x.md", None).is_err());
    let n = e.escribir("modelo-local", "A", "a");
    assert!(e.boveda.planear_promocion(&n.ruta, Some("../fuera.md")).is_err());
    assert!(e.boveda.planear_promocion(&n.ruta, Some("/abs.md")).is_err());
}

// --- búsqueda ----------------------------------------------------------------

#[test]
fn la_busqueda_respeta_los_limites() {
    let e = entorno();
    let mut s = e.store();
    for i in 0..30 {
        let cuerpo = format!("{} palabra-clave final", "relleno largo ".repeat(400));
        e.boveda
            .escribir_nota(&mut s, "claude-code", &nueva(&format!("Nota {i}"), &cuerpo, None), &Limites::AMPLIOS)
            .unwrap();
    }
    let local = e.boveda.buscar(&mut s, "relleno", None, Some(100), &Limites::LOCAL).unwrap();
    assert_eq!(local.len(), Limites::LOCAL.max_resultados);
    let amplia = e.boveda.buscar(&mut s, "relleno", None, Some(100), &Limites::AMPLIOS).unwrap();
    assert_eq!(amplia.len(), Limites::AMPLIOS.max_resultados);
    let pocas = e.boveda.buscar(&mut s, "relleno", None, Some(2), &Limites::AMPLIOS).unwrap();
    assert_eq!(pocas.len(), 2);
    for r in &local {
        assert!(r.fragmento.chars().count() <= Limites::LOCAL.max_fragmento + 1, "{}", r.fragmento.len());
    }
    // Una lectura devuelve un trozo y dice cómo seguir.
    let l = e.boveda.leer(&local[0].ruta, 0, &Limites::LOCAL).unwrap();
    assert_eq!(l.contenido.chars().count(), Limites::LOCAL.max_lectura);
    assert!(l.truncado);
    let sig = e.boveda.leer(&local[0].ruta, l.siguiente_desde.unwrap(), &Limites::LOCAL).unwrap();
    assert_eq!(sig.desde, Limites::LOCAL.max_lectura);
    // Y una escritura mayor que la cota del modelo local se rechaza.
    let grande = "x".repeat(Limites::LOCAL.max_escritura + 1);
    assert!(e.boveda.escribir_nota(&mut s, "modelo-local", &nueva("G", &grande, None), &Limites::LOCAL).is_err());
}

#[test]
fn la_busqueda_ignora_acentos_y_sintaxis_de_operadores() {
    let e = entorno();
    let mut s = e.store();
    e.escribir("modelo-local", "Café y reuniones", "La reunión del lunes es a las nueve.");
    assert_eq!(e.boveda.buscar(&mut s, "cafe", None, None, &Limites::LOCAL).unwrap().len(), 1);
    assert_eq!(e.boveda.buscar(&mut s, "REUNION lunes", None, None, &Limites::LOCAL).unwrap().len(), 1);
    // Texto con sintaxis FTS5 o comillas no rompe ni se interpreta.
    for raro in ["\"", "a OR b", "NEAR(", "col:valor", "*", "reunión \" AND (", "-lunes"] {
        let _ = e.boveda.buscar(&mut s, raro, None, None, &Limites::LOCAL);
    }
    assert!(e.boveda.buscar(&mut s, "!!!", None, None, &Limites::LOCAL).is_err());
    assert!(e.boveda.buscar(&mut s, "x", Some("inventada"), None, &Limites::LOCAL).is_err());
}

#[test]
fn la_sincronizacion_ve_ediciones_externas_y_ignora_lo_que_no_es_de_las_regiones() {
    let e = entorno();
    let mut s = e.store();
    let raiz = e.boveda.raiz();
    std::fs::create_dir_all(raiz.join("compartida")).unwrap();
    std::fs::create_dir_all(raiz.join("privado")).unwrap();
    // Nota del usuario sin cabecera, en la compartida; y una fuera de las regiones.
    std::fs::write(raiz.join("compartida/del-usuario.md"), "# Recetas\nlentejas con chorizo\n").unwrap();
    std::fs::write(raiz.join("privado/diario.md"), "lentejas secretas\n").unwrap();
    std::fs::write(raiz.join("compartida/.oculta.md"), "lentejas ocultas\n").unwrap();

    let r = e.boveda.buscar(&mut s, "lentejas", None, None, &Limites::AMPLIOS).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].titulo, "Recetas");
    assert!(e.boveda.leer("privado/diario.md", 0, &Limites::AMPLIOS).is_err());

    // Edición externa y borrado externo se reflejan.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(raiz.join("compartida/del-usuario.md"), "# Recetas\ngarbanzos\n").unwrap();
    assert!(e.boveda.buscar(&mut s, "lentejas", None, None, &Limites::AMPLIOS).unwrap().is_empty());
    assert_eq!(e.boveda.buscar(&mut s, "garbanzos", None, None, &Limites::AMPLIOS).unwrap().len(), 1);
    std::fs::remove_file(raiz.join("compartida/del-usuario.md")).unwrap();
    assert!(e.boveda.buscar(&mut s, "garbanzos", None, None, &Limites::AMPLIOS).unwrap().is_empty());
}
