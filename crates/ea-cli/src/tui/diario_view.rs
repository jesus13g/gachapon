use ea_store::Event;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use crate::commands::fmt_ts;

pub fn render(frame: &mut Frame, area: Rect, events: &[Event]) {
    let header = Row::new(vec!["Cuándo", "Actor", "Acción", "Trabajo", "Resultado"]);
    let rows: Vec<Row> = events
        .iter()
        .map(|e| {
            Row::new(vec![
                fmt_ts(e.ts),
                e.actor.clone(),
                e.action.clone(),
                e.trabajo.clone().unwrap_or_default(),
                e.outcome
                    .map(|o| o.as_str().to_string())
                    .unwrap_or_default(),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(21),
        Constraint::Length(14),
        Constraint::Length(20),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Diario de eventos ({})", events.len())),
    );
    frame.render_widget(table, area);
}
