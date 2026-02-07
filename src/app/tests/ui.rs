//! UI and output panel tests.
//!
//! Tests for the TUI output display including:
//! - Output truncation behavior
//! - Scroll key handling
//! - Visual line counting for wrapped text

use super::helpers::*;
use crate::app::*;
use crate::tui::widgets::{MAX_OUTPUT_LINES, OutputLine, calculate_visual_line_count};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// =============================================================================
// Output Truncation Tests
// =============================================================================

/// Helper to create app with output lines.
fn create_app_with_output(num_lines: usize) -> App {
    let mut app = create_test_app_with_lines(&["test"], 0, 0);
    for i in 0..num_lines {
        app.flow_ui
            .output
            .push(OutputLine::stdout(format!("Line {i}")));
    }
    app
}

#[test]
fn output_not_truncated_at_4999_lines() {
    let mut app = create_app_with_output(4999);
    assert_eq!(app.flow_ui.output.len(), 4999, "Should have 4999 lines");
    assert!(
        !app.flow_ui.output_truncated,
        "Should not be marked as truncated"
    );

    // Add one more line via event processing
    app.flow_ui.output.push(OutputLine::stdout("Line 4999"));

    assert_eq!(app.flow_ui.output.len(), 5000, "Should have 5000 lines");
    assert!(
        !app.flow_ui.output_truncated,
        "Should not be truncated at exactly 5000 lines"
    );
}

#[test]
fn output_is_truncated_at_5001_lines() {
    let mut app = create_app_with_output(5000);
    let original_offset = app.flow_ui.output_scroll.offset;

    // Simulate receiving a new output line that exceeds the limit
    // Manually trigger the truncation logic from process_events
    app.flow_ui.output.push(OutputLine::stdout("Line 5000"));

    if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
        let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
        app.flow_ui.output.drain(0..drain_count);
        app.flow_ui.output_scroll.offset =
            app.flow_ui.output_scroll.offset.saturating_sub(drain_count);
        app.flow_ui.output_truncated = true;
    }

    assert_eq!(
        app.flow_ui.output.len(),
        MAX_OUTPUT_LINES,
        "Should be truncated to exactly MAX_OUTPUT_LINES"
    );
    assert!(
        app.flow_ui.output_truncated,
        "Should be marked as truncated"
    );
    assert_eq!(
        app.flow_ui.output_scroll.offset,
        original_offset.saturating_sub(1),
        "Scroll offset should be adjusted"
    );
}

#[test]
fn truncation_removes_oldest_lines() {
    let mut app = create_app_with_output(5000);

    // First line should be "Line 0"
    assert_eq!(app.flow_ui.output[0].text, "Line 0");

    // Add one more line to trigger truncation
    app.flow_ui.output.push(OutputLine::stdout("Line 5000"));
    if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
        let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
        app.flow_ui.output.drain(0..drain_count);
        app.flow_ui.output_truncated = true;
    }

    // First line should now be "Line 1" (oldest was removed)
    assert_eq!(
        app.flow_ui.output[0].text, "Line 1",
        "Oldest line should be removed"
    );
    // Last line should be the new one
    assert_eq!(
        app.flow_ui.output[app.flow_ui.output.len() - 1].text,
        "Line 5000",
        "Newest line should be at end"
    );
}

#[test]
fn scroll_offset_adjusted_when_truncation_removes_lines() {
    let mut app = create_app_with_output(5000);
    app.flow_ui.output_scroll.offset = 100;

    // Add multiple lines to trigger truncation
    for i in 5000..5010 {
        app.flow_ui
            .output
            .push(OutputLine::stdout(format!("Line {i}")));
        if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
            let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
            app.flow_ui.output.drain(0..drain_count);
            app.flow_ui.output_scroll.offset =
                app.flow_ui.output_scroll.offset.saturating_sub(drain_count);
            app.flow_ui.output_truncated = true;
        }
    }

    // Offset should be adjusted down by the number of removed lines (10)
    assert_eq!(
        app.flow_ui.output_scroll.offset, 90,
        "Scroll offset should be decremented by drain count"
    );
    assert_eq!(
        app.flow_ui.output.len(),
        MAX_OUTPUT_LINES,
        "Should maintain max lines"
    );
}

#[test]
fn truncated_flag_set_correctly() {
    let mut app = create_app_with_output(4990);
    assert!(
        !app.flow_ui.output_truncated,
        "Should not be truncated initially"
    );

    // Add lines up to and past the limit
    for i in 4990..5005 {
        app.flow_ui
            .output
            .push(OutputLine::stdout(format!("Line {i}")));
        if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
            let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
            app.flow_ui.output.drain(0..drain_count);
            app.flow_ui.output_truncated = true;
        }
    }

    assert!(
        app.flow_ui.output_truncated,
        "Truncated flag should be set after exceeding limit"
    );
}

