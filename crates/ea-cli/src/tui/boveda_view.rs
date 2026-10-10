use ea_store::ResultadoBusqueda;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

pub fn render(frame: &mut Frame, area: Rect, raiz: &str, notas: &[ResultadoBusqueda]) {
    let header = Row::new(vec!["Región", "Ruta", "Título", "Autor", "Actualizada", "Promoción"]);
    let rows: Vec<Row> = notas
        .iter()
        .map(|n| {
            Row::new(vec![
                n.region.clone(),
                n.ruta.clone(),
                n.titulo.clone(),
                n.autor.clone().unwrap_or_else(|| "-".into()),
                n.actualizada.clone().unwrap_or_else(|| "-".into()),
                n.promocion.clone().unwrap_or_else(|| "-".into()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(13),
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Length(13),
        Constraint::Length(21),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths).header(header).block(
        Block::default().borders(Borders::ALL).title(format!(
            "Bóveda ({} recientes) en {raiz} — promueve con 'ea boveda promover <ruta>'",
            notas.len()
        )),
    );
    frame.render_widget(table, area);
}
