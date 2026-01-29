//! Slash command suggestion popup widget.
//!
//! Displays a popup with available slash commands when the user types
//! `/` at the start of input.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Widget},
};

use crate::tui::Theme;

/// Maximum number of visible rows in the command popup.
pub const MAX_COMMAND_POPUP_ROWS: usize = 8;

/// A matched slash command for display.
#[derive(Debug, Clone)]
pub struct CommandMatch {
    /// Command name (without slash).
    pub name: &'static str,
    /// Command description.
    pub description: &'static str,
}

/// State of the slash command suggestion popup.
#[derive(Debug, Clone, Default)]
pub enum CommandPopupState {
    /// Popup is not visible.
    #[default]
    Hidden,
    /// Popup is showing matching commands.
    Showing {
        /// List of matching commands (name, description).
        matches: Vec<CommandMatch>,
        /// Currently selected index.
        selected: usize,
    },
}

impl CommandPopupState {
    /// Returns true if the popup is visible.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        matches!(self, Self::Showing { .. })
    }

    /// Move selection up by one.
    pub fn select_up(&mut self) {
        if let Self::Showing { selected, .. } = self {
            *selected = selected.saturating_sub(1);
        }
    }

    /// Move selection down by one.
    pub fn select_down(&mut self) {
        if let Self::Showing { matches, selected } = self {
            *selected = (*selected + 1).min(matches.len().saturating_sub(1));
        }
    }

    /// Returns the currently selected command name, if any.
    #[must_use]
    pub fn selected_command(&self) -> Option<&str> {
        if let Self::Showing { matches, selected } = self {
            matches.get(*selected).map(|m| m.name)
        } else {
            None
        }
    }

    /// Returns the number of matches if in Showing state.
    #[must_use]
    pub fn match_count(&self) -> usize {
        match self {
            Self::Showing { matches, .. } => matches.len(),
            Self::Hidden => 0,
        }
    }

    /// Returns the currently selected index if in Showing state.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        match self {
            Self::Showing { selected, .. } => Some(*selected),
            Self::Hidden => None,
        }
    }
}

