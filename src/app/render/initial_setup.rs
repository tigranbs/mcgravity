//! Initial setup modal rendering.
//!
//! This module contains the rendering logic for the first-run initial setup modal
//! that displays when no settings file exists. The modal allows users to select
//! their default planning and execution models.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, InitialSetupField};
use crate::core::Model;

impl App {
    /// Renders the initial setup modal as a centered overlay.
    ///
    /// This modal is displayed on first run when no `.mcgravity/settings.json` exists.
    /// It prompts the user to select their default planning and execution models,
    /// showing availability indicators and error messages for unavailable CLIs.
    pub(crate) fn render_initial_setup(&self, frame: &mut Frame) {
        let area = frame.area();

        // Calculate centered popup dimensions
        // Taller than settings to fit welcome message and error lines
        let popup_width = 56u16;
        let popup_height = 18u16;
        let x = area.width.saturating_sub(popup_width) / 2;
        let y = area.height.saturating_sub(popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        // Clear background (required for popups per ratatui best practices)
        frame.render_widget(Clear, popup_area);

        // Get initial setup state, falling back to defaults if not present
        let setup_state = self.initial_setup.as_ref();
        let selected_field = setup_state.map_or(InitialSetupField::default(), |s| s.selected_field);
        let planning_model = setup_state.map_or(self.settings.planning_model, |s| s.planning_model);
        let execution_model =
            setup_state.map_or(self.settings.execution_model, |s| s.execution_model);

        // Build content lines
        let mut content_lines = Vec::new();

        // Welcome header
        content_lines.push(Line::from(Span::styled(
            "Welcome to McGravity",
            self.theme.header_style(),
        )));
        content_lines.push(Line::from(Span::styled(
            "Select your default AI CLI tools.",
            self.theme.muted_style(),
        )));
        content_lines.push(Line::from(""));

        // Planning Model selection
        let is_planning_selected = selected_field == InitialSetupField::PlanningModel;
        content_lines.push(self.render_model_field(
            InitialSetupField::PlanningModel.label(),
            planning_model,
            is_planning_selected,
        ));

        // Planning model error line if unavailable
        if !self.settings.is_model_available(planning_model) {
            content_lines.push(self.render_unavailable_error(planning_model));
        }

        content_lines.push(Line::from(""));

        // Execution Model selection
        let is_execution_selected = selected_field == InitialSetupField::ExecutionModel;
        content_lines.push(self.render_model_field(
            InitialSetupField::ExecutionModel.label(),
            execution_model,
            is_execution_selected,
        ));

        // Execution model error line if unavailable
        if !self.settings.is_model_available(execution_model) {
            content_lines.push(self.render_unavailable_error(execution_model));
        }

        // Spacing before footer
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(""));

        // Footer hints
        content_lines.push(Line::from(vec![
            Span::styled("[↑/↓] ", self.theme.highlight_style()),
            Span::styled("Navigate  ", self.theme.muted_style()),
            Span::styled("[Enter] ", self.theme.highlight_style()),
            Span::styled("Change  ", self.theme.muted_style()),
            Span::styled("[C] ", self.theme.highlight_style()),
            Span::styled("Confirm", self.theme.muted_style()),
        ]));

        // Render the popup
        let block = Block::default()
            .title(" Initial Setup ")
            .title_style(self.theme.header_style())
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        let paragraph = Paragraph::new(content_lines)
            .block(block)
            .alignment(ratatui::layout::Alignment::Left);

        frame.render_widget(paragraph, popup_area);
    }

    /// Renders a model selection field line.
    fn render_model_field(&self, label: &str, model: Model, is_selected: bool) -> Line<'static> {
        let prefix = if is_selected { "› " } else { "  " };
        let value = model.name();

        if is_selected {
            Line::from(vec![
                Span::styled(prefix.to_string(), self.theme.highlight_style()),
                Span::styled(format!("{label:<18}"), self.theme.highlight_style()),
                Span::styled(format!("[{value}]"), self.theme.highlight_style()),
            ])
        } else {
            Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled(format!("{label:<18}"), self.theme.normal_style()),
                Span::styled(format!("[{value}]"), self.theme.muted_style()),
            ])
        }
    }

    /// Renders an error line for an unavailable CLI.
    ///
    /// This is used by both the initial setup modal and the settings panel
    /// to display a consistent error message when a CLI tool is unavailable.
    pub(crate) fn render_unavailable_error(&self, model: Model) -> Line<'static> {
        let command = model.command();
        Line::from(vec![
            Span::raw("    "), // Indent to align with model field
            Span::styled(
                format!("⚠ `{command}` is not available or not executable"),
                self.theme.error_style(),
            ),
        ])
    }
}
