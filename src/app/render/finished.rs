//! Finished dialog rendering.
//!
//! This module contains the rendering logic for the flow completion modal overlay.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

impl App {
    /// Renders the finished dialog as a centered overlay.
    pub(crate) fn render_finished_dialog(&self, frame: &mut Frame) {
        let area = frame.area();

        // Calculate centered popup dimensions
        let popup_width = 52u16;
        let popup_height = 10u16;
        let x = area.width.saturating_sub(popup_width) / 2;
        let y = area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        // Clear background
        frame.render_widget(Clear, popup_area);

        let content_lines = vec![
            Line::from(Span::styled("Flow Complete", self.theme.header_style())),
            Line::from(Span::styled(
                "No more tasks to process.",
                self.theme.muted_style(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Enter] ", self.theme.highlight_style()),
                Span::styled(
                    "Start new session (clears task history)",
                    self.theme.normal_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("[q] ", self.theme.highlight_style()),
                Span::styled("Quit", self.theme.normal_style()),
            ]),
        ];

        let block = Block::default()
            .title(" Finished ")
            .title_style(self.theme.header_style())
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        let paragraph = Paragraph::new(content_lines)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(paragraph, popup_area);
    }
}
