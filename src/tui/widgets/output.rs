//! CLI output viewer widget.
//!
//! This module provides a unified output widget that displays both CLI output
//! (stdout/stderr) and system messages (info, success, warning, error, running).
//! This consolidation eliminates the need for a separate log panel.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
        Widget,
    },
};
use unicode_width::UnicodeWidthChar;

use crate::tui::Theme;

/// Maximum number of output lines to keep in buffer.
/// Lines beyond this are truncated from the beginning to prevent unbounded memory growth.
/// The value of 5000 provides better history retention while still preventing memory issues.
pub const MAX_OUTPUT_LINES: usize = 5000;

/// Types of output lines for different styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputLineType {
    /// Standard stdout from CLI.
    #[default]
    Stdout,
    /// Standard stderr from CLI.
    Stderr,
    /// System info message.
    SystemInfo,
    /// Success message.
    SystemSuccess,
    /// Warning message.
    SystemWarning,
    /// Error message.
    SystemError,
    /// Progress/running message.
    SystemRunning,
}

/// A line of output with type for styling.
#[derive(Debug, Clone)]
pub struct OutputLine {
    /// The text content.
    pub text: String,
    /// The line type for styling.
    pub line_type: OutputLineType,
}

impl OutputLine {
    /// Creates a new stdout line.
    #[must_use]
    pub fn stdout(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            line_type: OutputLineType::Stdout,
        }
    }

    /// Creates a new stderr line.
    #[must_use]
    pub fn stderr(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            line_type: OutputLineType::Stderr,
        }
    }

    /// Creates a system info line.
    #[must_use]
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: format!("  {}", text.into()),
            line_type: OutputLineType::SystemInfo,
        }
    }

    /// Creates a system success line.
    #[must_use]
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: format!("+ {}", text.into()),
            line_type: OutputLineType::SystemSuccess,
        }
    }

    /// Creates a system warning line.
    #[must_use]
    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: format!("! {}", text.into()),
            line_type: OutputLineType::SystemWarning,
        }
    }

    /// Creates a system error line.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: format!("✗ {}", text.into()),
            line_type: OutputLineType::SystemError,
        }
    }

    /// Creates a system running line.
    #[must_use]
    pub fn running(text: impl Into<String>) -> Self {
        Self {
            text: format!("> {}", text.into()),
            line_type: OutputLineType::SystemRunning,
        }
    }

    /// Returns true if this is a stderr line (for backward compatibility).
    #[must_use]
    pub fn is_stderr(&self) -> bool {
        self.line_type == OutputLineType::Stderr
    }
}

/// A scrollable CLI output viewer widget.
pub struct OutputWidget<'a> {
    /// Output lines to display.
    lines: &'a [OutputLine],
    /// Current scroll offset.
    scroll_offset: usize,
    /// Title for the widget.
    title: &'a str,
    /// Theme for styling.
    theme: &'a Theme,
    /// Whether some lines have been truncated from the beginning.
    is_truncated: bool,
}

impl<'a> OutputWidget<'a> {
    /// Creates a new output widget.
    #[must_use]
    pub const fn new(
        lines: &'a [OutputLine],
        scroll_offset: usize,
        title: &'a str,
        theme: &'a Theme,
    ) -> Self {
        Self {
            lines,
            scroll_offset,
            title,
            theme,
            is_truncated: false,
        }
    }

    /// Creates a new output widget with truncation indicator.
    #[must_use]
    pub const fn with_truncation(
        lines: &'a [OutputLine],
        scroll_offset: usize,
        title: &'a str,
        theme: &'a Theme,
        is_truncated: bool,
    ) -> Self {
        Self {
            lines,
            scroll_offset,
            title,
            theme,
            is_truncated,
        }
    }
}

/// Calculates the total number of visual lines after wrapping for scroll calculations.
///
/// This is used by the App to determine proper scroll offsets when navigating
/// through CLI output that may contain long lines that wrap.
#[must_use]
pub fn calculate_visual_line_count(lines: &[OutputLine], content_width: usize) -> usize {
    if content_width == 0 {
        return lines.len();
    }

    lines
        .iter()
        .map(|line| {
            if line.text.is_empty() {
                1
            } else {
                wrap_line_to_width(&line.text, content_width).len()
            }
        })
        .sum()
}

