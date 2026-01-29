//! Text input handling for the App.
//!
//! This module contains methods related to text input operations
//! using `tui-textarea` for the core editing functionality.
//! This module handles:
//! - @ token detection for file autocomplete
//! - Text submission and task saving
//! - Visual line wrapping calculations
//! - Rapid input detection for paste fallback

use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use unicode_width::UnicodeWidthChar;

use super::App;

// === Rapid Input Detection Constants ===
// These thresholds help detect paste operations when bracketed paste mode
// is not supported. Human typing maxes out around 12-15 chars/sec for fast typists.
// Paste operations deliver hundreds of chars/sec.

/// Minimum time between key events to be considered part of a rapid sequence (in ms).
/// Events arriving faster than this are likely from a paste operation.
const RAPID_INPUT_THRESHOLD_MS: u64 = 150;

/// Number of rapid key events required to trigger rapid input mode.
/// Reduced from 5 to 3 to catch shorter paste operations with embedded newlines.
/// This prevents false positives from occasional fast typing while allowing
/// faster detection of paste sequences.
const RAPID_INPUT_COUNT_THRESHOLD: usize = 3;

/// Time after which rapid input state resets if no new keys arrive (in ms).
/// This ensures normal typing resumes after a paste completes.
const RAPID_INPUT_RESET_MS: u64 = 500;

/// Result of processing a key event for rapid input detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RapidInputResult {
    /// Whether this key is considered part of a rapid input sequence.
    pub is_rapid: bool,
    /// Whether the rapid input mode was just activated (transitioned from inactive to active).
    pub just_activated: bool,
    /// Whether the rapid input mode was just deactivated (transitioned from active to inactive).
    pub just_deactivated: bool,
    /// The elapsed time since the last key event in milliseconds.
    pub elapsed_ms: u64,
}

/// Detects rapid input sequences (paste operations) when bracketed paste mode is not supported.
///
/// This detector tracks timing between key events to identify paste operations:
/// - Events arriving faster than `RAPID_INPUT_THRESHOLD_MS` are counted as rapid
/// - After `RAPID_INPUT_COUNT_THRESHOLD` rapid events, rapid mode is activated
/// - Rapid mode deactivates after `RAPID_INPUT_RESET_MS` without rapid events
///
/// The detector also tracks whether the rapid sequence contains character keys,
/// which helps distinguish paste operations from rapid Enter key presses.
#[derive(Debug, Clone, Default)]
pub struct RapidInputDetector {
    /// Timestamp of the last key event.
    last_key_time: Option<Instant>,
    /// Count of key events in the current rapid sequence.
    rapid_key_count: usize,
    /// Whether we're currently in a rapid input sequence (likely paste).
    in_rapid_input: bool,
    /// Whether the current rapid sequence contains at least one character key.
    /// This helps distinguish paste operations (which have characters) from
    /// rapid Enter key presses (which don't).
    rapid_has_chars: bool,
}

