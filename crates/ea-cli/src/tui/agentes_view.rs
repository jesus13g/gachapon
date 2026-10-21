use ea_ajustes::{Ajustes, Rol};
use ea_store::ResumenAgente;
use ratatui::layout::{Constraint, Rect};
use ratatui::widgets::{Block, Borders, Row, Table};
use ratatui::Frame;

use crate::commands::fmt_ts;

/// Qué agente ocupa cada rol, con qué modelo, y qué hizo el rol por última
/// vez según el diario (nunca se le pregunta a él, §12.2).
pub fn render(frame: &mut Frame, area: Rect, agentes: &[ResumenAgente], ajustes: &Ajustes) {
    let rows: Vec<Row> = agentes
        .iter()
        .map(|a| {
            let (rol, agente, modelo) = match Rol::desde(a.nombre) {
                Some(r) => {
                    let (id, _) = ajustes.agente_de(r);
                    let nombre = ea_ajustes::ficha(id).map_or(id, |f| f.nombre);
                    let modelo = ajustes.modelo_de(id).map_or("su modelo propio".to_string(), |m| m.nombre.clone());
                    (r.nombre(), nombre.to_string(), modelo)
                }
                None => (a.nombre, String::new(), String::new()),
            };
            let ultima = a
                .ultimo_evento
                .as_ref()
                .map(|e| {
                    let por = a.agente.as_deref().map(|x| format!(" ({x})")).unwrap_or_default();
                    format!("{} — {}{por}", fmt_ts(e.ts), e.action)
                })
                .unwrap_or_else(|| "(sin actividad)".to_string());
            Row::new(vec![rol.to_string(), agente, modelo, ultima])
        })
        .collect();
    let widths = [Constraint::Length(13), Constraint::Length(13), Constraint::Length(18), Constraint::Min(20)];
    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Rol", "Agente", "Modelo", "Última actividad"]))
        .block(Block::default().borders(Borders::ALL).title("Agentes"));
    frame.render_widget(table, area);
}