#[test]
fn multiple_truncation_events() {
    let mut app = create_app_with_output(5000);

    // First truncation
    app.flow_ui.output.push(OutputLine::stdout("First extra"));
    if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
        let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
        app.flow_ui.output.drain(0..drain_count);
        app.flow_ui.output_truncated = true;
    }
    assert_eq!(app.flow_ui.output.len(), MAX_OUTPUT_LINES);

    // Second truncation
    app.flow_ui.output.push(OutputLine::stdout("Second extra"));
    if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
        let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
        app.flow_ui.output.drain(0..drain_count);
    }
    assert_eq!(app.flow_ui.output.len(), MAX_OUTPUT_LINES);

    // Third truncation
    app.flow_ui.output.push(OutputLine::stdout("Third extra"));
    if app.flow_ui.output.len() > MAX_OUTPUT_LINES {
        let drain_count = app.flow_ui.output.len() - MAX_OUTPUT_LINES;
        app.flow_ui.output.drain(0..drain_count);
    }

    assert_eq!(
        app.flow_ui.output.len(),
        MAX_OUTPUT_LINES,
        "Should maintain max after multiple truncations"
    );
    assert!(
        app.flow_ui.output_truncated,
        "Truncated flag should remain set"
    );
}

// =============================================================================
// Scroll Key Handling Tests
// =============================================================================

/// Helper to create app with many output lines for scrolling.
fn create_scrollable_app() -> App {
    let mut app = create_test_app_with_lines(&["test"], 0, 0);
    // Add 100 lines for scrolling tests
    for i in 0..100 {
        app.flow_ui
            .output
            .push(OutputLine::stdout(format!("Line {i}")));
    }
    // Set layout dimensions for scroll calculations
    app.layout.chat.output_visible_height = 20;
    app.layout.chat.output_content_width = 80;
    app
}

#[test]
fn ctrl_up_scrolls_output_up() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 10;

    let key = KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, 9,
        "Ctrl+Up should decrement offset"
    );
    assert!(
        !app.flow_ui.output_scroll.auto_scroll,
        "Auto-scroll should be disabled"
    );
}

#[test]
fn ctrl_down_scrolls_output_down() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 10;

    let key = KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, 11,
        "Ctrl+Down should increment offset"
    );
}

#[test]
fn ctrl_down_caps_at_max_scroll() {
    let mut app = create_scrollable_app();
    let max_scroll = 100 - 20; // content_len - visible_height = 80
    app.flow_ui.output_scroll.offset = max_scroll;

    let key = KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, max_scroll,
        "Should not exceed max scroll"
    );
    assert!(
        app.flow_ui.output_scroll.auto_scroll,
        "Should enable auto-scroll at bottom"
    );
}

#[test]
fn page_up_scrolls_by_page_size() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 50;

    let key = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, 40,
        "PageUp should scroll by SCROLL_PAGE_SIZE (10)"
    );
    assert!(
        !app.flow_ui.output_scroll.auto_scroll,
        "Auto-scroll should be disabled"
    );
}

#[test]
fn page_down_scrolls_by_page_size() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 10;

    let key = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, 20,
        "PageDown should scroll by SCROLL_PAGE_SIZE (10)"
    );
}

#[test]
fn ctrl_home_scrolls_to_top() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 50;
    app.flow_ui.output_scroll.auto_scroll = true;

    let key = KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL);
    app.handle_key(key);

    assert_eq!(
        app.flow_ui.output_scroll.offset, 0,
        "Ctrl+Home should jump to top"
    );
    assert!(
        !app.flow_ui.output_scroll.auto_scroll,
        "Auto-scroll should be disabled"
    );
}

#[test]
fn ctrl_end_scrolls_to_bottom() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 10;
    app.flow_ui.output_scroll.auto_scroll = false;

    let key = KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL);
    app.handle_key(key);

    let max_scroll = 100 - 20; // 80
    assert_eq!(
        app.flow_ui.output_scroll.offset, max_scroll,
        "Ctrl+End should jump to bottom"
    );
    assert!(
        app.flow_ui.output_scroll.auto_scroll,
        "Auto-scroll should be re-enabled"
    );
}

