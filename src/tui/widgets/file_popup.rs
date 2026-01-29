//! File suggestion popup widget for @ mentions.
//!
//! Displays a popup with file path suggestions when the user types
//! an @ token in the text input.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Widget},
};

use crate::file_search::FileMatch;
use crate::tui::Theme;

/// Maximum number of visible rows in the popup.
pub const MAX_POPUP_ROWS: usize = 8;

/// State of the file suggestion popup.
#[derive(Debug, Clone, Default)]
pub enum PopupState {
    /// No popup should be shown.
    #[default]
    Hidden,
    /// Currently loading results.
    Loading,
    /// No matches found for the query.
    NoMatches,
    /// Showing file suggestions.
    Showing {
        /// The matched files.
        matches: Vec<FileMatch>,
        /// Currently selected index (0-indexed).
        selected: usize,
    },
}

impl PopupState {
    /// Returns true if the popup is visible (not hidden).
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        !matches!(self, Self::Hidden)
    }

    /// Returns the number of matches if in Showing state.
    #[must_use]
    pub fn match_count(&self) -> usize {
        match self {
            Self::Showing { matches, .. } => matches.len(),
            _ => 0,
        }
    }

    /// Returns the currently selected index if in Showing state.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        match self {
            Self::Showing { selected, .. } => Some(*selected),
            _ => None,
        }
    }
}

/// A popup widget for displaying file suggestions.
pub struct FileSuggestionPopup<'a> {
    /// The popup state.
    state: &'a PopupState,
    /// The search query (displayed in title).
    query: &'a str,
    /// Theme for styling.
    theme: &'a Theme,
}

impl<'a> FileSuggestionPopup<'a> {
    /// Creates a new file suggestion popup.
    #[must_use]
    pub const fn new(state: &'a PopupState, query: &'a str, theme: &'a Theme) -> Self {
        Self {
            state,
            query,
            theme,
        }
    }

    /// Calculates the preferred size for the popup.
    ///
    /// Returns (width, height) in terminal cells.
    #[must_use]
    pub fn preferred_size(&self) -> (u16, u16) {
        let width = 50u16; // Fixed width for now
        let height = match self.state {
            PopupState::Hidden => 0,
            PopupState::Loading | PopupState::NoMatches => 3, // Border + 1 line + border
            PopupState::Showing { matches, .. } => {
                let content_rows = matches.len().min(MAX_POPUP_ROWS);
                // Safe cast: MAX_POPUP_ROWS is 8, so content_rows fits in u16
                #[allow(clippy::cast_possible_truncation)]
                let rows = content_rows as u16;
                rows + 2 // +2 for borders
            }
        };
        (width, height)
    }
}