impl RapidInputDetector {
    /// Creates a new rapid input detector with default state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes a key event and returns the detection result.
    ///
    /// This method updates internal state and determines whether the key
    /// is part of a rapid input sequence (likely a paste operation).
    ///
    /// # Arguments
    ///
    /// * `key` - The key event to process
    ///
    /// # Returns
    ///
    /// A `RapidInputResult` containing the detection state.
    pub fn process_key(&mut self, key: &KeyEvent) -> RapidInputResult {
        let now = Instant::now();

        // Track previous state for transition detection
        let prev_in_rapid = self.in_rapid_input;

        // Compute elapsed time since last key BEFORE updating last_key_time.
        // If last_key_time is None, this is the first key event.
        let (elapsed_ms, is_first_key) = if let Some(last_time) = self.last_key_time {
            let elapsed = now.duration_since(last_time);
            // Saturate to u64::MAX if elapsed exceeds u64 range (shouldn't happen in practice)
            #[allow(clippy::cast_possible_truncation)]
            let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
            (ms, false)
        } else {
            // First key: no elapsed time to compare, use a sentinel value
            (u64::MAX, true)
        };

        // Check if this key is part of a rapid sequence
        let is_this_key_rapid = elapsed_ms < RAPID_INPUT_THRESHOLD_MS;

        // Track whether this is a character key (for distinguishing paste from rapid Enter presses)
        let is_char_key = matches!(key.code, KeyCode::Char(_));

        if is_first_key {
            // First key event - start tracking
            self.rapid_key_count = 1;
            self.rapid_has_chars = is_char_key;
        } else if is_this_key_rapid {
            // Key arrived quickly - likely part of a paste
            self.rapid_key_count += 1;
            if is_char_key {
                self.rapid_has_chars = true;
            }
            if self.rapid_key_count >= RAPID_INPUT_COUNT_THRESHOLD {
                self.in_rapid_input = true;
            }
        } else if elapsed_ms > RAPID_INPUT_RESET_MS {
            // Pause detected - reset rapid input state
            self.reset();
        }
        // Note: For intermediate elapsed times (between threshold and reset),
        // we keep the current state but don't increment the count.

        // Update last_key_time for the next key event
        self.last_key_time = Some(now);

        // Determine if this specific key should be treated as rapid.
        // Either the flag is already set, OR we're building up rapid input with
        // characters and this key arrived quickly (covers short pastes like "A\n").
        // The rapid_has_chars check prevents rapid Enter key presses from being
        // treated as paste (only sequences with actual characters count).
        let is_rapid = self.in_rapid_input
            || (self.rapid_has_chars && self.rapid_key_count > 0 && is_this_key_rapid);

        RapidInputResult {
            is_rapid,
            just_activated: !prev_in_rapid && self.in_rapid_input,
            just_deactivated: prev_in_rapid && !self.in_rapid_input,
            elapsed_ms,
        }
    }

    /// Activates rapid input mode manually.
    ///
    /// This is useful when Enter arrives during a building rapid sequence
    /// and should be treated as part of a paste.
    pub fn activate(&mut self) {
        self.in_rapid_input = true;
    }

    /// Resets the rapid input detection state.
    ///
    /// This should be called after a successful submit to clear
    /// the rapid input detection counters for the next input session.
    pub fn reset(&mut self) {
        self.rapid_key_count = 0;
        self.in_rapid_input = false;
        self.last_key_time = None;
        self.rapid_has_chars = false;
    }

    /// Returns whether rapid input mode is currently active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.in_rapid_input
    }

    /// Returns whether the current rapid sequence contains character keys.
    #[must_use]
    pub fn has_chars(&self) -> bool {
        self.rapid_has_chars
    }

    /// Returns the current rapid key count.
    #[must_use]
    pub fn key_count(&self) -> usize {
        self.rapid_key_count
    }

    /// Returns the count threshold for activating rapid mode.
    #[must_use]
    pub const fn count_threshold() -> usize {
        RAPID_INPUT_COUNT_THRESHOLD
    }

    /// Returns the last key time (for debug logging).
    #[must_use]
    pub fn last_key_time(&self) -> Option<Instant> {
        self.last_key_time
    }
}
use crate::app::AtToken;
use crate::tui::widgets::OutputLine;

/// Result of wrapping lines for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapResult {
    /// Visual lines after wrapping.
    pub visual_lines: Vec<String>,
    /// Visual row of the cursor (0-indexed).
    pub visual_cursor_row: usize,
    /// Visual column of the cursor within the visual row.
    pub visual_cursor_col: usize,
}

