//! Input handling tests for the app module.
//!
//! This module contains tests for:
//! - Paste handling
//! - Key bindings (Enter, `Shift+Enter`, `Ctrl+Enter`, etc.)
//! - Rapid input detection
//! - Text input state management

use super::helpers::*;
use crate::app::state::AppMode;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

// =============================================================================
// Paste Handling Tests
// =============================================================================

mod paste_handling_tests {
    use super::*;

    #[test]
    fn test_paste_single_character() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("a");
        assert_eq!(app.text_input.lines(), vec!["a"]);
        assert_eq!(app.text_input.cursor().1, 1);
    }

    #[test]
    fn test_paste_single_line() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("hello world");
        assert_eq!(app.text_input.lines(), vec!["hello world"]);
        assert_eq!(app.text_input.cursor().1, 11);
    }

    #[test]
    fn test_paste_multi_line_unix() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("line1\nline2\nline3");
        assert_eq!(app.text_input.lines(), vec!["line1", "line2", "line3"]);
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_windows_line_endings() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("line1\r\nline2");
        // Should normalize \r\n to single newline
        assert_eq!(app.text_input.lines(), vec!["line1", "line2"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_old_mac_line_endings() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("line1\rline2");
        // Old Mac style \r should also create newlines
        assert_eq!(app.text_input.lines(), vec!["line1", "line2"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_mixed_line_endings() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Mix of \r\n and \n
        app.handle_paste("line1\r\nline2\nline3\r\nline4");
        assert_eq!(
            app.text_input.lines(),
            vec!["line1", "line2", "line3", "line4"]
        );
        assert_eq!(app.text_input.cursor().0, 3);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_at_symbol_triggers_token_detection() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("@src/main.rs");
        assert_eq!(app.text_input.lines(), vec!["@src/main.rs"]);
        // The @ token detection is triggered by insert_char
        // The token should be detected
        assert!(app.text_input.at_token.is_some());
    }

    #[test]
    fn test_paste_in_middle_of_text() {
        let mut app = create_test_app_with_lines(&["abc"], 0, 1);
        app.handle_paste("XY");
        assert_eq!(app.text_input.lines(), vec!["aXYbc"]);
        assert_eq!(app.text_input.cursor().1, 3);
    }

    #[test]
    fn test_paste_at_line_boundary() {
        let mut app = create_test_app_with_lines(&["line1", ""], 1, 0);
        app.handle_paste("pasted");
        assert_eq!(app.text_input.lines(), vec!["line1", "pasted"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 6);
    }

    #[test]
    fn test_paste_empty_string() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.handle_paste("");
        // Should do nothing
        assert_eq!(app.text_input.lines(), vec!["hello"]);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_in_settings_mode_ignored() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.mode = AppMode::Settings;
        app.handle_paste("pasted text");
        // Should be ignored in settings mode
        assert_eq!(app.text_input.lines(), vec!["hello"]);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_with_trailing_newline() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("text\n");
        assert_eq!(app.text_input.lines(), vec!["text", ""]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_paste_with_leading_newline() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("\ntext");
        assert_eq!(app.text_input.lines(), vec!["", "text"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 4);
    }

    #[test]
    fn test_paste_only_newlines() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("\n\n\n");
        assert_eq!(app.text_input.lines(), vec!["", "", "", ""]);
        assert_eq!(app.text_input.cursor().0, 3);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_paste_unicode_content() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("Hello 世界\n日本語");
        assert_eq!(app.text_input.lines(), vec!["Hello 世界", "日本語"]);
        assert_eq!(app.text_input.cursor().0, 1);
        // "日本語" is 3 characters (cursor position is character-based)
        assert_eq!(app.text_input.cursor().1, 3);
    }

    #[test]
    fn test_paste_emoji() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("emoji 🎉🚀");
        assert_eq!(app.text_input.lines(), vec!["emoji 🎉🚀"]);
    }

    #[test]
    fn test_paste_control_characters_filtered() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Control characters other than newlines should be filtered
        app.handle_paste("hello\x00\x07world");
        assert_eq!(app.text_input.lines(), vec!["helloworld"]);
    }

    #[test]
    fn test_paste_preserves_existing_content() {
        let mut app = create_test_app_with_lines(&["existing text"], 0, 8);
        // Cursor after "existing"
        app.handle_paste(" NEW");
        assert_eq!(app.text_input.lines(), vec!["existing NEW text"]);
    }

    #[test]
    fn test_paste_multiline_in_middle() {
        let mut app = create_test_app_with_lines(&["hello world"], 0, 6);
        // Cursor after "hello "
        app.handle_paste("new\nlines");
        assert_eq!(app.text_input.lines(), vec!["hello new", "linesworld"]);
    }

    #[test]
    fn test_paste_at_beginning_of_line() {
        let mut app = create_test_app_with_lines(&["world"], 0, 0);
        app.handle_paste("hello ");
        assert_eq!(app.text_input.lines(), vec!["hello world"]);
        assert_eq!(app.text_input.cursor().1, 6);
    }

    #[test]
    fn test_paste_at_end_of_line() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.handle_paste(" world");
        assert_eq!(app.text_input.lines(), vec!["hello world"]);
        assert_eq!(app.text_input.cursor().1, 11);
    }

    #[test]
    fn test_paste_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("task text\n");
        // Verify the text was inserted but not submitted
        // (flow should not have been started, is_running should be false)
        assert!(!app.is_running());
        assert_eq!(app.text_input.lines(), vec!["task text", ""]);
    }

    /// Test that pasting multi-line text does not trigger submission.
    /// This is critical because plain Enter now submits the task, but pasted
    /// newlines should NOT submit - they should just insert newlines.
    #[test]
    fn test_paste_multiline_does_not_trigger_submission() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Simulate pasting multi-line text
        app.handle_paste("line1\nline2\nline3");

        // Verify: text was inserted correctly
        assert_eq!(app.text_input.lines(), vec!["line1", "line2", "line3"]);

        // Verify: flow was NOT started (is_running should still be false)
        assert!(
            !app.is_running(),
            "Paste should not trigger submission even with newlines"
        );

        // Verify: cursor is at correct position (end of last line)
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 5);

        // Verify: app is not quitting
        assert!(!app.should_quit());
    }

    /// Test that pasting text with trailing newline does not submit.
    /// This is important because a trailing newline in pasted text should
    /// create an empty line, not trigger submission.
    #[test]
    fn test_paste_with_trailing_newline_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Paste text ending with newline
        app.handle_paste("task description\n");

        // Verify: text was inserted with empty line at end
        assert_eq!(app.text_input.lines(), vec!["task description", ""]);

        // Verify: NOT submitted
        assert!(
            !app.is_running(),
            "Paste with trailing newline should not submit"
        );

        // Verify: cursor is on the new empty line
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    /// Test that multiple consecutive newlines in paste don't trigger submission.
    #[test]
    fn test_paste_multiple_newlines_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Paste text with multiple consecutive newlines
        app.handle_paste("paragraph1\n\n\nparagraph2");

        // Verify: text was inserted correctly with empty lines
        assert_eq!(
            app.text_input.lines(),
            vec!["paragraph1", "", "", "paragraph2"]
        );

        // Verify: NOT submitted
        assert!(
            !app.is_running(),
            "Paste with multiple newlines should not submit"
        );
    }

    /// Test paste with only newlines doesn't submit.
    #[test]
    fn test_paste_only_newlines_does_not_submit() {
        let mut app = create_test_app_with_lines(&["task"], 0, 4);

        // Paste just newlines
        app.handle_paste("\n\n");

        // Verify: newlines were inserted
        assert_eq!(app.text_input.lines(), vec!["task", "", ""]);

        // Verify: NOT submitted
        assert!(
            !app.is_running(),
            "Paste with only newlines should not submit"
        );
    }

    #[test]
    fn test_paste_multiline_preserves_existing_content() {
        let mut app = create_test_app_with_lines(&["existing", "content"], 1, 7);
        app.handle_paste(" more");
        assert_eq!(app.text_input.lines(), vec!["existing", "content more"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 12);
    }

    #[test]
    fn test_paste_updates_cursor_row_for_multiline() {
        let mut app = create_test_app_with_lines(&["start"], 0, 5);
        // Paste 3 lines at the end
        app.handle_paste("\nline2\nline3");
        assert_eq!(app.text_input.lines(), vec!["start", "line2", "line3"]);
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_with_crlf_normalization() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Windows-style line endings should become single newlines
        app.handle_paste("line1\r\nline2\r\nline3");
        assert_eq!(app.text_input.lines(), vec!["line1", "line2", "line3"]);
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_multiline_splits_current_line() {
        let mut app = create_test_app_with_lines(&["start end"], 0, 6);
        // Cursor is after "start " (position 6)
        app.handle_paste("line1\nline2\n");
        // Should result in: "start line1", "line2", "end"
        assert_eq!(app.text_input.lines(), vec!["start line1", "line2", "end"]);
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_paste_unicode_cursor_position() {
        let mut app = create_test_app_with_lines(&["hello "], 0, 6);
        app.handle_paste("世界");
        assert_eq!(app.text_input.lines(), vec!["hello 世界"]);
        // Cursor should be at character position after the unicode characters
        // "hello " is 6 characters, "世界" is 2 characters
        assert_eq!(app.text_input.cursor().1, 8);
    }

    #[test]
    fn test_paste_with_at_mention_triggers_detection() {
        let mut app = create_test_app_with_lines(&["check "], 0, 6);
        app.handle_paste("@src/main.rs");
        assert_eq!(app.text_input.lines(), vec!["check @src/main.rs"]);
        // @ token should be detected
        assert!(app.text_input.at_token.is_some());
        if let Some(token) = &app.text_input.at_token {
            assert_eq!(token.query, "src/main.rs");
        }
    }

    #[test]
    fn test_paste_multiple_at_mentions() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("check @file1.rs and @file2.rs");
        assert_eq!(
            app.text_input.lines(),
            vec!["check @file1.rs and @file2.rs"]
        );
        // Cursor is at end, so the @ token should be @file2.rs
        assert!(app.text_input.at_token.is_some());
        if let Some(token) = &app.text_input.at_token {
            assert_eq!(token.query, "file2.rs");
        }
    }

    #[test]
    fn test_paste_large_text() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Create a larger paste with many lines
        let lines: Vec<&str> = (0..100).map(|_| "line content").collect();
        let large_text = lines.join("\n");
        app.handle_paste(&large_text);
        assert_eq!(app.text_input.lines().len(), 100);
        assert_eq!(app.text_input.cursor().0, 99);
    }

    #[test]
    fn test_paste_tabs_filtered() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Tabs are control characters and should be filtered
        app.handle_paste("hello\tworld");
        assert_eq!(app.text_input.lines(), vec!["helloworld"]);
    }

    #[test]
    fn test_paste_in_multiline_document() {
        let mut app = create_test_app_with_lines(&["line1", "line2", "line3"], 1, 5);
        // Cursor at end of "line2"
        app.handle_paste(" extra");
        assert_eq!(
            app.text_input.lines(),
            vec!["line1", "line2 extra", "line3"]
        );
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 11);
    }

    #[test]
    fn test_paste_emoji_sequence() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Test various emoji including multi-codepoint sequences
        app.handle_paste("🎉🚀✨👋");
        assert_eq!(app.text_input.lines(), vec!["🎉🚀✨👋"]);
    }

    #[test]
    fn test_paste_mixed_content_unicode_and_ascii() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("Hello 世界! 🎉");
        assert_eq!(app.text_input.lines(), vec!["Hello 世界! 🎉"]);
    }

    #[test]
    fn test_paste_consecutive_newlines() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Multiple consecutive newlines should create empty lines
        app.handle_paste("line1\n\n\nline4");
        assert_eq!(app.text_input.lines(), vec!["line1", "", "", "line4"]);
        assert_eq!(app.text_input.cursor().0, 3);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_paste_whitespace_only() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.handle_paste("   ");
        assert_eq!(app.text_input.lines(), vec!["hello   "]);
        assert_eq!(app.text_input.cursor().1, 8);
    }

    #[test]
    fn test_paste_into_empty_document() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.handle_paste("first line\nsecond line");
        assert_eq!(app.text_input.lines(), vec!["first line", "second line"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 11);
    }
}

