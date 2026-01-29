//! Settings panel tests.
//!
//! Tests for the settings overlay including:
//! - Key navigation (up/down/j/k)
//! - Value toggling (Enter/Space)
//! - Snapshot tests for visual rendering

use super::helpers::*;
use crate::app::state::{AppMode, SettingsItem};
use crate::core::Model;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

mod settings_key_tests {
    use super::*;

    #[test]
    fn ctrl_s_opens_settings_from_text_input() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        assert_eq!(app.mode, AppMode::Chat);

        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        app.handle_key(key);

        assert_eq!(app.mode, AppMode::Settings);
        assert_eq!(app.settings.previous_mode, Some(AppMode::Chat));
    }

    #[test]
    fn ctrl_s_does_not_nest_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);

        // Open settings first
        app.open_settings();
        assert_eq!(app.mode, AppMode::Settings);
        assert_eq!(app.settings.previous_mode, Some(AppMode::Chat));

        // Try to open again - should be a no-op
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        app.handle_key(key);

        // Still in settings, previous mode unchanged
        assert_eq!(app.mode, AppMode::Settings);
        assert_eq!(app.settings.previous_mode, Some(AppMode::Chat));
    }

    #[test]
    fn esc_closes_settings_and_returns_to_text_input() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.mode, AppMode::Settings);

        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.handle_key(key);

        assert_eq!(app.mode, AppMode::Chat);
        assert!(app.settings.previous_mode.is_none());
    }

    #[test]
    fn q_closes_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.mode, AppMode::Settings);

        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        app.handle_key(key);

        assert_eq!(app.mode, AppMode::Chat);
    }

    #[test]
    fn ctrl_c_closes_settings_instead_of_quitting() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        app.handle_key(key);

        // Should close settings, not quit
        assert_eq!(app.mode, AppMode::Chat);
        assert!(!app.should_quit());
    }

    #[test]
    fn down_key_navigates_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        assert_eq!(app.settings.selected_index, 1);
    }

    #[test]
    fn up_key_navigates_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1;

        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(up);

        assert_eq!(app.settings.selected_index, 0);
    }

    #[test]
    fn j_key_navigates_down_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);

        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        app.handle_key(j);

        assert_eq!(app.settings.selected_index, 1);
    }

    #[test]
    fn k_key_navigates_up_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1;

        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        app.handle_key(k);

        assert_eq!(app.settings.selected_index, 0);
    }

    #[test]
    fn ctrl_n_navigates_down_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);

        let ctrl_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        app.handle_key(ctrl_n);

        assert_eq!(app.settings.selected_index, 1);
    }

    #[test]
    fn ctrl_p_navigates_up_in_settings() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1;

        let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        app.handle_key(ctrl_p);

        assert_eq!(app.settings.selected_index, 0);
    }

    #[test]
    fn enter_cycles_planning_model() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 0; // Planning model
        assert_eq!(app.settings.planning_model, Model::Codex);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        assert_eq!(app.settings.planning_model, Model::Claude);
    }

    #[test]
    fn enter_cycles_execution_model() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1; // Execution model
        assert_eq!(app.settings.execution_model, Model::Codex);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        assert_eq!(app.settings.execution_model, Model::Claude);
    }

    #[test]
    fn space_cycles_model() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.settings.planning_model, Model::Codex);

        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        app.handle_key(space);

        assert_eq!(app.settings.planning_model, Model::Claude);
    }

    #[test]
    fn navigation_bounds_check_at_top() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);

        // Try to go above first item
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(up);

        // Should stay at 0
        assert_eq!(app.settings.selected_index, 0);
    }

    #[test]
    fn navigation_bounds_check_at_bottom() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        // Go to last item
        let max_index = SettingsItem::all().len() - 1;
        app.settings.selected_index = max_index;

        // Try to go below last item
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        // Should stay at last
        assert_eq!(app.settings.selected_index, max_index);
    }

    #[test]
    fn settings_selection_resets_on_open() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);

        // First open with selection at 0
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);

        // Move to item 1
        app.settings.selected_index = 1;

        // Close
        app.close_settings();

        // Open again - should reset to 0
        app.open_settings();
        assert_eq!(app.settings.selected_index, 0);
    }

    #[test]
    fn settings_persist_after_close() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        // Change planning model
        app.settings.planning_model = Model::Claude;
        app.settings.execution_model = Model::Gemini;

        // Close
        app.close_settings();

        // Models should persist
        assert_eq!(app.settings.planning_model, Model::Claude);
        assert_eq!(app.settings.execution_model, Model::Gemini);
    }
}