/// Wraps logical lines into visual lines based on available width.
///
/// # Arguments
/// * `logical_lines` - The input lines as stored in the buffer
/// * `cursor_row` - Current cursor's logical row (0-indexed)
/// * `cursor_col` - Current cursor's byte position in the logical row
/// * `width` - Available width for each visual line
///
/// # Returns
/// A `WrapResult` containing visual lines and mapped cursor position.
#[must_use]
pub fn wrap_lines_for_display(
    logical_lines: &[String],
    cursor_row: usize,
    cursor_col: usize,
    width: usize,
) -> WrapResult {
    // Handle edge case: if width is 0, don't wrap (treat as infinite)
    let effective_width = if width == 0 { usize::MAX } else { width };

    let mut visual_lines = Vec::new();
    let mut visual_cursor_row = 0;
    let mut visual_cursor_col = 0;
    let mut found_cursor = false;

    for (line_idx, line) in logical_lines.iter().enumerate() {
        let is_cursor_line = line_idx == cursor_row;

        if line.is_empty() {
            // Empty line still creates one visual line
            if is_cursor_line && !found_cursor {
                visual_cursor_row = visual_lines.len();
                visual_cursor_col = 0;
                found_cursor = true;
            }
            visual_lines.push(String::new());
            continue;
        }

        // Track current visual line being built
        let mut current_visual = String::new();
        let mut current_visual_width = 0;
        let mut byte_pos = 0; // Byte position within the logical line

        for ch in line.chars() {
            let char_byte_len = ch.len_utf8();
            let char_display_width = ch.width().unwrap_or(1);

            // Check if adding this character would exceed the width
            if current_visual_width + char_display_width > effective_width
                && !current_visual.is_empty()
            {
                // Before pushing: check if cursor is in this visual line
                if is_cursor_line && !found_cursor && cursor_col < byte_pos {
                    // Cursor was in the line we're about to push
                    visual_cursor_row = visual_lines.len();
                    // Recalculate visual cursor col for this visual line
                    visual_cursor_col = calculate_visual_col(&current_visual, cursor_col, byte_pos);
                    found_cursor = true;
                }

                visual_lines.push(std::mem::take(&mut current_visual));
                current_visual_width = 0;
            }

            // Check if cursor is at this exact byte position
            if is_cursor_line && !found_cursor && byte_pos == cursor_col {
                visual_cursor_row = visual_lines.len();
                visual_cursor_col = current_visual_width;
                found_cursor = true;
            }

            current_visual.push(ch);
            current_visual_width += char_display_width;
            byte_pos += char_byte_len;
        }

        // Push remaining content
        if is_cursor_line && !found_cursor {
            // Cursor is at the end of the line or within the last segment
            visual_cursor_row = visual_lines.len();
            if cursor_col >= byte_pos {
                // Cursor at end of line
                visual_cursor_col = current_visual_width;
            } else {
                // Cursor somewhere in the last segment
                visual_cursor_col = calculate_visual_col_from_start(&current_visual, cursor_col);
            }
            found_cursor = true;
        }
        visual_lines.push(current_visual);
    }

    // Handle case where cursor_row is beyond available lines (shouldn't happen normally)
    if !found_cursor {
        visual_cursor_row = visual_lines.len().saturating_sub(1);
        visual_cursor_col = visual_lines
            .last()
            .map_or(0, |l| l.chars().map(|c| c.width().unwrap_or(1)).sum());
    }

    WrapResult {
        visual_lines,
        visual_cursor_row,
        visual_cursor_col,
    }
}

/// Calculate visual column for cursor within a visual line segment.
/// `cursor_byte_col` is the byte position in the original logical line.
/// `segment_end_byte` is the byte position where this visual segment ends.
fn calculate_visual_col(
    visual_line: &str,
    cursor_byte_col: usize,
    segment_end_byte: usize,
) -> usize {
    // Calculate byte offset where this segment starts
    let segment_start_byte = segment_end_byte - visual_line.len();

    if cursor_byte_col < segment_start_byte {
        return 0;
    }

    let cursor_offset_in_segment = cursor_byte_col - segment_start_byte;
    let mut visual_col = 0;
    let mut byte_offset = 0;

    for ch in visual_line.chars() {
        if byte_offset >= cursor_offset_in_segment {
            break;
        }
        visual_col += ch.width().unwrap_or(1);
        byte_offset += ch.len_utf8();
    }

    visual_col
}

/// Calculate visual column from the start of a visual line segment.
fn calculate_visual_col_from_start(visual_line: &str, cursor_byte_col: usize) -> usize {
    let mut visual_col = 0;
    let mut byte_offset = 0;

    for ch in visual_line.chars() {
        if byte_offset >= cursor_byte_col {
            break;
        }
        visual_col += ch.width().unwrap_or(1);
        byte_offset += ch.len_utf8();
    }

    visual_col
}

