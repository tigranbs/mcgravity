//! Integration tests for the app module.
//!
//! This module contains end-to-end integration tests and flow-related tests:
//! - Complete flow integration tests (task submission through execution)
//! - Multi-step user workflow tests (typing, pasting, submitting)

use super::helpers::*;
use crate::app::state::{AppMode, AtToken};
use crate::core::prompts::wrap_for_execution;
use crate::file_search::FileMatch;
use crate::fs::TASK_FILE;
use crate::tui::widgets::PopupState;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// =============================================================================
// Complete Flow Integration Tests (Task 027)
// =============================================================================
//
// These tests verify that the complete flow works correctly from task submission
// through execution to session reset, with completed tasks context from task.md
// but WITHOUT the verification step.

/// Test that `wrap_for_execution()` output contains task content and `COMPLETED_TASKS` section.
///
/// This verifies that the execution phase receives completed task summaries as context.
#[test]
fn test_wrap_for_execution_contains_completed_tasks_context() {
    let task = "# Task 005: Implement feature X\n\n## Objective\nAdd feature X";
    let completed_tasks =
        "- task-001.md: Setup database\n- task-002.md: Create models\n- task-003.md: Add API";

    let wrapped = wrap_for_execution(task, completed_tasks);

    // Verify the output contains the COMPLETED_TASKS section
    assert!(
        wrapped.contains("<COMPLETED_TASKS>"),
        "Execution input should contain COMPLETED_TASKS opening tag"
    );
    assert!(
        wrapped.contains("</COMPLETED_TASKS>"),
        "Execution input should contain COMPLETED_TASKS closing tag"
    );

    // Verify the completed tasks content is included
    assert!(
        wrapped.contains("task-001.md"),
        "Execution input should contain completed task references"
    );
    assert!(
        wrapped.contains("task-002.md"),
        "Execution input should contain completed task references"
    );
    assert!(
        wrapped.contains("task-003.md"),
        "Execution input should contain completed task references"
    );

    // Verify the task content is included
    assert!(
        wrapped.contains("# Task 005"),
        "Execution input should contain the task content"
    );
    assert!(
        wrapped.contains("Implement feature X"),
        "Execution input should contain the task title"
    );
}

/// Test that `wrap_for_execution()` does NOT contain verification instructions.
///
/// This is the critical test to ensure Tasks 021-023 changes are maintained:
/// execution phase should not have a step to verify if task is already done.
#[test]
fn test_wrap_for_execution_does_not_contain_verification_step() {
    let task = "# Task 005: Implement feature X";
    let completed_tasks = "- task-001.md: Some completed task";

    let wrapped = wrap_for_execution(task, completed_tasks);

    // Should NOT contain verification step instructions
    assert!(
        !wrapped.contains("Check Completed Tasks"),
        "Execution input should NOT contain 'Check Completed Tasks' verification step"
    );
    assert!(
        !wrapped.contains("skip this task"),
        "Execution input should NOT contain 'skip this task' instruction"
    );
    assert!(
        !wrapped.contains("already been completed"),
        "Execution input should NOT contain 'already been completed' language"
    );
    assert!(
        !wrapped.contains("task has been completed"),
        "Execution input should NOT contain 'task has been completed' language"
    );
    assert!(
        !wrapped.contains("Step 0"),
        "Execution input should NOT have a Step 0 (verification step)"
    );

    // Should have step numbers starting from 1, not 0
    assert!(
        wrapped.contains("Step 1"),
        "Execution input should have Step 1 (not Step 0)"
    );
}

/// Test that Finished mode is reached when flow completes successfully (`NoTodoFiles` phase).
///
/// This simulates the scenario where planning produces no new tasks.
#[tokio::test]
#[serial]
async fn test_finished_mode_on_no_todo_files() -> Result<()> {
    let _guard = CwdGuard::new()?;
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(temp_dir.path())?;

    let mut app = crate::app::App::new(None)?;

    // Set the phase to NoTodoFiles (as if planning completed with no new tasks)
    app.flow.phase = crate::core::FlowPhase::NoTodoFiles;

    // Send Done event
    app.event_tx
        .send(crate::app::state::FlowEvent::Done)
        .await?;

    // Process events
    app.process_events();

    // Verify mode is Finished
    assert_eq!(
        app.mode,
        AppMode::Finished,
        "Mode should be Finished when NoTodoFiles phase completes"
    );
    Ok(())
}

