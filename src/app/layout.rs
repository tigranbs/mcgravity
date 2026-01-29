//! Layout calculation helpers for the TUI.
//!
//! This module provides a single source of truth for layout definitions,
//! ensuring that dimension calculations in `App::update_layout_heights`
//! and rendering in `App::render` are always in sync.
//!
//! The main layout used is [`ChatLayout`] for the unified chat mode interface.

use ratatui::layout::{Constraint, Layout, Rect};

/// Layout information for the chat-like unified mode.
///
/// This layout combines text input, status indicator, and output areas
/// into a single chat-like interface where:
/// - Output/conversation scrolls above
/// - Status and progress are below the main panels
/// - Text input (composer) is at the bottom when idle; readonly task shares space with output when running
#[derive(Debug, Clone, Copy, Default)]
pub struct ChatLayout {
    /// Header area (minimal, 1 line).
    pub header: Rect,
    /// CLI output area (scrollable, main content).
    pub output: Rect,
    /// Status indicator area (2 lines for phase + operation).
    pub status: Rect,
    /// Progress bar area (1 line, may be hidden when idle).
    pub progress: Rect,
    /// Text input area (composer) or readonly task panel when running.
    pub input: Rect,
    /// Footer area (key hints, 1 line).
    pub footer: Rect,
    /// Visible height of output panel (excluding borders).
    pub output_visible_height: usize,
    /// Content width for output panel (excluding borders and scrollbar).
    pub output_content_width: usize,
    /// Inner content width for input area (excluding borders).
    pub input_inner_width: usize,
    /// Inner content height for input area (excluding borders).
    pub input_inner_height: usize,
}

/// Layout constraints for chat mode when idle (editable input).
const CHAT_LAYOUT_IDLE_CONSTRAINTS: [Constraint; 6] = [
    Constraint::Length(1), // Header (minimal)
    Constraint::Min(5),    // Output (grows)
    Constraint::Length(2), // Status (2 lines)
    Constraint::Length(1), // Progress bar
    Constraint::Length(5), // Input area (min 5 lines for comfortable typing)
    Constraint::Length(1), // Footer (key hints)
];

/// Layout constraints for chat mode when running (readonly task split).
const CHAT_LAYOUT_RUNNING_CONSTRAINTS: [Constraint; 6] = [
    Constraint::Length(1), // Header (minimal)
    Constraint::Fill(1),   // Output (shares remaining space)
    Constraint::Fill(1),   // Task (readonly, shares remaining space)
    Constraint::Length(2), // Status (2 lines)
    Constraint::Length(1), // Progress bar
    Constraint::Length(1), // Footer (key hints)
];

