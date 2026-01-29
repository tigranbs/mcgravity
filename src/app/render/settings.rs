//! Settings panel rendering.
//!
//! This module contains the rendering logic for the settings modal overlay.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, SettingsItem};

impl App {
    /// Renders the settings panel as a centered overlay.
    pub(crate) fn render_settings(&self, frame: &mut Frame) {
        let area = frame.area();

        // Calculate how many error lines we need (0, 1, or 2)
        let planning_unavailable = !self
            .settings
            .is_model_available(self.settings.planning_model);
        let execution_unavailable = !self
            .settings
            .is_model_available(self.settings.execution_model);
        let error_line_count = u16::from(planning_unavailable) + u16::from(execution_unavailable);

        // Calculate centered popup dimensions
        // Base height: 14 lines + error lines as needed
        let popup_width = 52u16;
        let popup_height = 14u16 + error_line_count;
        let x = area.width.saturating_sub(popup_width) / 2;
        let y = area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        // Clear background
        frame.render_widget(Clear, popup_area);

        // Build settings content
        let items = SettingsItem::all();
        let mut content_lines = Vec::new();

        // Header
        content_lines.push(Line::from(Span::styled(
            "McGravity Settings",
            self.theme.header_style(),
        )));
        content_lines.push(Line::from(Span::styled(
            "Configure AI model preferences.",
            self.theme.muted_style(),
        )));
        content_lines.push(Line::from(""));

        // Settings items
        for (i, item) in items.iter().enumerate() {
            let is_selected = i == self.settings.selected_index;
            let prefix = if is_selected { "› " } else { "  " };

            let value = match item {
                SettingsItem::PlanningModel => self.settings.planning_model.name(),
                SettingsItem::ExecutionModel => self.settings.execution_model.name(),
                SettingsItem::EnterBehavior => self.settings.enter_behavior.name(),
                SettingsItem::MaxIterations => self.settings.max_iterations.name(),
            };

            let line = if is_selected {
                Line::from(vec![
                    Span::styled(prefix, self.theme.highlight_style()),
                    Span::styled(
                        format!("{:<18}", item.label()),
                        self.theme.highlight_style(),
                    ),
                    Span::styled(format!("[{value}]"), self.theme.highlight_style()),
                ])
            } else {
                Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(format!("{:<18}", item.label()), self.theme.normal_style()),
                    Span::styled(format!("[{value}]"), self.theme.muted_style()),
                ])
            };
            content_lines.push(line);

            // Add error line below model selections if CLI is unavailable
            match item {
                SettingsItem::PlanningModel if planning_unavailable => {
                    content_lines.push(self.render_unavailable_error(self.settings.planning_model));
                }
                SettingsItem::ExecutionModel if execution_unavailable => {
                    content_lines
                        .push(self.render_unavailable_error(self.settings.execution_model));
                }
                _ => {}
            }
        }

        // Spacing before footer
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(""));

        // Footer hints
        content_lines.push(Line::from(vec![
            Span::styled("[↑/↓] ", self.theme.highlight_style()),
            Span::styled("Navigate  ", self.theme.muted_style()),
            Span::styled("[Enter] ", self.theme.highlight_style()),
            Span::styled("Change  ", self.theme.muted_style()),
            Span::styled("[Esc] ", self.theme.highlight_style()),
            Span::styled("Close", self.theme.muted_style()),
        ]));

        // Render the popup
        let block = Block::default()
            .title(" Settings ")
            .title_style(self.theme.header_style())
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        let paragraph = Paragraph::new(content_lines)
            .block(block)
            .alignment(ratatui::layout::Alignment::Left);

        frame.render_widget(paragraph, popup_area);
    }
}
