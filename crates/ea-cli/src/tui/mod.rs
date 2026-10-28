mod agentes_view;
mod boveda_view;
mod diario_view;
mod orquestador_view;
mod propuestas_view;
mod rutinas_view;
mod servicios_view;

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ea_store::{EstadoPropuesta, Event as EaEvent, EventFilter, Propuesta, PropuestaFilter, ResumenAgente, ResultadoBusqueda, Store};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Tabs};
use ratatui::{Frame, Terminal};


/// Resumen de una rutina para la pestaña `Rutinas`: solo lo que hace falta
/// para listar.
pub struct RutinaResumen {
    pub slug: String,
    pub version: i64,
    pub ultima_ejecucion: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Orquestador,
    Diario,
    Servicios,
    Agentes,
    Rutinas,
    Propuestas,
    Boveda,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Orquestador,
        Tab::Diario,
        Tab::Servicios,
        Tab::Agentes,
        Tab::Rutinas,
        Tab::Propuestas,
        Tab::Boveda,
    ];

    fn title(self) -> &'static str {
        match self {
            Tab::Orquestador => "Orquestador",
            Tab::Diario => "Diario",
            Tab::Servicios => "Servicios",
            Tab::Agentes => "Agentes",
            Tab::Rutinas => "Rutinas",
            Tab::Propuestas => "Propuestas",
            Tab::Boveda => "Bóveda",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap()
    }
}

struct AppData {
    events: Vec<EaEvent>,
    rutinas: Vec<RutinaResumen>,
    propuestas: Vec<Propuesta>,
    servicios: Vec<ea_acciones::salud::Servicio>,
    orquestador: String,
    agentes: Vec<ResumenAgente>,
    ajustes: ea_ajustes::Ajustes,
    boveda_raiz: String,
    boveda: Vec<ResultadoBusqueda>,
    orquestador_bus: orquestador_view::DatosOrquestador,
}

