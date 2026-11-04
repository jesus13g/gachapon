use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use ea_acciones::salud::Servicio;

/// Servicios residentes (preguntados a `systemd --user` al arrancar la TUI)
/// y salud del orquestador según su último latido (`orquestador.json` del bus).
pub fn render(frame: &mut Frame, area: Rect, servicios: &[Servicio], orquestador: &str) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let cabecera = Paragraph::new(orquestador.to_string()).block(Block::default().borders(Borders::ALL).title("Orquestador"));
    frame.render_widget(cabecera, chunks[0]);

    let rows: Vec<Row> =
        servicios.iter().map(|s| Row::new(vec![s.nombre.to_string(), s.activo.clone(), s.activo_s.map(duracion).unwrap_or_else(|| "-".to_string())])).collect();
    let widths = [Constraint::Length(20), Constraint::Length(14), Constraint::Min(10)];
    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Servicio", "Estado", "Activo desde hace"]))
        .block(Block::default().borders(Borders::ALL).title("Servicios"));
    frame.render_widget(table, chunks[1]);
}

fn duracion(s: i64) -> String {
    match s {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h {} min", s / 3600, s % 3600 / 60),
        s => format!("{} d {} h", s / 86_400, s % 86_400 / 3600),
    }
}
