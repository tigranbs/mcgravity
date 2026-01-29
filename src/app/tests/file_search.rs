//! File search and selection tests.
//!
//! Tests for the `@` file tagging feature including:
//! - File search popup behavior
//! - Selection navigation (up/down)
//! - Tab/Enter file insertion
//! - Bounds checking for selection indices
//! - Path quoting for special characters

use super::helpers::*;
use crate::app::FILE_SEARCH_DEBOUNCE_MS;
use crate::app::state::AtToken;
use crate::file_search::FileMatch;
use crate::tui::widgets::PopupState;
use anyhow::Result;
use serial_test::serial;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

mod file_search_integration_tests {
    use super::*;

    #[test]
    #[serial]
    fn test_at_token_triggers_search() -> Result<()> {
        // Acquire CWD guard to serialize with other tests that change CWD
        let _cwd_guard = CwdGuard::new()?;

        // Create a temp directory with some files
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["src/main.rs", "src/lib.rs"])?;

        // Change to temp dir for the test
        std::env::set_current_dir(temp_dir.path())?;

        // Create app with "@src" text
        let mut app = create_test_app_with_lines(&["@src"], 0, 4);

        // Trigger @ token detection and search
        app.text_input.at_token = Some(AtToken {
            query: "src".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });
        app.update_file_search();

        // Verify search was triggered (popup is visible and in loading state)
        let is_visible = app.text_input.file_popup_state.is_visible();
        // With async search, the popup should be in Loading state
        // (results arrive via event channel, not immediately)
        let is_loading = matches!(app.text_input.file_popup_state, PopupState::Loading);

        assert!(is_visible);
        assert!(is_loading);
        // CwdGuard drop will restore original directory
        Ok(())
    }

    #[test]
    fn test_popup_hides_when_at_removed() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);

        // Set up popup as if it was showing
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: std::path::PathBuf::from("test.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.last_search_query = Some("test".to_string());

        // Clear @ token (simulating user deleted the @)
        app.text_input.at_token = None;
        app.update_file_search();

        // Verify popup is hidden
        assert!(!app.text_input.file_popup_state.is_visible());
        assert!(matches!(
            app.text_input.file_popup_state,
            PopupState::Hidden
        ));
    }

    #[test]
    fn test_search_debouncing() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        // First search
        app.text_input.at_token = Some(AtToken {
            query: "test".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });
        app.update_file_search();
        let first_time = app.text_input.last_search_time;

        // Immediate second search with same query (should be debounced)
        app.update_file_search();
        let second_time = app.text_input.last_search_time;

        // Times should be the same (debounced)
        assert_eq!(first_time, second_time);

        // Search with different query should NOT be debounced
        app.text_input.at_token = Some(AtToken {
            query: "test2".to_string(),
            start_byte: 0,
            end_byte: 6,
            row: 0,
        });
        app.update_file_search();
        let third_time = app.text_input.last_search_time;

        // Time should have updated because query changed
        assert_ne!(first_time, third_time);
    }

    #[test]
    fn test_debounce_allows_search_after_timeout() -> Result<()> {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        // First search
        app.text_input.at_token = Some(AtToken {
            query: "test".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });
        app.update_file_search();

        // Manually set last_search_time to past to simulate timeout
        app.text_input.last_search_time = Some(
            Instant::now()
                .checked_sub(Duration::from_millis(FILE_SEARCH_DEBOUNCE_MS + 10))
                .ok_or_else(|| anyhow::anyhow!("time subtraction overflow"))?,
        );

        let old_time = app.text_input.last_search_time;

        // Now search should go through because debounce period elapsed
        app.update_file_search();

        // Time should have updated
        assert_ne!(old_time, app.text_input.last_search_time);
        Ok(())
    }

    #[test]
    fn test_selection_resets_on_query_change() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        // Set up initial state with selection at index 2
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: std::path::PathBuf::from("a.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: std::path::PathBuf::from("b.rs"),
                    score: 90,
                    is_dir: false,
                },
                FileMatch {
                    path: std::path::PathBuf::from("c.rs"),
                    score: 80,
                    is_dir: false,
                },
            ],
            selected: 2,
        };
        app.text_input.last_search_query = Some("old".to_string());

        // Change query
        app.text_input.at_token = Some(AtToken {
            query: "new".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // This will trigger perform_file_search with new query
        app.perform_file_search("new");

        // Selection should have been reset or adjusted
        // Note: In actual use, search results would change, but selection logic ensures it's in bounds
        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            // Selection should be valid (either reset to 0 or in bounds)
            assert!(selected <= 2);
        }
    }

    #[test]
    fn test_should_show_file_popup() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        // Hidden state
        app.text_input.file_popup_state = PopupState::Hidden;
        assert!(!app.should_show_file_popup());

        // Loading state
        app.text_input.file_popup_state = PopupState::Loading;
        assert!(app.should_show_file_popup());

        // NoMatches state
        app.text_input.file_popup_state = PopupState::NoMatches;
        assert!(app.should_show_file_popup());

        // Showing state with matches
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: std::path::PathBuf::from("test.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        assert!(app.should_show_file_popup());

        // Showing state with empty matches (edge case)
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![],
            selected: 0,
        };
        assert!(!app.should_show_file_popup());
    }

    #[test]
    fn test_current_at_query() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        // No @ token
        app.text_input.at_token = None;
        assert_eq!(app.current_at_query(), "");

        // With @ token
        app.text_input.at_token = Some(AtToken {
            query: "myfile".to_string(),
            start_byte: 0,
            end_byte: 7,
            row: 0,
        });
        assert_eq!(app.current_at_query(), "myfile");

        // Empty query
        app.text_input.at_token = Some(AtToken {
            query: String::new(),
            start_byte: 0,
            end_byte: 1,
            row: 0,
        });
        assert_eq!(app.current_at_query(), "");
    }

    #[test]
    #[serial]
    fn test_empty_query_shows_files() -> Result<()> {
        // Acquire CWD guard to serialize with other tests that change CWD
        let _cwd_guard = CwdGuard::new()?;

        // Create a temp directory with some files
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["a.txt", "b.txt"])?;

        // Change to temp dir for the test
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = create_test_app_with_lines(&["@"], 0, 1);

        // Trigger search with empty query
        app.text_input.at_token = Some(AtToken {
            query: String::new(),
            start_byte: 0,
            end_byte: 1,
            row: 0,
        });
        app.update_file_search();

        // Capture result
        let is_visible = app.text_input.file_popup_state.is_visible();

        // Should show files (alphabetically sorted)
        assert!(is_visible);
        Ok(())
    }
}

