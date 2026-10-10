mod diario_view;
mod placeholder_view;
mod propuestas_view;
mod rutinas_view;
mod tareas_view;

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ea_orchestrator::rutina;
use ea_store::{EstadoPropuesta, Event as EaEvent, EventFilter, Propuesta, PropuestaFilter, Store, Task, TaskFilter};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Tareas,
    Diario,
    Servicios,
    Agentes,
    Rutinas,
    Propuestas,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Tareas,
        Tab::Diario,
        Tab::Servicios,
        Tab::Agentes,
        Tab::Rutinas,
        Tab::Propuestas,
    ];

    fn title(self) -> &'static str {
        match self {
            Tab::Tareas => "Tareas",
            Tab::Diario => "Diario",
            Tab::Servicios => "Servicios",
            Tab::Agentes => "Agentes",
            Tab::Rutinas => "Rutinas",
            Tab::Propuestas => "Propuestas",
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
}

/// Lanza la interfaz de terminal interactiva. Es una fotografía de los datos
/// al arrancar (§13.1: las interfaces de terminal no son servicios, son
/// clientes bajo demanda) — para ver cambios se vuelve a lanzar `ea tui`.
pub fn run(store: &Store) -> Result<()> {
    let rutinas_dir = ea_orchestrator::config::load().rutinas_dir;
    let mut rutinas = Vec::new();
    for slug in rutina::listar_slugs(&rutinas_dir).unwrap_or_default() {
        let descripcion = rutina::cargar(&rutinas_dir, &slug)
            .map(|r| r.descripcion)
            .unwrap_or_else(|e| format!("(inválida: {e})"));
        let ultima_ejecucion = store.last_successful_run(&slug).ok().flatten().map(|r| fmt_ts(r.started_at));
        rutinas.push(RutinaResumen { slug, descripcion, ultima_ejecucion });
    }

    let data = AppData {
        tasks: store.list_tasks(&TaskFilter {
            limit: Some(100),
            ..Default::default()
        })?,
        events: store.list_events(&EventFilter {
            limit: Some(200),
            ..Default::default()
        })?,
        rutinas,
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
                Tab::Servicios => placeholder_view::render(frame, chunks[1], "Servicios", "no implementado en esta fase (ver fase 13 de la especificación)"),
                Tab::Agentes => placeholder_view::render(frame, chunks[1], "Agentes", "no implementado en esta fase (ver fase 3 de la especificación)"),
                Tab::Rutinas => rutinas_view::render(frame, chunks[1], &data.rutinas),
                Tab::Propuestas => propuestas_view::render(frame, chunks[1], &data.propuestas),
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
