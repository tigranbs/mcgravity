//! Task text persistence tests.
//!
//! Tests for saving and loading task descriptions:
//! - Round-trip serialization (save/load)
//! - Multi-line text handling
//! - File system integration
//! - Session reset and restoration behavior

use super::helpers::*;
use crate::app::App;
use crate::app::state::{AppMode, FlowEvent};
use crate::fs::{MCGRAVITY_DIR, TASK_FILE};
use anyhow::Result;
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

mod task_text_persistence_tests {
    use super::*;

    /// Simulates the save/load round-trip for task text.
    ///
    /// This mirrors the actual implementation:
    /// - Save: `lines.join("\n")`
    /// - Load: `content.split('\n').map(String::from).collect()`
    fn round_trip_lines(lines: &[&str]) -> Vec<String> {
        // Simulate save (collect_input_text)
        let string_lines: Vec<String> =
            lines.iter().map(std::string::ToString::to_string).collect();
        let saved = string_lines.join("\n");

        // Simulate load (split and collect)
        saved.split('\n').map(String::from).collect()
    }

    #[test]
    fn test_single_line_round_trip() {
        let original = vec!["hello world"];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["hello world"]);
    }

    #[test]
    fn test_multiple_lines_round_trip() {
        let original = vec!["line one", "line two", "line three"];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["line one", "line two", "line three"]);
    }

    #[test]
    fn test_empty_line_in_middle() {
        let original = vec!["before", "", "after"];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["before", "", "after"]);
    }

    #[test]
    fn test_trailing_empty_line() {
        // This simulates pressing Enter at the end of text
        let original = vec!["some text", ""];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["some text", ""]);
    }

    #[test]
    fn test_multiple_trailing_empty_lines() {
        let original = vec!["text", "", ""];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["text", "", ""]);
    }

    #[test]
    fn test_leading_empty_lines() {
        let original = vec!["", "", "text"];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["", "", "text"]);
    }

    #[test]
    fn test_only_empty_lines() {
        let original = vec!["", ""];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["", ""]);
    }

    #[test]
    fn test_unicode_content() {
        let original = vec!["Hello 世界", "日本語テスト", "emoji 🎉"];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["Hello 世界", "日本語テスト", "emoji 🎉"]);
    }

    #[test]
    fn test_lines_with_whitespace() {
        let original = vec!["  indented", "\ttabbed", "trailing  "];
        let restored = round_trip_lines(&original);
        assert_eq!(restored, vec!["  indented", "\ttabbed", "trailing  "]);
    }

    #[test]
    fn test_collect_input_text_joins_correctly() {
        let app = create_test_app_with_lines(&["line1", "line2", "line3"], 0, 0);
        let collected = app.text_input.collect_text();
        assert_eq!(collected, "line1\nline2\nline3");
    }

    #[test]
    fn test_collect_input_text_single_line() {
        let app = create_test_app_with_lines(&["single"], 0, 0);
        let collected = app.text_input.collect_text();
        assert_eq!(collected, "single");
    }

    #[test]
    fn test_collect_input_text_empty() {
        let app = create_test_app_with_lines(&[""], 0, 0);
        let collected = app.text_input.collect_text();
        assert_eq!(collected, "");
    }

    #[test]
    fn test_collect_input_text_with_empty_lines() {
        let app = create_test_app_with_lines(&["first", "", "third"], 0, 0);
        let collected = app.text_input.collect_text();
        assert_eq!(collected, "first\n\nthird");
    }

    #[test]
    fn test_empty_file_results_in_default_state() {
        // When content is empty, the app should start with a clean state
        let content = "";
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.split('\n').map(String::from).collect()
        };
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn test_newline_only_file() {
        // A file containing just "\n" should result in two empty lines
        let content = "\n";
        let lines: Vec<String> = content.split('\n').map(String::from).collect();
        assert_eq!(lines, vec!["", ""]);
    }
}

mod task_persistence_integration_tests {
    use super::*;

    #[tokio::test]
    #[serial]
    async fn test_load_from_existing_task_md() -> Result<()> {
        // Create temp directory and guard (acquires mutex)
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create .mcgravity directory and write task.md with known content
        fs::create_dir_all(MCGRAVITY_DIR)?;
        let content = "Line 1\nLine 2\nLine 3";
        fs::write(TASK_FILE, content)?;

        // Initialize App - it should load task.md automatically
        let app = App::new(None)?;

        // Verify the content was loaded
        assert_eq!(app.text_input.lines(), vec!["Line 1", "Line 2", "Line 3"]);
        // Cursor should be at the end
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 6); // "Line 3".len()
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_load_with_trailing_newline() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create .mcgravity directory and write task.md with trailing newline
        fs::create_dir_all(MCGRAVITY_DIR)?;
        let content = "Line 1\nLine 2\n";
        fs::write(TASK_FILE, content)?;