#[test]
fn auto_scroll_re_engages_when_scrolling_to_bottom_manually() {
    let mut app = create_scrollable_app();
    app.flow_ui.output_scroll.offset = 10;
    app.flow_ui.output_scroll.auto_scroll = false;

    // Scroll all the way down manually
    let max_scroll = 100 - 20;
    for _ in 0..(max_scroll - 10) {
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL);
        app.handle_key(key);
    }

    assert_eq!(app.flow_ui.output_scroll.offset, max_scroll);
    assert!(
        app.flow_ui.output_scroll.auto_scroll,
        "Auto-scroll should re-engage when reaching bottom"
    );
}

#[test]
fn scrolling_up_from_bottom_disables_auto_scroll() {
    let mut app = create_scrollable_app();
    let max_scroll = 100 - 20;
    app.flow_ui.output_scroll.offset = max_scroll;
    app.flow_ui.output_scroll.auto_scroll = true;

    let key = KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL);
    app.handle_key(key);

    assert_eq!(app.flow_ui.output_scroll.offset, max_scroll - 1);
    assert!(
        !app.flow_ui.output_scroll.auto_scroll,
        "Scrolling up should disable auto-scroll"
    );
}

// =============================================================================
// Visual Line Counting Tests
// =============================================================================

#[test]
fn short_lines_no_wrapping() {
    let lines = vec![
        OutputLine::stdout("Hello"),
        OutputLine::stdout("World"),
        OutputLine::stdout("Test"),
    ];
    let count = calculate_visual_line_count(&lines, 80);
    assert_eq!(count, 3, "Short lines should not wrap");
}

#[test]
fn long_lines_wrap_multiple_times() {
    let long_line = "a".repeat(100); // 100 chars
    let lines = vec![OutputLine::stdout(&long_line)];
    let count = calculate_visual_line_count(&lines, 20);
    assert_eq!(count, 5, "100-char line should wrap to 5 lines at width 20");
}

#[test]
fn mixed_short_and_long_lines() {
    let lines = vec![
        OutputLine::stdout("Short"),                    // 5 chars -> 1 line
        OutputLine::stdout("This is a very long line"), // 25 chars -> 2 lines at width 20
        OutputLine::stdout("Tiny"),                     // 4 chars -> 1 line
    ];
    let count = calculate_visual_line_count(&lines, 20);
    assert_eq!(count, 4, "Mixed lines should wrap correctly");
}

#[test]
fn various_content_widths_narrow() {
    let lines = vec![OutputLine::stdout("Hello, World!")]; // 13 chars
    let count_narrow = calculate_visual_line_count(&lines, 10);
    assert_eq!(count_narrow, 2, "Should wrap to 2 lines at narrow width");
}

#[test]
fn various_content_widths_wide() {
    let lines = vec![OutputLine::stdout("Hello, World!")]; // 13 chars
    let count_wide = calculate_visual_line_count(&lines, 100);
    assert_eq!(count_wide, 1, "Should not wrap at wide width");
}

#[test]
fn zero_width_content_area() {
    let lines = vec![OutputLine::stdout("Test 1"), OutputLine::stdout("Test 2")];
    let count = calculate_visual_line_count(&lines, 0);
    assert_eq!(count, 2, "Zero width should return logical line count");
}

#[test]
fn empty_lines_count_as_one() {
    let lines = vec![
        OutputLine::stdout("Text"),
        OutputLine::stdout(""),
        OutputLine::stdout("More"),
    ];
    let count = calculate_visual_line_count(&lines, 80);
    assert_eq!(count, 3, "Empty lines should count as 1 visual line");
}

#[test]
fn wrapping_exactly_at_boundary() {
    let line_20 = "a".repeat(20);
    let lines = vec![OutputLine::stdout(&line_20)];
    let count = calculate_visual_line_count(&lines, 20);
    assert_eq!(count, 1, "Line exactly at width should not wrap");
}

#[test]
fn wrapping_one_over_boundary() {
    let line_21 = "a".repeat(21);
    let lines = vec![OutputLine::stdout(&line_21)];
    let count = calculate_visual_line_count(&lines, 20);
    assert_eq!(count, 2, "Line one char over should wrap to 2 lines");
}

// =============================================================================
// Render-Path Text Source Tests (Task 003)
// =============================================================================
//
// These tests verify that the chat input rendering reads the correct text source
// (flow.input_text when running, text_input.textarea when idle) without mutating
// app state. This enforces the Ratatui immediate-mode principle: state owns data,
// rendering is a pure read.

