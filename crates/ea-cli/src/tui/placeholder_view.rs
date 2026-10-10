use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Pestaña de un apartado del sistema (§12.3) que todavía no tiene datos
/// reales. Se muestra, no se oculta: fija la navegación completa desde ya.
pub fn render(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let block = Block::default().borders(Borders::ALL).title(title.to_string());
    let paragraph = Paragraph::new(message.to_string())
        .alignment(Alignment::Center)
        .block(block);
    frame.render_widget(paragraph, area);
}