mod settings_integration_tests {
    use super::*;

    #[test]
    fn complete_settings_flow() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);

        // Start in text input
        assert_eq!(app.mode, AppMode::Chat);

        // Open settings with Ctrl+S
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(app.mode, AppMode::Settings);

        // Navigate and change planning model
        assert_eq!(app.settings.planning_model, Model::Codex);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.planning_model, Model::Claude);

        // Navigate to execution model
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.settings.selected_index, 1);

        // Change execution model
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.execution_model, Model::Claude);

        // Close settings
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Chat);

        // Settings should persist
        assert_eq!(app.settings.planning_model, Model::Claude);
        assert_eq!(app.settings.execution_model, Model::Claude);
    }

    #[test]
    fn cycle_all_models_for_planning() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 0; // Planning model

        // Start at Codex
        assert_eq!(app.settings.planning_model, Model::Codex);

        // Cycle to Claude
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.planning_model, Model::Claude);

        // Cycle to Gemini
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.planning_model, Model::Gemini);

        // Cycle back to Codex
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.planning_model, Model::Codex);
    }

    #[test]
    fn cycle_all_models_for_execution() {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1; // Execution model

        // Start at Codex
        assert_eq!(app.settings.execution_model, Model::Codex);

        // Cycle to Claude
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.execution_model, Model::Claude);

        // Cycle to Gemini
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.execution_model, Model::Gemini);

        // Cycle back to Codex
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.settings.execution_model, Model::Codex);
    }
}

