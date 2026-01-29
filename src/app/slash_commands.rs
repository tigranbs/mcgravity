//! Slash command detection and parsing.
//!
//! This module handles detecting `/` tokens in user input and parsing
//! complete slash commands for execution.
//!
//! ## Detection vs Parsing
//!
//! - Detection ([`detect_slash_token`]): Finds `/` tokens while typing for popup
//! - Parsing ([`parse_slash_command`]): Parses complete input on submission

use crate::app::App;
use crate::tui::widgets::{CommandMatch, CommandPopupState};

/// Information about a `/` slash command token being typed.
///
/// This struct tracks the location and content of a `/`-prefixed token
/// that the cursor is currently within or immediately after. It's used
/// for triggering command completion suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashToken {
    /// The command name (without the leading slash).
    pub name: String,
    /// Byte position where the `/` starts in the current line.
    pub start_byte: usize,
    /// Byte position where the token ends.
    pub end_byte: usize,
    /// The row (line index) containing the token.
    pub row: usize,
}

/// Parses the input text to check if it's a complete slash command.
///
/// Returns `Some((command_name, args))` if input starts with `/` and has no other content.
/// Only returns a command when the entire input is just the command (for submission).
///
/// # Arguments
///
/// * `input` - The full input text to parse
///
/// # Returns
///
/// * `Some((name, Some(args)))` - Command with arguments
/// * `Some((name, None))` - Command without arguments
/// * `None` - Not a valid slash command
///
/// # Examples
///
/// ```
/// use mcgravity::app::slash_commands::parse_slash_command;
///
/// assert_eq!(parse_slash_command("/exit"), Some(("exit", None)));
/// assert_eq!(parse_slash_command("/settings"), Some(("settings", None)));
/// assert_eq!(parse_slash_command("/help topic"), Some(("help", Some("topic"))));
/// assert_eq!(parse_slash_command("not a command"), None);
/// assert_eq!(parse_slash_command("/"), None);
/// assert_eq!(parse_slash_command("line1\n/exit"), None);
/// ```
#[must_use]
pub fn parse_slash_command(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    // Only parse as command if it's a single-line command
    if trimmed.contains('\n') {
        return None;
    }
    let without_slash = &trimmed[1..];
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let name = parts.next()?;
    if name.is_empty() {
        return None;
    }
    let args = parts.next().map(str::trim).filter(|s| !s.is_empty());
    Some((name, args))
}

/// Detects a slash command token at the given cursor position.
///
/// This function scans the current line around the cursor position to detect
/// if the cursor is within or immediately after a `/` token. The `/` must be
/// at a word boundary (line start or after whitespace) to be considered valid.
///
/// # Arguments
///
/// * `lines` - All lines of the input text
/// * `cursor_row` - The row (line index) where the cursor is
/// * `cursor_char_col` - The character column where the cursor is (not byte position)
///
/// # Returns
///
/// * `Some(SlashToken)` - If cursor is within or after a valid slash token
/// * `None` - If not a valid slash command pattern
///
/// # Examples
///
/// Given input "/exit" with cursor at end:
/// - Returns `SlashToken { name: "exit", start_byte: 0, end_byte: 5, row: 0 }`
///
/// Given input "some /cmd" with cursor after "cmd":
/// - Returns `SlashToken { name: "cmd", start_byte: 5, end_byte: 9, row: 0 }`
#[must_use]
pub fn detect_slash_token(
    lines: &[String],
    cursor_row: usize,
    cursor_char_col: usize,
) -> Option<SlashToken> {
    // Get the current line
    let line = lines.get(cursor_row)?;

    // Handle empty line
    if line.is_empty() {
        return None;
    }

    // Collect (byte_pos, char) pairs for the line
    let char_info: Vec<(usize, char)> = line.char_indices().collect();

    // cursor_char_col is a character index, clamp to valid range
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

    // Check if token starts with '/'
    let first_char = token.chars().next()?;
    if first_char != '/' {
        return None;
    }

    // Verify '/' is at a word boundary (preceded by whitespace or at line start)
    if left_char_idx > 0 {
        let (_, prev_ch) = char_info[left_char_idx - 1];
        if !prev_ch.is_whitespace() {
            return None;
        }
    }

    // Extract the command name (everything after '/')
    let name = token[1..].to_string();

    Some(SlashToken {
        name,
        start_byte,
        end_byte,
        row: cursor_row,
    })
}

// =============================================================================
// App Integration Methods
// =============================================================================