/// Test that Finished mode is reached when flow completes successfully (Completed phase).
///
/// This simulates the scenario where all tasks have been executed.
#[tokio::test]
#[serial]
async fn test_finished_mode_on_completed_phase() -> Result<()> {
    let _guard = CwdGuard::new()?;
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(temp_dir.path())?;

    let mut app = crate::app::App::new(None)?;

    // Set the phase to Completed
    app.flow.phase = crate::core::FlowPhase::Completed;

    // Send Done event
    app.event_tx
        .send(crate::app::state::FlowEvent::Done)
        .await?;

    // Process events
    app.process_events();

    // Verify mode is Finished
    assert_eq!(
        app.mode,
        AppMode::Finished,
        "Mode should be Finished when Completed phase"
    );
    Ok(())
}

/// Test that 'q' key quits from Finished mode.
#[test]
fn test_finished_mode_q_quits() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);
    app.mode = AppMode::Finished;

    // Press 'q' in Finished mode
    let q_key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    app.handle_key(q_key);

    // Verify should_quit is set
    assert!(
        app.should_quit(),
        "Pressing 'q' in Finished mode should set should_quit"
    );
}

/// Test that Enter key in Finished mode triggers `reset_session()`.
///
/// This verifies the complete user flow: after flow completion, pressing
/// Enter should start a new session with clean state.
#[tokio::test]
#[serial]
async fn test_finished_mode_enter_starts_new_session() -> Result<()> {
    let _guard = CwdGuard::new()?;
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Create app with some state
    let mut app = crate::app::App::new(None)?;

    // Set up task.md
    app.text_input
        .set_lines(vec!["Old task content".to_string()]);
    app.save_current_task()?;

    // Set up done folder with files
    let done_dir = std::path::Path::new(crate::fs::DONE_DIR);
    fs::create_dir_all(done_dir)?;
    let done_file = done_dir.join("task-001.md");
    fs::write(&done_file, "Completed task content")?;

    // Set mode to Finished
    app.mode = AppMode::Finished;

    // Verify preconditions
    assert!(
        std::path::Path::new(TASK_FILE).exists(),
        "task.md should exist before Enter"
    );
    assert!(done_file.exists(), "done file should exist before Enter");

    // Press Enter in Finished mode
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key(enter);

    // Verify new session started:
    // 1. Mode should be Chat
    assert_eq!(
        app.mode,
        AppMode::Chat,
        "Mode should be Chat after Enter in Finished mode"
    );

    // 2. task.md should be deleted
    assert!(
        !std::path::Path::new(TASK_FILE).exists(),
        "task.md should be deleted after starting new session"
    );

    // 3. done folder should be empty
    let entries: Vec<_> = fs::read_dir(done_dir)?.collect();
    assert!(
        entries.is_empty(),
        "done folder should be empty after starting new session"
    );

    // 4. text input should be cleared
    assert_eq!(
        app.text_input.lines(),
        vec![""],
        "Text input should be empty for new session"
    );

    // 5. is_running should be false
    assert!(
        !app.is_running(),
        "is_running should be false for new session"
    );
    Ok(())
}