impl App {
    // ===== Text Input Operations =====

    /// Submits the text input and starts the flow.
    ///
    /// First checks if the input is a slash command. If so, executes it.
    /// Otherwise, treats it as a task description and starts the flow.
    ///
    /// Uses the models configured in `SettingsState`. If never configured,
    /// defaults are used (as per `Model::default()`).
    ///
    /// After submission, the input is cleared for the next task (chat-like behavior).
    /// The application stays in Chat mode while the flow executes.
    pub(super) fn submit_text_input(&mut self) {
        let text = self.text_input.collect_text();
        if text.trim().is_empty() {
            // Don't allow empty submission
            return;
        }

        // Check for slash command first
        if self.try_execute_slash_command() {
            // Command was executed, input already cleared
            return;
        }

        // Not a command - proceed with normal task submission
        // Save task text to task.md for future reference
        // Errors are displayed in the TUI but don't prevent flow execution
        if let Err(e) = self.save_current_task() {
            self.flow_ui
                .output
                .push(OutputLine::warning(format!("Failed to save task.md: {e}")));
        }

        self.flow.set_input_text(text);

        // Clear input for next task (chat-like behavior)
        self.text_input.clear();

        // Reset rapid input detection so subsequent keys don't think they're part of a paste
        self.text_input.reset_rapid_input_state();

        // Start flow (stays in Chat mode)
        self.start_flow();
    }

    // ===== @ Token Detection =====

    /// Updates the `@` token detection state and triggers file search.
    ///
    /// Call this after any input modification or cursor movement.
    /// This method:
    /// 1. Detects if cursor is within an `@` token
    /// 2. Triggers file search based on the token query
    pub(super) fn update_at_token(&mut self) {
        self.text_input.at_token = self.detect_at_token();
        self.update_file_search();
    }

    /// Detects if the cursor is within or immediately after an `@` token.
    ///
    /// This function scans the current line around the cursor position to find
    /// any `@`-prefixed token. It handles UTF-8 safely by operating on character
    /// boundaries.
    ///
    /// # Algorithm
    ///
    /// 1. Find the word/token boundaries around the cursor
    /// 2. Check if the token starts with `@`
    /// 3. Verify the `@` is at a word boundary (preceded by whitespace or at line start)
    /// 4. Extract the query (text after `@`)
    /// 5. Return token info or `None` if no `@` token found
    ///
    /// # Returns
    ///
    /// `Some(AtToken)` if cursor is within/after an `@` token, `None` otherwise.
    fn detect_at_token(&self) -> Option<AtToken> {
        let lines = self.text_input.lines();
        let (cursor_row, cursor_char_col) = self.text_input.cursor();

        // Get the current line
        let line = lines.get(cursor_row)?;

        // Handle empty line
        if line.is_empty() {
            return None;
        }

        // Collect (byte_pos, char) pairs for the line
        let char_info: Vec<(usize, char)> = line.char_indices().collect();

        // cursor_char_col from tui-textarea is already a character index
        let cursor_char_idx = cursor_char_col.min(char_info.len());

        // Find left boundary: scan left to find whitespace or line start
        let mut left_char_idx = cursor_char_idx;
        while left_char_idx > 0 {
            let (_, ch) = char_info[left_char_idx - 1];
            if ch.is_whitespace() {
                break;
            }
            left_char_idx -= 1;
        }

        // Find right boundary: scan right from cursor to find whitespace or line end
        let mut right_char_idx = cursor_char_idx;
        while right_char_idx < char_info.len() {
            let (_, ch) = char_info[right_char_idx];
            if ch.is_whitespace() {
                break;
            }
            right_char_idx += 1;
        }

        // If cursor is on whitespace (left == right), check the token to the left
        if left_char_idx == cursor_char_idx && cursor_char_idx > 0 {
            // Cursor is on whitespace or at the end of a token
            // Check if the character before cursor forms a token
            let prev_char_idx = cursor_char_idx - 1;
            let (_, prev_ch) = char_info[prev_char_idx];

            if !prev_ch.is_whitespace() {
                // Re-find the left boundary from prev_char_idx
                left_char_idx = prev_char_idx;
                while left_char_idx > 0 {
                    let (_, ch) = char_info[left_char_idx - 1];
                    if ch.is_whitespace() {
                        break;
                    }
                    left_char_idx -= 1;
                }
                // Right boundary is the cursor position (end of the token)
                right_char_idx = cursor_char_idx;
            }
        }

        // No token found (cursor on whitespace with no adjacent token)
        if left_char_idx >= right_char_idx {
            return None;
        }

        // Get the token's byte boundaries
        let start_byte = char_info[left_char_idx].0;
        let end_byte = if right_char_idx < char_info.len() {
            char_info[right_char_idx].0
        } else {
            line.len()
        };

        // Extract the token string
        let token = &line[start_byte..end_byte];

        // Check if token starts with '@'
        let first_char = token.chars().next()?;
        if first_char != '@' {
            return None;
        }

        // Verify '@' is at a word boundary (preceded by whitespace or at line start)
        // This prevents triggering on email-like patterns like "email@domain.com"
        if left_char_idx > 0 {
            let (_, prev_ch) = char_info[left_char_idx - 1];
            if !prev_ch.is_whitespace() {
                return None;
            }
        }

        // Extract the query (everything after '@')
        let query = token[1..].to_string();

        Some(AtToken {
            query,
            start_byte,
            end_byte,
            row: cursor_row,
        })
    }
}

