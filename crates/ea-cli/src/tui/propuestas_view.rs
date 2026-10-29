use ea_store::Propuesta;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};

use crate::commands::fmt_ts;

pub fn render(frame: &mut Frame, area: Rect, propuestas: &[Propuesta]) {
    let header = Row::new(vec!["ID", "Estado", "Acción [riesgo]", "Creada", "Descripción"]);
    let rows: Vec<Row> = propuestas
        .iter()
        .map(|p| {
            Row::new(vec![
                p.id.to_string(),
                p.estado.as_str().to_string(),
                p.accion.as_ref().map(|a| format!("{} [{}]", a.herramienta, a.riesgo)).unwrap_or_else(|| "(texto)".into()),
                fmt_ts(p.creado_en),
                p.descripcion.clone(),
            ])
        })
        .collect();
    let widths = [Constraint::Length(6), Constraint::Length(10), Constraint::Length(36), Constraint::Length(22), Constraint::Percentage(50)];
    let table = Table::new(rows, widths).header(header).block(
        Block::default().borders(Borders::ALL).title(format!("Propuestas ({}) — decide con 'ea propuestas decidir' (aceptar ejecuta)", propuestas.len())),
    );
    frame.render_widget(table, area);
}