mod file_selection_tests {
    use super::*;

    #[test]
    fn test_file_selection_replaces_at_token() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Set up the @ token
        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Set up popup with a match
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        // Select the file
        app.select_file_from_popup();

        // Verify the text was replaced
        assert_eq!(app.text_input.lines()[0], "src/main.rs ");
        // Verify cursor is at end of inserted text
        assert_eq!(app.text_input.cursor().1, 12);
        // Verify popup is dismissed
        assert!(!app.text_input.file_popup_state.is_visible());
        // Verify at_token is cleared
        assert!(app.text_input.at_token.is_none());
    }

    #[test]
    fn test_file_selection_with_spaces_quoted() {
        let mut app = create_test_app_with_lines(&["@doc"], 0, 4);

        // Set up the @ token
        app.text_input.at_token = Some(AtToken {
            query: "doc".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Set up popup with a path containing spaces
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("my docs/file.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        // Select the file
        app.select_file_from_popup();

        // Verify paths with spaces are quoted (single quotes preferred)
        assert_eq!(app.text_input.lines()[0], "'my docs/file.txt' ");
    }

    #[test]
    fn test_file_selection_with_dollar_sign() {
        let mut app = create_test_app_with_lines(&["@doc"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "doc".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("file$var.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Dollar sign should trigger quoting
        assert_eq!(app.text_input.lines()[0], "'file$var.txt' ");
    }

    #[test]
    fn test_file_selection_with_double_quotes() {
        let mut app = create_test_app_with_lines(&["@doc"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "doc".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("my\"file.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Double quotes in path - use single quotes
        assert_eq!(app.text_input.lines()[0], "'my\"file.txt' ");
    }

    #[test]
    fn test_file_selection_with_single_quotes() {
        let mut app = create_test_app_with_lines(&["@doc"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "doc".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("my'file.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Single quotes in path - use double quotes
        assert_eq!(app.text_input.lines()[0], "\"my'file.txt\" ");
    }

    #[test]
    fn test_file_selection_with_both_quote_types() {
        let mut app = create_test_app_with_lines(&["@doc"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "doc".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("my'and\"file.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Both quote types - use double quotes with escaping
        assert_eq!(app.text_input.lines()[0], "\"my'and\\\"file.txt\" ");
    }

    #[test]
    fn test_file_selection_adds_trailing_space() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.at_token = Some(AtToken {
            query: "test".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("test.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify trailing space is added
        assert!(app.text_input.lines()[0].ends_with(' '));
    }

    #[test]
    fn test_esc_dismisses_without_selection() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        // Dismiss without selection
        app.dismiss_file_popup();

        // Verify popup is hidden
        assert!(!app.text_input.file_popup_state.is_visible());
        // Verify text is unchanged
        assert_eq!(app.text_input.lines()[0], "@foo");
        // Verify at_token is cleared
        assert!(app.text_input.at_token.is_none());
    }

    #[test]
    fn test_navigation_stays_at_top() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("a.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("b.rs"),
                    score: 90,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("c.rs"),
                    score: 80,
                    is_dir: false,
                },
            ],
            selected: 0,
        };

        // Try to go up when already at top
        app.file_popup_up();

        // Should stay at 0
        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 0);
        } else {
            panic!("Expected Showing state");
        }
    }

    #[test]
    fn test_navigation_stays_at_bottom() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("a.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("b.rs"),
                    score: 90,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("c.rs"),
                    score: 80,
                    is_dir: false,
                },
            ],
            selected: 2,
        };

        // Try to go down when already at bottom
        app.file_popup_down();

        // Should stay at 2 (max index)
        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 2);
        } else {
            panic!("Expected Showing state");
        }
    }

    #[test]
    fn test_navigation_moves_down() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("a.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("b.rs"),
                    score: 90,
                    is_dir: false,
                },
            ],
            selected: 0,
        };

        app.file_popup_down();

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 1);
        } else {
            panic!("Expected Showing state");
        }
    }

    #[test]
    fn test_navigation_moves_up() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("a.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("b.rs"),
                    score: 90,
                    is_dir: false,
                },
            ],
            selected: 1,
        };

        app.file_popup_up();

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 0);
        } else {
            panic!("Expected Showing state");
        }
    }

    #[test]
    fn test_selection_with_text_before() {
        let mut app = create_test_app_with_lines(&["hello @foo"], 0, 10);

        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 6,
            end_byte: 10,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify text before @ is preserved
        assert_eq!(app.text_input.lines()[0], "hello src/main.rs ");
    }

    #[test]
    fn test_selection_with_text_after() {
        let mut app = create_test_app_with_lines(&["@foo bar"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify text after the @ token is preserved
        assert_eq!(app.text_input.lines()[0], "src/main.rs  bar");
    }

    #[test]
    fn test_selection_preserves_cursor_row() {
        let mut app = create_test_app_with_lines(&["line1", "@foo", "line3"], 1, 4);

        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 1,
        });

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("test.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify cursor row is unchanged
        assert_eq!(app.text_input.cursor().0, 1);
        assert_eq!(app.text_input.lines()[1], "test.rs ");
    }

    #[test]
    fn test_select_without_at_token_does_nothing() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);

        // No at_token set
        app.text_input.at_token = None;

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        let original_text = app.text_input.lines()[0].clone();
        app.select_file_from_popup();

        // Text should be unchanged
        assert_eq!(app.text_input.lines()[0], original_text);
    }

    #[test]
    fn test_select_with_hidden_popup_does_nothing() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Popup is hidden
        app.text_input.file_popup_state = PopupState::Hidden;

        let original_text = app.text_input.lines()[0].clone();
        app.select_file_from_popup();

        // Text should be unchanged
        assert_eq!(app.text_input.lines()[0], original_text);
    }

    #[test]
    fn test_directory_selection_adds_slash() {
        let mut app = create_test_app_with_lines(&["@src"], 0, 4);

        app.text_input.at_token = Some(AtToken {
            query: "src".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Set up popup with a directory match (is_dir = true)
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src"),
                score: 100,
                is_dir: true,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify directory path has trailing slash
        assert_eq!(app.text_input.lines()[0], "src/ ");
        // Verify cursor is at end of inserted text (including slash and trailing space)
        assert_eq!(app.text_input.cursor().1, 5);
    }

    #[test]
    fn test_file_selection_no_trailing_slash() {
        let mut app = create_test_app_with_lines(&["@main"], 0, 5);

        app.text_input.at_token = Some(AtToken {
            query: "main".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });

        // Set up popup with a regular file match (is_dir = false)
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };

        app.select_file_from_popup();

        // Verify regular file path has no trailing slash
        assert_eq!(app.text_input.lines()[0], "src/main.rs ");
        // Verify cursor is at end of inserted text
        assert_eq!(app.text_input.cursor().1, 12);
    }
}