/// Escapes a file path for safe insertion into text.
///
/// This function handles:
/// - Paths with spaces (wraps in quotes)
/// - Paths with special characters (escapes or uses single quotes)
/// - Paths that are safe as-is (returns unquoted)
///
/// # Quoting Strategy
///
/// - Simple paths (no special chars): returned as-is with trailing space
/// - Paths with special chars but no single quotes: wrapped in single quotes
/// - Paths with single quotes: wrapped in double quotes with escaping
///
/// Single quotes are preferred when possible because they preserve the literal
/// meaning of all characters except the single quote itself in POSIX shells.
#[must_use]
pub fn escape_file_path(path: &str) -> String {
    // Characters that need special handling (shell metacharacters + whitespace)
    let needs_quoting = path.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '$' | '\\' | '!' | '*' | '?' | '[' | ']' | '(' | ')' | '{' | '}'
            )
    });

    if !needs_quoting {
        // Simple case: no special characters
        return format!("{path} ");
    }

    // Prefer single quotes if path doesn't contain single quotes
    // (single quotes preserve literal meaning of all characters except single quote)
    if !path.contains('\'') {
        return format!("'{path}' ");
    }

    // Path contains single quotes, use double quotes with escaping
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('!', "\\!");

    format!("\"{escaped}\" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_empty_input() {
        let lines = vec![String::new()];
        let result = wrap_lines_for_display(&lines, 0, 0, 10);
        assert_eq!(result.visual_lines, vec![""]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 0);
    }

    #[test]
    fn test_wrap_single_short_line() {
        let lines = vec!["hello".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 2, 10);
        assert_eq!(result.visual_lines, vec!["hello"]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 2);
    }

    #[test]
    fn test_wrap_single_long_line() {
        // "hello world" with width 5 should wrap to: "hello", " worl", "d"
        let lines = vec!["hello world".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 8, 5);
        assert_eq!(result.visual_lines, vec!["hello", " worl", "d"]);
        // Cursor at byte 8 is at 'r' in " world", which is in visual line 1
        // " worl" contains bytes 5-10 (inclusive), cursor at 8 is 'r' at visual col 3
        assert_eq!(result.visual_cursor_row, 1);
        assert_eq!(result.visual_cursor_col, 3);
    }

    #[test]
    fn test_wrap_cursor_at_end_of_line() {
        let lines = vec!["hello".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 5, 10);
        assert_eq!(result.visual_lines, vec!["hello"]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 5);
    }

    #[test]
    fn test_wrap_multiple_lines() {
        let lines = vec!["abc".to_string(), "def".to_string()];
        let result = wrap_lines_for_display(&lines, 1, 1, 10);
        assert_eq!(result.visual_lines, vec!["abc", "def"]);
        assert_eq!(result.visual_cursor_row, 1);
        assert_eq!(result.visual_cursor_col, 1);
    }

    #[test]
    fn test_wrap_cursor_in_wrapped_segment() {
        // "abcdefghij" with width 3 wraps to: "abc", "def", "ghi", "j"
        let lines = vec!["abcdefghij".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 4, 3);
        // Cursor at byte 4 is 'e', which is in visual line 1 at visual col 1
        assert_eq!(result.visual_lines, vec!["abc", "def", "ghi", "j"]);
        assert_eq!(result.visual_cursor_row, 1);
        assert_eq!(result.visual_cursor_col, 1);
    }

    #[test]
    fn test_wrap_width_zero_no_wrap() {
        let lines = vec!["hello world".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 5, 0);
        // Width 0 means no wrapping
        assert_eq!(result.visual_lines, vec!["hello world"]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 5);
    }

    #[test]
    fn test_wrap_multibyte_chars() {
        // "héllo" has multibyte 'é' (2 bytes: 0xC3 0xA9)
        let lines = vec!["héllo".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 3, 10);
        // Byte positions: h=0, é=1-2, l=3, l=4, o=5
        // Cursor at byte 3 is at 'l' (visual col 2, since 'h' and 'é' each take 1 display width)
        assert_eq!(result.visual_lines, vec!["héllo"]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 2);
    }

    #[test]
    fn test_wrap_wide_chars() {
        // Chinese characters are typically 2 display columns wide
        // "中文" - each character is 3 bytes and 2 display columns
        let lines = vec!["中文".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 3, 10);
        // Cursor at byte 3 is at '文' (visual col 2, since '中' takes 2 display columns)
        assert_eq!(result.visual_lines, vec!["中文"]);
        assert_eq!(result.visual_cursor_row, 0);
        assert_eq!(result.visual_cursor_col, 2);
    }

    #[test]
    fn test_wrap_wide_chars_causes_wrap() {
        // "中中中" with width 4 should wrap: "中中" (4 cols), "中" (2 cols)
        // Each '中' is 3 bytes, 2 display columns
        let lines = vec!["中中中".to_string()];
        let result = wrap_lines_for_display(&lines, 0, 6, 4);
        // Cursor at byte 6 is at the third '中', which is in visual line 1
        assert_eq!(result.visual_lines.len(), 2);
        assert_eq!(result.visual_cursor_row, 1);
        assert_eq!(result.visual_cursor_col, 0);
    }
}

