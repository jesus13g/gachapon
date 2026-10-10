mod agentes_view;
mod boveda_view;
mod diario_view;
mod propuestas_view;
mod rutinas_view;
mod servicios_view;
mod tareas_view;

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ea_orchestrator::rutina;
use ea_store::{EstadoPropuesta, Event as EaEvent, EventFilter, Propuesta, PropuestaFilter, ResumenAgente, ResultadoBusqueda, Store, Task, TaskFilter};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Tabs};
use ratatui::Terminal;

use crate::commands::fmt_ts;

/// Resumen de una rutina para la pestaña `Rutinas`: no es el tipo de
/// dominio completo de `ea-orchestrator::rutina::Rutina` (eso viviría en
/// disco, no en `ea-store`) — solo lo que hace falta para listar.
pub struct RutinaResumen {
    pub slug: String,
    pub descripcion: String,
    pub ultima_ejecucion: Option<String>,
}

/// Servicio residente para la pestaña `Servicios` (copia plana de
/// `ea_web::estado::Servicio`, que lleva un `&'static str`).
pub struct ServicioResumen {
    pub nombre: &'static str,
    pub activo: String,
    pub activo_s: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Tareas,
    Diario,
    Servicios,
    Agentes,
    Rutinas,
    Propuestas,
    Boveda,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Tareas,
        Tab::Diario,
        Tab::Servicios,
        Tab::Agentes,
        Tab::Rutinas,
        Tab::Propuestas,
        Tab::Boveda,
    ];

    fn title(self) -> &'static str {
        match self {
            Tab::Tareas => "Tareas",
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
    tasks: Vec<Task>,
    events: Vec<EaEvent>,
    rutinas: Vec<RutinaResumen>,
    propuestas: Vec<Propuesta>,
    servicios: Vec<ServicioResumen>,
    orquestador: String,
    agentes: Vec<ResumenAgente>,
    boveda_raiz: String,
    boveda: Vec<ResultadoBusqueda>,
}

/// Lanza la interfaz de terminal interactiva. Es una fotografía de los datos
/// al arrancar (§13.1: las interfaces de terminal no son servicios, son
/// clientes bajo demanda) — para ver cambios se vuelve a lanzar `ea tui`.
pub fn run(store: &mut Store) -> Result<()> {
    let boveda = ea_acciones::Boveda::configurada();
    let notas = boveda
        .listar(store, None, Some(50), &ea_boveda::Limites { max_resultados: 50, ..ea_boveda::Limites::AMPLIOS })
        .unwrap_or_default();
    let store: &Store = store;
    let rutinas_dir = ea_orchestrator::config::load().rutinas_dir;
    let mut rutinas = Vec::new();
    for slug in rutina::listar_slugs(&rutinas_dir).unwrap_or_default() {
        let descripcion = rutina::cargar(&rutinas_dir, &slug)
            .map(|r| r.descripcion)
            .unwrap_or_else(|e| format!("(inválida: {e})"));
        let ultima_ejecucion = store.last_successful_run(&slug).ok().flatten().map(|r| fmt_ts(r.started_at));
        rutinas.push(RutinaResumen { slug, descripcion, ultima_ejecucion });
    }

    // Misma consulta a `systemd --user` que hace `ea-web`; es asíncrona, así
    // que se resuelve con un runtime mínimo antes de entrar en modo raw.
    let servicios = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(ea_web::estado::servicios(true))
        .into_iter()
        .map(|s| ServicioResumen { nombre: s.nombre, activo: s.activo, activo_s: s.activo_s })
        .collect();

    let data = AppData {
        servicios,
        orquestador: crate::commands::status::estado_orquestador(store)?,
        agentes: store.resumen_agentes()?,
        tasks: store.list_tasks(&TaskFilter {
            limit: Some(100),
            ..Default::default()
        })?,
        events: store.list_events(&EventFilter {
            limit: Some(200),
            ..Default::default()
        })?,
        rutinas,
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

fn app_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, data: &AppData) -> Result<()> {
    let mut current = Tab::Tareas;

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)])
                .split(frame.area());

            let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
            let tabs = Tabs::new(titles)
                .block(Block::default().borders(Borders::ALL).title("ea — ecosistema de asistente (q para salir, ←/→ para cambiar de pestaña)"))
                .select(current.index())
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(tabs, chunks[0]);

            match current {
                Tab::Tareas => tareas_view::render(frame, chunks[1], &data.tasks),
                Tab::Diario => diario_view::render(frame, chunks[1], &data.events),
                Tab::Servicios => servicios_view::render(frame, chunks[1], &data.servicios, &data.orquestador),
                Tab::Agentes => agentes_view::render(frame, chunks[1], &data.agentes),
                Tab::Rutinas => rutinas_view::render(frame, chunks[1], &data.rutinas),
                Tab::Propuestas => propuestas_view::render(frame, chunks[1], &data.propuestas),
                Tab::Boveda => boveda_view::render(frame, chunks[1], &data.boveda_raiz, &data.boveda),
            }
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
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
        }
    }

    Ok(())
}
