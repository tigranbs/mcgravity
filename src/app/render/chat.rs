//! Chat mode rendering.
//!
//! This module contains all rendering logic for the unified chat interface,
//! including header, output, status, progress, input, footer, and file popup.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, LineGauge, Paragraph},
};
use tui_textarea::TextArea;

use crate::app::{App, wrap_lines_for_display};
use crate::core::FlowPhase;
use crate::tui::widgets::{CommandPopup, FileSuggestionPopup, OutputWidget, StatusIndicatorWidget};

impl App {
    /// Renders the unified chat-like interface.
    ///
    /// This layout combines:
    /// - Header (minimal, 1 line)
    /// - Output area (scrollable CLI output)
    /// - Status indicator (2 lines for phase + operation)
    /// - Progress bar (1 line)
    /// - Text input area (composer)
    /// - Footer with key hints
    ///
    /// Uses the cached layout from `self.layout.chat` which is calculated
    /// once per frame in `update_layout()`.
    pub(crate) fn render_chat(&self, frame: &mut Frame) {
        let layout = self.layout.chat;

        // 1. Header (minimal, single line)
        self.render_chat_header(frame, layout.header);

        // 2. Output area (reuse OutputWidget)
        self.render_chat_output(frame, layout.output);

        // 3. Status indicator (2 lines)
        self.render_chat_status(frame, layout.status);

        // 4. Progress bar (1 line)
        self.render_chat_progress(frame, layout.progress);

        // 5. Text input area (composer)
        let inner_area = self.render_chat_input(frame, layout.input);

        // 6. Footer with key hints
        self.render_chat_footer(frame, layout.footer);

        // Handle cursor positioning and file popup
        self.handle_cursor_and_popup(frame, inner_area);

        // Render command popup if visible
        if self.should_show_command_popup() {
            self.render_command_popup(frame, layout.input);
        }
    }

    /// Renders the chat header (minimal, single line).
    fn render_chat_header(&self, frame: &mut Frame, area: Rect) {
        let header = Line::from(vec![
            Span::styled(" McGravity ", self.theme.header_style()),
            Span::styled("[", self.theme.muted_style()),
            Span::styled(
                self.settings.planning_model.name(),
                self.theme.normal_style(),
            ),
            Span::styled("/", self.theme.muted_style()),
            Span::styled(
                self.settings.execution_model.name(),
                self.theme.normal_style(),
            ),
            Span::styled("]", self.theme.muted_style()),
        ]);
        frame.render_widget(Paragraph::new(header), area);
    }

    /// Renders the chat output area (reuses `OutputWidget`).
    fn render_chat_output(&self, frame: &mut Frame, area: Rect) {
        let title = if self.is_running {
            "Output"
        } else {
            "Output (waiting for input)"
        };

        let output_widget = OutputWidget::with_truncation(
            &self.flow_ui.output,
            self.flow_ui.output_scroll.offset,
            title,
            &self.theme,
            self.flow_ui.output_truncated,
        );
        frame.render_widget(output_widget, area);
    }

    /// Renders the chat status indicator (2 lines).
    fn render_chat_status(&self, frame: &mut Frame, area: Rect) {
        let status_widget = StatusIndicatorWidget::new(
            &self.flow.phase,
            self.flow_ui.current_file.as_deref(),
            self.flow.cycle_count,
            self.flow_ui.retry_wait,
            self.is_running,
            &self.theme,
            self.settings.max_iterations.value(),
        );
        frame.render_widget(status_widget, area);
    }

    /// Renders the chat progress bar (compact, 1 line).
    fn render_chat_progress(&self, frame: &mut Frame, area: Rect) {
        // Only show progress when actively processing
        if !self.is_running {
            let empty = Paragraph::new("");
            frame.render_widget(empty, area);
            return;
        }

        // Reuse existing render_compact_progress logic
        self.render_compact_progress(frame, area);
    }