#[cfg(test)]
mod path_escaping_tests {
    use super::escape_file_path;

    #[test]
    fn test_simple_path_no_escaping() {
        assert_eq!(escape_file_path("src/main.rs"), "src/main.rs ");
    }

    #[test]
    fn test_path_with_spaces_uses_single_quotes() {
        assert_eq!(escape_file_path("my file.txt"), "'my file.txt' ");
    }

    #[test]
    fn test_path_with_double_quotes_uses_single_quotes() {
        assert_eq!(escape_file_path("my\"file.txt"), "'my\"file.txt' ");
    }

    #[test]
    fn test_path_with_single_quotes_escapes_in_double_quotes() {
        assert_eq!(escape_file_path("my'file.txt"), "\"my'file.txt\" ");
    }

    #[test]
    fn test_path_with_both_quotes() {
        // Path: my'and"file.txt
        // Single quotes won't work, must escape in double quotes
        assert_eq!(
            escape_file_path("my'and\"file.txt"),
            "\"my'and\\\"file.txt\" "
        );
    }

    #[test]
    fn test_path_with_dollar_sign() {
        assert_eq!(escape_file_path("$HOME/file.txt"), "'$HOME/file.txt' ");
    }

    #[test]
    fn test_path_with_backticks() {
        assert_eq!(escape_file_path("file`cmd`.txt"), "'file`cmd`.txt' ");
    }

    #[test]
    fn test_path_with_backslash() {
        assert_eq!(escape_file_path("path\\file.txt"), "'path\\file.txt' ");
    }

