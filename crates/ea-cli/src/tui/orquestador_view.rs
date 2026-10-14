use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

/// Un trabajo del bus, ya resumido para la pestaña.
pub struct TrabajoResumen {
    pub id: String,
    pub modo: &'static str,
    pub titulo: String,
    pub estado: &'static str,
    /// «fase 2/5», «iteración 3» o vacío.
    pub avance: String,
    /// Resumen del informe (resultado, motivo, evidencia, fases), para Enter.
    pub informe: String,
}

/// Fotografía del bus: colas, semáforos y trabajos (el más reciente primero).
pub struct DatosOrquestador {
    pub cola_modelo_local: Vec<String>,
    pub cola_claude_code: Vec<String>,
    pub semaforos: Vec<String>,
    pub trabajos: Vec<TrabajoResumen>,
}

fn glifo(estado: &str) -> &'static str {
    match estado {
        "en_cola" => "○",
        "ejecutando" => "◐",
        "en_revision" => "◇",
        "en_espera" => "!",
        "cerrado" => "●",
        _ => "⊘",
    }
}

fn lista<'a>(titulo: String, filas: &'a [String]) -> List<'a> {
    let items: Vec<ListItem> = if filas.is_empty() {
        vec![ListItem::new("(vacía)")]
    } else {
        filas.iter().map(|f| ListItem::new(f.as_str())).collect()
    };
    List::new(items).block(Block::default().borders(Borders::ALL).title(titulo))
}

pub fn render(frame: &mut Frame, area: Rect, d: &DatosOrquestador, sel: usize, detalle: bool) {
    let filas = Layout::vertical([Constraint::Length(7), Constraint::Min(0)]).split(area);
    let arriba = Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(33), Constraint::Percentage(33)]).split(filas[0]);
    frame.render_widget(lista(format!("Cola modelo-local ({})", d.cola_modelo_local.len()), &d.cola_modelo_local), arriba[0]);
    frame.render_widget(lista(format!("Cola claude-code ({})", d.cola_claude_code.len()), &d.cola_claude_code), arriba[1]);
    let libre = [String::from("libre")];
    let sem = if d.semaforos.is_empty() { &libre[..] } else { &d.semaforos[..] };
    frame.render_widget(lista("Semáforo del worktree".into(), sem), arriba[2]);

    let abajo = if detalle && !d.trabajos.is_empty() {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(filas[1])
    } else {
        Layout::horizontal([Constraint::Percentage(100)]).split(filas[1])
    };
    let items: Vec<ListItem> = d
        .trabajos
        .iter()
        .map(|t| ListItem::new(format!("{} {:<11} {:<6} {}  {}  {}", glifo(t.estado), t.estado, t.modo, &t.id[9.min(t.id.len())..], t.titulo, t.avance)))
        .collect();
    let mut estado = ListState::default();
    if !d.trabajos.is_empty() {
        estado.select(Some(sel.min(d.trabajos.len() - 1)));
    }
    let widget = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Trabajos ({}) — ↑/↓ y Enter para ver el informe", d.trabajos.len())))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(widget, abajo[0], &mut estado);

    if detalle {
        if let Some(t) = d.trabajos.get(sel) {
            let texto: Vec<Line> = std::iter::once(Line::from(format!("{} · {} · {}", t.id, t.modo, t.estado)))
                .chain(t.informe.lines().map(|l| Line::from(l.to_string())))
                .collect();
            let p = Paragraph::new(texto)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(t.titulo.clone()));
            frame.render_widget(p, abajo[1]);
        }
    }
}
