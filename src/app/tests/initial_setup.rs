//! Initial setup (first-run) integration tests.
//!
//! Tests for the first-run detection, initial setup modal display, and the
//! complete first-run workflow including:
//! - Detection of missing `settings.json` file
//! - Proper initialization of `InitialSetup` mode
//! - Model selection and cycling behavior
//! - Settings persistence after confirmation
//! - Subsequent runs loading saved settings correctly

use super::helpers::*;
use crate::app::state::{AppMode, InitialSetupField};
use crate::core::Model;
use crate::fs::{McgravityPaths, PersistedSettings};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serial_test::serial;
use tempfile::TempDir;

// =============================================================================
// First-Run Detection Tests
// =============================================================================

mod first_run_detection_tests {
    use super::*;

    /// Tests that `is_first_run()` returns true when settings file doesn't exist.
    #[test]
    fn first_run_detection_returns_true_when_no_settings() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // No settings file in fresh temp directory
        assert!(
            paths.is_first_run(),
            "Should be first run when settings file doesn't exist"
        );
        Ok(())
    }

    /// Tests that `is_first_run()` returns false when settings file exists.
    #[test]
    fn first_run_detection_returns_false_when_settings_exist() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // Create settings file
        let settings = PersistedSettings::default();
        paths.save_settings(&settings)?;

        assert!(
            paths.settings_file().exists(),
            "Settings file should exist after save"
        );
        assert!(
            !paths.is_first_run(),
            "Should not be first run when settings file exists"
        );
        Ok(())
    }

    /// Tests that App starts in `InitialSetup` mode on first run.
    #[tokio::test]
    #[serial]
    async fn app_starts_in_initial_setup_mode_on_first_run() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // No settings file - should trigger first run
        let app = crate::app::App::new(None)?;

        assert_eq!(
            app.mode,
            AppMode::InitialSetup,
            "App should start in InitialSetup mode on first run"
        );
        assert!(
            app.initial_setup.is_some(),
            "initial_setup state should be Some on first run"
        );
        Ok(())
    }

    /// Tests that App starts in Chat mode on subsequent runs.
    #[tokio::test]
    #[serial]
    async fn app_starts_in_chat_mode_on_subsequent_run() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create settings file to simulate previous run
        let paths = McgravityPaths::new(temp_dir.path());
        let settings = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Gemini".to_string(),
            enter_behavior: "Submit".to_string(),
            max_iterations: "5".to_string(),
        };
        paths.save_settings(&settings)?;

        // Create app - should start in Chat mode
        let app = crate::app::App::new(None)?;

        assert_eq!(
            app.mode,
            AppMode::Chat,
            "App should start in Chat mode when settings file exists"
        );
        assert!(
            app.initial_setup.is_none(),
            "initial_setup state should be None on subsequent run"
        );
        Ok(())
    }
}

// =============================================================================
// Subsequent Run Settings Loading Tests
// =============================================================================

mod subsequent_run_tests {
    use super::*;
    use crate::app::state::EnterBehavior;

