//! Pruebas de la edición a mano del usuario: leer entera, guardar con
//! control de versión y firma inmutable, crear solo en la compartida,
//! renombrar con los enlaces al día, papelera y carpetas. Todo sobre
//! directorios y bases temporales.

use ea_boveda::{Boveda, ErrorBoveda, Limites, NuevaNota};
use ea_store::{EventFilter, Store};
use tempfile::TempDir;

struct Entorno {
    _dir: TempDir,
    boveda: Boveda,
    store: Store,
}

fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_at(&dir.path().join("t.db")).unwrap();
    let boveda = Boveda::en(dir.path().join("boveda"));
    std::fs::create_dir_all(boveda.raiz()).unwrap();
    Entorno { _dir: dir, boveda, store }
}

impl Entorno {
    fn agente(&mut self, autor: &str, ruta: &str, cuerpo: &str) -> String {
        let titulo = ruta.rsplit('/').next().unwrap();
        self.boveda
            .escribir_nota(
                &mut self.store,
                autor,
                &NuevaNota { titulo, cuerpo, ruta: Some(ruta), tags: &[], origen: "manual", sobrescribir: false },
                &Limites::AMPLIOS,
            )
            .unwrap()
            .ruta
    }
    fn usuario(&mut self, ruta: &str, cuerpo: &str) -> String {
        self.boveda.crear_usuario(&mut self.store, ruta, cuerpo, "usuario").unwrap().ruta
    }
    fn texto(&self, ruta: &str) -> String {
        std::fs::read_to_string(self.boveda.raiz().join(ruta)).unwrap()
    }
    fn version(&self, ruta: &str) -> String {
        self.boveda.leer_completa(ruta).unwrap().version
    }
    fn eventos(&self, accion: &str) -> Vec<ea_store::Event> {
        self.store.list_events(&EventFilter { limit: Some(1000), ..Default::default() }).unwrap().into_iter().filter(|e| e.action == accion).collect()
    }
    fn rutas(&self) -> Vec<String> {
        self.store.boveda_todas().unwrap().into_iter().map(|r| r.ruta).collect()
    }
}

#[test]
fn lee_la_nota_entera_con_su_version() {
    let mut e = entorno();
    let r = e.agente("claude-code", "ideas/x", "Cuerpo de x.");
    let n = e.boveda.leer_completa(&r).unwrap();
    assert!(n.contenido.starts_with("---\n") && n.contenido.contains("Cuerpo de x."));
    assert_eq!(n.version, ea_boveda::huella(&n.contenido));
    assert_eq!(n.cabecera.unwrap().autor, "claude-code");
    assert!(n.modificada.is_some());
    assert!(matches!(e.boveda.leer_completa("compartida/no.md"), Err(ErrorBoveda::NoExiste(_))));
}

#[test]
fn guardar_cambia_el_disco_reindexa_y_deja_evento() {
    let mut e = entorno();
    let r = e.agente("modelo-local", "receta", "Lentejas.");
    let n = e.boveda.leer_completa(&r).unwrap();
    let nuevo = n.contenido.replace("Lentejas.", "Garbanzos con espinacas.");
    let g = e.boveda.guardar_usuario(&mut e.store, &r, &nuevo, &n.version, "usuario").unwrap();
    assert!(e.texto(&r).contains("Garbanzos"));
    assert_ne!(g.version, n.version);
    // `actualizada` se pone al día; la firma sigue igual.
    let c = g.cabecera.unwrap();
    assert_eq!((c.autor.as_str(), c.region.as_str()), ("modelo-local", "modelo-local"));
    let hits = e.boveda.buscar(&mut e.store, "garbanzos", None, None, &Limites::AMPLIOS).unwrap();
    assert_eq!(hits.len(), 1);
    let ev = e.eventos("boveda_edicion");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].actor, "usuario");
}

#[test]
fn guardar_con_una_version_vieja_falla_y_no_toca_el_disco() {
    let mut e = entorno();
    let r = e.agente("claude-code", "x", "uno");
    let antes = e.texto(&r);
    let err = e.boveda.guardar_usuario(&mut e.store, &r, "otra cosa", "0000", "usuario").unwrap_err();
    assert!(matches!(err, ErrorBoveda::Cambio(_)), "{err}");
    assert_eq!(e.texto(&r), antes);
}