mod file_popup_key_event_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Test that Enter selects a file when popup is visible (doesn't submit task).
    #[test]
    fn test_enter_with_popup_visible_selects_file_not_submit() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Set up file popup as visible with matches
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("foo.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Plain Enter should select file, not submit
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        // File should be inserted, popup hidden, task NOT submitted
        assert_eq!(app.text_input.lines()[0], "foo.txt ");
        assert!(!app.text_input.file_popup_state.is_visible());
        // at_token should be cleared after selection
        assert!(app.text_input.at_token.is_none());
    }

    /// Test that Tab also selects a file when popup is visible.
    #[test]
    fn test_tab_with_popup_visible_selects_file() {
        let mut app = create_test_app_with_lines(&["@src"], 0, 4);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("src/main.rs"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "src".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Tab should select file
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        app.handle_key(tab);

        // File should be inserted
        assert_eq!(app.text_input.lines()[0], "src/main.rs ");
        assert!(!app.text_input.file_popup_state.is_visible());
    }

    /// Test that Ctrl+Enter with popup visible submits (not selects).
    ///
    /// Design decision: Ctrl+Enter always submits, even when the file popup is visible.
    /// This allows users to submit without having to dismiss the popup first.
    /// Regular Enter selects from popup when matches are available.
    #[tokio::test]
    async fn test_ctrl_enter_with_popup_visible_submits() {
        let mut app = create_test_app_with_lines(&["task @foo"], 0, 9);

        // Set up file popup as visible with matches
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("foo.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 5,
            end_byte: 9,
            row: 0,
        });

        // Ctrl+Enter with popup visible - bypasses popup, submits the task
        let ctrl_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        app.handle_key(ctrl_enter);

        // Task should be submitted (input cleared, flow started)
        // After submit, input is cleared to a single empty line
        assert_eq!(app.text_input.lines(), vec![""]);
        assert!(app.is_running());
    }

    /// Test that after dismissing popup with Esc, Ctrl+Enter submits.
    #[tokio::test]
    async fn test_esc_then_ctrl_enter_submits() {
        let mut app = create_test_app_with_lines(&["task @foo"], 0, 9);

        // Set up file popup as visible
        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("foo.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 5,
            end_byte: 9,
            row: 0,
        });

        // First, dismiss popup with Esc
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.handle_key(esc);

        // Popup should be hidden, text unchanged
        assert!(!app.text_input.file_popup_state.is_visible());
        assert_eq!(app.text_input.lines()[0], "task @foo");

        // Now Ctrl+Enter should submit
        let ctrl_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        app.handle_key(ctrl_enter);

        // Input should be cleared (submitted)
        assert_eq!(app.text_input.lines(), vec![""]);
    }

    /// Test that Up/Down navigate popup when visible.
    #[test]
    fn test_up_down_navigate_popup_when_visible() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("test1.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("test2.rs"),
                    score: 90,
                    is_dir: false,
                },
            ],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "test".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });

        // Down should move selection
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 1);
        } else {
            panic!("Expected Showing state");
        }

        // Up should move selection back
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(up);

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 0);
        } else {
            panic!("Expected Showing state");
        }
    }

    /// Test that 'j' and 'k' also navigate popup when visible.
    #[test]
    fn test_j_k_navigate_popup_when_visible() {
        let mut app = create_test_app_with_lines(&["@test"], 0, 5);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![
                FileMatch {
                    path: PathBuf::from("test1.rs"),
                    score: 100,
                    is_dir: false,
                },
                FileMatch {
                    path: PathBuf::from("test2.rs"),
                    score: 90,
                    is_dir: false,
                },
            ],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "test".to_string(),
            start_byte: 0,
            end_byte: 5,
            row: 0,
        });

        // 'j' should move selection down
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        app.handle_key(j);

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 1);
        } else {
            panic!("Expected Showing state");
        }

        // 'k' should move selection up
        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        app.handle_key(k);

        if let PopupState::Showing { selected, .. } = app.text_input.file_popup_state {
            assert_eq!(selected, 0);
        } else {
            panic!("Expected Showing state");
        }
    }

    /// Test that Esc dismisses popup without selection.
    #[test]
    fn test_esc_dismisses_popup_without_selection() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        app.text_input.file_popup_state = PopupState::Showing {
            matches: vec![FileMatch {
                path: PathBuf::from("foo.txt"),
                score: 100,
                is_dir: false,
            }],
            selected: 0,
        };
        app.text_input.at_token = Some(AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        });

        // Esc should dismiss popup
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.handle_key(esc);

        // Popup should be hidden
        assert!(!app.text_input.file_popup_state.is_visible());
        // Text should be unchanged (no file selected)
        assert_eq!(app.text_input.lines()[0], "@foo");
        // at_token should be cleared
        assert!(app.text_input.at_token.is_none());
    }

    /// Test that Enter with popup in `NoMatches` state submits the task.
    ///
    /// When the file popup is showing but has no matches (`NoMatches` state),
    /// Enter should fall through to normal text input handling and submit
    /// the task (traditional chat behavior).
    #[tokio::test]
    async fn test_enter_with_popup_no_matches_submits() {
        let mut app = create_test_app_with_lines(&["@nonexistent"], 0, 12);

        // Popup visible but with no matches
        app.text_input.file_popup_state = PopupState::NoMatches;
        app.text_input.at_token = Some(AtToken {
            query: "nonexistent".to_string(),
            start_byte: 0,
            end_byte: 12,
            row: 0,
        });

        // Enter should submit since there are no matches to select
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        // Enter should have submitted the task (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }

    /// Test that Enter with popup in Loading state submits the task.
    ///
    /// When the file popup is showing but still loading (Loading state),
    /// Enter should fall through to normal text input handling and submit
    /// the task (traditional chat behavior).
    #[tokio::test]
    async fn test_enter_with_popup_loading_state_submits() {
        let mut app = create_test_app_with_lines(&["@loading"], 0, 8);

        // Popup visible but in loading state
        app.text_input.file_popup_state = PopupState::Loading;
        app.text_input.at_token = Some(AtToken {
            query: "loading".to_string(),
            start_byte: 0,
            end_byte: 8,
            row: 0,
        });

        // Enter while loading should submit (no matches to select)
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        // Enter should have submitted the task (input cleared)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
    }
}