    /// Tests that subsequent runs load and apply saved settings correctly.
    #[tokio::test]
    #[serial]
    async fn subsequent_run_loads_saved_settings() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create settings file with non-default values
        let paths = McgravityPaths::new(temp_dir.path());
        let settings = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Gemini".to_string(),
            enter_behavior: "Newline".to_string(),
            max_iterations: "10".to_string(),
        };
        paths.save_settings(&settings)?;

        // Create app - should load saved settings
        let app = crate::app::App::new(None)?;

        assert_eq!(
            app.settings.planning_model,
            Model::Claude,
            "Planning model should be loaded from settings"
        );
        assert_eq!(
            app.settings.execution_model,
            Model::Gemini,
            "Execution model should be loaded from settings"
        );
        assert_eq!(
            app.settings.enter_behavior,
            EnterBehavior::Newline,
            "Enter behavior should be loaded from settings"
        );
        Ok(())
    }

    /// Tests that invalid settings in file fall back to defaults.
    #[tokio::test]
    #[serial]
    async fn subsequent_run_with_invalid_settings_uses_defaults() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create settings file with invalid values
        let paths = McgravityPaths::new(temp_dir.path());
        let settings = PersistedSettings {
            planning_model: "InvalidModel".to_string(),
            execution_model: String::new(),
            enter_behavior: "unknown".to_string(),
            max_iterations: "999".to_string(),
        };
        paths.save_settings(&settings)?;

        // Create app - should fall back to defaults for invalid values
        let app = crate::app::App::new(None)?;

        // Invalid values should fall back to defaults
        assert_eq!(
            app.settings.planning_model,
            Model::Codex,
            "Invalid planning model should fall back to Codex"
        );
        assert_eq!(
            app.settings.execution_model,
            Model::Codex,
            "Empty execution model should fall back to Codex"
        );
        assert_eq!(
            app.settings.enter_behavior,
            EnterBehavior::Submit,
            "Invalid enter behavior should fall back to Submit"
        );
        Ok(())
    }
}

// =============================================================================
// Initial Setup Navigation Tests
// =============================================================================

mod initial_setup_navigation_tests {
    use super::*;

    /// Tests that Down key navigates from `PlanningModel` to `ExecutionModel`.
    #[tokio::test]
    #[serial]
    async fn down_key_navigates_to_execution_model() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Should start with PlanningModel selected
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::PlanningModel
        );

        // Press Down
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::ExecutionModel
        );
        Ok(())
    }

    /// Tests that Up key navigates from `ExecutionModel` to `PlanningModel`.
    #[tokio::test]
    #[serial]
    async fn up_key_navigates_to_planning_model() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Navigate to ExecutionModel first
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::ExecutionModel
        );

        // Press Up
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(up);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::PlanningModel
        );
        Ok(())
    }

    /// Tests that j key navigates down.
    #[tokio::test]
    #[serial]
    async fn j_key_navigates_down() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        app.handle_key(j);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::ExecutionModel
        );
        Ok(())
    }

    /// Tests that k key navigates up.
    #[tokio::test]
    #[serial]
    async fn k_key_navigates_up() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Navigate to ExecutionModel first
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        // Press k
        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        app.handle_key(k);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::PlanningModel
        );
        Ok(())
    }

    /// Tests that navigation wraps around at boundaries.
    #[tokio::test]
    #[serial]
    async fn navigation_wraps_around() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Press Up from PlanningModel - should wrap to ExecutionModel
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_key(up);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::ExecutionModel
        );

        // Press Down from ExecutionModel - should wrap to PlanningModel
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::PlanningModel
        );
        Ok(())
    }
}

// =============================================================================
// Model Cycling Tests
// =============================================================================

mod model_cycling_tests {
    use super::*;

    /// Tests that Enter key cycles planning model.
    #[tokio::test]
    #[serial]
    async fn enter_cycles_planning_model() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Should be on PlanningModel field with default Codex
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Codex);

        // Press Enter to cycle
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Claude);

        // Cycle again
        app.handle_key(enter);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Gemini);

        // Cycle back to Codex
        app.handle_key(enter);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Codex);
        Ok(())
    }

    /// Tests that Space key cycles planning model.
    #[tokio::test]
    #[serial]
    async fn space_cycles_planning_model() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        app.handle_key(space);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Claude);
        Ok(())
    }

    /// Tests that Enter key cycles execution model when that field is selected.
    #[tokio::test]
    #[serial]
    async fn enter_cycles_execution_model() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Navigate to ExecutionModel
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(
            initial_setup.selected_field,
            InitialSetupField::ExecutionModel
        );
        assert_eq!(initial_setup.execution_model, Model::Codex);

        // Press Enter to cycle execution model
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.execution_model, Model::Claude);

        // Planning model should remain unchanged
        assert_eq!(initial_setup.planning_model, Model::Codex);
        Ok(())
    }

    /// Tests cycling all models for execution field.
    #[tokio::test]
    #[serial]
    async fn cycle_all_execution_models() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Navigate to ExecutionModel
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        // Codex -> Claude
        app.handle_key(enter);
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.execution_model, Model::Claude);

        // Claude -> Gemini
        app.handle_key(enter);
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.execution_model, Model::Gemini);

        // Gemini -> Codex
        app.handle_key(enter);
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.execution_model, Model::Codex);
        Ok(())
    }
}

