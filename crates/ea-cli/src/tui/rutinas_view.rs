use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use super::RutinaResumen;

pub fn render(frame: &mut Frame, area: Rect, rutinas: &[RutinaResumen]) {
    let header = Row::new(vec!["Rutina", "Descripción", "Última ejecución"]);
    let rows: Vec<Row> = rutinas
        .iter()
        .map(|r| {
            Row::new(vec![
                r.slug.clone(),
                r.descripcion.clone(),
                r.ultima_ejecucion.clone().unwrap_or_else(|| "nunca".to_string()),
            ])
        })
        .collect();
    let widths = [Constraint::Length(20), Constraint::Percentage(55), Constraint::Length(22)];
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Rutinas ({})", rutinas.len())),
    );
    frame.render_widget(table, area);
}
