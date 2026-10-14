use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use super::RutinaResumen;

pub fn render(frame: &mut Frame, area: Rect, rutinas: &[RutinaResumen]) {
    let header = Row::new(vec!["Rutina", "Última ejecución"]);
    let rows: Vec<Row> = rutinas
        .iter()
        .map(|r| {
            Row::new(vec![
                r.slug.clone(),
                r.ultima_ejecucion.clone().unwrap_or_else(|| "nunca".to_string()),
            ])
        })
        .collect();
    let widths = [Constraint::Percentage(60), Constraint::Length(22)];
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Rutinas ({})", rutinas.len())),
    );
    frame.render_widget(table, area);
}