// =============================================================================
// Key Binding Tests - Comprehensive tests for key binding behavior
// =============================================================================

mod key_binding_tests {
    use super::*;

    // =========================================================================
    // Submit Key Tests (Ctrl+Enter and Ctrl+D)
    // =========================================================================

    #[tokio::test]
    async fn test_ctrl_enter_submits() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Ctrl+Enter should submit
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // After submission, the input should be cleared
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_ctrl_d_submits() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Ctrl+D should also submit (alternative submit method)
        let ctrl_d = KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        app.handle_key(ctrl_d);

        // After submission, the input should be cleared
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_multiline_submit_clears_all_lines() {
        let mut app = create_test_app_with_lines(&["line1", "line2", "line3"], 2, 5);
        // Multiple lines of content

        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // All lines should be cleared after submission
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_ctrl_enter_with_empty_input_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Empty input should NOT be submitted
        // The submit_text_input() has a guard for empty text
        assert_eq!(app.text_input.lines(), vec![""]);
        assert!(!app.is_running);
    }

    #[test]
    fn test_ctrl_enter_with_whitespace_only_does_not_submit() {
        let mut app = create_test_app_with_lines(&["   "], 0, 3);

        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Whitespace-only input should NOT be submitted
        // The submit_text_input() trims and checks for empty
        assert_eq!(app.text_input.lines(), vec!["   "]);
        assert!(!app.is_running);
    }

