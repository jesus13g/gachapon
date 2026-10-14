use ea_store::ResumenAgente;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use crate::commands::fmt_ts;

/// Qué hace cada agente según el diario (nunca se le pregunta
/// a él, §12.2).
pub fn render(frame: &mut Frame, area: Rect, agentes: &[ResumenAgente]) {
    let rows: Vec<Row> = agentes
        .iter()
        .map(|a| {
            Row::new(vec![
                a.nombre.to_string(),
                a.ultimo_evento
                    .as_ref()
                    .map(|e| format!("{} — {}", fmt_ts(e.ts), e.action))
                    .unwrap_or_else(|| "(sin actividad)".to_string()),
            ])
        })
        .collect();
    let widths = [Constraint::Length(16), Constraint::Min(20)];
    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Agente", "Última actividad"]))
        .block(Block::default().borders(Borders::ALL).title("Agentes"));
    frame.render_widget(table, area);
}
