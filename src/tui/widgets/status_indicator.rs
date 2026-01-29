//! Compact status indicator widget for chat mode.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::core::FlowPhase;
use crate::tui::Theme;

/// A compact 2-line status indicator widget.
///
/// Displays the current flow status without borders, designed for
/// the chat-like interface where space is at a premium.
pub struct StatusIndicatorWidget<'a> {
    /// Current flow phase.
    phase: &'a FlowPhase,
    /// Current file being processed (if any).
    current_file: Option<&'a str>,
    /// Cycle count.
    cycle_count: u32,
    /// Retry wait time remaining (if waiting).
    retry_wait: Option<u64>,
    /// Whether the flow is actively running.
    is_running: bool,
    /// Theme for styling.
    theme: &'a Theme,
    /// Maximum iterations (None = unlimited).
    max_iterations: Option<u32>,
}

impl<'a> StatusIndicatorWidget<'a> {
    /// Creates a new status indicator widget.
    #[must_use]
    pub const fn new(
        phase: &'a FlowPhase,
        current_file: Option<&'a str>,
        cycle_count: u32,
        retry_wait: Option<u64>,
        is_running: bool,
        theme: &'a Theme,
        max_iterations: Option<u32>,
    ) -> Self {
        Self {
            phase,
            current_file,
            cycle_count,
            retry_wait,
            is_running,
            theme,
            max_iterations,
        }
    }

    /// Gets the icon for the current phase.
    fn phase_icon(&self) -> &'static str {
        match self.phase {
            FlowPhase::Idle => "·",
            FlowPhase::ReadingInput
            | FlowPhase::CheckingDoneFiles
            | FlowPhase::RunningPlanning { .. }
            | FlowPhase::CheckingTodoFiles
            | FlowPhase::ProcessingTodos { .. }
            | FlowPhase::RunningExecution { .. }
            | FlowPhase::MovingCompletedFiles => "▶",
            FlowPhase::NoTodoFiles | FlowPhase::CycleComplete { .. } | FlowPhase::Completed => "✓",
            FlowPhase::Failed { .. } => "✗",
        }
    }

    /// Gets the iteration prefix for active phases.
    fn iteration_prefix(&self) -> String {
        if self.cycle_count > 0 {
            match self.max_iterations {
                Some(max) => format!("Iteration {}/{}", self.cycle_count, max),
                None => format!("Iteration #{}", self.cycle_count),
            }
        } else {
            "Iteration #1".to_string()
        }
    }

    /// Formats retry information if attempt > 1.
    fn retry_suffix(&self, attempt: u32) -> String {
        if attempt > 1 {
            if let Some(wait_secs) = self.retry_wait {
                format!(" (retry {attempt}, {wait_secs}s)")
            } else {
                format!(" (retry {attempt})")
            }
        } else {
            String::new()
        }
    }

    /// Gets the primary status text for line 1.
    fn primary_status(&self) -> String {
        match self.phase {
            FlowPhase::Idle => "Waiting for input".to_string(),
            FlowPhase::ReadingInput => format!("{} | Reading input...", self.iteration_prefix()),
            FlowPhase::CheckingDoneFiles => {
                format!("{} | Checking completed tasks...", self.iteration_prefix())
            }
            FlowPhase::RunningPlanning {
                model_name,
                attempt,
            } => {
                let retry = self.retry_suffix(*attempt);
                format!(
                    "{} | Planning Mode ({model_name}){retry}",
                    self.iteration_prefix()
                )
            }
            FlowPhase::CheckingTodoFiles => {
                format!("{} | Checking for todo files...", self.iteration_prefix())
            }
            FlowPhase::NoTodoFiles => "No todo files found - complete!".to_string(),
            FlowPhase::ProcessingTodos { current, total } => {
                format!(
                    "{} | Processing todo files ({current}/{total})",
                    self.iteration_prefix()
                )
            }
            FlowPhase::RunningExecution {
                model_name,
                attempt,
                ..
            } => {
                let file_name = self.current_file.unwrap_or("unknown");
                let retry = self.retry_suffix(*attempt);
                format!(
                    "{} | Coding Mode ({model_name}){retry} | {file_name}",
                    self.iteration_prefix()
                )
            }
            FlowPhase::CycleComplete { iteration } => {
                format!("Iteration #{iteration} complete")
            }
            FlowPhase::MovingCompletedFiles => {
                format!("{} | Updating summary...", self.iteration_prefix())
            }
            FlowPhase::Completed => "All iterations completed!".to_string(),
            FlowPhase::Failed { reason } => format!("Failed: {reason}"),
        }
    }

    /// Gets the secondary status text for line 2.
    fn secondary_status(&self) -> String {
        match self.phase {
            FlowPhase::Idle => "Ready to process tasks".to_string(),
            FlowPhase::Completed | FlowPhase::NoTodoFiles => {
                "Enter a new task to continue".to_string()
            }
            FlowPhase::Failed { .. } => "Check logs for details".to_string(),
            FlowPhase::RunningPlanning { .. } => "Creating task breakdown...".to_string(),
            FlowPhase::RunningExecution { .. } => "Implementing task...".to_string(),
            FlowPhase::CycleComplete { .. } => "Preparing next iteration...".to_string(),
            _ => self.phase.description().to_string(),
        }
    }

    /// Gets the style for the icon based on phase.
    fn icon_style(&self) -> ratatui::style::Style {
        match self.phase {
            FlowPhase::Idle => self.theme.muted_style(),
            FlowPhase::Completed | FlowPhase::NoTodoFiles | FlowPhase::CycleComplete { .. } => {
                self.theme.success_style()
            }
            FlowPhase::Failed { .. } => self.theme.error_style(),
            _ => self.theme.highlight_style(),
        }
    }
}