    #[test]
    fn test_complex_path_with_multiple_special_chars() {
        // Test a path that has single quotes and other special chars
        // The backslash and dollar sign must be escaped in double quotes
        assert_eq!(
            escape_file_path("file's $name.txt"),
            "\"file's \\$name.txt\" "
        );
    }

    #[test]
    fn test_path_with_glob_characters() {
        assert_eq!(escape_file_path("*.txt"), "'*.txt' ");
        assert_eq!(escape_file_path("file[0-9].txt"), "'file[0-9].txt' ");
        assert_eq!(escape_file_path("file?.txt"), "'file?.txt' ");
    }

    #[test]
    fn test_path_with_parentheses() {
        assert_eq!(escape_file_path("file(1).txt"), "'file(1).txt' ");
    }

    #[test]
    fn test_path_with_braces() {
        assert_eq!(escape_file_path("file{a,b}.txt"), "'file{a,b}.txt' ");
    }

    #[test]
    fn test_path_with_exclamation() {
        assert_eq!(escape_file_path("important!.txt"), "'important!.txt' ");
    }

    #[test]
    fn test_path_with_single_quote_and_backslash() {
        // Both single quote and backslash - needs double quoting with escaping
        assert_eq!(
            escape_file_path("path's\\file.txt"),
            "\"path's\\\\file.txt\" "
        );
    }

    #[test]
    fn test_path_with_tab_character() {
        // Tab is whitespace, needs quoting
        assert_eq!(escape_file_path("file\tname.txt"), "'file\tname.txt' ");
    }