impl Widget for FileSuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if matches!(self.state, PopupState::Hidden) {
            return;
        }

        // Clear the area first
        Clear.render(area, buf);

        let title = if self.query.is_empty() {
            " Files ".to_string()
        } else {
            format!(" Files matching @{} ", self.query)
        };

        let block = Block::default()
            .title(title)
            .title_style(self.theme.header_style())
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        let inner = block.inner(area);
        block.render(area, buf);

        match self.state {
            PopupState::Hidden => {}
            PopupState::Loading => {
                let text = Line::from("Loading...").style(self.theme.muted_style());
                Widget::render(text, inner, buf);
            }
            PopupState::NoMatches => {
                let text = Line::from("No matches").style(self.theme.muted_style());
                Widget::render(text, inner, buf);
            }
            PopupState::Showing { matches, selected } => {
                // Render file list with selection
                let items: Vec<ListItem> = matches
                    .iter()
                    .enumerate()
                    .take(MAX_POPUP_ROWS)
                    .map(|(i, file_match)| {
                        // Append trailing slash for directories
                        let path_str = if file_match.is_dir {
                            format!("{}/", file_match.path.display())
                        } else {
                            file_match.path.display().to_string()
                        };
                        let is_selected = i == *selected;
                        let style = if is_selected {
                            self.theme.highlight_style()
                        } else {
                            self.theme.normal_style()
                        };
                        let prefix = if is_selected { "> " } else { "  " };
                        ListItem::new(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(path_str, style),
                        ]))
                    })
                    .collect();

                let list = List::new(items);
                Widget::render(list, inner, buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn create_theme() -> Theme {
        Theme::default()
    }

    fn create_test_matches(count: usize) -> Vec<FileMatch> {
        (0..count)
            .map(|i| FileMatch {
                path: PathBuf::from(format!("src/file_{i}.rs")),
                // Safe: test uses small counts well below u32::MAX
                score: 100_u32.saturating_sub(u32::try_from(i).unwrap_or(u32::MAX)),
                is_dir: false,
            })
            .collect()
    }

    // =========================================================================
    // PopupState Tests
    // =========================================================================

    mod popup_state {
        use super::*;

        /// Tests that Hidden state is the default.
        #[test]
        fn default_is_hidden() {
            let state = PopupState::default();
            assert!(matches!(state, PopupState::Hidden));
        }

        /// Tests `is_visible` returns false for Hidden state.
        #[test]
        fn is_visible_hidden_returns_false() {
            let state = PopupState::Hidden;
            assert!(!state.is_visible());
        }

        /// Tests `is_visible` returns true for Loading state.
        #[test]
        fn is_visible_loading_returns_true() {
            let state = PopupState::Loading;
            assert!(state.is_visible());
        }

        /// Tests `is_visible` returns true for `NoMatches` state.
        #[test]
        fn is_visible_no_matches_returns_true() {
            let state = PopupState::NoMatches;
            assert!(state.is_visible());
        }

        /// Tests `is_visible` returns true for Showing state.
        #[test]
        fn is_visible_showing_returns_true() {
            let state = PopupState::Showing {
                matches: vec![],
                selected: 0,
            };
            assert!(state.is_visible());
        }

        /// Tests `match_count` returns 0 for Hidden state.
        #[test]
        fn match_count_hidden_returns_zero() {
            let state = PopupState::Hidden;
            assert_eq!(state.match_count(), 0);
        }

        /// Tests `match_count` returns 0 for Loading state.
        #[test]
        fn match_count_loading_returns_zero() {
            let state = PopupState::Loading;
            assert_eq!(state.match_count(), 0);
        }

        /// Tests `match_count` returns correct count for Showing state.
        #[test]
        fn match_count_showing_returns_count() {
            let matches = create_test_matches(5);
            let state = PopupState::Showing {
                matches,
                selected: 0,
            };
            assert_eq!(state.match_count(), 5);
        }

        /// Tests `selected_index` returns None for Hidden state.
        #[test]
        fn selected_index_hidden_returns_none() {
            let state = PopupState::Hidden;
            assert!(state.selected_index().is_none());
        }

        /// Tests `selected_index` returns Some for Showing state.
        #[test]
        fn selected_index_showing_returns_index() {
            let state = PopupState::Showing {
                matches: create_test_matches(3),
                selected: 2,
            };
            assert_eq!(state.selected_index(), Some(2));
        }

        /// Tests that `PopupState` can be cloned.
        #[test]
        fn clone_preserves_state() {
            let original = PopupState::Showing {
                matches: create_test_matches(2),
                selected: 1,
            };
            let cloned = original.clone();
            assert_eq!(cloned.match_count(), 2);
            assert_eq!(cloned.selected_index(), Some(1));
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let state = PopupState::Loading;
            let debug_str = format!("{state:?}");
            assert!(debug_str.contains("Loading"));
        }
    }

    // =========================================================================
    // FileSuggestionPopup Tests
    // =========================================================================

    mod file_suggestion_popup {
        use super::*;

        /// Tests creating a popup with `new()`.
        #[test]
        fn new_stores_parameters() {
            let theme = create_theme();
            let state = PopupState::Loading;
            let popup = FileSuggestionPopup::new(&state, "test", &theme);

            assert_eq!(popup.query, "test");
        }

        /// Tests `preferred_size` for Hidden state returns zero height.
        #[test]
        fn preferred_size_hidden_returns_zero_height() {
            let theme = create_theme();
            let state = PopupState::Hidden;
            let popup = FileSuggestionPopup::new(&state, "", &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 50);
            assert_eq!(height, 0);
        }

        /// Tests `preferred_size` for Loading state.
        #[test]
        fn preferred_size_loading_returns_minimal_height() {
            let theme = create_theme();
            let state = PopupState::Loading;
            let popup = FileSuggestionPopup::new(&state, "", &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 50);
            assert_eq!(height, 3); // borders + 1 line
        }

        /// Tests `preferred_size` for `NoMatches` state.
        #[test]
        fn preferred_size_no_matches_returns_minimal_height() {
            let theme = create_theme();
            let state = PopupState::NoMatches;
            let popup = FileSuggestionPopup::new(&state, "", &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 50);
            assert_eq!(height, 3);
        }

        /// Tests `preferred_size` for Showing state with few matches.
        #[test]
        fn preferred_size_showing_scales_with_matches() {
            let theme = create_theme();
            let state = PopupState::Showing {
                matches: create_test_matches(3),
                selected: 0,
            };
            let popup = FileSuggestionPopup::new(&state, "", &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 50);
            assert_eq!(height, 5); // 3 rows + 2 borders
        }

        /// Tests `preferred_size` caps at `MAX_POPUP_ROWS`.
        #[test]
        fn preferred_size_caps_at_max_rows() -> Result<()> {
            let theme = create_theme();
            let state = PopupState::Showing {
                matches: create_test_matches(20), // More than MAX_POPUP_ROWS
                selected: 0,
            };
            let popup = FileSuggestionPopup::new(&state, "", &theme);

            let (width, height) = popup.preferred_size();
            assert_eq!(width, 50);
            let expected_height = u16::try_from(MAX_POPUP_ROWS)? + 2;
            assert_eq!(height, expected_height);
            Ok(())
        }
    }

    // =========================================================================
    // Widget Rendering Tests
    // =========================================================================

    mod rendering {
        use super::*;

        fn render_popup(state: &PopupState, query: &str) -> Result<Terminal<TestBackend>> {
            let backend = TestBackend::new(60, 12);
            let mut terminal = Terminal::new(backend)?;
            let theme = create_theme();

            terminal.draw(|f| {
                let popup = FileSuggestionPopup::new(state, query, &theme);
                let area = Rect::new(0, 0, 50, 10);
                f.render_widget(popup, area);
            })?;

            Ok(terminal)
        }

        /// Tests that Hidden state renders nothing.
        #[test]
        fn hidden_renders_nothing() -> Result<()> {
            let state = PopupState::Hidden;
            let terminal = render_popup(&state, "")?;

            // Hidden state should not render any visible content
            let buffer = terminal.backend().buffer();
            // The first cell should be empty (space)
            assert_eq!(buffer[(0, 0)].symbol(), " ");
            Ok(())
        }

        /// Tests that Loading state shows loading message.
        #[test]
        fn loading_shows_message() -> Result<()> {
            let state = PopupState::Loading;
            let terminal = render_popup(&state, "test")?;

            let buffer = terminal.backend().buffer();
            let content: String = (0..50).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(content.contains("Loading"));
            Ok(())
        }

        /// Tests that `NoMatches` state shows appropriate message.
        #[test]
        fn no_matches_shows_message() -> Result<()> {
            let state = PopupState::NoMatches;
            let terminal = render_popup(&state, "xyz")?;

            let buffer = terminal.backend().buffer();
            let content: String = (0..50).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(content.contains("No matches"));
            Ok(())
        }

        /// Tests that Showing state renders file list.
        #[test]
        fn showing_renders_files() -> Result<()> {
            let state = PopupState::Showing {
                matches: create_test_matches(3),
                selected: 0,
            };
            let terminal = render_popup(&state, "file")?;

            let buffer = terminal.backend().buffer();
            // Check that file paths are rendered
            let row1: String = (0..50).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(row1.contains("src/file_0.rs"));
            Ok(())
        }

        /// Tests that selected item has selection indicator.
        #[test]
        fn selection_indicator_shown() -> Result<()> {
            let state = PopupState::Showing {
                matches: create_test_matches(3),
                selected: 1,
            };
            let terminal = render_popup(&state, "")?;

            let buffer = terminal.backend().buffer();
            // Check row 2 (second item, selected) starts with ">"
            let row2: String = (0..10).map(|x| buffer[(x, 2)].symbol()).collect();
            assert!(row2.contains('>'));
            Ok(())
        }

        /// Tests title shows query.
        #[test]
        fn title_shows_query() -> Result<()> {
            let state = PopupState::Loading;
            let terminal = render_popup(&state, "main.rs")?;

            let buffer = terminal.backend().buffer();
            let title: String = (0..50).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title.contains("@main.rs"));
            Ok(())
        }

        /// Tests title with empty query.
        #[test]
        fn title_with_empty_query() -> Result<()> {
            let state = PopupState::Loading;
            let terminal = render_popup(&state, "")?;

            let buffer = terminal.backend().buffer();
            let title: String = (0..50).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(title.contains("Files"));
            Ok(())
        }
    }
}