/// Test that rendering in running mode displays `flow.input_text`, not textarea content.
///
/// When the app is running, `render_chat_input` creates a temporary `TextArea` from
/// `flow.input_text`. The rendered buffer should contain the flow text, not the
/// editable textarea content.
#[test]
fn render_chat_input_running_uses_flow_input_text() -> Result<()> {
    let mut app = create_test_app_with_lines(&["User draft content"], 0, 0);

    // Set up distinct content in flow.input_text vs textarea
    app.flow.input_text = "Flow task text\nWith completed tasks".to_string();
    app.is_running = true;

    let terminal = render_app_to_terminal(&mut app, 60, 20)?;
    terminal
        .backend()
        .assert_buffer_lines(styled_lines_from_buffer(
            &terminal,
            &[
                " McGravity [Codex/Codex]",
                "┌Output────────────────────────────────────────────────────┐",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "└──────────────────────────────────────────────────────────┘",
                "┌ Task (Readonly) ─────────────────────────────────────────┐",
                "│Flow task text                                            │",
                "│With completed tasks                                      │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "└ \\+Enter for newline ─────────────────────────────────────┘",
                " · Waiting for input",
                "   Ready to process tasks",
                " 0/1 files  ────────────────────────────────────────────────",
                " [Esc] Cancel",
            ],
        ));

    Ok(())
}

/// Test that rendering in idle mode displays `text_input.textarea`, not `flow.input_text`.
///
/// When the app is not running, `render_chat_input` uses the textarea clone.
/// The rendered buffer should contain the editable textarea content.
#[test]
fn render_chat_input_idle_uses_textarea() -> Result<()> {
    let mut app = create_test_app_with_lines(&["Editable task text"], 0, 0);

    // Set different content in flow.input_text (should NOT appear in idle mode)
    app.flow.input_text = "Stale flow text from previous run".to_string();
    app.is_running = false;

    let terminal = render_app_to_terminal(&mut app, 60, 20)?;
    terminal
        .backend()
        .assert_buffer_lines(styled_lines_from_buffer(
            &terminal,
            &[
                " McGravity [Codex/Codex]",
                "┌Output (waiting for input)────────────────────────────────┐",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "│                                                          │",
                "└──────────────────────────────────────────────────────────┘",
                " · Waiting for input",
                "   Ready to process tasks",
                "",
                "┌ Task Text ───────────────────────────────────────────────┐",
                "│Editable task text                                        │",
                "│                                                          │",
                "│                                                          │",
                "└ \\+Enter for newline ─────────────────────────────────────┘",
                " [Enter] Submit  [Ctrl+S] Settings",
            ],
        ));

    Ok(())
}

/// Test that rendering does not mutate app state (render purity).
///
/// Ratatui immediate-mode rendering must be a pure read of current state.
/// After rendering, no app fields should be changed.
#[test]
fn render_chat_does_not_mutate_state() -> Result<()> {
    let mut app = create_test_app_with_lines(&["Test task content"], 0, 0);
    app.flow.input_text = "Flow text for readonly display".to_string();
    app.is_running = true;

    // Capture state before rendering
    let flow_text_before = app.flow.input_text.clone();
    let textarea_lines_before: Vec<String> = app.text_input.lines().to_vec();
    let is_running_before = app.is_running;
    let mode_before = app.mode;
    let phase_before = app.flow.phase.clone();

    // Render (should be a pure read)
    let _terminal = render_app_to_terminal(&mut app, 60, 20)?;

    // Verify nothing was mutated
    assert_eq!(
        app.flow.input_text, flow_text_before,
        "Rendering should not mutate flow.input_text"
    );
    assert_eq!(
        app.text_input.lines(),
        textarea_lines_before.as_slice(),
        "Rendering should not mutate text_input.textarea"
    );
    assert_eq!(
        app.is_running, is_running_before,
        "Rendering should not mutate is_running"
    );
    assert_eq!(app.mode, mode_before, "Rendering should not mutate mode");
    assert_eq!(
        app.flow.phase, phase_before,
        "Rendering should not mutate flow.phase"
    );

    Ok(())
}

/// Test that rendering in idle mode does not mutate state either.
#[test]
fn render_chat_idle_does_not_mutate_state() -> Result<()> {
    let mut app = create_test_app_with_lines(&["Idle task"], 0, 0);
    app.is_running = false;

    let flow_text_before = app.flow.input_text.clone();
    let textarea_lines_before: Vec<String> = app.text_input.lines().to_vec();

    let _terminal = render_app_to_terminal(&mut app, 60, 20)?;

    assert_eq!(
        app.flow.input_text, flow_text_before,
        "Idle rendering should not mutate flow.input_text"
    );
    assert_eq!(
        app.text_input.lines(),
        textarea_lines_before.as_slice(),
        "Idle rendering should not mutate text_input.textarea"
    );

    Ok(())
}