// =============================================================================
// Confirmation Tests
// =============================================================================

mod confirmation_tests {
    use super::*;

    /// Tests that pressing 'C' confirms initial setup and transitions to Chat mode.
    #[tokio::test]
    #[serial]
    async fn c_key_confirms_and_transitions_to_chat() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Press 'C' to confirm
        let c = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE);
        app.handle_key(c);

        assert_eq!(app.mode, AppMode::Chat, "Should transition to Chat mode");
        assert!(
            app.initial_setup.is_none(),
            "initial_setup should be cleared after confirmation"
        );
        Ok(())
    }

    /// Tests that lowercase 'c' also confirms (without Ctrl).
    #[tokio::test]
    #[serial]
    async fn lowercase_c_key_confirms() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Press lowercase 'c' to confirm
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        app.handle_key(c);

        assert_eq!(app.mode, AppMode::Chat, "Should transition to Chat mode");
        Ok(())
    }

    /// Tests that Ctrl+C quits instead of confirming.
    #[tokio::test]
    #[serial]
    async fn ctrl_c_quits_not_confirms() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Press Ctrl+C
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        app.handle_key(ctrl_c);

        // Should quit, not confirm
        assert!(app.should_quit(), "Ctrl+C should trigger quit");
        assert_eq!(
            app.mode,
            AppMode::InitialSetup,
            "Mode should remain InitialSetup"
        );
        Ok(())
    }

    /// Tests that Esc does not dismiss the modal.
    #[tokio::test]
    #[serial]
    async fn esc_does_not_dismiss_modal() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Press Esc
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        app.handle_key(esc);

        // Should remain in InitialSetup mode
        assert_eq!(
            app.mode,
            AppMode::InitialSetup,
            "Esc should not dismiss initial setup modal"
        );
        assert!(
            app.initial_setup.is_some(),
            "initial_setup should still be present"
        );
        Ok(())
    }

    /// Tests that confirmation saves selected models to settings file.
    #[tokio::test]
    #[serial]
    async fn confirmation_saves_settings_to_file() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        assert_eq!(app.mode, AppMode::InitialSetup);

        // Change planning model to Claude
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.planning_model, Model::Claude);

        // Navigate to execution model and change to Gemini
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);
        app.handle_key(enter); // Claude
        app.handle_key(enter); // Gemini
        let initial_setup = app
            .initial_setup
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("initial_setup should be Some"))?;
        assert_eq!(initial_setup.execution_model, Model::Gemini);

        // Confirm
        let c = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE);
        app.handle_key(c);

        // Verify settings file was created
        let paths = McgravityPaths::new(temp_dir.path());
        assert!(
            paths.settings_file().exists(),
            "Settings file should be created"
        );

        // Verify settings values
        assert_eq!(
            app.settings.planning_model,
            Model::Claude,
            "Planning model should be saved to settings"
        );
        assert_eq!(
            app.settings.execution_model,
            Model::Gemini,
            "Execution model should be saved to settings"
        );
        Ok(())
    }

    /// Tests that subsequent app launch loads the saved settings.
    #[tokio::test]
    #[serial]
    async fn subsequent_launch_loads_confirmed_settings() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // First app instance: configure and confirm
        {
            let mut app = crate::app::App::new(None)?;
            assert_eq!(app.mode, AppMode::InitialSetup);

            // Change to Claude/Gemini
            let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
            app.handle_key(enter); // Planning -> Claude

            let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
            app.handle_key(down);
            app.handle_key(enter); // Execution -> Claude
            app.handle_key(enter); // Execution -> Gemini

            // Confirm
            let c = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE);
            app.handle_key(c);
        }

        // Second app instance: should start in Chat mode with saved settings
        {
            let app = crate::app::App::new(None)?;
            assert_eq!(
                app.mode,
                AppMode::Chat,
                "Second launch should start in Chat mode"
            );
            assert_eq!(
                app.settings.planning_model,
                Model::Claude,
                "Should load saved planning model"
            );
            assert_eq!(
                app.settings.execution_model,
                Model::Gemini,
                "Should load saved execution model"
            );
        }
        Ok(())
    }
}