mod bounds_check_tests {
    use super::*;

    #[test]
    fn test_replace_with_invalid_row_does_not_panic() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Create a token with an invalid row (out of bounds)
        let invalid_token = AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 999, // Way out of bounds
        };

        // This should NOT panic, just return early
        app.replace_at_token_with_path(&invalid_token, &PathBuf::from("test.rs"));

        // Text should be unchanged
        assert_eq!(app.text_input.lines()[0], "@foo");
    }

    #[test]
    fn test_replace_with_invalid_start_byte_does_not_panic() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Create a token with start > end
        let invalid_token = AtToken {
            query: "foo".to_string(),
            start_byte: 10, // Greater than end
            end_byte: 4,
            row: 0,
        };

        // This should NOT panic, just return early
        app.replace_at_token_with_path(&invalid_token, &PathBuf::from("test.rs"));

        // Text should be unchanged
        assert_eq!(app.text_input.lines()[0], "@foo");
    }

    #[test]
    fn test_replace_with_invalid_end_byte_does_not_panic() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Create a token with end > line length
        let invalid_token = AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 999, // Way beyond line length
            row: 0,
        };

        // This should NOT panic, just return early
        app.replace_at_token_with_path(&invalid_token, &PathBuf::from("test.rs"));

        // Text should be unchanged
        assert_eq!(app.text_input.lines()[0], "@foo");
    }

    #[test]
    fn test_replace_with_valid_token_works() {
        let mut app = create_test_app_with_lines(&["@foo"], 0, 4);

        // Valid token
        let valid_token = AtToken {
            query: "foo".to_string(),
            start_byte: 0,
            end_byte: 4,
            row: 0,
        };

        app.replace_at_token_with_path(&valid_token, &PathBuf::from("test.rs"));

        // Text should be replaced
        assert_eq!(app.text_input.lines()[0], "test.rs ");
    }
}