/// Test that `reset_session()` properly clears all session state.
///
/// This is a direct unit test for the `reset_session` method.
#[tokio::test]
#[serial]
async fn test_reset_session_clears_all_state() -> Result<()> {
    let _guard = CwdGuard::new()?;
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(temp_dir.path())?;

    let mut app = crate::app::App::new(None)?;

    // Set up various state
    app.text_input
        .set_lines(vec!["Task text".to_string(), "Line 2".to_string()]);
    app.save_current_task()?;
    app.flow.phase = crate::core::FlowPhase::Completed;
    app.is_running = true;
    app.flow
        .todo_files
        .push(std::path::PathBuf::from("todo/task-001.md"));
    app.flow.cycle_count = 3;

    // Set up done folder
    let done_dir = std::path::Path::new(crate::fs::DONE_DIR);
    fs::create_dir_all(done_dir)?;
    fs::write(done_dir.join("task-001.md"), "Done")?;
    fs::write(done_dir.join("task-002.md"), "Done")?;

    // Call reset_session
    app.reset_session();

    // Verify state is cleared:

    // 1. Text input cleared
    assert_eq!(
        app.text_input.lines(),
        vec![""],
        "Text input should be cleared"
    );

    // 2. task.md deleted
    assert!(
        !std::path::Path::new(TASK_FILE).exists(),
        "task.md should be deleted"
    );

    // 3. done folder cleared
    let entries: Vec<_> = fs::read_dir(done_dir)?.collect();
    assert!(entries.is_empty(), "done folder should be cleared");

    // 4. Mode is Chat
    assert_eq!(app.mode, AppMode::Chat, "Mode should be Chat");

    // 5. is_running is false
    assert!(!app.is_running, "is_running should be false");

    // 6. flow state reset
    assert_eq!(
        app.flow.phase,
        crate::core::FlowPhase::Idle,
        "Flow phase should be Idle"
    );
    assert!(app.flow.todo_files.is_empty(), "todo_files should be empty");
    assert_eq!(app.flow.cycle_count, 0, "cycle_count should be 0");
    Ok(())
}

/// Integration test for the complete flow: submission → execution → finish → new session.
///
/// This tests the complete user journey without actually running the AI executors.
#[tokio::test]
#[serial]
async fn test_complete_flow_without_verification_integration() -> Result<()> {
    let _guard = CwdGuard::new()?;
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Step 1: Create app and set task text
    let mut app = crate::app::App::new(None)?;
    app.text_input
        .set_lines(vec!["Implement feature X".to_string()]);
    app.save_current_task()?;

    // Verify task was saved
    assert!(
        std::path::Path::new(TASK_FILE).exists(),
        "task.md should be created"
    );

    // Step 2: Simulate planning phase completed - create summary context
    // (legacy done files are migrated to task.md COMPLETED_TASKS block on first cycle)
    let done_dir = std::path::Path::new(crate::fs::DONE_DIR);
    fs::create_dir_all(done_dir)?;
    fs::write(
        done_dir.join("task-001.md"),
        "# Task 001: Setup database\n\nCompleted.",
    )?;

    // Step 3: Verify execution prompt would receive completed tasks context
    let task_content = "# Task 002: Implement feature X\n\n## Objective\nImplement the feature";
    let completed_summary = "- task-001.md: Setup database";
    let execution_prompt = wrap_for_execution(task_content, completed_summary);

    // Verify completed tasks context is in the prompt
    assert!(
        execution_prompt.contains("task-001.md"),
        "Execution prompt should contain completed tasks context"
    );

    // Verify NO verification step is in the prompt
    assert!(
        !execution_prompt.contains("Check Completed Tasks"),
        "Execution prompt should NOT have verification step"
    );
    assert!(
        !execution_prompt.contains("skip this task"),
        "Execution prompt should NOT have skip instruction"
    );

    // Step 4: Simulate flow completion
    app.flow.phase = crate::core::FlowPhase::Completed;
    app.event_tx
        .send(crate::app::state::FlowEvent::Done)
        .await?;
    app.process_events();

    // Verify Finished mode
    assert_eq!(
        app.mode,
        AppMode::Finished,
        "Mode should be Finished after flow completion"
    );

    // Step 5: User starts new session
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key(enter);

    // Verify clean slate
    assert_eq!(
        app.mode,
        AppMode::Chat,
        "Mode should be Chat for new session"
    );
    assert!(
        !std::path::Path::new(TASK_FILE).exists(),
        "task.md should be deleted for new session"
    );
    let done_entries: Vec<_> = fs::read_dir(done_dir)?.collect();
    assert!(
        done_entries.is_empty(),
        "done folder should be empty for new session"
    );
    assert_eq!(
        app.text_input.lines(),
        vec![""],
        "Text input should be empty for new session"
    );
    Ok(())
}