// =============================================================================
// Rendering Snapshot Tests
// =============================================================================

mod render_tests {
    use super::*;

    /// Tests initial setup modal renders correctly with default selections.
    #[tokio::test]
    #[serial]
    async fn initial_setup_modal_default_snapshot() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };
        assert_eq!(app.mode, AppMode::InitialSetup);

        let terminal = render_app_to_terminal(&mut app, 70, 25)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)──────────────────────────────────────────┐",
                    "│                                                                    │",
                    "│      ┌ Initial Setup ───────────────────────────────────────┐      │",
                    "│      │Welcome to McGravity                                  │      │",
                    "│      │Select your default AI CLI tools.                     │      │",
                    "│      │                                                      │      │",
                    "│      │› Planning Model    [Codex]                           │      │",
                    "│      │                                                      │      │",
                    "│      │  Execution Model   [Codex]                           │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │[↑/↓] Navigate  [Enter] Change  [C] Confirm           │      │",
                    "│      │                                                      │      │",
                    "└──────│                                                      │──────┘",
                    " · Wait│                                                      │",
                    "   Read│                                                      │",
                    "       │                                                      │",
                    "┌ Task │                                                      │──────┐",
                    "│ Type └──────────────────────────────────────────────────────┘      │",
                    "│                                                                    │",
                    "│                                                                    │",
                    "└ \\+Enter for newline ───────────────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    /// Tests initial setup modal with execution model selected.
    #[tokio::test]
    #[serial]
    async fn initial_setup_modal_execution_selected_snapshot() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };

        // Navigate to execution model
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);

        let terminal = render_app_to_terminal(&mut app, 70, 25)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)──────────────────────────────────────────┐",
                    "│                                                                    │",
                    "│      ┌ Initial Setup ───────────────────────────────────────┐      │",
                    "│      │Welcome to McGravity                                  │      │",
                    "│      │Select your default AI CLI tools.                     │      │",
                    "│      │                                                      │      │",
                    "│      │  Planning Model    [Codex]                           │      │",
                    "│      │                                                      │      │",
                    "│      │› Execution Model   [Codex]                           │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │[↑/↓] Navigate  [Enter] Change  [C] Confirm           │      │",
                    "│      │                                                      │      │",
                    "└──────│                                                      │──────┘",
                    " · Wait│                                                      │",
                    "   Read│                                                      │",
                    "       │                                                      │",
                    "┌ Task │                                                      │──────┐",
                    "│ Type └──────────────────────────────────────────────────────┘      │",
                    "│                                                                    │",
                    "│                                                                    │",
                    "└ \\+Enter for newline ───────────────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    /// Tests initial setup modal with different model selections.
    #[tokio::test]
    #[serial]
    async fn initial_setup_modal_changed_models_snapshot() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };

        // Change planning model to Claude
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);

        // Navigate to execution model and change to Gemini
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(down);
        app.handle_key(enter); // Claude
        app.handle_key(enter); // Gemini

        let terminal = render_app_to_terminal(&mut app, 70, 25)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)──────────────────────────────────────────┐",
                    "│                                                                    │",
                    "│      ┌ Initial Setup ───────────────────────────────────────┐      │",
                    "│      │Welcome to McGravity                                  │      │",
                    "│      │Select your default AI CLI tools.                     │      │",
                    "│      │                                                      │      │",
                    "│      │  Planning Model    [Claude Code]                     │      │",
                    "│      │                                                      │      │",
                    "│      │› Execution Model   [Gemini]                          │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │[↑/↓] Navigate  [Enter] Change  [C] Confirm           │      │",
                    "│      │                                                      │      │",
                    "└──────│                                                      │──────┘",
                    " · Wait│                                                      │",
                    "   Read│                                                      │",
                    "       │                                                      │",
                    "┌ Task │                                                      │──────┐",
                    "│ Type └──────────────────────────────────────────────────────┘      │",
                    "│                                                                    │",
                    "│                                                                    │",
                    "└ \\+Enter for newline ───────────────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    /// Tests initial setup modal with unavailable model error message.
    #[tokio::test]
    #[serial]
    async fn initial_setup_modal_unavailable_model_snapshot() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;

        // Override model availability to simulate unavailable models
        // This tests that error messages appear for unavailable CLIs
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: false,
            claude: false,
            gemini: false,
        };

        let terminal = render_app_to_terminal(&mut app, 70, 25)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌Output (waiting for input)──────────────────────────────────────────┐",
                    "│                                                                    │",
                    "│      ┌ Initial Setup ───────────────────────────────────────┐      │",
                    "│      │Welcome to McGravity                                  │      │",
                    "│      │Select your default AI CLI tools.                     │      │",
                    "│      │                                                      │      │",
                    "│      │› Planning Model    [Codex]                           │      │",
                    "│      │    ⚠ `codex` is not available or not executable      │      │",
                    "│      │                                                      │      │",
                    "│      │  Execution Model   [Codex]                           │      │",
                    "│      │    ⚠ `codex` is not available or not executable      │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "│      │                                                      │      │",
                    "└──────│[↑/↓] Navigate  [Enter] Change  [C] Confirm           │──────┘",
                    " · Wait│                                                      │",
                    "   Read│                                                      │",
                    "       │                                                      │",
                    "┌ Task │                                                      │──────┐",
                    "│ Type └──────────────────────────────────────────────────────┘      │",
                    "│                                                                    │",
                    "│                                                                    │",
                    "└ \\+Enter for newline ───────────────────────────────────────────────┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }

    /// Tests initial setup modal at narrow width.
    #[tokio::test]
    #[serial]
    async fn initial_setup_modal_narrow_width_snapshot() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = crate::app::App::new(None)?;
        app.settings.model_availability = crate::core::ModelAvailability {
            codex: true,
            claude: true,
            gemini: true,
        };

        let terminal = render_app_to_terminal(&mut app, 60, 20)?;
        terminal
            .backend()
            .assert_buffer_lines(styled_lines_from_buffer(
                &terminal,
                &[
                    " McGravity [Codex/Codex]",
                    "┌O┌ Initial Setup ───────────────────────────────────────┐─┐",
                    "│ │Welcome to McGravity                                  │ │",
                    "│ │Select your default AI CLI tools.                     │ │",
                    "│ │                                                      │ │",
                    "│ │› Planning Model    [Codex]                           │ │",
                    "│ │                                                      │ │",
                    "│ │  Execution Model   [Codex]                           │ │",
                    "│ │                                                      │ │",
                    "│ │                                                      │ │",
                    "└─│                                                      │─┘",
                    " ·│[↑/↓] Navigate  [Enter] Change  [C] Confirm           │",
                    "  │                                                      │",
                    "  │                                                      │",
                    "┌ │                                                      │─┐",
                    "│ │                                                      │ │",
                    "│ │                                                      │ │",
                    "│ │                                                      │ │",
                    "└ └──────────────────────────────────────────────────────┘─┘",
                    " [Enter] Submit  [Ctrl+S] Settings",
                ],
            ));
        Ok(())
    }
}