#[test]
fn la_firma_no_se_cambia_ni_se_falsifica() {
    let mut e = entorno();
    let r = e.agente("claude-code", "x", "uno");
    let n = e.boveda.leer_completa(&r).unwrap();
    // Cambiar el autor (y con él la región) o quitar la cabecera: no.
    let falso = n.contenido.replace("autor: claude-code", "autor: modelo-local").replace("region: claude-code", "region: modelo-local");
    let err = e.boveda.guardar_usuario(&mut e.store, &r, &falso, &n.version, "usuario").unwrap_err();
    assert!(matches!(err, ErrorBoveda::Cabecera(_)), "{err}");
    let creada = n.contenido.lines().find(|l| l.starts_with("creada:")).unwrap().to_string();
    let otra = n.contenido.replace(&creada, "creada: 2001-01-01T00:00:00Z");
    assert!(matches!(e.boveda.guardar_usuario(&mut e.store, &r, &otra, &n.version, "usuario"), Err(ErrorBoveda::Cabecera(_))));
    assert!(matches!(e.boveda.guardar_usuario(&mut e.store, &r, "sin cabecera", &n.version, "usuario"), Err(ErrorBoveda::Cabecera(_))));
    // El título y las etiquetas sí.
    let bien = n.contenido.replace("titulo: \"x\"", "titulo: \"Otro título\"");
    let g = e.boveda.guardar_usuario(&mut e.store, &r, &bien, &n.version, "usuario").unwrap();
    assert_eq!(g.cabecera.unwrap().titulo, "Otro título");

    // A una nota sin cabecera no se le pone una firmada por un agente.
    std::fs::write(e.boveda.raiz().join("claude-code/suelta.md"), "# Suelta\n").unwrap();
    let v = e.version("claude-code/suelta.md");
    let firmada = "---\ncontrato: 1\ntitulo: \"S\"\nautor: claude-code\nregion: claude-code\ncreada: 2026-01-01T00:00:00Z\nactualizada: 2026-01-01T00:00:00Z\norigen: \"manual\"\npromocion: ninguna\n---\n\nS\n";
    let err = e.boveda.guardar_usuario(&mut e.store, "claude-code/suelta.md", firmada, &v, "usuario").unwrap_err();
    assert!(matches!(err, ErrorBoveda::Prohibida(_)), "{err}");
    // Pero se edita sin cabecera.
    e.boveda.guardar_usuario(&mut e.store, "claude-code/suelta.md", "# Suelta\n\nmás\n", &v, "usuario").unwrap();
}

#[test]
fn los_agentes_no_editan_a_mano() {
    let mut e = entorno();
    let r = e.agente("claude-code", "x", "uno");
    let v = e.version(&r);
    for quien in ["claude-code", "modelo-local", "orquestador", ""] {
        let err = e.boveda.guardar_usuario(&mut e.store, &r, "x", &v, quien).unwrap_err();
        assert!(matches!(err, ErrorBoveda::Prohibida(_)), "{quien}: {err}");
        assert!(matches!(e.boveda.crear_usuario(&mut e.store, "compartida/n.md", "", quien), Err(ErrorBoveda::Prohibida(_))));
        assert!(matches!(e.boveda.a_papelera(&mut e.store, &r, &v, quien), Err(ErrorBoveda::Prohibida(_))));
        assert!(matches!(e.boveda.renombrar(&mut e.store, &r, "claude-code/y.md", &v, quien), Err(ErrorBoveda::Prohibida(_))));
        assert!(matches!(e.boveda.crear_carpeta(&mut e.store, "compartida/c", quien), Err(ErrorBoveda::Prohibida(_))));
    }
}

#[test]
fn crear_solo_en_la_compartida_y_sin_pisar() {
    let mut e = entorno();
    let n = e.boveda.crear_usuario(&mut e.store, "compartida/ideas/nueva", "", "usuario").unwrap();
    assert_eq!(n.ruta, "compartida/ideas/nueva.md");
    let c = n.cabecera.unwrap();
    assert_eq!((c.autor.as_str(), c.region.as_str(), c.titulo.as_str()), ("usuario", "compartida", "nueva"));
    assert!(n.contenido.contains("# nueva"));
    assert_eq!(e.rutas(), vec!["compartida/ideas/nueva.md".to_string()]);
    assert_eq!(e.eventos("boveda_creacion").len(), 1);
    assert!(matches!(e.boveda.crear_usuario(&mut e.store, "compartida/ideas/nueva.md", "", "usuario"), Err(ErrorBoveda::YaExiste(_))));
    for region in ["claude-code/x.md", "modelo-local/x.md"] {
        assert!(matches!(e.boveda.crear_usuario(&mut e.store, region, "", "usuario"), Err(ErrorBoveda::Prohibida(_))));
    }
    assert!(matches!(e.boveda.crear_usuario(&mut e.store, "compartida/../x.md", "", "usuario"), Err(ErrorBoveda::Ruta(_))));
    let cab = "---\ncontrato: 1\n---\n";
    assert!(matches!(e.boveda.crear_usuario(&mut e.store, "compartida/y.md", cab, "usuario"), Err(ErrorBoveda::Argumentos(_))));
}