    /// Renders the chat input area and returns the inner area for cursor positioning.
    fn render_chat_input(&self, frame: &mut Frame, area: Rect) -> Rect {
        let title = if self.is_running {
            " Task (Readonly) "
        } else {
            " Task Text "
        };

        let block = Block::bordered()
            .title(title)
            .title_style(self.theme.header_style())
            .title_bottom(Line::from(vec![
                Span::styled(" \\", self.theme.highlight_style()),
                Span::styled("+Enter for newline ", self.theme.muted_style()),
            ]))
            .border_style(self.theme.border_style());

        let inner = block.inner(area);

        // Create a clone of the textarea widget with the styled block
        let mut textarea = if self.is_running {
            let lines: Vec<String> = self.flow.input_text.split('\n').map(String::from).collect();
            TextArea::new(lines)
        } else {
            self.text_input.textarea.clone()
        };
        textarea.set_block(block);
        textarea.set_style(self.theme.normal_style());
        textarea.set_cursor_line_style(ratatui::style::Style::default()); // No highlight on cursor line
        textarea.set_placeholder_style(self.theme.placeholder_style());

        // Render the textarea widget - it handles text display, wrapping, and cursor
        // Note: Placeholder text is configured via textarea.set_placeholder_text() in TextInputState::new()
        frame.render_widget(&textarea, area);

        inner // Return for popup positioning
    }

    /// Renders the chat footer with key hints (single line).
    fn render_chat_footer(&self, frame: &mut Frame, area: Rect) {
        let footer_content = if self.is_running {
            vec![
                Span::styled(" [Esc] ", self.theme.highlight_style()),
                Span::styled("Cancel", self.theme.muted_style()),
            ]
        } else if self.should_show_file_popup() {
            vec![
                Span::styled(" [↑/↓] ", self.theme.highlight_style()),
                Span::styled("Navigate  ", self.theme.muted_style()),
                Span::styled("[Tab/Enter] ", self.theme.highlight_style()),
                Span::styled("Select  ", self.theme.muted_style()),
                Span::styled("[Esc] ", self.theme.highlight_style()),
                Span::styled("Dismiss", self.theme.muted_style()),
            ]
        } else {
            vec![
                Span::styled(" [Enter] ", self.theme.highlight_style()),
                Span::styled("Submit  ", self.theme.muted_style()),
                Span::styled("[Ctrl+S] ", self.theme.highlight_style()),
                Span::styled("Settings", self.theme.muted_style()),
            ]
        };

        let footer = Paragraph::new(Line::from(footer_content));
        frame.render_widget(footer, area);
    }

    /// Handles cursor positioning and file popup for chat mode.
    ///
    /// Note: `tui-textarea` handles cursor display internally. This function
    /// calculates the visual cursor position for the file suggestion popup.
    fn handle_cursor_and_popup(&self, frame: &mut Frame, inner_area: Rect) {
        // Only calculate cursor position if we need to show the popup
        if !self.should_show_file_popup() {
            return;
        }

        // Get cursor position from textarea (character-based)
        let (cursor_row, cursor_char_col) = self.text_input.cursor();
        let lines = self.text_input.lines();

        // Convert character column to byte column for wrap_lines_for_display
        let cursor_byte_col = if cursor_row < lines.len() {
            lines[cursor_row]
                .char_indices()
                .nth(cursor_char_col)
                .map_or(lines[cursor_row].len(), |(byte_pos, _)| byte_pos)
        } else {
            0
        };

        let wrap_result = wrap_lines_for_display(
            lines,
            cursor_row,
            cursor_byte_col,
            self.layout.input_visible_width(),
        );

        // Calculate scroll offset to keep the cursor visible within the viewport.
        // Use visual_cursor_row (not logical cursor_row) for scroll calculations
        // to correctly handle wrapped lines.
        let visual_height = inner_area.height as usize;
        let scroll_offset = wrap_result
            .visual_cursor_row
            .saturating_sub(visual_height / 2);
        let scroll_offset =
            scroll_offset.min(wrap_result.visual_lines.len().saturating_sub(visual_height));

        let visible_row = wrap_result.visual_cursor_row.saturating_sub(scroll_offset);
        // Clamp to ensure we never exceed inner area bounds
        let visible_row = visible_row.min(visual_height.saturating_sub(1));
        #[allow(clippy::cast_possible_truncation)] // Cursor position fits in terminal dimensions
        let cursor_x = inner_area.x + wrap_result.visual_cursor_col as u16;
        #[allow(clippy::cast_possible_truncation)] // Row position fits in terminal dimensions
        let cursor_y = inner_area.y + visible_row as u16;

        // Render file popup at the calculated cursor position
        self.render_file_popup(frame, cursor_x, cursor_y);
    }