        let app = App::new(None)?;

        // Trailing newline should result in an empty last line
        assert_eq!(app.text_input.lines(), vec!["Line 1", "Line 2", ""]);
        assert_eq!(app.text_input.cursor().0, 2);
        assert_eq!(app.text_input.cursor().1, 0);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_load_empty_file_uses_default() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create .mcgravity directory and write empty task.md
        fs::create_dir_all(MCGRAVITY_DIR)?;
        fs::write(TASK_FILE, "")?;

        let app = App::new(None)?;

        // Empty file should result in default state (single empty line)
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_load_no_task_md_uses_default() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Don't create task.md

        let app = App::new(None)?;

        // No file should result in default state
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.text_input.cursor().0, 0);
        assert_eq!(app.text_input.cursor().1, 0);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_save_current_task_creates_file() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create app with no task.md
        let mut app = App::new(None)?;

        // Set some content
        app.text_input
            .set_lines(vec!["Test line 1".to_string(), "Test line 2".to_string()]);

        // Save
        app.save_current_task()?;

        // Verify file was created with correct content in .mcgravity/task.md
        let saved_content = fs::read_to_string(TASK_FILE)?;
        assert_eq!(saved_content, "Test line 1\nTest line 2");
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_save_with_empty_lines() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = App::new(None)?;

        // Set content with empty lines
        let expected_text = "Line 1\n\nLine 3";
        app.text_input.set_lines(vec![
            "Line 1".to_string(),
            String::new(),
            "Line 3".to_string(),
        ]);

        // Verify state before saving to distinguish between state issue and IO issue
        assert_eq!(
            app.text_input.collect_text(),
            expected_text,
            "TextArea content incorrect before save"
        );

        app.save_current_task()?;

        // Verify file was created with correct content in .mcgravity/task.md
        let saved_content = fs::read_to_string(TASK_FILE)?;
        assert_eq!(saved_content, expected_text);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_save_load_round_trip() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create first app and set content
        let mut app1 = App::new(None)?;
        app1.text_input.set_lines(vec![
            "First line".to_string(),
            "Second line".to_string(),
            "Third line".to_string(),
        ]);
        app1.save_current_task()?;

        // Create second app - it should load the saved content
        let app2 = App::new(None)?;
        assert_eq!(app2.text_input.lines(), app1.text_input.lines());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_save_load_round_trip_with_trailing_newline() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app1 = App::new(None)?;
        // Simulate pressing Enter at the end
        app1.text_input
            .set_lines(vec!["Some text".to_string(), String::new()]);
        app1.save_current_task()?;

        let app2 = App::new(None)?;
        assert_eq!(app2.text_input.lines(), app1.text_input.lines());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_save_load_unicode_content() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app1 = App::new(None)?;
        app1.text_input.set_lines(vec![
            "Hello 世界".to_string(),
            "日本語テスト".to_string(),
            "emoji 🎉🚀".to_string(),
        ]);
        app1.save_current_task()?;

        let app2 = App::new(None)?;
        assert_eq!(app2.text_input.lines(), app1.text_input.lines());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_load_does_not_affect_input_path_mode() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create .mcgravity/task.md
        fs::create_dir_all(MCGRAVITY_DIR)?;
        fs::write(TASK_FILE, "saved content")?;

        // Create a dummy input file
        fs::write("input.txt", "input content")?;

        // Create app with input file - should NOT load task.md
        let input_path = temp_dir.path().join("input.txt");
        let app = App::new(Some(input_path))?;

        // When an input file is provided, task.md should not be loaded
        // (the input comes from the specified file instead)
        assert_ne!(
            app.text_input.lines(),
            vec!["saved content".to_string()],
            "task.md should not be loaded when input file is provided"
        );
        Ok(())
    }

    /// Test that task text is restored from task.md when flow is cancelled (non-Completed phase).
    ///
    /// This tests the behavior in `FlowEvent::Done` handler where task text is restored
    /// for retry when the flow ends in a non-successful state.
    #[tokio::test]
    #[serial]
    async fn test_task_text_restored_on_cancel() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create an app and simulate a task submission
        let mut app = App::new(None)?;

        // Set task text and save it (simulating submission)
        let original_task = "Fix the bug in module X";
        app.text_input.set_lines(vec![original_task.to_string()]);
        app.save_current_task()?;

        // Clear the text input to simulate that the flow is running
        app.text_input.set_lines(vec![String::new()]);
        assert_eq!(app.text_input.lines(), vec![""]);

        // Simulate flow ending with a Failed phase (cancellation/error)
        app.flow.phase = crate::core::FlowPhase::Failed {
            reason: "User cancelled".to_string(),
        };

        // Send the Done event through the channel
        app.event_tx.send(FlowEvent::Done).await?;

        // Process events
        app.process_events();

        // Verify task text was restored
        assert_eq!(
            app.text_input.lines(),
            vec![original_task],
            "Task text should be restored from task.md after cancellation"
        );

        // Verify mode is back to Chat
        assert_eq!(app.mode, AppMode::Chat);
        Ok(())
    }

    /// Test that task.md is cleared when `reset_session()` is called.
    ///
    /// This happens when the user starts a new session after successful flow completion.
    #[tokio::test]
    #[serial]
    async fn test_task_md_cleared_on_reset_session() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create an app with task text
        let mut app = App::new(None)?;
        app.text_input
            .set_lines(vec!["My task description".to_string()]);
        app.save_current_task()?;

        // Verify task.md exists
        assert!(
            std::path::Path::new(TASK_FILE).exists(),
            "task.md should exist before reset"
        );

        // Call reset_session
        app.reset_session();

        // Verify task.md is deleted
        assert!(
            !std::path::Path::new(TASK_FILE).exists(),
            "task.md should be deleted after reset_session()"
        );

        // Verify text input is cleared
        assert_eq!(
            app.text_input.lines(),
            vec![""],
            "Text input should be empty after reset"
        );
        Ok(())
    }

    /// Test that done folder is cleared when `reset_session()` is called.
    ///
    /// This happens when the user starts a new session after successful flow completion.
    /// The done folder should be emptied (recreated empty) so the new session starts fresh.
    #[tokio::test]
    #[serial]
    async fn test_done_folder_cleared_on_reset_session() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create the done directory with some files
        let done_dir = std::path::Path::new(crate::fs::DONE_DIR);
        fs::create_dir_all(done_dir)?;

        // Create test files in done folder
        let done_file1 = done_dir.join("completed_task_1.md");
        let done_file2 = done_dir.join("completed_task_2.md");
        fs::write(&done_file1, "Task 1 content")?;
        fs::write(&done_file2, "Task 2 content")?;

        // Verify files exist before reset
        assert!(done_file1.exists(), "done file 1 should exist before reset");
        assert!(done_file2.exists(), "done file 2 should exist before reset");

        // Create app and call reset_session
        let mut app = App::new(None)?;
        app.reset_session();

        // Verify done folder exists but is empty
        assert!(
            done_dir.exists(),
            "done directory should exist after reset (recreated)"
        );

        let entries: Vec<_> = fs::read_dir(done_dir)?.collect();
        assert!(
            entries.is_empty(),
            "done directory should be empty after reset_session()"
        );
        Ok(())
    }

    /// Test that pressing Enter in Finished mode triggers `reset_session()`.
    ///
    /// When the user presses Enter in Finished mode (after successful flow completion),
    /// the app should reset the session by clearing task.md, the done folder, and
    /// returning to Chat mode.
    #[tokio::test]
    #[serial]
    async fn test_finished_mode_enter_triggers_reset_session() -> Result<()> {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create an app with task text and a file in the done folder
        let mut app = App::new(None)?;
        app.text_input
            .set_lines(vec!["My completed task".to_string()]);
        app.save_current_task()?;

        // Create a file in the done folder
        let done_dir = std::path::Path::new(crate::fs::DONE_DIR);
        fs::create_dir_all(done_dir)?;
        let done_file = done_dir.join("completed_task.md");
        fs::write(&done_file, "Task content")?;

        // Set mode to Finished (as if flow completed successfully)
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

        // Verify reset_session was triggered:
        // 1. Mode should be Chat
        assert_eq!(
            app.mode,
            AppMode::Chat,
            "Mode should be Chat after Enter in Finished mode"
        );

        // 2. task.md should be deleted
        assert!(
            !std::path::Path::new(TASK_FILE).exists(),
            "task.md should be deleted after Enter in Finished mode"
        );

        // 3. done folder should be empty (recreated)
        assert!(done_dir.exists(), "done directory should exist (recreated)");
        let entries: Vec<_> = fs::read_dir(done_dir)?.collect();
        assert!(
            entries.is_empty(),
            "done directory should be empty after Enter in Finished mode"
        );

        // 4. Text input should be cleared
        assert_eq!(
            app.text_input.lines(),
            vec![""],
            "Text input should be empty after reset"
        );
        Ok(())
    }

    /// Test that task.md is NOT cleared when flow is cancelled (via ESC).
    ///
    /// The task.md file should persist so the user can retry their task.
    #[tokio::test]
    #[serial]
    async fn test_task_md_preserved_on_cancel() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create an app with task text
        let mut app = App::new(None)?;
        let task_content = "Important task to complete";
        app.text_input.set_lines(vec![task_content.to_string()]);
        app.save_current_task()?;

        // Simulate flow ending with a Failed phase (cancellation)
        app.flow.phase = crate::core::FlowPhase::Failed {
            reason: "Cancelled by user".to_string(),
        };

        // Send Done event and process
        app.event_tx.send(FlowEvent::Done).await?;
        app.process_events();

        // Verify task.md still exists with original content
        assert!(
            std::path::Path::new(TASK_FILE).exists(),
            "task.md should still exist after cancellation"
        );
        let saved_content = fs::read_to_string(TASK_FILE)?;
        assert_eq!(
            saved_content, task_content,
            "task.md content should be preserved"
        );
        Ok(())
    }

    /// Test that task text is NOT restored when flow completes successfully.
    ///
    /// On successful completion (Completed or `NoTodoFiles` phase), the app transitions
    /// to Finished mode without restoring task text.
    #[tokio::test]
    #[serial]
    async fn test_no_restore_on_successful_completion() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Create an app with task text
        let mut app = App::new(None)?;
        app.text_input.set_lines(vec!["Original task".to_string()]);
        app.save_current_task()?;

        // Clear input to simulate running flow
        app.text_input.set_lines(vec![String::new()]);

        // Test with Completed phase
        app.flow.phase = crate::core::FlowPhase::Completed;
        app.event_tx.send(FlowEvent::Done).await?;
        app.process_events();

        // Verify task text is NOT restored (input remains empty)
        assert_eq!(
            app.text_input.lines(),
            vec![""],
            "Task text should NOT be restored on successful completion"
        );

        // Verify mode is Finished, not Chat
        assert_eq!(
            app.mode,
            AppMode::Finished,
            "Mode should be Finished after successful completion"
        );
        Ok(())
    }

    /// Test that `NoTodoFiles` phase also transitions to Finished without restore.
    #[tokio::test]
    #[serial]
    async fn test_no_restore_on_no_todo_files() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        let mut app = App::new(None)?;
        app.text_input
            .set_lines(vec!["Task with no todos".to_string()]);
        app.save_current_task()?;

        // Clear input
        app.text_input.set_lines(vec![String::new()]);

        // Set NoTodoFiles phase
        app.flow.phase = crate::core::FlowPhase::NoTodoFiles;
        app.event_tx.send(FlowEvent::Done).await?;
        app.process_events();

        // Verify no restore
        assert_eq!(app.text_input.lines(), vec![""]);
        assert_eq!(app.mode, AppMode::Finished);
        Ok(())
    }

    /// Integration test for the full cancel-and-retry flow.
    ///
    /// This tests the complete user flow:
    /// 1. Submit a task
    /// 2. Cancel with ESC (simulated via Failed phase)
    /// 3. Verify task text is restored
    /// 4. Modify the task
    /// 5. Submit again
    #[tokio::test]
    #[serial]
    async fn test_cancel_and_retry_flow() -> Result<()> {
        let _guard = CwdGuard::new()?;
        let temp_dir = TempDir::new()?;
        std::env::set_current_dir(temp_dir.path())?;

        // Step 1: Create app and submit initial task
        let mut app = App::new(None)?;
        let initial_task = "Implement feature X";
        app.text_input.set_lines(vec![initial_task.to_string()]);
        app.save_current_task()?;

        // Simulate task being cleared during flow execution
        app.text_input.set_lines(vec![String::new()]);
        app.is_running = true;

        // Step 2: Simulate cancellation (ESC leads to Failed phase)
        app.flow.phase = crate::core::FlowPhase::Failed {
            reason: "User pressed ESC".to_string(),
        };
        app.event_tx.send(FlowEvent::Done).await?;
        app.process_events();

        // Step 3: Verify task text is restored
        assert_eq!(
            app.text_input.lines(),
            vec![initial_task],
            "Task should be restored after cancel"
        );
        assert_eq!(app.mode, AppMode::Chat);
        assert!(!app.is_running);

        // Step 4: Modify the task
        let modified_task = "Implement feature X with better error handling";
        app.text_input.set_lines(vec![modified_task.to_string()]);
        app.save_current_task()?;

        // Verify modified task is saved
        let saved = fs::read_to_string(TASK_FILE)?;
        assert_eq!(saved, modified_task);

        // Step 5: Simulate second submission and successful completion
        app.text_input.set_lines(vec![String::new()]);
        app.is_running = true;
        app.flow.phase = crate::core::FlowPhase::Completed;
        app.event_tx.send(FlowEvent::Done).await?;
        app.process_events();

        // Verify successful completion
        assert_eq!(app.mode, AppMode::Finished);
        assert!(!app.is_running);
        Ok(())
    }
}