#[test]
fn renombrar_mueve_y_reescribe_los_enlaces() {
    let mut e = entorno();
    let x = e.usuario("compartida/x", "Soy x.");
    e.usuario("compartida/otra", "[[x]] [[compartida/x|alias]] [[x#ancla]] [v](x.md) [w](./x.md#a) [[otra]]");
    e.agente("claude-code", "nota", "Desde fuera: [[x]] y [z](../compartida/x.md)");
    let v = e.version(&x);
    let r = e.boveda.renombrar(&mut e.store, &x, "compartida/sub/y", &v, "usuario").unwrap();
    assert_eq!(r.nota.ruta, "compartida/sub/y.md");
    assert!(!e.boveda.raiz().join(&x).exists());
    let mut act = r.actualizadas.clone();
    act.sort();
    assert_eq!(act, vec!["claude-code/nota.md".to_string(), "compartida/otra.md".to_string()]);
    assert!(r.ambiguas.is_empty());
    let otra = e.texto("compartida/otra.md");
    assert!(otra.contains("[[y]] [[sub/y|alias]] [[y#ancla]] [v](sub/y.md) [w](./sub/y.md#a) [[otra]]"), "{otra}");
    let fuera = e.texto("claude-code/nota.md");
    assert!(fuera.contains("[[y]] y [z](../compartida/sub/y.md)"), "{fuera}");
    // Índice y diario al día.
    let rutas = e.rutas();
    assert!(rutas.contains(&"compartida/sub/y.md".to_string()) && !rutas.contains(&x));
    assert_eq!(e.eventos("boveda_renombrado").len(), 1);
}

#[test]
fn renombrar_avisa_de_los_ambiguos_y_no_pisa_ni_cambia_de_region() {
    let mut e = entorno();
    let a = e.usuario("compartida/a/x", "a");
    e.usuario("compartida/b/x", "b");
    e.usuario("compartida/c", "[[x]]");
    let v = e.version(&a);
    let r = e.boveda.renombrar(&mut e.store, &a, "compartida/a/z", &v, "usuario").unwrap();
    assert_eq!(r.ambiguas, vec!["compartida/c.md".to_string()]);
    assert!(e.texto("compartida/c.md").contains("[[x]]"));

    let z = r.nota.ruta;
    let v = e.version(&z);
    assert!(matches!(e.boveda.renombrar(&mut e.store, &z, "compartida/b/x", &v, "usuario"), Err(ErrorBoveda::YaExiste(_))));
    assert!(e.texto("compartida/b/x.md").contains('b'));
    assert!(matches!(e.boveda.renombrar(&mut e.store, &z, "claude-code/z", &v, "usuario"), Err(ErrorBoveda::Prohibida(_))));
    assert!(matches!(e.boveda.renombrar(&mut e.store, &z, "compartida/w", "vieja", "usuario"), Err(ErrorBoveda::Cambio(_))));
    assert!(matches!(e.boveda.renombrar(&mut e.store, &z, "compartida/.oculta", &v, "usuario"), Err(ErrorBoveda::Ruta(_))));
}