/// Test that execution prefix references completed tasks as context (not for verification).
#[test]
fn test_execution_prefix_completed_tasks_is_context_only() {
    use crate::core::prompts::EXECUTION_PREFIX_TEMPLATE as EXECUTION_PREFIX;

    // Should reference COMPLETED_TASKS
    assert!(
        EXECUTION_PREFIX.contains("COMPLETED_TASKS"),
        "EXECUTION_PREFIX should reference COMPLETED_TASKS"
    );

    // Should indicate it's for reference only
    assert!(
        EXECUTION_PREFIX.contains("for reference only")
            || EXECUTION_PREFIX.contains("previously completed"),
        "EXECUTION_PREFIX should indicate completed tasks are for reference"
    );

    // Should NOT have verification instructions
    assert!(
        !EXECUTION_PREFIX.contains("Check Completed Tasks"),
        "EXECUTION_PREFIX should NOT have 'Check Completed Tasks' step"
    );
    assert!(
        !EXECUTION_PREFIX.contains("already been completed"),
        "EXECUTION_PREFIX should NOT have 'already been completed' language"
    );
}

/// Test that execution prefix has correct step ordering (no Step 0).
#[test]
fn test_execution_prefix_step_ordering() {
    use crate::core::prompts::EXECUTION_PREFIX_TEMPLATE as EXECUTION_PREFIX;

    // Should NOT have Step 0
    assert!(
        !EXECUTION_PREFIX.contains("Step 0"),
        "EXECUTION_PREFIX should NOT have Step 0"
    );

    // Should have steps 1-4 in order
    let step1_pos = EXECUTION_PREFIX
        .find("Step 1")
        .expect("Step 1 should exist");
    let step2_pos = EXECUTION_PREFIX
        .find("Step 2")
        .expect("Step 2 should exist");
    let step3_pos = EXECUTION_PREFIX
        .find("Step 3")
        .expect("Step 3 should exist");
    let step4_pos = EXECUTION_PREFIX
        .find("Step 4")
        .expect("Step 4 should exist");

    assert!(step1_pos < step2_pos, "Step 1 should come before Step 2");
    assert!(step2_pos < step3_pos, "Step 2 should come before Step 3");
    assert!(step3_pos < step4_pos, "Step 3 should come before Step 4");
}

// =============================================================================
// Integration Tests - Holistic tests for key binding behavior
// These tests simulate complete user workflows from start to finish
// =============================================================================

/// Integration test: User enters a multi-line task using Shift+Enter for newlines.
///
/// This test simulates the complete user workflow:
/// 1. Type "Line 1"
/// 2. Press Shift+Enter (insert newline)
/// 3. Type "Line 2"
/// 4. Press Alt+Enter (insert newline, alternative)
/// 5. Type "Line 3"
/// 6. Press Enter (submit)
#[tokio::test]
async fn integration_multiline_task_entry() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // User types "Line 1"
    for c in "Line 1".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.lines(), vec!["Line 1"]);

    // User presses Shift+Enter (should insert newline)
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    assert_eq!(app.text_input.lines(), vec!["Line 1", ""]);
    assert!(!app.is_running, "Shift+Enter should NOT submit");

    // User types "Line 2"
    for c in "Line 2".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.lines(), vec!["Line 1", "Line 2"]);

    // User presses Alt+Enter (should insert newline, alternative method)
    app.handle_key(enter_key(KeyModifiers::ALT));
    assert_eq!(app.text_input.lines(), vec!["Line 1", "Line 2", ""]);
    assert!(!app.is_running, "Alt+Enter should NOT submit");

    // User types "Line 3"
    for c in "Line 3".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.lines(), vec!["Line 1", "Line 2", "Line 3"]);

    // Simulate user pausing before submitting (resets rapid input detection).
    app.text_input.reset_rapid_input_state();

    // User presses Enter (should submit)
    app.handle_key(enter_key(KeyModifiers::NONE));
    assert_eq!(
        app.text_input.lines(),
        vec![""],
        "Input should be cleared after submit"
    );
    // Flow should have been started
    assert!(app.is_running, "Enter should start the flow");
}