    /// Renders a compact single-line progress bar.
    #[allow(clippy::cast_precision_loss)] // Precision loss acceptable for progress ratio
    fn render_compact_progress(&self, frame: &mut Frame, area: Rect) {
        let (current, total) = match &self.flow.phase {
            FlowPhase::ProcessingTodos { current, total } => (*current, *total),
            FlowPhase::RunningExecution { file_index, .. } => {
                (*file_index, self.flow.todo_files.len())
            }
            _ => (0, self.flow.todo_files.len().max(1)),
        };

        let ratio = if total > 0 {
            current as f64 / total as f64
        } else {
            0.0
        };

        let label = format!(" {current}/{total} files ");

        let gauge = LineGauge::default()
            .ratio(ratio)
            .label(label)
            .filled_style(self.theme.success_style())
            .unfilled_style(self.theme.muted_style());

        frame.render_widget(gauge, area);
    }

    // =========================================================================
    // File Suggestion Popup Rendering
    // =========================================================================

    /// Calculates the position for the file suggestion popup.
    ///
    /// The popup should appear below the cursor if there's room,
    /// otherwise above. It should be horizontally aligned with the
    /// @ token.
    ///
    /// # Arguments
    ///
    /// * `cursor_x` - Cursor X position on screen
    /// * `cursor_y` - Cursor Y position on screen
    /// * `popup_width` - Width of the popup
    /// * `popup_height` - Height of the popup
    /// * `screen` - Total screen area
    ///
    /// # Returns
    ///
    /// The Rect where the popup should be rendered.
    fn calculate_popup_position(
        cursor_x: u16,
        cursor_y: u16,
        popup_width: u16,
        popup_height: u16,
        screen: Rect,
    ) -> Rect {
        // Try to position below cursor first
        let below_space = screen.height.saturating_sub(cursor_y + 1);
        let above_space = cursor_y;

        let y = if below_space >= popup_height {
            // Enough room below
            cursor_y + 1
        } else if above_space >= popup_height {
            // Position above cursor
            cursor_y.saturating_sub(popup_height)
        } else {
            // Use whichever side has more room
            if below_space >= above_space {
                cursor_y + 1
            } else {
                cursor_y.saturating_sub(popup_height.min(above_space))
            }
        };

        // Horizontal positioning - align left edge with cursor, but keep on screen
        let x = if cursor_x + popup_width <= screen.width {
            cursor_x
        } else {
            screen.width.saturating_sub(popup_width)
        };

        // Clamp height to available space
        let actual_height = popup_height.min(screen.height.saturating_sub(y));

        Rect::new(x, y, popup_width, actual_height)
    }

    /// Renders the file suggestion popup.
    fn render_file_popup(&self, frame: &mut Frame, cursor_x: u16, cursor_y: u16) {
        let popup = FileSuggestionPopup::new(
            &self.text_input.file_popup_state,
            self.current_at_query(),
            &self.theme,
        );

        let (popup_width, popup_height) = popup.preferred_size();

        if popup_height == 0 {
            return; // Nothing to render
        }

        let popup_area = Self::calculate_popup_position(
            cursor_x,
            cursor_y,
            popup_width,
            popup_height,
            frame.area(),
        );

        // Render the popup on top of everything
        frame.render_widget(popup, popup_area);
    }

    // =========================================================================
    // Command Popup Rendering
    // =========================================================================

    /// Renders the slash command suggestion popup.
    ///
    /// The popup appears above the input area, aligned to the left edge.
    fn render_command_popup(&self, frame: &mut Frame, input_area: Rect) {
        let popup = CommandPopup::new(&self.text_input.command_popup_state, &self.theme);

        let (popup_width, popup_height) = popup.preferred_size();

        if popup_height == 0 {
            return; // Nothing to render
        }

        let popup_area = calculate_command_popup_area(popup_width, popup_height, input_area);

        // Render the popup on top of everything
        frame.render_widget(popup, popup_area);
    }
}

/// Calculates the area for the command popup.
///
/// The popup is positioned above the input area, aligned to the left edge.
/// Width and height are provided by the widget's `preferred_size()` method.
fn calculate_command_popup_area(popup_width: u16, popup_height: u16, input_area: Rect) -> Rect {
    // Clamp width to input area width
    let width = popup_width.min(input_area.width);

    // Position: above input area, aligned to left
    Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(popup_height),
        width,
        height: popup_height,
    }
}
