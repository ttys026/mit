use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::tui::shared::centered_rect;

pub(crate) fn draw_bootstrap_splash(frame: &mut ratatui::Frame<'_>, spinner_index: usize) {
    let popup = centered_rect(60, 35, frame.area());
    let spinner = ["|", "/", "-", "\\"][spinner_index % 4];
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
        Paragraph::new(format!(
            "mit\n\n{spinner} Loading devices...\n\nStarting background sync\n\nPress q to quit"
        ))
        .block(Block::default().borders(crate::tui::shared::top_bottom_borders()))
        .wrap(Wrap { trim: true }),
        popup,
    );
}