#[test]
fn la_papelera_guarda_y_restaura() {
    let mut e = entorno();
    let r = e.agente("modelo-local", "dir/x", "contenido valioso");
    let v = e.version(&r);
    assert!(matches!(e.boveda.a_papelera(&mut e.store, &r, "vieja", "usuario"), Err(ErrorBoveda::Cambio(_))));
    let p = e.boveda.a_papelera(&mut e.store, &r, &v, "usuario").unwrap();
    assert!(!e.boveda.raiz().join(&r).exists());
    assert!(e.boveda.raiz().join(".papelera").join(&p.id).join(&r).exists());
    assert!(e.rutas().is_empty());
    assert_eq!(e.boveda.papelera().unwrap(), vec![p.clone()]);
    assert!(p.borrada.ends_with('Z'));
    assert_eq!(e.eventos("boveda_papelera").len(), 1);
    // Lo de la papelera no se indexa ni se ve en el árbol.
    let arbol = e.boveda.arbol(&mut e.store).unwrap();
    assert!(arbol.notas.is_empty());
    assert_eq!(arbol.en_papelera, 1);

    // Si en su sitio hay otra, no se restaura.
    e.agente("modelo-local", "dir/x", "otra");
    assert!(matches!(e.boveda.restaurar(&mut e.store, &p.id, "usuario"), Err(ErrorBoveda::YaExiste(_))));
    let v2 = e.version(&r);
    e.boveda.a_papelera(&mut e.store, &r, &v2, "usuario").unwrap();
    let n = e.boveda.restaurar(&mut e.store, &p.id, "usuario").unwrap();
    assert!(n.contenido.contains("contenido valioso"));
    assert_eq!(e.rutas(), vec![r.clone()]);
    assert!(!e.boveda.raiz().join(".papelera").join(&p.id).exists());
    assert_eq!(e.boveda.papelera().unwrap().len(), 1);
    assert_eq!(e.eventos("boveda_restauracion").len(), 1);
    for malo in ["../x", ".oculto", "no-existe", ""] {
        assert!(e.boveda.restaurar(&mut e.store, malo, "usuario").is_err(), "{malo}");
    }
}

#[cfg(unix)]
#[test]
fn rutas_malas_y_enlaces_simbolicos_se_rechazan() {
    let mut e = entorno();
    let r = e.usuario("compartida/x", "x");
    let v = e.version(&r);
    for mala in ["../x.md", "/etc/passwd", "compartida/../../x.md", ".papelera/x.md", "otra/x.md", "compartida/.x.md"] {
        assert!(e.boveda.leer_completa(mala).is_err(), "{mala}");
        assert!(e.boveda.guardar_usuario(&mut e.store, mala, "x", &v, "usuario").is_err(), "{mala}");
        assert!(e.boveda.a_papelera(&mut e.store, mala, &v, "usuario").is_err(), "{mala}");
    }
    let fuera = e._dir.path().join("fuera.md");
    std::fs::write(&fuera, "secreto").unwrap();
    std::os::unix::fs::symlink(&fuera, e.boveda.raiz().join("compartida/enlace.md")).unwrap();
    assert!(matches!(e.boveda.leer_completa("compartida/enlace.md"), Err(ErrorBoveda::FueraDeLaBoveda(_))));
    std::os::unix::fs::symlink(e._dir.path(), e.boveda.raiz().join("compartida/dir")).unwrap();
    assert!(e.boveda.crear_usuario(&mut e.store, "compartida/dir/nueva", "", "usuario").is_err());
    assert!(!e._dir.path().join("nueva.md").exists());
    assert!(e.boveda.renombrar(&mut e.store, &r, "compartida/dir/x", &v, "usuario").is_err());
    assert!(e.boveda.raiz().join(&r).exists());
}

#[test]
fn el_arbol_incluye_carpetas_vacias_y_se_crean_carpetas() {
    let mut e = entorno();
    e.agente("claude-code", "a/b/x", "x");
    assert_eq!(e.boveda.crear_carpeta(&mut e.store, "modelo-local/vacia/", "usuario").unwrap(), "modelo-local/vacia");
    assert!(matches!(e.boveda.crear_carpeta(&mut e.store, "modelo-local/vacia", "usuario"), Err(ErrorBoveda::YaExiste(_))));
    assert!(matches!(e.boveda.crear_carpeta(&mut e.store, "compartida", "usuario"), Err(ErrorBoveda::Argumentos(_))));
    assert!(e.boveda.crear_carpeta(&mut e.store, "compartida/x.md", "usuario").is_err());
    assert!(e.boveda.crear_carpeta(&mut e.store, "compartida/.oculta", "usuario").is_err());
    std::fs::create_dir_all(e.boveda.raiz().join("compartida/.obsidian")).unwrap();
    let a = e.boveda.arbol(&mut e.store).unwrap();
    assert_eq!(a.carpetas, vec!["claude-code", "claude-code/a", "claude-code/a/b", "compartida", "modelo-local", "modelo-local/vacia"]);
    assert_eq!(a.notas.len(), 1);
    assert_eq!(e.eventos("boveda_carpeta").len(), 1);
}

// --- fase 2: carpetas y vaciar la papelera ---