/// Integration test: User pastes, edits, then submits.
#[tokio::test]
async fn integration_paste_edit_submit() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // User pastes multi-line text
    app.handle_paste("Pasted line 1\nPasted line 2");
    assert_eq!(
        app.text_input.lines(),
        vec!["Pasted line 1", "Pasted line 2"]
    );
    assert!(!app.is_running, "Paste should NOT submit");

    // User adds more text using Shift+Enter (newline)
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    for c in "Added line 3".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(
        app.text_input.lines(),
        vec!["Pasted line 1", "Pasted line 2", "Added line 3"]
    );

    // Simulate user pausing before submitting (resets rapid input detection)
    app.text_input.reset_rapid_input_state();

    // User submits with Enter
    app.handle_key(enter_key(KeyModifiers::NONE));
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(app.is_running, "Enter should start the flow");
}

/// Integration test: `handle_paste` correctly handles multi-line text.
#[test]
fn integration_paste_multiline_text() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // Paste multi-line text via handle_paste (bracketed paste)
    app.handle_paste("Rapid\nText\nEntry");

    // Verify all text was entered, nothing was submitted
    assert_eq!(app.text_input.lines(), vec!["Rapid", "Text", "Entry"]);
    assert!(!app.is_running, "Paste should NOT submit");
}

/// Integration test: @ popup behavior with Enter when no matches.
#[tokio::test]
async fn integration_file_popup_enter_no_matches_submits() {
    let mut app = create_test_app_with_lines(&["@nonexistent"], 0, 12);

    // Setup popup in NoMatches state (user typed @ but no files match)
    app.text_input.file_popup_state = PopupState::NoMatches;
    app.text_input.at_token = Some(AtToken {
        query: "nonexistent".to_string(),
        start_byte: 0,
        end_byte: 12,
        row: 0,
    });

    // Enter should submit since there are no matches to select
    app.handle_key(enter_key(KeyModifiers::NONE));

    // Verify Enter submitted the task (input cleared)
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(
        app.is_running,
        "Enter with popup (no matches) should submit task"
    );
}

/// Integration test: @ popup with matches - Enter selects file.
#[test]
fn integration_file_popup_enter_with_matches() {
    let mut app = create_test_app_with_lines(&["@src"], 0, 4);

    // Setup popup in Showing state with matches
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

    // Enter should select the file
    app.handle_key(enter_key(KeyModifiers::NONE));

    // Verify file was selected (not submitted, not newline inserted)
    assert_eq!(app.text_input.lines(), vec!["src/main.rs "]);
    assert!(!app.is_running, "Enter with popup should NOT submit task");
    assert!(!app.text_input.file_popup_state.is_visible());
}

/// Integration test: Ctrl+Enter submits even when popup is visible.
#[tokio::test]
async fn integration_file_popup_ctrl_enter_submits() {
    let mut app = create_test_app_with_lines(&["task @src"], 0, 9);

    // Setup popup in Showing state with matches
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
        start_byte: 5,
        end_byte: 9,
        row: 0,
    });

    // Ctrl+Enter should bypass popup and submit
    app.handle_key(enter_key(KeyModifiers::CONTROL));

    // Verify task was submitted
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(
        app.is_running,
        "Ctrl+Enter should submit even with popup visible"
    );
}