/// Wraps a single line of text to fit within the given width.
///
/// Uses Unicode-aware width calculation to properly handle multi-byte characters.
/// Each output line represents one visual row in the terminal.
fn wrap_line_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    if text.is_empty() {
        return vec![String::new()];
    }

    let mut result = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);

        if current_width + char_width > width {
            // Start a new line
            result.push(current_line);
            current_line = String::new();
            current_width = 0;
        }

        current_line.push(ch);
        current_width += char_width;
    }

    // Don't forget the last line
    result.push(current_line);

    result
}

/// A wrapped visual line with style information.
struct VisualLine {
    text: String,
    line_type: OutputLineType,
}

impl Widget for OutputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Build the block first to calculate inner area
        let block = Block::default()
            .title(Line::from(vec![Span::styled(
                self.title,
                self.theme.header_style(),
            )]))
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        // Get inner area for content (excluding borders)
        let inner_area = block.inner(area);
        let visible_height = inner_area.height as usize;

        // Account for scrollbar width (1 character on the right)
        let content_width = inner_area.width.saturating_sub(1) as usize;

        // Pre-wrap all lines to calculate visual line count
        let visual_lines: Vec<VisualLine> = self
            .lines
            .iter()
            .flat_map(|line| {
                let wrapped = wrap_line_to_width(&line.text, content_width);
                wrapped.into_iter().map(move |text| VisualLine {
                    text,
                    line_type: line.line_type,
                })
            })
            .collect();

        let total_visual_lines = visual_lines.len();

        // Calculate which visual lines to display based on scroll offset
        let visible_lines: Vec<Line> = visual_lines
            .into_iter()
            .skip(self.scroll_offset)
            .take(visible_height)
            .map(|vline| {
                let style = match vline.line_type {
                    OutputLineType::Stdout => self.theme.normal_style(),
                    OutputLineType::Stderr | OutputLineType::SystemWarning => {
                        self.theme.warning_style()
                    }
                    OutputLineType::SystemInfo => self.theme.muted_style(),
                    OutputLineType::SystemSuccess => self.theme.success_style(),
                    OutputLineType::SystemError => self.theme.error_style(),
                    OutputLineType::SystemRunning => self.theme.highlight_style(),
                };
                Line::from(Span::styled(vline.text, style))
            })
            .collect();

        // Build title with truncation and scroll info
        let truncation_info = if self.is_truncated {
            " [truncated]"
        } else {
            ""
        };
        let scroll_info = if total_visual_lines > visible_height {
            format!(
                " ({}-{}/{})",
                self.scroll_offset + 1,
                (self.scroll_offset + visible_height).min(total_visual_lines),
                total_visual_lines
            )
        } else {
            String::new()
        };

        let title = format!("{}{truncation_info}{scroll_info}", self.title);

        // Rebuild block with updated title
        let block = Block::default()
            .title(Line::from(vec![Span::styled(
                title,
                self.theme.header_style(),
            )]))
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        // Render the block
        block.render(area, buf);

        // Render paragraph content in inner area (no wrap since we pre-wrapped)
        let paragraph = Paragraph::new(visible_lines);
        paragraph.render(inner_area, buf);

        // Render scrollbar if there's more content than visible
        if total_visual_lines > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .track_style(self.theme.scrollbar_track_style())
                .thumb_style(self.theme.scrollbar_thumb_style());

            let mut scrollbar_state = ScrollbarState::new(total_visual_lines)
                .position(self.scroll_offset)
                .viewport_content_length(visible_height);

            // Render scrollbar in the inner area (right side)
            scrollbar.render(inner_area, buf, &mut scrollbar_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    // =========================================================================
    // Render Tests (using TestBackend)
    // =========================================================================

    mod render_tests {
        use super::*;
        use ratatui::{Terminal, backend::TestBackend};

        /// Tests that `OutputWidget` renders an empty output correctly.
        #[test]
        fn renders_empty_output() -> Result<()> {
            let backend = TestBackend::new(40, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines: Vec<OutputLine> = vec![];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Verify the title is present
            let title_line: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title_line.contains("Output"));

            // Verify borders are present
            assert_eq!(buffer[(0, 0)].symbol(), "┌");
            assert_eq!(buffer[(39, 0)].symbol(), "┐");
            assert_eq!(buffer[(0, 4)].symbol(), "└");
            assert_eq!(buffer[(39, 4)].symbol(), "┘");
            Ok(())
        }

        /// Tests that `OutputWidget` renders stdout lines correctly.
        #[test]
        fn renders_stdout_lines() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Hello, World!")];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "CLI Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Extract content line (row 1, inside borders)
            let content_line: String = (1..49).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(content_line.contains("Hello, World!"));
            Ok(())
        }

        /// Tests that `OutputWidget` renders stderr lines correctly.
        #[test]
        fn renders_stderr_lines() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stderr("Error occurred!")];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "CLI Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Extract content line
            let content_line: String = (1..49).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(content_line.contains("Error occurred!"));
            Ok(())
        }

        /// Tests that stdout uses normal style.
        #[test]
        fn stdout_uses_normal_style() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Normal output")];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Check style of content (x=1 is after border)
            let content_style = buffer[(1, 1)].style();
            assert_eq!(content_style.fg, Some(theme.fg));
            Ok(())
        }

        /// Tests that stderr uses warning style.
        #[test]
        fn stderr_uses_warning_style() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stderr("Error output")];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Check style of content - stderr should use warning style (yellow)
            let content_style = buffer[(1, 1)].style();
            assert_eq!(content_style.fg, Some(theme.warning));
            Ok(())
        }

        /// Tests that `OutputWidget` renders mixed stdout/stderr correctly.
        #[test]
        fn renders_mixed_output() -> Result<()> {
            let backend = TestBackend::new(50, 6);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let output_lines = vec![
                OutputLine::stdout("stdout line"),
                OutputLine::stderr("stderr line"),
            ];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&output_lines, 0, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // First line should be stdout with normal style
            let line1: String = (1..49).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(line1.contains("stdout line"));
            assert_eq!(buffer[(1, 1)].style().fg, Some(theme.fg));

            // Second line should be stderr with warning style
            let line2: String = (1..49).map(|x| buffer[(x, 2)].symbol()).collect();
            assert!(line2.contains("stderr line"));
            assert_eq!(buffer[(1, 2)].style().fg, Some(theme.warning));
            Ok(())
        }

        /// Tests that `OutputWidget` respects scroll offset.
        #[test]
        fn respects_scroll_offset() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let output_lines = vec![
                OutputLine::stdout("First"),
                OutputLine::stdout("Second"),
                OutputLine::stdout("Third"),
            ];

            // Render with scroll offset of 1 (skip first entry)
            terminal.draw(|frame| {
                let widget = OutputWidget::new(&output_lines, 1, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // First visible line should be "Second", not "First"
            let line1: String = (1..49).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(line1.contains("Second"));
            assert!(!line1.contains("First"));
            Ok(())
        }

        /// Tests that `OutputWidget` shows scroll info when content overflows.
        #[test]
        fn shows_scroll_info_when_overflowing() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            // Height 5 - 2 (borders) = 3 visible lines, but we have 5 entries
            let lines = vec![
                OutputLine::stdout("One"),
                OutputLine::stdout("Two"),
                OutputLine::stdout("Three"),
                OutputLine::stdout("Four"),
                OutputLine::stdout("Five"),
            ];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Output", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Title should show scroll info
            let title_line: String = (0..50).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title_line.contains("1-3/5") || title_line.contains("(1-3/5)"));
            Ok(())
        }

        /// Tests that truncation indicator is shown when enabled.
        #[test]
        fn shows_truncation_indicator() -> Result<()> {
            let backend = TestBackend::new(60, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Test")];

            terminal.draw(|frame| {
                let widget = OutputWidget::with_truncation(&lines, 0, "Output", &theme, true);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Title should contain "[truncated]"
            let title_line: String = (0..60).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title_line.contains("[truncated]"));
            Ok(())
        }

        /// Tests that truncation indicator is not shown when disabled.
        #[test]
        fn no_truncation_indicator_when_disabled() -> Result<()> {
            let backend = TestBackend::new(60, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Test")];

            terminal.draw(|frame| {
                let widget = OutputWidget::with_truncation(&lines, 0, "Output", &theme, false);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Title should NOT contain "[truncated]"
            let title_line: String = (0..60).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(!title_line.contains("[truncated]"));
            Ok(())
        }

        /// Tests that long lines are wrapped correctly.
        #[test]
        fn wraps_long_lines() -> Result<()> {
            let backend = TestBackend::new(20, 6);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            // Content width = 20 - 2 (borders) - 1 (scrollbar) = 17
            // Line will wrap at 17 chars
            let output_lines = vec![OutputLine::stdout("This is a very long line that wraps")];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&output_lines, 0, "Out", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // The long line should be visible across multiple rows
            let line1: String = (1..19).map(|x| buffer[(x, 1)].symbol()).collect();
            let line2: String = (1..19).map(|x| buffer[(x, 2)].symbol()).collect();

            // First row should have start of text
            assert!(line1.contains("This is"));
            // Second row should have continuation
            assert!(!line2.trim().is_empty());
            Ok(())
        }

        /// Tests that title styling uses header style.
        #[test]
        fn title_uses_header_style() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines: Vec<OutputLine> = vec![];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Test Title", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Find where the title text starts (after border character)
            // Title should have header style (cyan + bold)
            let title_style = buffer[(1, 0)].style();
            assert_eq!(title_style.fg, Some(theme.accent));
            Ok(())
        }

        /// Tests border styling.
        #[test]
        fn borders_use_border_style() -> Result<()> {
            let backend = TestBackend::new(50, 5);
            let mut terminal = Terminal::new(backend)?;

            let theme = Theme::default();
            let lines: Vec<OutputLine> = vec![];

            terminal.draw(|frame| {
                let widget = OutputWidget::new(&lines, 0, "Title", &theme);
                frame.render_widget(widget, frame.area());
            })?;

            let buffer = terminal.backend().buffer();

            // Top-left corner should have border style
            let corner_style = buffer[(0, 0)].style();
            assert_eq!(corner_style.fg, Some(theme.border));
            Ok(())
        }
    }

    // =========================================================================
    // OutputLine Tests
    // =========================================================================

    mod output_line {
        use super::*;

        /// Tests that `stdout()` creates an `Stdout` line type.
        #[test]
        fn stdout_sets_line_type() {
            let line = OutputLine::stdout("output text");

            assert_eq!(line.text, "output text");
            assert_eq!(line.line_type, OutputLineType::Stdout);
            assert!(!line.is_stderr());
        }

        /// Tests that `stderr()` creates a `Stderr` line type.
        #[test]
        fn stderr_sets_line_type() {
            let line = OutputLine::stderr("error text");

            assert_eq!(line.text, "error text");
            assert_eq!(line.line_type, OutputLineType::Stderr);
            assert!(line.is_stderr());
        }

        /// Tests that `info()` creates a `SystemInfo` line with prefix.
        #[test]
        fn info_sets_line_type_with_prefix() {
            let line = OutputLine::info("info message");

            assert_eq!(line.text, "  info message");
            assert_eq!(line.line_type, OutputLineType::SystemInfo);
        }

        /// Tests that `success()` creates a `SystemSuccess` line with prefix.
        #[test]
        fn success_sets_line_type_with_prefix() {
            let line = OutputLine::success("success message");

            assert_eq!(line.text, "+ success message");
            assert_eq!(line.line_type, OutputLineType::SystemSuccess);
        }

        /// Tests that `warning()` creates a `SystemWarning` line with prefix.
        #[test]
        fn warning_sets_line_type_with_prefix() {
            let line = OutputLine::warning("warning message");

            assert_eq!(line.text, "! warning message");
            assert_eq!(line.line_type, OutputLineType::SystemWarning);
        }

        /// Tests that `error()` creates a `SystemError` line with prefix.
        #[test]
        fn error_sets_line_type_with_prefix() {
            let line = OutputLine::error("error message");

            assert_eq!(line.text, "✗ error message");
            assert_eq!(line.line_type, OutputLineType::SystemError);
        }

        /// Tests that `running()` creates a `SystemRunning` line with prefix.
        #[test]
        fn running_sets_line_type_with_prefix() {
            let line = OutputLine::running("running message");

            assert_eq!(line.text, "> running message");
            assert_eq!(line.line_type, OutputLineType::SystemRunning);
        }

        /// Tests that factory methods accept String.
        #[test]
        fn factories_accept_string() {
            let stdout = OutputLine::stdout(String::from("owned stdout"));
            let stderr = OutputLine::stderr(String::from("owned stderr"));
            let info = OutputLine::info(String::from("owned info"));

            assert_eq!(stdout.text, "owned stdout");
            assert_eq!(stderr.text, "owned stderr");
            assert_eq!(info.text, "  owned info");
        }

        /// Tests that `OutputLine` can be cloned.
        #[test]
        fn clone_preserves_all_fields() {
            let original = OutputLine::stderr("Clone test");
            let cloned = original.clone();

            assert_eq!(original.text, cloned.text);
            assert_eq!(original.line_type, cloned.line_type);
        }

        /// Tests that modifying clone doesn't affect original.
        #[test]
        fn clone_is_independent() {
            let original = OutputLine::stdout("Original");
            let mut cloned = original.clone();
            cloned.text = String::from("Modified");
            cloned.line_type = OutputLineType::Stderr;

            assert_eq!(original.text, "Original");
            assert_eq!(original.line_type, OutputLineType::Stdout);
            assert_eq!(cloned.text, "Modified");
            assert_eq!(cloned.line_type, OutputLineType::Stderr);
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let line = OutputLine::stdout("Debug test");
            let debug_str = format!("{line:?}");

            assert!(debug_str.contains("OutputLine"));
            assert!(debug_str.contains("Debug test"));
            assert!(debug_str.contains("line_type"));
        }

        /// Tests stdout with empty text.
        #[test]
        fn stdout_empty_text() {
            let line = OutputLine::stdout("");

            assert!(line.text.is_empty());
            assert_eq!(line.line_type, OutputLineType::Stdout);
        }

        /// Tests stderr with multiline text.
        #[test]
        fn stderr_multiline_text() {
            let multiline = "Line 1\nLine 2\nLine 3";
            let line = OutputLine::stderr(multiline);

            assert_eq!(line.text, multiline);
            assert_eq!(line.line_type, OutputLineType::Stderr);
        }

        /// Tests `is_stderr()` backward compatibility method.
        #[test]
        fn is_stderr_backward_compat() {
            assert!(OutputLine::stderr("test").is_stderr());
            assert!(!OutputLine::stdout("test").is_stderr());
            assert!(!OutputLine::info("test").is_stderr());
            assert!(!OutputLine::success("test").is_stderr());
            assert!(!OutputLine::warning("test").is_stderr());
            assert!(!OutputLine::error("test").is_stderr());
            assert!(!OutputLine::running("test").is_stderr());
        }
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    mod constants {
        use super::*;

        /// Tests that `MAX_OUTPUT_LINES` has expected value.
        #[test]
        fn max_output_lines_is_5000() {
            assert_eq!(MAX_OUTPUT_LINES, 5000);
        }
    }

    // =========================================================================
    // OutputWidget Tests
    // =========================================================================

    mod output_widget {
        use super::*;

        /// Tests creating an `OutputWidget` with `new()`.
        #[test]
        fn new_stores_parameters() {
            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Test")];
            let widget = OutputWidget::new(&lines, 5, "Title", &theme);

            assert_eq!(widget.scroll_offset, 5);
            assert_eq!(widget.title, "Title");
            assert_eq!(widget.lines.len(), 1);
            assert!(!widget.is_truncated);
        }

        /// Tests creating a widget with empty lines.
        #[test]
        fn new_with_empty_lines() {
            let theme = Theme::default();
            let lines: Vec<OutputLine> = vec![];
            let widget = OutputWidget::new(&lines, 0, "Empty", &theme);

            assert_eq!(widget.lines.len(), 0);
            assert!(!widget.is_truncated);
        }

        /// Tests creating a widget with mixed stdout/stderr lines.
        #[test]
        fn new_with_mixed_lines() {
            let theme = Theme::default();
            let lines = vec![
                OutputLine::stdout("stdout 1"),
                OutputLine::stderr("stderr 1"),
                OutputLine::stdout("stdout 2"),
            ];
            let widget = OutputWidget::new(&lines, 0, "Mixed", &theme);

            assert_eq!(widget.lines.len(), 3);
            assert!(!widget.lines[0].is_stderr());
            assert!(widget.lines[1].is_stderr());
            assert!(!widget.lines[2].is_stderr());
            assert!(!widget.is_truncated);
        }

        /// Tests creating a widget with truncation indicator.
        #[test]
        fn with_truncation_sets_flag() {
            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Test")];
            let widget = OutputWidget::with_truncation(&lines, 0, "Title", &theme, true);

            assert!(widget.is_truncated);
        }

        /// Tests that `with_truncation(false)` is equivalent to `new()`.
        #[test]
        fn with_truncation_false_same_as_new() {
            let theme = Theme::default();
            let lines = vec![OutputLine::stdout("Test")];
            let widget = OutputWidget::with_truncation(&lines, 5, "Title", &theme, false);

            assert_eq!(widget.scroll_offset, 5);
            assert_eq!(widget.title, "Title");
            assert!(!widget.is_truncated);
        }
    }

    // =========================================================================
    // Line Wrapping Tests
    // =========================================================================

    mod line_wrapping {
        use super::*;

        /// Tests that short lines that fit within width are not wrapped.
        #[test]
        fn short_line_not_wrapped() {
            let result = wrap_line_to_width("hello", 10);
            assert_eq!(result, vec!["hello"]);
        }

        /// Tests that lines exactly at width are not wrapped.
        #[test]
        fn exact_width_not_wrapped() {
            let result = wrap_line_to_width("hello", 5);
            assert_eq!(result, vec!["hello"]);
        }

        /// Tests that long lines are wrapped correctly.
        #[test]
        fn long_line_wrapped() {
            let result = wrap_line_to_width("helloworld", 5);
            assert_eq!(result, vec!["hello", "world"]);
        }

        /// Tests wrapping with multiple segments.
        #[test]
        fn multiple_wraps() {
            let result = wrap_line_to_width("abcdefghijklmnop", 5);
            assert_eq!(result, vec!["abcde", "fghij", "klmno", "p"]);
        }

        /// Tests that empty strings produce a single empty line.
        #[test]
        fn empty_string_produces_empty_line() {
            let result = wrap_line_to_width("", 10);
            assert_eq!(result, vec![""]);
        }

        /// Tests that zero width produces a single empty line.
        #[test]
        fn zero_width_produces_empty_line() {
            let result = wrap_line_to_width("hello", 0);
            assert_eq!(result, vec![""]);
        }

        /// Tests wrapping with spaces in text.
        #[test]
        fn wraps_with_spaces() {
            let result = wrap_line_to_width("ab cd ef", 4);
            assert_eq!(result, vec!["ab c", "d ef"]);
        }
    }

    // =========================================================================
    // Visual Line Count Tests
    // =========================================================================

    mod visual_line_count {
        use super::*;

        /// Tests count with empty lines.
        #[test]
        fn empty_lines_returns_zero() {
            let lines: Vec<OutputLine> = vec![];
            assert_eq!(calculate_visual_line_count(&lines, 80), 0);
        }

        /// Tests count with short lines (no wrapping needed).
        #[test]
        fn short_lines_count_correctly() {
            let lines = vec![
                OutputLine::stdout("line1"),
                OutputLine::stdout("line2"),
                OutputLine::stdout("line3"),
            ];
            assert_eq!(calculate_visual_line_count(&lines, 80), 3);
        }

        /// Tests count with long lines that wrap.
        #[test]
        fn long_lines_count_wrapped() {
            let lines = vec![OutputLine::stdout("abcdefghij")]; // 10 chars
            assert_eq!(calculate_visual_line_count(&lines, 5), 2); // wraps to 2 lines
        }

        /// Tests count with mixed short and long lines.
        #[test]
        fn mixed_lines_count_correctly() {
            let lines = vec![
                OutputLine::stdout("short"),          // 1 visual line
                OutputLine::stdout("this is longer"), // 15 chars, wraps at width 10 = 2 lines
                OutputLine::stdout("tiny"),           // 1 visual line
            ];
            assert_eq!(calculate_visual_line_count(&lines, 10), 4);
        }

        /// Tests count with empty text lines (they count as 1).
        #[test]
        fn empty_text_lines_count_as_one() {
            let lines = vec![
                OutputLine::stdout("text"),
                OutputLine::stdout(""),
                OutputLine::stdout("more"),
            ];
            assert_eq!(calculate_visual_line_count(&lines, 80), 3);
        }

        /// Tests count with zero width (fallback to logical count).
        #[test]
        fn zero_width_returns_logical_count() {
            let lines = vec![OutputLine::stdout("line1"), OutputLine::stdout("line2")];
            assert_eq!(calculate_visual_line_count(&lines, 0), 2);
        }
    }
}