impl Widget for StatusIndicatorWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            // Not enough space, render just the primary status
            let line = Line::from(vec![
                Span::styled(format!(" {} ", self.phase_icon()), self.icon_style()),
                Span::styled(self.primary_status(), self.theme.normal_style()),
            ]);
            Paragraph::new(line).render(area, buf);
            return;
        }

        let icon = self.phase_icon();
        let primary = self.primary_status();
        let secondary = self.secondary_status();

        let icon_style = self.icon_style();
        let text_style = if self.is_running {
            self.theme.normal_style()
        } else {
            self.theme.muted_style()
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(format!(" {icon} "), icon_style),
                Span::styled(primary, text_style),
            ]),
            Line::from(vec![
                Span::styled("   ", text_style), // Indent to align with text above
                Span::styled(secondary, self.theme.muted_style()),
            ]),
        ];

        Paragraph::new(lines).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    // =========================================================================
    // StatusIndicatorWidget Tests
    // =========================================================================

    mod status_indicator_widget {
        use super::*;

        /// Tests that idle phase shows waiting message.
        #[test]
        fn idle_shows_waiting() {
            let theme = Theme::default();
            let phase = FlowPhase::Idle;
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, false, &theme, None);

            assert_eq!(widget.primary_status(), "Waiting for input");
            assert_eq!(widget.secondary_status(), "Ready to process tasks");
            assert_eq!(widget.phase_icon(), "·");
        }

        /// Tests that running planning shows iteration and mode with model name.
        #[test]
        fn running_planning_shows_model() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningPlanning {
                model_name: Cow::Borrowed("Claude"),
                attempt: 1,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, None, true, &theme, None);

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Planning Mode (Claude)"
            );
            assert_eq!(widget.secondary_status(), "Creating task breakdown...");
            assert_eq!(widget.phase_icon(), "▶");
        }

        /// Tests that retry attempts are shown in primary status.
        #[test]
        fn running_with_retry_shows_attempt() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningPlanning {
                model_name: Cow::Borrowed("Codex"),
                attempt: 3,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, None, true, &theme, None);

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Planning Mode (Codex) (retry 3)"
            );
        }

        /// Tests that retry wait countdown is shown in primary status with attempt.
        #[test]
        fn retry_wait_shows_countdown() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningPlanning {
                model_name: Cow::Borrowed("Claude"),
                attempt: 2,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, Some(30), true, &theme, None);

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Planning Mode (Claude) (retry 2, 30s)"
            );
        }

        /// Tests that execution phase shows iteration, mode, model name and file.
        #[test]
        fn execution_shows_file() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningExecution {
                model_name: Cow::Borrowed("Claude"),
                file_index: 2,
                attempt: 1,
            };
            let widget = StatusIndicatorWidget::new(
                &phase,
                Some("task-001.md"),
                1,
                None,
                true,
                &theme,
                None,
            );

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Coding Mode (Claude) | task-001.md"
            );
            assert_eq!(widget.secondary_status(), "Implementing task...");
        }

        /// Tests that failed phase shows error icon and reason.
        #[test]
        fn failed_shows_error() {
            let theme = Theme::default();
            let phase = FlowPhase::Failed {
                reason: "Connection timeout".to_string(),
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, None, false, &theme, None);

            assert_eq!(widget.primary_status(), "Failed: Connection timeout");
            assert_eq!(widget.secondary_status(), "Check logs for details");
            assert_eq!(widget.phase_icon(), "✗");
        }

        /// Tests that completed phase shows success icon.
        #[test]
        fn completed_shows_success() {
            let theme = Theme::default();
            let phase = FlowPhase::Completed;
            let widget = StatusIndicatorWidget::new(&phase, None, 3, None, false, &theme, None);

            assert_eq!(widget.primary_status(), "All iterations completed!");
            assert_eq!(widget.secondary_status(), "Enter a new task to continue");
            assert_eq!(widget.phase_icon(), "✓");
        }

        /// Tests that processing todos shows progress with iteration.
        #[test]
        fn processing_todos_shows_progress() {
            let theme = Theme::default();
            let phase = FlowPhase::ProcessingTodos {
                current: 3,
                total: 10,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, None, true, &theme, None);

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Processing todo files (3/10)"
            );
            assert_eq!(widget.phase_icon(), "▶");
        }

        /// Tests that cycle complete shows iteration.
        #[test]
        fn cycle_complete_shows_iteration() {
            let theme = Theme::default();
            let phase = FlowPhase::CycleComplete { iteration: 2 };
            let widget = StatusIndicatorWidget::new(&phase, None, 2, None, true, &theme, None);

            assert_eq!(widget.primary_status(), "Iteration #2 complete");
            assert_eq!(widget.secondary_status(), "Preparing next iteration...");
            assert_eq!(widget.phase_icon(), "✓");
        }

        /// Tests that no todo files shows complete message.
        #[test]
        fn no_todo_files_shows_complete() {
            let theme = Theme::default();
            let phase = FlowPhase::NoTodoFiles;
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, false, &theme, None);

            assert_eq!(widget.primary_status(), "No todo files found - complete!");
            assert_eq!(widget.secondary_status(), "Enter a new task to continue");
            assert_eq!(widget.phase_icon(), "✓");
        }

        /// Tests icon style for idle phase.
        #[test]
        fn icon_style_idle_is_muted() {
            let theme = Theme::default();
            let phase = FlowPhase::Idle;
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, false, &theme, None);

            assert_eq!(widget.icon_style(), theme.muted_style());
        }

        /// Tests icon style for completed phase.
        #[test]
        fn icon_style_completed_is_success() {
            let theme = Theme::default();
            let phase = FlowPhase::Completed;
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, false, &theme, None);

            assert_eq!(widget.icon_style(), theme.success_style());
        }

        /// Tests icon style for failed phase.
        #[test]
        fn icon_style_failed_is_error() {
            let theme = Theme::default();
            let phase = FlowPhase::Failed {
                reason: "error".to_string(),
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, false, &theme, None);

            assert_eq!(widget.icon_style(), theme.error_style());
        }

        /// Tests icon style for running phases.
        #[test]
        fn icon_style_running_is_highlight() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningPlanning {
                model_name: Cow::Borrowed("Test"),
                attempt: 1,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 0, None, true, &theme, None);

            assert_eq!(widget.icon_style(), theme.highlight_style());
        }

        /// Tests execution without file shows "unknown".
        #[test]
        fn execution_without_file_shows_unknown() {
            let theme = Theme::default();
            let phase = FlowPhase::RunningExecution {
                model_name: Cow::Borrowed("Claude"),
                file_index: 0,
                attempt: 1,
            };
            let widget = StatusIndicatorWidget::new(&phase, None, 1, None, true, &theme, None);

            assert_eq!(
                widget.primary_status(),
                "Iteration #1 | Coding Mode (Claude) | unknown"
            );
        }

        /// Tests primary status shows iteration with max when `max_iterations` is set.
        #[test]
        fn primary_status_shows_iteration_with_max() {
            let theme = Theme::default();
            let phase = FlowPhase::CheckingTodoFiles;
            let widget = StatusIndicatorWidget::new(&phase, None, 2, None, true, &theme, Some(5));

            assert!(widget.primary_status().contains("Iteration 2/5"));
        }

        /// Tests primary status shows iteration without max when unlimited.
        #[test]
        fn primary_status_shows_iteration_without_max() {
            let theme = Theme::default();
            let phase = FlowPhase::CheckingTodoFiles;
            let widget = StatusIndicatorWidget::new(&phase, None, 3, None, true, &theme, None);

            let status = widget.primary_status();
            assert!(status.contains("Iteration #3"));
            assert!(!status.contains("Iteration #3/"));
        }
    }
}