/// Integration test: Settings mode Enter behavior.
///
/// Enter in settings mode should cycle options, not affect text input.
/// After closing settings, Enter should resume normal submit behavior.
#[tokio::test]
async fn integration_settings_mode_enter() {
    let mut app = create_test_app_with_lines(&["existing text"], 0, 13);

    // Open settings with Ctrl+S
    app.handle_key(KeyEvent {
        code: KeyCode::Char('s'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    assert_eq!(app.mode, AppMode::Settings);

    // Press Enter in settings (should cycle option, not affect text)
    app.handle_key(enter_key(KeyModifiers::NONE));

    // Text should be unchanged
    assert_eq!(app.text_input.lines(), vec!["existing text"]);

    // Close settings with Esc
    app.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    assert_eq!(app.mode, AppMode::Chat);

    // Now Enter should submit the task in chat mode
    app.handle_key(enter_key(KeyModifiers::NONE));
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(
        app.is_running,
        "Enter should submit task after returning from settings"
    );
}

/// Integration test: Complete typing workflow.
#[tokio::test]
async fn integration_complete_typing_workflow() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // Step 1: Type first line
    for c in "Fix the login bug".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.lines(), vec!["Fix the login bug"]);

    // Step 2: Press Shift+Enter to add newline
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    assert_eq!(app.text_input.lines(), vec!["Fix the login bug", ""]);

    // Step 3: Type second line
    for c in "Add error handling".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(
        app.text_input.lines(),
        vec!["Fix the login bug", "Add error handling"]
    );

    // Verify not submitted yet
    assert!(!app.is_running);

    // Simulate user pausing before submitting (resets rapid input detection)
    app.text_input.reset_rapid_input_state();

    // Step 4: Submit with Enter
    app.handle_key(enter_key(KeyModifiers::NONE));
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(app.is_running);
}

/// Integration test: Mixed modifier combinations.
#[tokio::test]
async fn integration_mixed_modifiers() {
    let mut app = create_test_app_with_lines(&["task"], 0, 4);

    // Shift+Enter = newline
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    assert_eq!(app.text_input.lines(), vec!["task", ""]);
    assert!(!app.is_running);

    // Type more
    for c in "more".chars() {
        app.handle_key(char_key(c));
    }

    // Alt+Enter = newline
    app.handle_key(enter_key(KeyModifiers::ALT));
    assert_eq!(app.text_input.lines(), vec!["task", "more", ""]);
    assert!(!app.is_running);

    // Type more
    for c in "stuff".chars() {
        app.handle_key(char_key(c));
    }

    // Another Shift+Enter = newline
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    assert_eq!(app.text_input.lines(), vec!["task", "more", "stuff", ""]);
    assert!(!app.is_running);

    // Type final content
    for c in "final".chars() {
        app.handle_key(char_key(c));
    }

    // Simulate user pausing before submitting (resets rapid input detection)
    app.text_input.reset_rapid_input_state();

    // Plain Enter = submit (traditional chat behavior)
    app.handle_key(enter_key(KeyModifiers::NONE));
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(app.is_running);
}

/// Integration test: Paste with various line endings.
#[test]
fn integration_paste_line_endings() {
    // Test Unix line endings (\n)
    let mut app1 = create_test_app_with_lines(&[""], 0, 0);
    app1.handle_paste("line1\nline2\nline3");
    assert_eq!(app1.text_input.lines(), vec!["line1", "line2", "line3"]);
    assert!(!app1.is_running);

    // Test Windows line endings (\r\n)
    let mut app2 = create_test_app_with_lines(&[""], 0, 0);
    app2.handle_paste("line1\r\nline2\r\nline3");
    assert_eq!(app2.text_input.lines(), vec!["line1", "line2", "line3"]);
    assert!(!app2.is_running);

    // Test old Mac line endings (\r)
    let mut app3 = create_test_app_with_lines(&[""], 0, 0);
    app3.handle_paste("line1\rline2\rline3");
    assert_eq!(app3.text_input.lines(), vec!["line1", "line2", "line3"]);
    assert!(!app3.is_running);

    // Test mixed line endings
    let mut app4 = create_test_app_with_lines(&[""], 0, 0);
    app4.handle_paste("line1\nline2\r\nline3\rline4");
    assert_eq!(
        app4.text_input.lines(),
        vec!["line1", "line2", "line3", "line4"]
    );
    assert!(!app4.is_running);
}

/// Integration test: Empty and whitespace input behavior.
#[test]
fn integration_empty_whitespace_input() {
    // Empty input - Ctrl+Enter should not submit
    let mut app1 = create_test_app_with_lines(&[""], 0, 0);
    app1.handle_key(enter_key(KeyModifiers::CONTROL));
    assert!(!app1.is_running, "Empty input should not submit");

    // Whitespace-only input - Ctrl+Enter should not submit
    let mut app2 = create_test_app_with_lines(&["   "], 0, 3);
    app2.handle_key(enter_key(KeyModifiers::CONTROL));
    assert!(!app2.is_running, "Whitespace-only input should not submit");

    // Multiple empty lines - Ctrl+Enter should not submit
    let mut app3 = create_test_app_with_lines(&["", "", ""], 2, 0);
    app3.handle_key(enter_key(KeyModifiers::CONTROL));
    assert!(!app3.is_running, "Empty lines only should not submit");

    // Mixed whitespace lines - Ctrl+Enter should not submit
    let mut app4 = create_test_app_with_lines(&["  ", "  ", "  "], 2, 2);
    app4.handle_key(enter_key(KeyModifiers::CONTROL));
    assert!(!app4.is_running, "Whitespace-only lines should not submit");
}

/// Integration test: Cursor position after various actions.
#[test]
fn integration_cursor_position_tracking() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // Type "hello"
    for c in "hello".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.cursor().0, 0);
    assert_eq!(app.text_input.cursor().1, 5);

    // Press Shift+Enter (newline)
    app.handle_key(enter_key(KeyModifiers::SHIFT));
    assert_eq!(app.text_input.cursor().0, 1);
    assert_eq!(app.text_input.cursor().1, 0);

    // Type "world"
    for c in "world".chars() {
        app.handle_key(char_key(c));
    }
    assert_eq!(app.text_input.cursor().0, 1);
    assert_eq!(app.text_input.cursor().1, 5);

    // Press Alt+Enter (another way to insert newline)
    app.handle_key(enter_key(KeyModifiers::ALT));
    assert_eq!(app.text_input.cursor().0, 2);
    assert_eq!(app.text_input.cursor().1, 0);

    // Type "!"
    app.handle_key(char_key('!'));
    assert_eq!(app.text_input.cursor().0, 2);
    assert_eq!(app.text_input.cursor().1, 1);

    // Final content
    assert_eq!(app.text_input.lines(), vec!["hello", "world", "!"]);
}