    #[test]
    fn test_path_with_multiple_spaces() {
        assert_eq!(
            escape_file_path("my   spaced   file.txt"),
            "'my   spaced   file.txt' "
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod at_token_tests {
    use super::*;
    use crate::app::{AppMode, FlowUiState, LayoutState, SettingsState, TextInputState};
    use tui_textarea::{CursorMove, TextArea};

    /// Helper to create a minimal App for testing token detection.
    ///
    /// Note: `cursor_col` is the character index (not byte index) for compatibility
    /// with tui-textarea's cursor positioning.
    fn create_test_app(lines: &[&str], cursor_row: usize, cursor_col: usize) -> App {
        let (search_tx, _search_rx) = tokio::sync::mpsc::channel(16);

        // Create a TextArea with the given content
        let lines_owned: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
        let mut textarea = TextArea::new(lines_owned);

        // Position the cursor
        // First move to the beginning, then to the target row
        textarea.move_cursor(CursorMove::Top);
        for _ in 0..cursor_row {
            textarea.move_cursor(CursorMove::Down);
        }
        // Move to the target column (character-wise)
        textarea.move_cursor(CursorMove::Head);
        for _ in 0..cursor_col {
            textarea.move_cursor(CursorMove::Forward);
        }

        App {
            paths: crate::fs::McgravityPaths::from_cwd(),
            flow: crate::core::FlowState::new_without_file(),
            theme: crate::tui::Theme::default(),
            mode: AppMode::Chat,
            should_quit: false,
            is_running: false,
            event_rx: tokio::sync::mpsc::channel(1).1,
            event_tx: tokio::sync::mpsc::channel(1).0,
            shutdown_tx: tokio::sync::watch::channel(false).0,
            text_input: TextInputState {
                textarea,
                at_token: None,
                file_popup_state: crate::tui::widgets::PopupState::default(),
                last_search_query: None,
                last_search_time: None,
                search_tx,
                search_generation: 0,
                rapid_input: RapidInputDetector::new(),
                // Autosave state
                last_edit_time: None,
                is_dirty: false,
                // Slash command state
                command_popup_state: crate::tui::widgets::CommandPopupState::default(),
                slash_token: None,
            },
            settings: SettingsState::default(),
            flow_ui: FlowUiState::default(),
            layout: LayoutState::default(),
            initial_setup: None,
            command_registry: crate::core::CommandRegistry::with_builtins(),
        }
    }

    #[test]
    fn test_no_at_token() {
        // Text without @ should return None
        let app = create_test_app(&["hello world"], 0, 5);
        assert!(app.detect_at_token().is_none());
    }

    #[test]
    fn test_at_token_at_cursor() {
        // "@foo|" with cursor after foo (at end of token)
        let app = create_test_app(&["@foo"], 0, 4);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 4);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_at_token_cursor_in_middle() {
        // "@fo|o" with cursor in middle (after 'o' at byte 2)
        let app = create_test_app(&["@foo"], 0, 2);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 4);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_at_token_just_at_symbol() {
        // Just "@|" should return empty query
        let app = create_test_app(&["@"], 0, 1);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 1);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_at_token_cursor_on_at() {
        // "|@foo" with cursor at the @ symbol
        let app = create_test_app(&["@foo"], 0, 0);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 4);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_at_token_with_spaces() {
        // "hello @foo bar" with cursor after foo (at position 10)
        let app = create_test_app(&["hello @foo bar"], 0, 10);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 6);
        assert_eq!(token.end_byte, 10);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_at_token_at_line_start() {
        // "@foo" at the beginning of line
        let app = create_test_app(&["@foo bar"], 0, 2);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 4);
        assert_eq!(token.row, 0);
    }

    #[test]
    fn test_email_pattern_no_trigger() {
        // "email@domain.com" should NOT trigger (no space before @)
        let app = create_test_app(&["email@domain.com"], 0, 10);
        assert!(app.detect_at_token().is_none());
    }

    #[test]
    fn test_at_token_multiple_at_symbols() {
        // "email@domain @file" should pick @file when cursor is near it
        let app = create_test_app(&["email@domain @file"], 0, 18);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "file");
        assert_eq!(token.start_byte, 13);
        assert_eq!(token.end_byte, 18);
    }

    #[test]
    fn test_at_token_cursor_on_whitespace_after_token() {
        // "hello @foo bar" cursor on space after @foo (at position 10)
        // h=0, e=1, l=2, l=3, o=4, space=5, @=6, f=7, o=8, o=9, space=10
        // Should detect @foo because cursor is immediately after the token
        let app = create_test_app(&["hello @foo bar"], 0, 10);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
        assert_eq!(token.start_byte, 6);
        assert_eq!(token.end_byte, 10);
    }

    #[test]
    fn test_empty_line() {
        // Empty line should return None
        let app = create_test_app(&[""], 0, 0);
        assert!(app.detect_at_token().is_none());
    }

    #[test]
    fn test_at_token_with_path() {
        // "@src/main.rs" should detect the full path
        let app = create_test_app(&["@src/main.rs"], 0, 12);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "src/main.rs");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 12);
    }

    #[test]
    fn test_at_token_multiline() {
        // Multiple lines, cursor on second line with @token
        let app = create_test_app(&["first line", "@second"], 1, 7);
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "second");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 7);
        assert_eq!(token.row, 1);
    }

    #[test]
    fn test_at_token_unicode() {
        // "@文件" with unicode characters after @
        let app = create_test_app(&["@文件"], 0, 7); // @ is 1 byte, 文 is 3 bytes, 件 is 3 bytes
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "文件");
        assert_eq!(token.start_byte, 0);
        assert_eq!(token.end_byte, 7);
    }

    #[test]
    fn test_cursor_between_at_tokens() {
        // "@foo @bar" with cursor in space between them
        // @=0, f=1, o=2, o=3, space=4, @=5, b=6, a=7, r=8
        // Cursor at position 4 is on the space after @foo
        let app = create_test_app(&["@foo @bar"], 0, 4);
        // Should detect @foo since cursor is immediately after the token
        let token = app
            .detect_at_token()
            .expect("test expects token to be detected");
        assert_eq!(token.query, "foo");
    }

    #[test]
    fn test_at_token_cursor_at_space_start() {
        // " @foo" cursor at the leading space (position 0)
        let app = create_test_app(&[" @foo"], 0, 0);
        // Cursor is on whitespace with nothing to the left
        assert!(app.detect_at_token().is_none());
    }

    #[test]
    fn test_no_at_token_word_without_at() {
        // "foo" without @ should return None
        let app = create_test_app(&["foo"], 0, 3);
        assert!(app.detect_at_token().is_none());
    }
}