#[test]
fn renombrar_una_carpeta_mueve_sus_notas_y_pone_los_enlaces_al_dia() {
    let mut e = entorno();
    e.usuario("compartida/a/x", "Soy x. [[y]]");
    e.usuario("compartida/a/sub/y", "Soy y. [x](../x.md)");
    e.boveda.crear_carpeta(&mut e.store, "compartida/a/vacia", "usuario").unwrap();
    e.usuario("compartida/fuera", "[[x]] [v](a/sub/y.md)");
    let r = e.boveda.renombrar_carpeta(&mut e.store, "compartida/a/", "compartida/b/c", "usuario").unwrap();
    assert_eq!(r.carpeta, "compartida/b/c");
    assert_eq!(r.movidas.len(), 2);
    assert_eq!(r.actualizadas, vec!["compartida/fuera.md".to_string()]);
    assert!(!e.boveda.raiz().join("compartida/a").exists());
    assert!(e.boveda.raiz().join("compartida/b/c/vacia").is_dir());
    assert!(e.texto("compartida/b/c/sub/y.md").contains("[x](../x.md)"));
    let fuera = e.texto("compartida/fuera.md");
    assert!(fuera.contains("[[x]]") && fuera.contains("[v](b/c/sub/y.md)"), "{fuera}");
    let mut rutas = e.rutas();
    rutas.sort();
    assert_eq!(rutas, vec!["compartida/b/c/sub/y.md", "compartida/b/c/x.md", "compartida/fuera.md"]);
    assert_eq!(e.eventos("boveda_carpeta_renombrada").len(), 1);
    assert_eq!(e.eventos("boveda_renombrado").len(), 2);
}

#[test]
fn renombrar_una_carpeta_rechaza_lo_dudoso() {
    let mut e = entorno();
    e.usuario("compartida/a/x", "x");
    e.usuario("compartida/b/x", "otra");
    let ren = |e: &mut Entorno, de: &str, a: &str| e.boveda.renombrar_carpeta(&mut e.store, de, a, "usuario");
    assert!(matches!(ren(&mut e, "compartida", "compartida/z"), Err(ErrorBoveda::Argumentos(_))));
    assert!(matches!(ren(&mut e, "compartida/a", "claude-code/a"), Err(ErrorBoveda::Prohibida(_))));
    assert!(matches!(ren(&mut e, "compartida/a", "compartida/b"), Err(ErrorBoveda::YaExiste(_))));
    assert!(matches!(ren(&mut e, "compartida/a", "compartida/a/dentro"), Err(ErrorBoveda::Argumentos(_))));
    assert!(matches!(ren(&mut e, "compartida/a", "compartida/a"), Err(ErrorBoveda::Argumentos(_))));
    assert!(matches!(ren(&mut e, "compartida/no", "compartida/z"), Err(ErrorBoveda::NoExiste(_))));
    assert!(ren(&mut e, "compartida/a", "compartida/.oculta").is_err());
    // Un fichero que no es nota: no se mueve nada.
    std::fs::write(e.boveda.raiz().join("compartida/a/foto.png"), b"x").unwrap();
    assert!(matches!(ren(&mut e, "compartida/a", "compartida/z"), Err(ErrorBoveda::Argumentos(_))));
    assert!(e.boveda.raiz().join("compartida/a/x.md").exists());
    for quien in ["modelo-local", "claude-code", "orquestador", ""] {
        assert!(e.boveda.renombrar_carpeta(&mut e.store, "compartida/b", "compartida/z", quien).is_err(), "{quien}");
    }
    assert!(e.boveda.raiz().join("compartida/b/x.md").exists());
}