/// Calculates the layout for chat mode.
///
/// This function should be used by both `update_layout_heights` and
/// `render_chat` to ensure consistent dimension calculations.
/// When `is_running` is true, the output and task panels split the main space.
#[must_use]
pub fn calculate_chat_layout(area: Rect, is_running: bool) -> ChatLayout {
    let chunks = if is_running {
        Layout::vertical(CHAT_LAYOUT_RUNNING_CONSTRAINTS).split(area)
    } else {
        Layout::vertical(CHAT_LAYOUT_IDLE_CONSTRAINTS).split(area)
    };

    let (output, input, status, progress, footer) = if is_running {
        (chunks[1], chunks[2], chunks[3], chunks[4], chunks[5])
    } else {
        (chunks[1], chunks[4], chunks[2], chunks[3], chunks[5])
    };

    // Calculate inner dimensions
    // Output: subtract 2 for borders (top + bottom), 1 for scrollbar
    let output_visible_height = output.height.saturating_sub(2) as usize;
    let output_content_width = output.width.saturating_sub(3) as usize;
    // Input: subtract 2 for borders on each axis
    let input_inner_width = input.width.saturating_sub(2) as usize;
    let input_inner_height = input.height.saturating_sub(2) as usize;

    ChatLayout {
        header: chunks[0],
        output,
        status,
        progress,
        input,
        footer,
        output_visible_height,
        output_content_width,
        input_inner_width,
        input_inner_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_layout_calculation() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = calculate_chat_layout(area, false);

        // Header should be 1 line (minimal)
        assert_eq!(layout.header.height, 1);
        // Status should be 2 lines
        assert_eq!(layout.status.height, 2);
        // Progress should be 1 line
        assert_eq!(layout.progress.height, 1);
        // Input should be 5 lines
        assert_eq!(layout.input.height, 5);
        // Footer should be 1 line
        assert_eq!(layout.footer.height, 1);
        // Output should take the rest (24 - 1 - 2 - 1 - 5 - 1 = 14)
        assert_eq!(layout.output.height, 14);

        // Inner dimensions account for borders
        assert_eq!(layout.output_visible_height, 12); // 14 - 2
        assert_eq!(layout.output_content_width, 77); // 80 - 3
        assert_eq!(layout.input_inner_width, 78); // 80 - 2
        assert_eq!(layout.input_inner_height, 3); // 5 - 2
    }

    #[test]
    fn test_chat_layout_small_terminal() {
        // Test with minimum viable terminal size
        let area = Rect::new(0, 0, 40, 15);
        let layout = calculate_chat_layout(area, false);

        // Should not panic and should produce valid layout
        // Fixed height elements: 1 + 2 + 1 + 5 + 1 = 10
        // Remaining for output: 15 - 10 = 5
        assert_eq!(layout.header.height, 1);
        assert_eq!(layout.status.height, 2);
        assert_eq!(layout.progress.height, 1);
        assert_eq!(layout.input.height, 5);
        assert_eq!(layout.footer.height, 1);
        assert_eq!(layout.output.height, 5);

        // Output visible height should be 3 (5 - 2 for borders)
        assert_eq!(layout.output_visible_height, 3);
    }

    #[test]
    fn test_chat_layout_all_areas_are_positioned_correctly() {
        let area = Rect::new(0, 0, 100, 30);
        let layout = calculate_chat_layout(area, false);

        // Header starts at y=0
        assert_eq!(layout.header.y, 0);
        // Output starts after header
        assert_eq!(layout.output.y, 1);
        // Status starts after output (1 + remaining height = 20)
        // Output gets: 30 - 1 - 2 - 1 - 5 - 1 = 20
        assert_eq!(layout.status.y, 21);
        // Progress starts after status
        assert_eq!(layout.progress.y, 23);
        // Input starts after progress
        assert_eq!(layout.input.y, 24);
        // Footer starts after input
        assert_eq!(layout.footer.y, 29);

        // All areas have full width
        assert_eq!(layout.header.width, 100);
        assert_eq!(layout.output.width, 100);
        assert_eq!(layout.status.width, 100);
        assert_eq!(layout.progress.width, 100);
        assert_eq!(layout.input.width, 100);
        assert_eq!(layout.footer.width, 100);
    }

    #[test]
    fn test_chat_layout_running_balanced_split() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = calculate_chat_layout(area, true);

        assert_eq!(layout.header.height, 1);
        assert_eq!(layout.status.height, 2);
        assert_eq!(layout.progress.height, 1);
        assert_eq!(layout.footer.height, 1);

        let fixed_height = layout.header.height
            + layout.status.height
            + layout.progress.height
            + layout.footer.height;
        let remaining = area.height.saturating_sub(fixed_height);

        assert_eq!(layout.output.height + layout.input.height, remaining);
        assert!(layout.output.height > 5);
        assert!(layout.input.height > 5);
        assert!(layout.output.height.abs_diff(layout.input.height) <= 1);

        assert_eq!(layout.output.y, layout.header.y + layout.header.height);
        assert_eq!(layout.input.y, layout.output.y + layout.output.height);
        assert_eq!(layout.status.y, layout.input.y + layout.input.height);
        assert_eq!(layout.progress.y, layout.status.y + layout.status.height);
        assert_eq!(layout.footer.y, layout.progress.y + layout.progress.height);
    }
}