/// Lanza la interfaz de terminal interactiva. Es una fotografía de los datos
/// al arrancar (§13.1: las interfaces de terminal no son servicios, son
/// clientes bajo demanda) — para ver cambios se vuelve a lanzar `ea tui`.
pub fn run(store: &mut Store) -> Result<()> {
    let boveda = ea_boveda::Boveda::configurada();
    let notas = boveda
        .listar(store, None, Some(50), &ea_boveda::Limites { max_resultados: 50, ..ea_boveda::Limites::AMPLIOS })
        .unwrap_or_default();
    let store: &Store = store;
    let mut rutinas = Vec::new();
    // Solo las que están en uso: las retiradas, con `ea rutina listar --retiradas`.
    let dir = ea_ajustes::cargar().orquestador.rutinas_dir;
    let en_uso = ea_bus::rutinas_por_ubicacion(store.listar_rutinas().unwrap_or_default(), &dir, false);
    let mut ultimas = crate::commands::rutinas::ultimas_ejecuciones(&en_uso);
    for (r, _) in en_uso {
        let ultima_ejecucion = ultimas.remove(&r.slug).map(|(_, e)| format!("{} ({})", e.actualizado_en, e.estado.as_str()));
        rutinas.push(RutinaResumen { slug: r.slug, version: r.version, ultima_ejecucion });
    }

    // Misma consulta a `systemd --user` que hace `ea-web`; es asíncrona, así
    // que se resuelve con un runtime mínimo antes de entrar en modo raw.
    let servicios = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(ea_acciones::salud::servicios(true));

    let data = AppData {
        servicios,
        orquestador: crate::commands::status::estado_orquestador(store)?,
        agentes: store.resumen_agentes()?,
        ajustes: ea_ajustes::cargar(),
        events: store.list_events(&EventFilter {
            limit: Some(200),
            ..Default::default()
        })?,
        rutinas,
        orquestador_bus: datos_bus(),
        boveda_raiz: boveda.raiz().display().to_string(),
        boveda: notas,
        propuestas: store.list_propuestas(&PropuestaFilter {
            estado: Some(EstadoPropuesta::Pendiente),
            limit: Some(100),
        })?,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app_loop(&mut terminal, &data);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

/// Colas, semáforos y trabajos del bus; si no se puede leer, vacío.
fn datos_bus() -> orquestador_view::DatosOrquestador {
    match ea_bus::Bus::abrir() {
        Ok(bus) => datos_de(&bus),
        Err(_) => orquestador_view::DatosOrquestador { cola_modelo_local: vec![], cola_claude_code: vec![], semaforos: vec![], trabajos: vec![] },
    }
}

fn datos_de(bus: &ea_bus::Bus) -> orquestador_view::DatosOrquestador {
    use ea_bus::{Rol, EstadoTrabajo};
    let mut d = orquestador_view::DatosOrquestador {
        cola_modelo_local: vec![],
        cola_claude_code: vec![],
        semaforos: vec![],
        trabajos: vec![],
    };
    let titulo = |id: &str| bus.leer_orden(id).map(|o| o.titulo).unwrap_or_else(|_| id.to_string());
    for (agente, destino) in [(Rol::Ejecutor, &mut d.cola_modelo_local), (Rol::Planificador, &mut d.cola_claude_code)] {
        *destino = bus
            .listar_cola(agente)
            .unwrap_or_default()
            .iter()
            .map(|e| format!("{}  ({})", titulo(&e.trabajo), e.motivo))
            .collect();
    }
    d.semaforos = ea_bus::listar_semaforos_en(bus.raiz())
        .into_iter()
        .map(|(_, o)| format!("{} · {} · desde {}", o.agente, titulo(&o.trabajo), o.desde))
        .collect();
    let mut ids = bus.listar_trabajos().unwrap_or_default();
    ids.reverse();
    for r in ids.iter().filter_map(|id| bus.resumen_trabajo(id)) {
        let avance = if r.fases_total > 0 {
            format!("fase {}/{}", r.fase_actual.unwrap_or(1).min(r.fases_total as u32), r.fases_total)
        } else if r.iteracion > 0 {
            format!("iteración {}", r.iteracion)
        } else {
            String::new()
        };
        let mut informe = String::new();
        match &r.informe {
            None => informe.push_str("Todavía no hay informe.\n"),
            Some(i) => {
                informe.push_str(&format!(
                    "resultado: {} · obra_completa: {} · {} fases hechas · {}s\n",
                    i.resultado.as_str(),
                    i.obra_completa,
                    i.fases_hechas,
                    i.duracion_s
                ));
                if let Some(m) = &i.motivo {
                    informe.push_str(&format!("motivo: {m}\n"));
                }
                for f in &i.fases {
                    informe.push_str(&format!("fase {} [{}] {}\n", f.numero, f.resultado.as_str(), f.titulo));
                }
                if !i.evidencia.trim().is_empty() {
                    informe.push_str(&format!("\nevidencia:\n{}\n", i.evidencia.trim_end()));
                }
                if let Some(s) = &i.diff_stat {
                    informe.push_str(&format!("\ndiff stat:\n{}\n", s.trim_end()));
                }
            }
        }
        if r.estado == EstadoTrabajo::EnEspera {
            informe.push_str("\nPaquete de corrección en espera: confírmalo con `ea trabajo confirmar` o desde ea-web.\n");
        }
        d.trabajos.push(orquestador_view::TrabajoResumen { id: r.id, modo: r.modo.as_str(), titulo: r.titulo, estado: r.estado, avance, informe });
    }
    d
}

/// Una pantalla. Buena parte de lo que pintan las pestañas lo escriben los
/// agentes (títulos, informes, evidencias, descripciones) y el backend manda
/// cada celda tal cual a la terminal: al final ninguna lleva caracteres de
/// control, así que tampoco secuencias ANSI u OSC (como `commands::limpio`).
fn pintar(frame: &mut Frame, data: &AppData, current: Tab, sel: usize, detalle: bool) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)])
        .split(frame.area());

    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("ea — ecosistema de asistente (q para salir, ←/→ para cambiar de pestaña)"))
        .select(current.index())
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[0]);

    match current {
        Tab::Orquestador => orquestador_view::render(frame, chunks[1], &data.orquestador_bus, sel, detalle),
        Tab::Diario => diario_view::render(frame, chunks[1], &data.events),
        Tab::Servicios => servicios_view::render(frame, chunks[1], &data.servicios, &data.orquestador),
        Tab::Agentes => agentes_view::render(frame, chunks[1], &data.agentes, &data.ajustes),
        Tab::Rutinas => rutinas_view::render(frame, chunks[1], &data.rutinas),
        Tab::Propuestas => propuestas_view::render(frame, chunks[1], &data.propuestas),
        Tab::Boveda => boveda_view::render(frame, chunks[1], &data.boveda_raiz, &data.boveda),
    }

    for celda in frame.buffer_mut().content.iter_mut() {
        if celda.symbol().contains(char::is_control) {
            let limpio: String = celda.symbol().chars().filter(|c| !c.is_control()).collect();
            celda.set_symbol(if limpio.is_empty() { " " } else { &limpio });
        }
    }
}