/// Integration test: Simulates user pasting multi-line code with newlines.
#[test]
fn integration_paste_multiline_code() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // Simulate pasting a code snippet
    let code = "fn main() {\n    println!(\"Hello\");\n}";
    app.handle_paste(code);

    // Verify code is inserted correctly
    assert_eq!(app.text_input.lines().len(), 3);
    assert_eq!(app.text_input.lines()[0], "fn main() {");
    assert_eq!(app.text_input.lines()[1], "    println!(\"Hello\");");
    assert_eq!(app.text_input.lines()[2], "}");

    // Cursor should be at the end of the last line
    assert_eq!(app.text_input.cursor().0, 2);
    assert_eq!(app.text_input.cursor().1, 1);

    // Flow should NOT have started
    assert!(!app.is_running);
}

/// Integration test: Paste followed by manual Enter should submit.
#[tokio::test]
async fn integration_paste_then_manual_enter_submits() {
    let mut app = create_test_app_with_lines(&[""], 0, 0);

    // Paste some text
    app.handle_paste("task description");

    // Simulate time passing (reset rapid input state as would happen after timeout)
    app.text_input.reset_rapid_input_state();

    // Now manual Enter should submit
    app.handle_key(enter_key(KeyModifiers::NONE));

    // Should have submitted (input cleared)
    assert_eq!(app.text_input.lines(), vec![""]);
    assert!(app.is_running);
}