/// Widget for rendering slash command suggestions.
pub struct CommandPopup<'a> {
    /// The popup state.
    state: &'a CommandPopupState,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> CommandPopup<'a> {
    /// Creates a new command popup widget.
    #[must_use]
    pub const fn new(state: &'a CommandPopupState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }

    /// Calculates the preferred size for the popup.
    ///
    /// Returns (width, height) in terminal cells.
    #[must_use]
    pub fn preferred_size(&self) -> (u16, u16) {
        match self.state {
            CommandPopupState::Hidden => (0, 0),
            CommandPopupState::Showing { matches, .. } => {
                if matches.is_empty() {
                    return (0, 0);
                }

                // Calculate width based on longest command + description
                let max_name_len = matches.iter().map(|m| m.name.len()).max().unwrap_or(0);
                let max_desc_len = matches
                    .iter()
                    .map(|m| m.description.len())
                    .max()
                    .unwrap_or(0);

                // Format: "/name  description" + padding
                // Safe cast: reasonable command lengths won't overflow u16
                #[allow(clippy::cast_possible_truncation)]
                let width = (1 + max_name_len + 2 + max_desc_len + 4).min(60) as u16;

                let content_rows = matches.len().min(MAX_COMMAND_POPUP_ROWS);
                // Safe cast: MAX_COMMAND_POPUP_ROWS is 8, so content_rows fits in u16
                #[allow(clippy::cast_possible_truncation)]
                let height = (content_rows + 2) as u16; // +2 for borders

                (width, height)
            }
        }
    }
}

impl Widget for CommandPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let CommandPopupState::Showing { matches, selected } = self.state else {
            return;
        };

        if matches.is_empty() {
            return;
        }

        // Clear the area first
        Clear.render(area, buf);

        let block = Block::default()
            .title(" Commands ")
            .title_style(self.theme.header_style())
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        let inner = block.inner(area);
        block.render(area, buf);

        // Calculate max name length for alignment
        let max_name_len = matches.iter().map(|m| m.name.len()).max().unwrap_or(0);

        // Render each command
        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .take(inner.height as usize)
            .map(|(i, cmd_match)| {
                let is_selected = i == *selected;
                let style = if is_selected {
                    self.theme.highlight_style()
                } else {
                    self.theme.normal_style()
                };
                let prefix = if is_selected { "> " } else { "  " };
                let line = format!(
                    "/{:<width$}  {}",
                    cmd_match.name,
                    cmd_match.description,
                    width = max_name_len
                );
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(line, style),
                ]))
            })
            .collect();

        let list = List::new(items);
        Widget::render(list, inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn create_theme() -> Theme {
        Theme::default()
    }

    fn create_test_commands(count: usize) -> Vec<CommandMatch> {
        let commands = [
            ("help", "Show available commands"),
            ("clear", "Clear the screen"),
            ("quit", "Exit the application"),
            ("settings", "Open settings"),
            ("save", "Save current state"),
            ("load", "Load saved state"),
            ("undo", "Undo last action"),
            ("redo", "Redo last action"),
        ];

        commands
            .iter()
            .take(count)
            .map(|(name, desc)| CommandMatch {
                name,
                description: desc,
            })
            .collect()
    }

    // =========================================================================
    // CommandPopupState Tests
    // =========================================================================

    mod command_popup_state {
        use super::*;

        /// Tests that Hidden state is the default.
        #[test]
        fn default_is_hidden() {
            let state = CommandPopupState::default();
            assert!(matches!(state, CommandPopupState::Hidden));
        }

        /// Tests `is_visible` returns false for Hidden state.
        #[test]
        fn is_visible_hidden_returns_false() {
            let state = CommandPopupState::Hidden;
            assert!(!state.is_visible());
        }

        /// Tests `is_visible` returns true for Showing state.
        #[test]
        fn is_visible_showing_returns_true() {
            let state = CommandPopupState::Showing {
                matches: vec![],
                selected: 0,
            };
            assert!(state.is_visible());
        }

        /// Tests `select_up` decrements selection.
        #[test]
        fn select_up_decrements() {
            let mut state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 2,
            };
            state.select_up();
            assert_eq!(state.selected_index(), Some(1));
        }

        /// Tests `select_up` saturates at zero.
        #[test]
        fn select_up_saturates_at_zero() {
            let mut state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 0,
            };
            state.select_up();
            assert_eq!(state.selected_index(), Some(0));
        }

        /// Tests `select_down` increments selection.
        #[test]
        fn select_down_increments() {
            let mut state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 0,
            };
            state.select_down();
            assert_eq!(state.selected_index(), Some(1));
        }

        /// Tests `select_down` saturates at max.
        #[test]
        fn select_down_saturates_at_max() {
            let mut state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 2,
            };
            state.select_down();
            assert_eq!(state.selected_index(), Some(2));
        }

        /// Tests `selected_command` returns correct command.
        #[test]
        fn selected_command_returns_name() {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 1,
            };
            assert_eq!(state.selected_command(), Some("clear"));
        }

        /// Tests `selected_command` returns None for Hidden state.
        #[test]
        fn selected_command_hidden_returns_none() {
            let state = CommandPopupState::Hidden;
            assert!(state.selected_command().is_none());
        }

        /// Tests `match_count` returns correct count.
        #[test]
        fn match_count_returns_count() {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(5),
                selected: 0,
            };
            assert_eq!(state.match_count(), 5);
        }

        /// Tests `match_count` returns 0 for Hidden state.
        #[test]
        fn match_count_hidden_returns_zero() {
            let state = CommandPopupState::Hidden;
            assert_eq!(state.match_count(), 0);
        }

        /// Tests `selected_index` returns Some for Showing state.
        #[test]
        fn selected_index_showing_returns_index() {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 2,
            };
            assert_eq!(state.selected_index(), Some(2));
        }

        /// Tests `selected_index` returns None for Hidden state.
        #[test]
        fn selected_index_hidden_returns_none() {
            let state = CommandPopupState::Hidden;
            assert!(state.selected_index().is_none());
        }

        /// Tests that `CommandPopupState` can be cloned.
        #[test]
        fn clone_preserves_state() {
            let original = CommandPopupState::Showing {
                matches: create_test_commands(2),
                selected: 1,
            };
            let cloned = original.clone();
            assert_eq!(cloned.match_count(), 2);
            assert_eq!(cloned.selected_index(), Some(1));
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let state = CommandPopupState::Hidden;
            let debug_str = format!("{state:?}");
            assert!(debug_str.contains("Hidden"));
        }
    }

    // =========================================================================
    // CommandPopup Tests
    // =========================================================================

    mod command_popup {
        use super::*;

        /// Tests creating a popup with `new()`.
        #[test]
        fn new_stores_parameters() {
            let theme = create_theme();
            let state = CommandPopupState::Hidden;
            let _popup = CommandPopup::new(&state, &theme);
            // If it compiles, it works
        }

        /// Tests `preferred_size` for Hidden state returns zero.
        #[test]
        fn preferred_size_hidden_returns_zero() {
            let theme = create_theme();
            let state = CommandPopupState::Hidden;
            let popup = CommandPopup::new(&state, &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 0);
            assert_eq!(height, 0);
        }

        /// Tests `preferred_size` for empty matches returns zero.
        #[test]
        fn preferred_size_empty_matches_returns_zero() {
            let theme = create_theme();
            let state = CommandPopupState::Showing {
                matches: vec![],
                selected: 0,
            };
            let popup = CommandPopup::new(&state, &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 0);
            assert_eq!(height, 0);
        }

        /// Tests `preferred_size` for Showing state with matches.
        #[test]
        fn preferred_size_showing_scales_with_matches() {
            let theme = create_theme();
            let state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 0,
            };
            let popup = CommandPopup::new(&state, &theme);

            let (width, height) = popup.preferred_size();
            assert!(width > 0);
            assert_eq!(height, 5); // 3 rows + 2 borders
        }

        /// Tests `preferred_size` caps at `MAX_COMMAND_POPUP_ROWS`.
        #[test]
        fn preferred_size_caps_at_max_rows() -> Result<()> {
            let theme = create_theme();
            let state = CommandPopupState::Showing {
                matches: create_test_commands(8), // At MAX_COMMAND_POPUP_ROWS
                selected: 0,
            };
            let popup = CommandPopup::new(&state, &theme);

            let (_, height) = popup.preferred_size();
            let expected_height = u16::try_from(MAX_COMMAND_POPUP_ROWS)? + 2;
            assert_eq!(height, expected_height);
            Ok(())
        }
    }

    // =========================================================================
    // Widget Rendering Tests
    // =========================================================================

    mod rendering {
        use super::*;

        fn render_popup(state: &CommandPopupState) -> Result<Terminal<TestBackend>> {
            let backend = TestBackend::new(60, 12);
            let mut terminal = Terminal::new(backend)?;
            let theme = create_theme();

            terminal.draw(|f| {
                let popup = CommandPopup::new(state, &theme);
                let area = Rect::new(0, 0, 50, 10);
                f.render_widget(popup, area);
            })?;

            Ok(terminal)
        }

        /// Tests that Hidden state renders nothing.
        #[test]
        fn hidden_renders_nothing() -> Result<()> {
            let state = CommandPopupState::Hidden;
            let terminal = render_popup(&state)?;

            // Hidden state should not render any visible content
            let buffer = terminal.backend().buffer();
            // The first cell should be empty (space)
            assert_eq!(buffer[(0, 0)].symbol(), " ");
            Ok(())
        }

        /// Tests that empty matches renders nothing.
        #[test]
        fn empty_matches_renders_nothing() -> Result<()> {
            let state = CommandPopupState::Showing {
                matches: vec![],
                selected: 0,
            };
            let terminal = render_popup(&state)?;

            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].symbol(), " ");
            Ok(())
        }

        /// Tests that Showing state renders command list.
        #[test]
        fn showing_renders_commands() -> Result<()> {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 0,
            };
            let terminal = render_popup(&state)?;

            let buffer = terminal.backend().buffer();
            // Check that command names are rendered (look for "help")
            let row1: String = (0..50).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(row1.contains("/help"));
            Ok(())
        }

        /// Tests that selected item has selection indicator.
        #[test]
        fn selection_indicator_shown() -> Result<()> {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(3),
                selected: 1,
            };
            let terminal = render_popup(&state)?;

            let buffer = terminal.backend().buffer();
            // Check row 2 (second item, selected) starts with ">"
            let row2: String = (0..10).map(|x| buffer[(x, 2)].symbol()).collect();
            assert!(row2.contains('>'));
            Ok(())
        }

        /// Tests title shows "Commands".
        #[test]
        fn title_shows_commands() -> Result<()> {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(2),
                selected: 0,
            };
            let terminal = render_popup(&state)?;

            let buffer = terminal.backend().buffer();
            let title: String = (0..50).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title.contains("Commands"));
            Ok(())
        }

        /// Tests that description is rendered.
        #[test]
        fn description_is_rendered() -> Result<()> {
            let state = CommandPopupState::Showing {
                matches: create_test_commands(1),
                selected: 0,
            };
            let terminal = render_popup(&state)?;

            let buffer = terminal.backend().buffer();
            let row1: String = (0..50).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(row1.contains("Show available commands"));
            Ok(())
        }
    }
}