fn app_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, data: &AppData) -> Result<()> {
    let mut current = Tab::Orquestador;
    let mut sel = 0usize;
    let mut detalle = false;

    loop {
        terminal.draw(|frame| pintar(frame, data, current, sel, detalle))?;

        if event::poll(std::time::Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down if current == Tab::Orquestador => {
                    sel = (sel + 1).min(data.orquestador_bus.trabajos.len().saturating_sub(1));
                }
                KeyCode::Up if current == Tab::Orquestador => sel = sel.saturating_sub(1),
                KeyCode::Enter if current == Tab::Orquestador => detalle = !detalle,
                KeyCode::Right | KeyCode::Tab => {
                    let idx = (current.index() + 1) % Tab::ALL.len();
                    current = Tab::ALL[idx];
                }
                KeyCode::Left | KeyCode::BackTab => {
                    let idx = (current.index() + Tab::ALL.len() - 1) % Tab::ALL.len();
                    current = Tab::ALL[idx];
                }
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use ratatui::backend::TestBackend;

    /// Lo que la pestaña Orquestador saca de un bus conocido: las colas, los
    /// trabajos (el más reciente primero) con su avance y el texto del informe.
    #[test]
    fn los_datos_del_bus_dicen_lo_mismo_que_siempre() {
        use ea_bus::{Evento, Fase, Orden};
        let dir = tempfile::tempdir().unwrap();
        let bus = ea_bus::Bus::abrir_en(&dir.path().join("estado")).unwrap();
        let wt = dir.path().to_string_lossy().into_owned();
        let ids = ["20260101T000001-00000001", "20260101T000002-00000002", "20260101T000003-00000003"];
        let fase = |n: u32| Fase { numero: n, titulo: format!("fase {n}"), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
        bus.crear_trabajo(Orden { id: ids[0].into(), ..Orden::mision("misión 1", &wt, "o", "true", "prueba") }).unwrap();
        bus.crear_trabajo(Orden { id: ids[1].into(), ..Orden::obra("una obra", &wt, "o", vec![fase(1), fase(2), fase(3)], None, "prueba") }).unwrap();
        bus.crear_trabajo(Orden { id: ids[2].into(), ..Orden::mision("misión 3", &wt, "o", "true", "prueba") }).unwrap();
        for id in [ids[0], ids[1]] {
            bus.retirar_de_colas(id).unwrap();
            bus.aplicar(id, Evento::Tomar).unwrap();
        }
        let informe = serde_json::from_value(serde_json::json!({
            "resultado": "bloqueado", "motivo": "no pasa", "fases_hechas": 1, "duracion_s": 30, "evidencia": "salida\n",
            "diff_stat": " a.txt | 1 +\n", "fases": [{ "numero": 1, "titulo": "fase 1", "resultado": "hecha" }, { "numero": 2, "titulo": "fase 2", "resultado": "bloqueada" }],
        }))
        .unwrap();
        bus.aplicar(ids[1], Evento::Entregar(informe)).unwrap();
        bus.aplicar(ids[0], Evento::Entregar(serde_json::from_value(serde_json::json!({ "resultado": "terminado", "obra_completa": true, "duracion_s": 5 })).unwrap())).unwrap();
        bus.aplicar(ids[0], Evento::Cerrar).unwrap();

        let d = datos_de(&bus);
        assert_eq!(d.cola_modelo_local, ["misión 3  (orden nueva)"]);
        assert_eq!(d.cola_claude_code, ["una obra  (bloqueado)"]);
        assert!(d.semaforos.is_empty());
        let t: Vec<_> = d.trabajos.iter().map(|t| (t.id.as_str(), t.modo, t.titulo.as_str(), t.estado.as_str(), t.avance.as_str(), t.informe.as_str())).collect();
        assert_eq!(
            t,
            [
                (ids[2], "mision", "misión 3", "en_cola", "", "Todavía no hay informe.\n"),
                (
                    ids[1],
                    "obra",
                    "una obra",
                    "en_revision",
                    "fase 2/3",
                    "resultado: bloqueado · obra_completa: false · 1 fases hechas · 30s\nmotivo: no pasa\nfase 1 [hecha] fase 1\nfase 2 [bloqueada] fase 2\n\nevidencia:\nsalida\n\ndiff stat:\n a.txt | 1 +\n",
                ),
                (ids[0], "mision", "misión 1", "cerrado", "", "resultado: terminado · obra_completa: true · 0 fases hechas · 5s\n"),
            ]
        );
    }

    /// S-19 en la TUI: el informe de un trabajo (evidencia, motivo) y su título
    /// llevan un OSC 52 y un borrado de pantalla; ninguna celda los conserva.
    #[test]
    fn ninguna_celda_lleva_caracteres_de_control() {
        let d = tempfile::tempdir().unwrap();
        let sec = "\x1b]52;c;ZWNobyBob2xh\x07\x1b[2J\u{9b}0m\r";
        let trabajo = orquestador_view::TrabajoResumen {
            id: "20260101T000000-00000001".into(),
            modo: "mision",
            titulo: format!("titulo{sec}"),
            estado: ea_bus::EstadoTrabajo::EnRevision,
            avance: String::new(),
            informe: format!("motivo: m{sec}\n\nevidencia:\nsalida{sec}\tfin\n"),
        };
        let data = AppData {
            events: vec![],
            rutinas: vec![],
            propuestas: vec![],
            servicios: vec![],
            orquestador: String::new(),
            agentes: vec![],
            ajustes: ea_ajustes::cargar_de(&ea_ajustes::Fuentes::aislada(d.path())).ajustes,
            boveda_raiz: String::new(),
            boveda: vec![],
            orquestador_bus: orquestador_view::DatosOrquestador {
                cola_modelo_local: vec![format!("cola{sec}")],
                cola_claude_code: vec![],
                semaforos: vec![],
                trabajos: vec![trabajo],
            },
        };
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        for detalle in [false, true] {
            let buf = t.draw(|f| pintar(f, &data, Tab::Orquestador, 0, detalle)).unwrap().buffer.clone();
            let texto: String = buf.content.iter().map(|c| c.symbol()).collect();
            assert!(!texto.contains(char::is_control), "detalle={detalle}: {texto:?}");
            // Cada carácter de control ocupaba su celda: queda en blanco y el resto se ve.
            assert!(texto.contains("cola ]52;c;ZWNobyBob2xh  [2J 0m"), "{texto}");
            if detalle {
                assert!(texto.contains("salida ]52;c;ZWNobyBob2xh  [2J 0m"), "{texto}");
            }
        }
    }
}
