use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};

use super::RutinaResumen;

pub fn render(frame: &mut Frame, area: Rect, rutinas: &[RutinaResumen]) {
    let header = Row::new(vec!["Rutina", "Versión", "Última ejecución"]);
    let rows: Vec<Row> = rutinas
        .iter()
        .map(|r| Row::new(vec![r.slug.clone(), format!("v{}", r.version), r.ultima_ejecucion.clone().unwrap_or_else(|| "nunca".to_string())]))
        .collect();
    let widths = [Constraint::Percentage(60), Constraint::Length(9), Constraint::Length(22)];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!("Rutinas registradas ({}) — ea rutina listar", rutinas.len())));
    frame.render_widget(table, area);
}