#[test]
fn borrar_una_carpeta_la_manda_a_la_papelera_y_se_restaura_entera() {
    let mut e = entorno();
    e.usuario("compartida/d/x", "uno");
    e.usuario("compartida/d/sub/y", "dos");
    e.usuario("compartida/otra", "fuera");
    let p = e.boveda.carpeta_a_papelera(&mut e.store, "compartida/d", "usuario").unwrap();
    let id = p.id.clone().unwrap();
    assert_eq!(p.notas, vec!["compartida/d/sub/y.md".to_string(), "compartida/d/x.md".to_string()]);
    assert!(!e.boveda.raiz().join("compartida/d").exists());
    assert_eq!(e.rutas(), vec!["compartida/otra.md".to_string()]);
    assert_eq!(e.boveda.papelera().unwrap().len(), 2);
    assert_eq!(e.eventos("boveda_papelera_carpeta").len(), 1);

    // Si una de las rutas está ocupada, no se mueve ninguna.
    e.usuario("compartida/d/x", "otra nota");
    assert!(matches!(e.boveda.restaurar(&mut e.store, &id, "usuario"), Err(ErrorBoveda::YaExiste(_))));
    assert!(!e.boveda.raiz().join("compartida/d/sub/y.md").exists());
    let v = e.version("compartida/d/x.md");
    e.boveda.a_papelera(&mut e.store, "compartida/d/x.md", &v, "usuario").unwrap();

    e.boveda.restaurar(&mut e.store, &id, "usuario").unwrap();
    assert!(e.texto("compartida/d/x.md").contains("uno") && e.texto("compartida/d/sub/y.md").contains("dos"));
    let mut rutas = e.rutas();
    rutas.sort();
    assert_eq!(rutas, vec!["compartida/d/sub/y.md", "compartida/d/x.md", "compartida/otra.md"]);
    assert_eq!(e.eventos("boveda_restauracion").len(), 2);
    assert!(!e.boveda.raiz().join(".papelera").join(&id).exists());
}

#[test]
fn borrar_una_carpeta_vacia_o_una_region_o_con_ficheros_ajenos() {
    let mut e = entorno();
    e.boveda.crear_carpeta(&mut e.store, "compartida/v/w", "usuario").unwrap();
    let p = e.boveda.carpeta_a_papelera(&mut e.store, "compartida/v", "usuario").unwrap();
    assert_eq!(p.id, None);
    assert!(!e.boveda.raiz().join("compartida/v").exists());
    assert_eq!(e.eventos("boveda_carpeta_eliminada").len(), 1);
    assert!(e.boveda.papelera().unwrap().is_empty());
    assert!(matches!(e.boveda.carpeta_a_papelera(&mut e.store, "compartida", "usuario"), Err(ErrorBoveda::Argumentos(_))));
    assert!(matches!(e.boveda.carpeta_a_papelera(&mut e.store, "compartida/no", "usuario"), Err(ErrorBoveda::NoExiste(_))));
    e.usuario("compartida/f/x", "x");
    std::fs::write(e.boveda.raiz().join("compartida/f/foto.png"), b"x").unwrap();
    assert!(matches!(e.boveda.carpeta_a_papelera(&mut e.store, "compartida/f", "usuario"), Err(ErrorBoveda::Argumentos(_))));
    assert!(e.boveda.raiz().join("compartida/f/x.md").exists());
    for quien in ["modelo-local", "claude-code", "orquestador", ""] {
        assert!(e.boveda.carpeta_a_papelera(&mut e.store, "compartida/f", quien).is_err(), "{quien}");
    }
}

#[test]
fn vaciar_la_papelera_borra_para_siempre_solo_ella_y_solo_el_usuario() {
    let mut e = entorno();
    let r = e.usuario("compartida/a/x", "uno");
    e.usuario("compartida/a/y", "dos");
    e.usuario("compartida/queda", "queda");
    assert_eq!(e.boveda.simular_vaciar_papelera().unwrap().entradas, 0);
    let v = e.version(&r);
    e.boveda.a_papelera(&mut e.store, &r, &v, "usuario").unwrap();
    e.boveda.carpeta_a_papelera(&mut e.store, "compartida/a", "usuario").unwrap();
    let sim = e.boveda.simular_vaciar_papelera().unwrap();
    assert_eq!((sim.entradas, sim.notas), (2, 2));
    assert!(sim.bytes > 0);
    for quien in ["modelo-local", "claude-code", "orquestador", ""] {
        assert!(e.boveda.vaciar_papelera(&mut e.store, quien).is_err(), "{quien}");
    }
    assert_eq!(e.boveda.papelera().unwrap().len(), 2);
    let hecho = e.boveda.vaciar_papelera(&mut e.store, "usuario").unwrap();
    assert_eq!(hecho, sim);
    assert!(e.boveda.papelera().unwrap().is_empty());
    assert_eq!(e.eventos("boveda_papelera_vaciada").len(), 1);
    assert!(e.texto("compartida/queda.md").contains("queda"));
    // Vacía otra vez: no hace nada y no deja otro evento.
    assert_eq!(e.boveda.vaciar_papelera(&mut e.store, "usuario").unwrap().entradas, 0);
    assert_eq!(e.eventos("boveda_papelera_vaciada").len(), 1);
}