    #[test]
    fn test_ctrl_d_empty_input_not_submitted() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        let ctrl_d = KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        app.handle_key(ctrl_d);

        // Should still have empty input
        assert_eq!(app.text_input.lines(), vec![""]);
        assert!(!app.is_running);
    }

    #[test]
    fn test_ctrl_enter_only_empty_lines_not_submitted() {
        let mut app = create_test_app_with_lines(&["", "", ""], 2, 0);

        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Should NOT have submitted (all empty lines = no content)
        // Lines may be unchanged or reduced, but is_running should be false
        assert!(!app.is_running);
    }

    // =========================================================================
    // Newline Key Tests (Shift+Enter, Alt+Enter)
    // =========================================================================

    #[test]
    fn test_shift_enter_inserts_newline() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Shift+Enter should insert a newline (not submit)
        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should have two lines now
        assert_eq!(app.text_input.lines(), vec!["line1", ""]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_shift_enter_does_not_submit() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Record the original line count
        let original_lines_count = app.text_input.lines().len();

        // Shift+Enter should NOT submit
        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Lines should increase (newline inserted), NOT be cleared (which would happen on submit)
        assert_eq!(app.text_input.lines().len(), original_lines_count + 1);
        // The original text should still be there
        assert_eq!(app.text_input.lines()[0], "task text");
    }

    #[test]
    fn test_alt_enter_inserts_newline() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Alt+Enter should insert newline (alternative for terminals with Shift+Enter issues)
        app.handle_key(enter_key(KeyModifiers::ALT));

        // The original text should still be there (with a newline added)
        assert_eq!(app.text_input.lines(), vec!["task text", ""]);
        assert_eq!(app.text_input.cursor().0, 1);
    }

    #[test]
    fn test_multiple_shift_enters_create_multiple_lines() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Press Shift+Enter multiple times
        app.handle_key(enter_key(KeyModifiers::SHIFT));
        app.handle_key(enter_key(KeyModifiers::SHIFT));
        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should have 4 lines now (original + 3 newlines)
        assert_eq!(app.text_input.lines().len(), 4);
    }

    #[test]
    fn test_shift_enter_in_middle_of_line() {
        let mut app = create_test_app_with_lines(&["helloworld"], 0, 5);
        // Cursor is after "hello"

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should split the line
        assert_eq!(app.text_input.lines(), vec!["hello", "world"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_shift_enter_at_start_of_line() {
        let mut app = create_test_app_with_lines(&["text"], 0, 0);
        // Cursor at start of line

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should create empty line before the text
        assert_eq!(app.text_input.lines(), vec!["", "text"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_shift_enter_at_end_of_line() {
        let mut app = create_test_app_with_lines(&["text"], 0, 4);
        // Cursor at end of line

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should create empty line after the text
        assert_eq!(app.text_input.lines(), vec!["text", ""]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_shift_enter_preserves_text_before_cursor() {
        let mut app = create_test_app_with_lines(&["abc123"], 0, 3);
        // Cursor after "abc"

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // First line should have "abc", second line should have "123"
        assert_eq!(app.text_input.lines()[0], "abc");
        assert_eq!(app.text_input.lines()[1], "123");
    }

    #[test]
    fn test_shift_enter_on_second_line() {
        let mut app = create_test_app_with_lines(&["first", "second"], 1, 3);
        // Cursor in the middle of "second"

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should have three lines: "first", "sec", "ond"
        assert_eq!(app.text_input.lines().len(), 3);
        assert_eq!(app.text_input.lines()[0], "first");
        assert_eq!(app.text_input.lines()[1], "sec");
        assert_eq!(app.text_input.lines()[2], "ond");
        assert_eq!(app.text_input.cursor().0, 2);
    }

    #[test]
    fn test_shift_enter_with_unicode() {
        let mut app = create_test_app_with_lines(&["hello世界"], 0, 5);
        // Cursor after "hello", before unicode

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should split correctly around unicode
        assert_eq!(app.text_input.lines()[0], "hello");
        assert_eq!(app.text_input.lines()[1], "世界");
    }

    // =========================================================================
    // Plain Enter Submit Tests
    // Plain Enter (no modifiers) should submit the task (traditional chat behavior)
    // =========================================================================

    #[tokio::test]
    async fn test_plain_enter_submits_task() {
        let mut app = create_test_app_with_lines(&["my task"], 0, 7);

        // Plain Enter should submit the task
        app.handle_key(enter_key(KeyModifiers::NONE));

        // After Enter, the task should be submitted (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_empty_input_enter_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Enter on empty input should not submit (nothing to submit)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert!(!app.is_running);
    }

    #[test]
    fn test_whitespace_only_enter_does_not_submit() {
        let mut app = create_test_app_with_lines(&["   "], 0, 3);

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Enter on whitespace-only input should not submit (no real content)
        assert_eq!(app.text_input.lines(), vec!["   "]);
        assert!(!app.is_running);
    }

    #[tokio::test]
    async fn test_enter_at_start_of_line_submits() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 0);

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Enter submits the task regardless of cursor position (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[tokio::test]
    async fn test_enter_in_middle_of_text_submits() {
        let mut app = create_test_app_with_lines(&["helloworld"], 0, 5);

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Enter submits the task (input cleared), regardless of cursor position
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[tokio::test]
    async fn test_multiple_enter_presses_first_submits_then_noop() {
        let mut app = create_test_app_with_lines(&["task"], 0, 4);

        let enter = enter_key(KeyModifiers::NONE);

        // First Enter submits
        app.handle_key(enter);
        assert_eq!(app.text_input.lines(), vec![""]);

        // Subsequent Enters on empty input do nothing
        app.handle_key(enter);
        app.handle_key(enter);

        // Input should still be empty (no multiple submissions)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
    }

    // =========================================================================
    // Modifier Combination Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ctrl_shift_enter_submits() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Ctrl+Shift+Enter should submit (CONTROL modifier is present)
        // The SHIFT modifier is ignored when CONTROL is present
        app.handle_key(enter_key(KeyModifiers::CONTROL | KeyModifiers::SHIFT));

        // After submission, the input should be cleared
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_ctrl_alt_enter_submits() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Ctrl+Alt+Enter should submit (CONTROL modifier is present)
        app.handle_key(enter_key(KeyModifiers::CONTROL | KeyModifiers::ALT));

        // After submission, the input should be cleared
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_ctrl_alt_shift_enter_submits() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Even with multiple modifiers, as long as CONTROL or no SHIFT, it submits
        // SHIFT is present here, but CONTROL takes precedence
        app.handle_key(enter_key(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));

        // After submission, the input should be cleared
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[test]
    fn test_shift_alt_enter_inserts_newline() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Shift+Alt+Enter should insert newline (SHIFT modifier present, no CONTROL)
        app.handle_key(enter_key(KeyModifiers::SHIFT | KeyModifiers::ALT));

        // Should have two lines now (newline inserted, not submitted)
        assert_eq!(app.text_input.lines(), vec!["line1", ""]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[tokio::test]
    async fn test_super_enter_submits() {
        let mut app = create_test_app_with_lines(&["text"], 0, 4);

        app.handle_key(enter_key(KeyModifiers::SUPER));

        // SUPER modifier (without CONTROL/SHIFT/ALT) should submit
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_hyper_enter_submits() {
        let mut app = create_test_app_with_lines(&["text"], 0, 4);

        app.handle_key(enter_key(KeyModifiers::HYPER));

        // HYPER modifier (without CONTROL/SHIFT/ALT) should submit
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_ctrl_super_enter_submits() {
        let mut app = create_test_app_with_lines(&["task"], 0, 4);

        app.handle_key(enter_key(KeyModifiers::CONTROL | KeyModifiers::SUPER));

        // Control modifier should make it submit
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    // =========================================================================
    // Multi-line Task Tests
    // =========================================================================

    #[test]
    fn test_multiline_task_creation() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Shift+Enter to add newline
        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Type line2
        for c in "line2".chars() {
            app.handle_key(char_key(c));
        }

        assert_eq!(app.text_input.lines(), vec!["line1", "line2"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[tokio::test]
    async fn test_multiline_submission() {
        let mut app = create_test_app_with_lines(&["line1", "line2", "line3"], 2, 5);

        // Use Ctrl+Enter to submit
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // All lines should be submitted (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_ctrl_enter_multiline_clears_all() {
        let mut app = create_test_app_with_lines(&["line1", "line2", "line3"], 2, 5);

        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // All lines should be cleared after submit
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    #[test]
    fn test_newline_in_middle_of_text() {
        let mut app = create_test_app_with_lines(&["helloworld"], 0, 5);

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Text should be split
        assert_eq!(app.text_input.lines(), vec!["hello", "world"]);
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    // =========================================================================
    // Paste Workflow Tests (paste then submit sequences)
    // Pure paste behavior tests are in paste_handling_tests module.
    // =========================================================================

    #[tokio::test]
    async fn test_paste_then_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Paste text
        app.handle_paste("pasted task");

        assert_eq!(app.text_input.lines(), vec!["pasted task"]);

        // Submit with Ctrl+Enter
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Should be submitted
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    #[tokio::test]
    async fn test_enter_after_paste_submits() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Paste some text
        app.handle_paste("pasted content");

        // Now press Enter
        app.handle_key(enter_key(KeyModifiers::NONE));

        // Enter should submit the pasted content (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
    }

    #[tokio::test]
    async fn test_ctrl_enter_after_paste_submits() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Paste some text
        app.handle_paste("task to submit");

        // Now press Ctrl+Enter to submit
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Should have submitted (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
    }
}

// =============================================================================
// Rapid Input Detection Tests
// =============================================================================

mod rapid_input_detection_tests {
    use super::*;
    use crate::app::input::RapidInputDetector;

    /// Test that Enter submits when NOT in rapid input mode.
    ///
    /// With Enter = submit behavior, plain Enter during normal typing submits the task.
    /// However, during rapid input mode (paste fallback), Enter inserts a newline
    /// to prevent submitting each line of pasted text individually.
    #[tokio::test]
    async fn enter_submits_when_not_in_rapid_input() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Not in rapid input mode (default state)
        assert!(!app.text_input.rapid_input.is_active());

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Should have submitted (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    /// Test that Enter inserts newline when in rapid input mode (paste fallback).
    ///
    /// When rapid input mode is active (indicating a paste operation in terminals
    /// without bracketed paste support), Enter should insert a newline instead of
    /// submitting. This prevents accidental submission of multi-line pasted text.
    #[tokio::test]
    async fn enter_inserts_newline_during_rapid_input() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Simulate rapid input mode (paste detected)
        app.text_input.rapid_input.activate();

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Should have inserted newline (not submitted)
        assert_eq!(app.text_input.lines(), vec!["line1", ""]);
        // Cursor should be on line 2
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    /// Test that Ctrl+Enter always submits even during rapid input (paste fallback).
    ///
    /// Unlike plain Enter which inserts newlines during rapid input mode,
    /// Ctrl+Enter should always submit as an explicit user action to force submit.
    #[tokio::test]
    async fn ctrl_enter_submits_during_rapid_input() {
        let mut app = create_test_app_with_lines(&["task during paste"], 0, 17);

        // Simulate rapid input mode (paste detected)
        app.text_input.rapid_input.activate();

        // Ctrl+Enter should still submit (explicit submit action)
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Input should be cleared (submitted)
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    /// Test that Ctrl+D always submits even during rapid input.
    #[tokio::test]
    async fn ctrl_d_submits_during_rapid_input() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // In rapid input mode
        app.text_input.rapid_input.activate();

        let ctrl_d = KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        app.handle_key(ctrl_d);

        // Ctrl+D should submit regardless of rapid input
        assert_eq!(app.text_input.lines(), vec![""]);

        // Flow should have started
        assert!(app.is_running);
    }

    /// Test that Enter submits after a pause when not in rapid input mode.
    ///
    /// By default, rapid input is inactive, so Enter should submit normally.
    #[tokio::test]
    async fn enter_submits_after_pause() {
        let mut app = create_test_app_with_lines(&["task"], 0, 4);

        // Ensure we're not in rapid input mode (default)
        assert!(!app.text_input.rapid_input.is_active());

        // Plain Enter should submit when not in rapid input mode
        app.handle_key(enter_key(KeyModifiers::NONE));

        // Task should be submitted (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    /// Test that reset clears all fields.
    #[test]
    fn reset_rapid_input_state_clears_all_fields() {
        let mut detector = RapidInputDetector::new();

        // Activate rapid input mode
        detector.activate();
        assert!(detector.is_active());

        // Reset
        detector.reset();

        // All fields should be cleared
        assert!(!detector.is_active());
        assert_eq!(detector.key_count(), 0);
    }

    /// Test that Shift+Enter always inserts newline regardless of rapid input.
    #[test]
    fn shift_enter_inserts_newline_regardless_of_rapid_input() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Not in rapid input mode (default)
        assert!(!app.text_input.rapid_input.is_active());

        app.handle_key(enter_key(KeyModifiers::SHIFT));

        // Should insert newline
        assert_eq!(app.text_input.lines(), vec!["line1", ""]);
    }

    /// Test that Alt+Enter always inserts newline regardless of rapid input.
    #[test]
    fn alt_enter_inserts_newline_regardless_of_rapid_input() {
        let mut app = create_test_app_with_lines(&["line1"], 0, 5);

        // Not in rapid input mode (default)
        assert!(!app.text_input.rapid_input.is_active());

        app.handle_key(enter_key(KeyModifiers::ALT));

        // Should insert newline
        assert_eq!(app.text_input.lines(), vec!["line1", ""]);
    }

    /// Test that character input increments rapid key count.
    #[test]
    fn character_input_increments_rapid_count() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // First character
        let a = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        app.handle_key(a);

        // First key sets count to 1
        assert_eq!(app.text_input.rapid_input.key_count(), 1);
    }

    /// Test that the default `RapidInputDetector` has rapid input disabled.
    #[test]
    fn default_rapid_input_detector_has_rapid_input_disabled() {
        let detector = RapidInputDetector::new();

        assert!(!detector.is_active());
        assert_eq!(detector.key_count(), 0);
        assert!(!detector.has_chars());
    }

    /// Verifies rapid input state resets after successful submission.
    #[tokio::test]
    async fn rapid_input_resets_after_submission() {
        let mut app = create_test_app_with_lines(&["task text"], 0, 9);

        // Simulate being in rapid input mode
        app.text_input.rapid_input.activate();

        // Submit with Ctrl+Enter (bypasses rapid input)
        app.handle_key(enter_key(KeyModifiers::CONTROL));

        // Rapid input state should be reset after submission
        assert!(!app.text_input.rapid_input.is_active());
        assert_eq!(app.text_input.rapid_input.key_count(), 0);
    }

    /// Verifies rapid input mode activates after threshold count is reached.
    #[test]
    fn rapid_input_activates_after_threshold() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);

        // Initially not in rapid input mode
        assert!(!app.text_input.rapid_input.is_active());
        assert_eq!(app.text_input.rapid_input.key_count(), 0);

        // Manually activate rapid input (simulating threshold reached)
        app.text_input.rapid_input.activate();

        // Now Enter should insert newline
        app.handle_key(enter_key(KeyModifiers::NONE));

        // Should have inserted newline, not submitted
        assert_eq!(app.text_input.lines().len(), 2);
        assert!(!app.is_running);
    }

    /// Verifies that when rapid input is activated, the flag is respected.
    #[test]
    fn rapid_input_flag_respected_when_activated() {
        let mut app = create_test_app_with_lines(&["text"], 0, 4);

        // Activate rapid input mode
        app.text_input.rapid_input.activate();

        // The flag should be respected
        app.handle_key(enter_key(KeyModifiers::NONE));

        // Should insert newline because flag is set
        assert_eq!(app.text_input.lines().len(), 2);
        assert!(!app.is_running);
    }

    /// Verifies cursor position after newline insertion during rapid input.
    #[test]
    fn cursor_correct_after_rapid_input_newline() {
        let mut app = create_test_app_with_lines(&["hello world"], 0, 5);

        // In rapid input mode
        app.text_input.rapid_input.activate();

        // Enter should insert newline at cursor position
        app.handle_key(enter_key(KeyModifiers::NONE));

        // Verify line split correctly
        assert_eq!(app.text_input.lines().len(), 2);
        assert_eq!(app.text_input.lines()[0], "hello");
        assert_eq!(app.text_input.lines()[1], " world");

        // Cursor should be at start of new line
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    /// Verifies multiple Enter presses during rapid input create multiple newlines.
    #[test]
    fn multiple_enters_during_rapid_input_create_multiple_newlines() {
        let mut app = create_test_app_with_lines(&["text"], 0, 4);

        // In rapid input mode
        app.text_input.rapid_input.activate();

        // Multiple Enter presses
        for _ in 0..3 {
            app.handle_key(enter_key(KeyModifiers::NONE));
        }

        // Should have 4 lines (original + 3 newlines)
        assert_eq!(app.text_input.lines().len(), 4);
        assert_eq!(app.text_input.lines()[0], "text");
        assert_eq!(app.text_input.lines()[1], "");
        assert_eq!(app.text_input.lines()[2], "");
        assert_eq!(app.text_input.lines()[3], "");

        // Flow should NOT have started
        assert!(!app.is_running);
    }

    /// Empty input with Enter does not submit (regardless of rapid input).
    #[test]
    fn enter_with_empty_input_does_not_submit() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        // Not in rapid input mode (default)
        assert!(!app.text_input.rapid_input.is_active());

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Empty input should not submit
        assert!(!app.is_running);
    }

    /// Empty input with Enter during rapid input mode inserts newline.
    #[test]
    fn enter_with_empty_input_during_rapid_input_inserts_newline() {
        let mut app = create_test_app_with_lines(&[""], 0, 0);
        app.text_input.rapid_input.activate();

        app.handle_key(enter_key(KeyModifiers::NONE));

        // Empty input should not submit (just insert newline)
        assert!(!app.is_running);
        // But a newline should be inserted
        assert_eq!(app.text_input.lines().len(), 2);
    }
}