impl App {
    /// Detects if the cursor is within or immediately after a `/` token at line start.
    ///
    /// This method wraps [`detect_slash_token`] but adds an additional constraint:
    /// the `/` must be at the beginning of the line (not just after whitespace).
    /// This is different from `@` mentions which can appear anywhere.
    ///
    /// # Returns
    ///
    /// `Some(SlashToken)` if cursor is within/after a `/` token at line start, `None` otherwise.
    pub(crate) fn detect_slash_token(&self) -> Option<SlashToken> {
        let lines = self.text_input.lines();
        let (cursor_row, cursor_char_col) = self.text_input.cursor();

        // Use the existing detect_slash_token function
        let token = detect_slash_token(lines, cursor_row, cursor_char_col)?;

        // Additional check: only trigger if `/` is at the start of the line
        // (start_byte == 0 means the token starts at column 0)
        if token.start_byte != 0 {
            return None;
        }

        Some(token)
    }

    /// Updates the slash command popup based on current input.
    ///
    /// This method:
    /// 1. Detects if there's a slash token at line start
    /// 2. Gets matching commands from the registry
    /// 3. Updates the popup state with matches
    pub(crate) fn update_slash_command_popup(&mut self) {
        self.text_input.slash_token = self.detect_slash_token();

        if let Some(token) = &self.text_input.slash_token {
            // Get matching commands from registry
            let matches: Vec<CommandMatch> = self
                .command_registry
                .matching(&token.name)
                .into_iter()
                .map(|cmd| CommandMatch {
                    name: cmd.name(),
                    description: cmd.description(),
                })
                .collect();

            if matches.is_empty() {
                self.text_input.command_popup_state = CommandPopupState::Hidden;
            } else {
                // Preserve selection if valid
                let selected = if let CommandPopupState::Showing { selected: prev, .. } =
                    &self.text_input.command_popup_state
                {
                    (*prev).min(matches.len().saturating_sub(1))
                } else {
                    0
                };

                self.text_input.command_popup_state =
                    CommandPopupState::Showing { matches, selected };
            }
        } else {
            self.text_input.command_popup_state = CommandPopupState::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // parse_slash_command tests
    // =========================================================================

    #[test]
    fn parse_simple_command() {
        assert_eq!(parse_slash_command("/exit"), Some(("exit", None)));
    }

    #[test]
    fn parse_settings_command() {
        assert_eq!(parse_slash_command("/settings"), Some(("settings", None)));
    }

    #[test]
    fn parse_clear_command() {
        assert_eq!(parse_slash_command("/clear"), Some(("clear", None)));
    }

    #[test]
    fn parse_command_with_args() {
        assert_eq!(
            parse_slash_command("/help topic"),
            Some(("help", Some("topic")))
        );
    }

    #[test]
    fn parse_command_with_multiple_args() {
        assert_eq!(
            parse_slash_command("/run test --verbose"),
            Some(("run", Some("test --verbose")))
        );
    }

    #[test]
    fn parse_command_with_leading_whitespace() {
        assert_eq!(parse_slash_command("  /exit"), Some(("exit", None)));
    }

    #[test]
    fn parse_command_with_trailing_whitespace() {
        assert_eq!(parse_slash_command("/exit  "), Some(("exit", None)));
    }

    #[test]
    fn parse_command_with_whitespace_between() {
        assert_eq!(
            parse_slash_command("/help   topic"),
            Some(("help", Some("topic")))
        );
    }

    #[test]
    fn parse_empty_input() {
        assert_eq!(parse_slash_command(""), None);
    }

    #[test]
    fn parse_just_slash() {
        assert_eq!(parse_slash_command("/"), None);
    }

    #[test]
    fn parse_just_slash_with_whitespace() {
        assert_eq!(parse_slash_command("  /  "), None);
    }

    #[test]
    fn parse_non_command_input() {
        assert_eq!(parse_slash_command("not a command"), None);
    }

    #[test]
    fn parse_slash_in_middle() {
        assert_eq!(parse_slash_command("some text /exit"), None);
    }

    #[test]
    fn parse_multiline_with_slash_on_first_line() {
        assert_eq!(parse_slash_command("/exit\nmore text"), None);
    }

    #[test]
    fn parse_multiline_with_slash_on_second_line() {
        assert_eq!(parse_slash_command("line1\n/exit"), None);
    }

    #[test]
    fn parse_whitespace_only() {
        assert_eq!(parse_slash_command("   "), None);
    }

    #[test]
    fn parse_slash_with_numbers() {
        assert_eq!(parse_slash_command("/cmd123"), Some(("cmd123", None)));
    }

    #[test]
    fn parse_slash_with_hyphens() {
        assert_eq!(
            parse_slash_command("/my-command"),
            Some(("my-command", None))
        );
    }

    #[test]
    fn parse_slash_with_underscores() {
        assert_eq!(
            parse_slash_command("/my_command"),
            Some(("my_command", None))
        );
    }

    // =========================================================================
    // detect_slash_token tests
    // =========================================================================

    #[test]
    fn detect_slash_at_line_start() {
        let lines = vec!["/exit".to_string()];
        let result = detect_slash_token(&lines, 0, 5);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "exit".to_string(),
                start_byte: 0,
                end_byte: 5,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_cursor_in_middle() {
        let lines = vec!["/settings".to_string()];
        let result = detect_slash_token(&lines, 0, 3);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "settings".to_string(),
                start_byte: 0,
                end_byte: 9,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_after_whitespace() {
        let lines = vec!["some /cmd".to_string()];
        let result = detect_slash_token(&lines, 0, 9);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 5,
                end_byte: 9,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_in_second_line() {
        let lines = vec!["first line".to_string(), "/cmd".to_string()];
        let result = detect_slash_token(&lines, 1, 4);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 0,
                end_byte: 4,
                row: 1,
            })
        );
    }

    #[test]
    fn detect_empty_slash_returns_empty_name() {
        let lines = vec!["/ ".to_string()];
        let result = detect_slash_token(&lines, 0, 1);
        // Token is just "/" which has empty name after slash
        assert_eq!(
            result,
            Some(SlashToken {
                name: String::new(),
                start_byte: 0,
                end_byte: 1,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_no_slash_returns_none() {
        let lines = vec!["hello world".to_string()];
        let result = detect_slash_token(&lines, 0, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_empty_line_returns_none() {
        let lines = vec![String::new()];
        let result = detect_slash_token(&lines, 0, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_invalid_row_returns_none() {
        let lines = vec!["hello".to_string()];
        let result = detect_slash_token(&lines, 5, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_slash_not_at_word_boundary() {
        // Slash preceded by non-whitespace (like in a path)
        let lines = vec!["path/to/file".to_string()];
        let result = detect_slash_token(&lines, 0, 7);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_cursor_at_end_of_token() {
        // Cursor at end of line with no trailing space - should detect token
        let lines = vec!["/cmd".to_string()];
        let result = detect_slash_token(&lines, 0, 4);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 0,
                end_byte: 4,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_cursor_on_trailing_whitespace_returns_none() {
        // When cursor is at position 5 (past end) in "/cmd " (which has trailing space),
        // the character before cursor is whitespace, so no token is detected
        let lines = vec!["/cmd ".to_string()];
        let result = detect_slash_token(&lines, 0, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_cursor_on_whitespace_after_token() {
        // When cursor is directly on the space character immediately after the token,
        // it detects the token to the left (this is the desired behavior for autocomplete)
        let lines = vec!["/cmd ".to_string()];
        let result = detect_slash_token(&lines, 0, 4);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 0,
                end_byte: 4,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_with_unicode() {
        let lines = vec!["/über".to_string()];
        let result = detect_slash_token(&lines, 0, 5);
        assert_eq!(
            result,
            Some(SlashToken {
                name: "über".to_string(),
                start_byte: 0,
                end_byte: 6, // "über" is 5 bytes in UTF-8
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_after_unicode_text() {
        let lines = vec!["日本語 /cmd".to_string()];
        // "日本語 " is 10 bytes (3 chars * 3 bytes + 1 space)
        let result = detect_slash_token(&lines, 0, 8); // cursor at char position 8 (end of cmd)
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 10,
                end_byte: 14,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_cursor_at_start() {
        let lines = vec!["/cmd".to_string()];
        let result = detect_slash_token(&lines, 0, 0);
        // Cursor at position 0, right before the slash
        // This should detect the token since cursor is at start of the token
        assert_eq!(
            result,
            Some(SlashToken {
                name: "cmd".to_string(),
                start_byte: 0,
                end_byte: 4,
                row: 0,
            })
        );
    }

    #[test]
    fn detect_slash_only_at_start() {
        let lines = vec!["/".to_string()];
        let result = detect_slash_token(&lines, 0, 1);
        assert_eq!(
            result,
            Some(SlashToken {
                name: String::new(),
                start_byte: 0,
                end_byte: 1,
                row: 0,
            })
        );
    }
}