mod settings_render_tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn settings_panel_default_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        let terminal = render_app_to_terminal(&mut app, 60, 20)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)────────────────────────────────┐",
                    "│                                                          │",
                    "│   ┌ Settings ────────────────────────────────────────┐   │",
                    "│   │McGravity Settings                                │   │",
                    "│   │Configure AI model preferences.                   │   │",
                    "│   │                                                  │   │",
                    "│   │› Planning Model    [Codex]                       │   │",
                    "│   │  Execution Model   [Codex]                       │   │",
                    "│   │  Enter Key         [Submit]                      │   │",
                    "└───│  Max Iterations    [5]                           │───┘",
                    " · W│                                                  │",
                    "   R│                                                  │",
                    "    │[↑/↓] Navigate  [Enter] Change  [Esc] Close       │",
                    "┌ Ta│                                                  │───┐",
                    "│hel│                                                  │   │",
                    "│   └──────────────────────────────────────────────────┘   │",
                    "│                                                          │",
                    "└ \\+Enter for newline ─────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    #[test]
    fn settings_panel_execution_selected_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.selected_index = 1; // Select execution model

        let terminal = render_app_to_terminal(&mut app, 60, 20)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)────────────────────────────────┐",
                    "│                                                          │",
                    "│   ┌ Settings ────────────────────────────────────────┐   │",
                    "│   │McGravity Settings                                │   │",
                    "│   │Configure AI model preferences.                   │   │",
                    "│   │                                                  │   │",
                    "│   │  Planning Model    [Codex]                       │   │",
                    "│   │› Execution Model   [Codex]                       │   │",
                    "│   │  Enter Key         [Submit]                      │   │",
                    "└───│  Max Iterations    [5]                           │───┘",
                    " · W│                                                  │",
                    "   R│                                                  │",
                    "    │[↑/↓] Navigate  [Enter] Change  [Esc] Close       │",
                    "┌ Ta│                                                  │───┐",
                    "│hel│                                                  │   │",
                    "│   └──────────────────────────────────────────────────┘   │",
                    "│                                                          │",
                    "└ \\+Enter for newline ─────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    #[test]
    fn settings_panel_with_different_models_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.planning_model = Model::Claude;
        app.settings.execution_model = Model::Gemini;

        let terminal = render_app_to_terminal(&mut app, 60, 20)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Claude Code/Gemini]",
                    "┌Output (waiting for input)────────────────────────────────┐",
                    "│                                                          │",
                    "│   ┌ Settings ────────────────────────────────────────┐   │",
                    "│   │McGravity Settings                                │   │",
                    "│   │Configure AI model preferences.                   │   │",
                    "│   │                                                  │   │",
                    "│   │› Planning Model    [Claude Code]                 │   │",
                    "│   │  Execution Model   [Gemini]                      │   │",
                    "│   │  Enter Key         [Submit]                      │   │",
                    "└───│  Max Iterations    [5]                           │───┘",
                    " · W│                                                  │",
                    "   R│                                                  │",
                    "    │[↑/↓] Navigate  [Enter] Change  [Esc] Close       │",
                    "┌ Ta│                                                  │───┐",
                    "│hel│                                                  │   │",
                    "│   └──────────────────────────────────────────────────┘   │",
                    "│                                                          │",
                    "└ \\+Enter for newline ─────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    #[test]
    fn settings_panel_all_gemini_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();
        app.settings.planning_model = Model::Gemini;
        app.settings.execution_model = Model::Gemini;
        app.settings.selected_index = 1;

        let terminal = render_app_to_terminal(&mut app, 60, 20)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Gemini/Gemini]",
                    "┌Output (waiting for input)────────────────────────────────┐",
                    "│                                                          │",
                    "│   ┌ Settings ────────────────────────────────────────┐   │",
                    "│   │McGravity Settings                                │   │",
                    "│   │Configure AI model preferences.                   │   │",
                    "│   │                                                  │   │",
                    "│   │  Planning Model    [Gemini]                      │   │",
                    "│   │› Execution Model   [Gemini]                      │   │",
                    "│   │  Enter Key         [Submit]                      │   │",
                    "└───│  Max Iterations    [5]                           │───┘",
                    " · W│                                                  │",
                    "   R│                                                  │",
                    "    │[↑/↓] Navigate  [Enter] Change  [Esc] Close       │",
                    "┌ Ta│                                                  │───┐",
                    "│hel│                                                  │   │",
                    "│   └──────────────────────────────────────────────────┘   │",
                    "│                                                          │",
                    "└ \\+Enter for newline ─────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    #[test]
    fn settings_panel_narrow_width_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        // Test with narrow terminal width
        let terminal = render_app_to_terminal(&mut app, 55, 15)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " ┌ Settings ────────────────────────────────────────┐",
                    "┌│McGravity Settings                                │─┐",
                    "││Configure AI model preferences.                   │ │",
                    "││                                                  │ │",
                    "││› Planning Model    [Codex]                       │ │",
                    "└│  Execution Model   [Codex]                       │─┘",
                    " │  Enter Key         [Submit]                      │",
                    " │  Max Iterations    [5]                           │",
                    " │                                                  │",
                    "┌│                                                  │─┐",
                    "││[↑/↓] Navigate  [Enter] Change  [Esc] Close       │ │",
                    "││                                                  │ │",
                    "││                                                  │ │",
                    "└└──────────────────────────────────────────────────┘─┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    #[test]
    fn settings_panel_wide_width_snapshot() -> Result<()> {
        let mut app = create_test_app_with_lines(&["hello"], 0, 5);
        app.open_settings();

        // Test with wider terminal
        let terminal = render_app_to_terminal(&mut app, 80, 25)?;
        terminal.backend().assert_buffer_lines(styled_lines_from_buffer(&terminal, &[
            " McGravity [Codex/Codex]",
            "┌Output (waiting for input)────────────────────────────────────────────────────┐",
            "│                                                                              │",
            "│                                                                              │",
            "│                                                                              │",
            "│             ┌ Settings ────────────────────────────────────────┐             │",
            "│             │McGravity Settings                                │             │",
            "│             │Configure AI model preferences.                   │             │",
            "│             │                                                  │             │",
            "│             │› Planning Model    [Codex]                       │             │",
            "│             │  Execution Model   [Codex]                       │             │",
            "│             │  Enter Key         [Submit]                      │             │",
            "│             │  Max Iterations    [5]                           │             │",
            "│             │                                                  │             │",
            "│             │                                                  │             │",
            "└─────────────│[↑/↓] Navigate  [Enter] Change  [Esc] Close       │─────────────┘",
            " · Waiting for│                                                  │",
            "   Ready to pr│                                                  │",
            "              └──────────────────────────────────────────────────┘",
            "┌ Task Text ───────────────────────────────────────────────────────────────────┐",
            "│hello                                                                         │",
            "│                                                                              │",
            "│                                                                              │",
            "└ \\+Enter for newline ─────────────────────────────────────────────────────────┘",
            " [Enter] Submit  [Ctrl+S] Settings",
        ]));
        Ok(())
    }
}
