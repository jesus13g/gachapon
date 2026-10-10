use ea_store::Task;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

pub fn render(frame: &mut Frame, area: Rect, tasks: &[Task]) {
    let header = Row::new(vec!["ID", "Título", "Asignada a", "Estado", "Origen"]);
    let rows: Vec<Row> = tasks
        .iter()
        .map(|t| {
            Row::new(vec![
                t.id.to_string(),
                t.title.clone(),
                t.assigned_to.clone(),
                t.status.as_str().to_string(),
                t.origin.as_str().to_string(),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(6),
        Constraint::Percentage(40),
        Constraint::Length(16),
        Constraint::Length(14),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Tareas ({})", tasks.len())),
    );
    frame.render_widget(table, area);
}
