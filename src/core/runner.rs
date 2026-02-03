//! Flow orchestration and execution logic.
//!
//! This module contains the main flow runner with generic retry handling
//! that works with any AI CLI executor implementation.

use std::borrow::Cow;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::fs as async_fs;
use tokio::sync::{mpsc, watch};

use std::path::Path;

use crate::app::FlowEvent;
use crate::core::task_utils::{
    extract_completed_tasks_summary, summarize_completed_tasks, summarize_task_files,
    upsert_completed_task_summary,
};
use crate::core::{
    AiCliExecutor, CliOutput, FlowPhase, RetryConfig, wrap_for_execution, wrap_for_planning,
};
use crate::fs::{McgravityPaths, move_to_done, read_file_content, scan_todo_files};
use crate::tui::widgets::OutputLine;

async fn stop_if_shutdown(
    shutdown_rx: &watch::Receiver<bool>,
    tx: &mpsc::Sender<FlowEvent>,
) -> bool {
    let shutdown = *shutdown_rx.borrow();
    if shutdown {
        tx.send(FlowEvent::Done).await.ok();
    }
    shutdown
}

/// Runs the orchestration flow.
///
/// This is spawned as a separate task and communicates with the UI via events.
///
/// The flow sequence is:
/// 1. Read input
/// 2. Load completed task context from task.md's `<COMPLETED_TASKS>` block
/// 3. Run planning phase (receives completed task summaries in context)
/// 4. Check for new todo files
/// 5. Process todo files with execution model (updates task.md summary, removes completed todos)
/// 6. Repeat from step 2
///
/// # Arguments
///
/// * `input_path` - Path to the input file (None if text was entered directly)
/// * `input_text_direct` - Directly entered text (used when `input_path` is None)
/// * `tx` - Event sender for UI updates
/// * `shutdown_rx` - Shutdown signal receiver
/// * `planning_executor` - Executor to use for planning phase
/// * `execution_executor` - Executor to use for task execution
/// * `max_iterations` - Maximum number of cycles before stopping (None = unlimited)
/// * `paths` - Mcgravity paths configuration
///
/// # Errors
///
/// Returns an error if the input file cannot be read or if CLI execution fails
/// after all retry attempts.
#[allow(clippy::too_many_lines)] // Orchestration keeps phases together for clarity.
#[allow(clippy::too_many_arguments)] // Flow orchestration requires multiple config parameters.
pub async fn run_flow(
    input_path: Option<PathBuf>,
    input_text_direct: String,
    tx: mpsc::Sender<FlowEvent>,
    shutdown_rx: watch::Receiver<bool>,
    planning_executor: &dyn AiCliExecutor,
    execution_executor: &dyn AiCliExecutor,
    max_iterations: Option<u32>,
    paths: McgravityPaths,
) -> Result<()> {
    let retry_config = RetryConfig::default();

    // Phase: Reading input
    let input_text = read_input_phase(input_path, input_text_direct, &tx).await?;
    if stop_if_shutdown(&shutdown_rx, &tx).await {
        return Ok(());
    }

    // Maintain mutable task_text that accumulates completed task summaries
    // This is persisted to task.md and used for both planning and execution context
    let mut task_text = input_text.clone();

    // On first cycle, check for any legacy done files from a previous run
    // and migrate their file references into task_text's COMPLETED_TASKS block (one-time migration)
    let done_files = scan_done_files_phase(&tx, &paths.done_dir()).await?;
    if !done_files.is_empty() {
        // summarize_completed_tasks returns one file reference per line (e.g., "- .mcgravity/todo/done/task-001.md")
        let file_references = summarize_completed_tasks(&done_files);
        for line in file_references.lines() {
            if line.starts_with("- ") {
                task_text = upsert_completed_task_summary(&task_text, line);
            }
        }
        // Persist the migrated task_text
        if let Err(e) = persist_task_text(&task_text, &paths.task_file()).await {
            tx.send(FlowEvent::Output(OutputLine::warning(format!(
                "Failed to persist migrated task.md: {e}"
            ))))
            .await
            .ok();
        } else {
            // Notify UI of task text update so the read-only Task Text panel stays in sync
            tx.send(FlowEvent::TaskTextUpdated(task_text.clone()))
                .await
                .ok();
        }
    }
    if stop_if_shutdown(&shutdown_rx, &tx).await {
        return Ok(());
    }

    let mut cycle_count = 0u32;

    // Main orchestration loop
    loop {
        if stop_if_shutdown(&shutdown_rx, &tx).await {
            return Ok(());
        }
        cycle_count += 1;

        // Check if we've reached max iterations
        if let Some(max) = max_iterations
            && cycle_count > max
        {
            tx.send(FlowEvent::Output(OutputLine::info(format!(
                "Reached maximum iterations ({max}). Stopping flow."
            ))))
            .await
            .ok();
            tx.send(FlowEvent::PhaseChanged(FlowPhase::Completed))
                .await
                .ok();
            tx.send(FlowEvent::Done).await.ok();
            return Ok(());
        }

        // Phase: Pre-planning scan for pending tasks
        // Scan todo files before planning to provide context about existing tasks
        let pending_tasks = scan_todo_files(&paths.todo_dir()).await?;
        if stop_if_shutdown(&shutdown_rx, &tx).await {
            return Ok(());
        }

        // Extract completed tasks summary from task_text for planning context
        let completed_tasks_summary = extract_completed_tasks_summary(&task_text);

        // Phase: Running planning model
        let planning_data = PlanningData {
            input_text: &task_text,
            pending_tasks: &pending_tasks,
            completed_tasks_summary: &completed_tasks_summary,
            cycle_count,
        };
        run_planning_phase(
            &planning_data,
            planning_executor,
            &retry_config,
            &tx,
            &shutdown_rx,
        )
        .await?;
        if stop_if_shutdown(&shutdown_rx, &tx).await {
            return Ok(());
        }

        // Phase: Checking todo files
        let Some(todo_files) = check_todos_phase(&tx, &paths.todo_dir()).await? else {
            return Ok(()); // No todo files found, flow complete
        };
        if stop_if_shutdown(&shutdown_rx, &tx).await {
            return Ok(());
        }

        // Phase: Processing todos
        // This updates task_text with completed task summaries, persists to task.md,
        // and removes completed todo files
        process_todos_phase(
            &todo_files,
            &mut task_text,
            execution_executor,
            &retry_config,
            &tx,
            &shutdown_rx,
            &paths,
        )
        .await?;
        if stop_if_shutdown(&shutdown_rx, &tx).await {
            return Ok(());
        }

        // Phase: Cycle complete
        tx.send(FlowEvent::PhaseChanged(FlowPhase::CycleComplete {
            iteration: cycle_count,
        }))
        .await
        .ok();
        tx.send(FlowEvent::Output(OutputLine::info(format!(
            "Cycle {cycle_count} complete, starting next cycle..."
        ))))
        .await
        .ok();

        // Clear current file
        tx.send(FlowEvent::CurrentFile(None)).await.ok();
    }
}

/// Reads input from a file or uses directly entered text.
///
/// # Arguments
///
/// * `input_path` - Path to the input file (None if text was entered directly)
/// * `input_text_direct` - Directly entered text (used when `input_path` is None)
/// * `tx` - Event sender for UI updates
///
/// # Errors
///
/// Returns an error if the input file cannot be read.
async fn read_input_phase(
    input_path: Option<PathBuf>,
    input_text_direct: String,
    tx: &mpsc::Sender<FlowEvent>,
) -> Result<String> {
    tx.send(FlowEvent::PhaseChanged(FlowPhase::ReadingInput))
        .await
        .ok();

    if let Some(path) = input_path {
        tx.send(FlowEvent::Output(OutputLine::running(
            "Reading input file...",
        )))
        .await
        .ok();
        let text = read_file_content(&path)
            .await
            .context("Failed to read input file")?;
        let file_size = text.len();
        tx.send(FlowEvent::Output(OutputLine::success(format!(
            "Read input file ({file_size} bytes)"
        ))))
        .await
        .ok();
        Ok(text)
    } else {
        let text_size = input_text_direct.len();
        tx.send(FlowEvent::Output(OutputLine::success(format!(
            "Using entered task text ({text_size} bytes)"
        ))))
        .await
        .ok();
        Ok(input_text_direct)
    }
}

/// Scans for legacy done files to migrate into task.md's `<COMPLETED_TASKS>` block.
///
/// This is a one-time migration check on the first cycle. Any files found in the
/// done directory are migrated to the task.md summary format.
///
/// # Arguments
///
/// * `tx` - Event sender for UI updates
///
/// # Returns
///
/// Returns a list of legacy done file paths for migration, or empty vec if none found.
///
/// # Errors
///
/// Returns an error if scanning the done directory fails.
async fn scan_done_files_phase(
    tx: &mpsc::Sender<FlowEvent>,
    done_dir: &Path,
) -> Result<Vec<PathBuf>> {
    tx.send(FlowEvent::PhaseChanged(FlowPhase::CheckingDoneFiles))
        .await
        .ok();

    let done_files = scan_todo_files(done_dir).await?;

    if done_files.is_empty() {
        tx.send(FlowEvent::Output(OutputLine::info(
            "No legacy done files to migrate",
        )))
        .await
        .ok();
    } else {
        let file_count = done_files.len();
        tx.send(FlowEvent::Output(OutputLine::info(format!(
            "Found {file_count} legacy done file(s) to migrate"
        ))))
        .await
        .ok();
    }

    Ok(done_files)
}

/// Data inputs for the planning phase.
///
/// This struct groups the data arguments for `run_planning_phase` to reduce
/// the number of function parameters and improve readability.
struct PlanningData<'a> {
    /// The user's input text describing the task.
    input_text: &'a str,
    /// List of pending task files to include in the planning context.
    pending_tasks: &'a [PathBuf],
    /// Summary of completed tasks extracted from the `task_text` `<COMPLETED_TASKS>` block.
    completed_tasks_summary: &'a str,
    /// Current cycle iteration number.
    cycle_count: u32,
}

/// Runs the planning phase with retry logic.
///
/// # Arguments
///
/// * `data` - Planning data containing input text and task file lists
/// * `planning_executor` - Executor to use for planning
/// * `retry_config` - Configuration for retry behavior
/// * `tx` - Event sender for UI updates
/// * `shutdown_rx` - Shutdown signal receiver
///
/// # Errors
///
/// Returns an error if planning fails after all retry attempts.
async fn run_planning_phase(
    data: &PlanningData<'_>,
    planning_executor: &dyn AiCliExecutor,
    retry_config: &RetryConfig,
    tx: &mpsc::Sender<FlowEvent>,
    shutdown_rx: &watch::Receiver<bool>,
) -> Result<()> {
    let planning_name = planning_executor.name();

    tx.send(FlowEvent::PhaseChanged(FlowPhase::RunningPlanning {
        model_name: Cow::Borrowed(planning_name),
        attempt: 1,
    }))
    .await
    .ok();
    tx.send(FlowEvent::Output(OutputLine::running(format!(
        "Starting {planning_name} CLI (cycle {})...",
        data.cycle_count
    ))))
    .await
    .ok();

    // Clear output for new command
    tx.send(FlowEvent::ClearOutput).await.ok();

    // Generate pending tasks summary
    let pending_tasks_summary = summarize_task_files(data.pending_tasks).await;

    // Run planning with retry (using pre-extracted completed tasks summary)
    let wrapped_input = wrap_for_planning(
        data.input_text,
        &pending_tasks_summary,
        data.completed_tasks_summary,
    );
    let planning_result = run_with_retry(
        &wrapped_input,
        planning_executor,
        |attempt| FlowPhase::RunningPlanning {
            model_name: Cow::Borrowed(planning_name),
            attempt,
        },
        retry_config,
        tx,
        shutdown_rx,
    )
    .await;

    if let Err(e) = planning_result {
        tx.send(FlowEvent::PhaseChanged(FlowPhase::Failed {
            reason: format!("{planning_name} failed after max retries: {e}"),
        }))
        .await
        .ok();
        tx.send(FlowEvent::Output(OutputLine::error(format!(
            "{planning_name} failed: {e}"
        ))))
        .await
        .ok();
        tx.send(FlowEvent::Done).await.ok();
        return Err(e);
    }

    tx.send(FlowEvent::Output(OutputLine::success(format!(
        "{planning_name} completed successfully"
    ))))
    .await
    .ok();

    Ok(())
}

/// Checks for todo files and returns them if found.
///
/// # Arguments
///
/// * `tx` - Event sender for UI updates
///
/// # Returns
///
/// Returns `Some(files)` if todo files exist, `None` if no files found (signals completion).
///
/// # Errors
///
/// Returns an error if scanning todo files fails.
async fn check_todos_phase(
    tx: &mpsc::Sender<FlowEvent>,
    todo_dir: &Path,
) -> Result<Option<Vec<PathBuf>>> {
    tx.send(FlowEvent::PhaseChanged(FlowPhase::CheckingTodoFiles))
        .await
        .ok();
    tx.send(FlowEvent::Output(OutputLine::running(
        "Checking for todo files...",
    )))
    .await
    .ok();

    let todo_files = scan_todo_files(todo_dir).await?;

    if todo_files.is_empty() {
        tx.send(FlowEvent::PhaseChanged(FlowPhase::NoTodoFiles))
            .await
            .ok();
        tx.send(FlowEvent::Output(OutputLine::success(
            "No todo files found - all done!",
        )))
        .await
        .ok();
        tx.send(FlowEvent::Done).await.ok();
        return Ok(None);
    }

    let file_count = todo_files.len();
    tx.send(FlowEvent::TodoFilesUpdated(todo_files.clone()))
        .await
        .ok();
    tx.send(FlowEvent::Output(OutputLine::success(format!(
        "Found {file_count} todo files"
    ))))
    .await
    .ok();

    Ok(Some(todo_files))
}

/// Processes each todo file with the execution executor.
///
/// After each successful execution:
/// 1. Generates a one-line summary from the task file content
/// 2. Upserts the summary into the `<COMPLETED_TASKS>` block of `input_task_text`
/// 3. Persists the updated `input_task_text` to the task file
/// 4. Archives the completed todo file to done folder
///
/// # Arguments
///
/// * `todo_files` - List of todo files to process
/// * `input_task_text` - The canonical task text to update with completed task summaries
/// * `execution_executor` - Executor to use for task execution
/// * `retry_config` - Configuration for retry behavior
/// * `tx` - Event sender for UI updates
/// * `shutdown_rx` - Shutdown signal receiver
/// * `paths` - Mcgravity paths configuration
///
/// # Returns
///
/// The updated task text with all completed task summaries.
///
/// # Errors
///
/// Returns an error if reading a todo file fails. Individual execution failures
/// are logged but do not stop processing of remaining files.
#[allow(clippy::too_many_lines)] // Orchestration keeps todo processing steps together for clarity.
async fn process_todos_phase(
    todo_files: &[PathBuf],
    input_task_text: &mut String,
    execution_executor: &dyn AiCliExecutor,
    retry_config: &RetryConfig,
    tx: &mpsc::Sender<FlowEvent>,
    shutdown_rx: &watch::Receiver<bool>,
    paths: &McgravityPaths,
) -> Result<()> {
    let file_count = todo_files.len();
    tx.send(FlowEvent::PhaseChanged(FlowPhase::ProcessingTodos {
        current: 0,
        total: file_count,
    }))
    .await
    .ok();

    let execution_name = execution_executor.name();

    // Extract completed tasks summary from the task text
    let mut completed_tasks_summary = extract_completed_tasks_summary(input_task_text);

    for (index, file_path) in todo_files.iter().enumerate() {
        if *shutdown_rx.borrow() {
            return Ok(());
        }
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        tx.send(FlowEvent::CurrentFile(Some(file_name.clone())))
            .await
            .ok();
        tx.send(FlowEvent::PhaseChanged(FlowPhase::RunningExecution {
            model_name: Cow::Borrowed(execution_name),
            file_index: index + 1,
            attempt: 1,
        }))
        .await
        .ok();
        tx.send(FlowEvent::Output(OutputLine::running(format!(
            "Processing: {file_name} with {execution_name}"
        ))))
        .await
        .ok();

        // Clear output for new command
        tx.send(FlowEvent::ClearOutput).await.ok();

        // Read file content
        let todo_task_content = read_file_content(file_path).await?;
        let wrapped_task = wrap_for_execution(&todo_task_content, &completed_tasks_summary);

        // Run execution with retry
        let file_index = index + 1;
        let exec_result = run_with_retry(
            &wrapped_task,
            execution_executor,
            |attempt| FlowPhase::RunningExecution {
                model_name: Cow::Borrowed(execution_name),
                file_index,
                attempt,
            },
            retry_config,
            tx,
            shutdown_rx,
        )
        .await;

        if *shutdown_rx.borrow() {
            return Ok(());
        }
        if let Err(e) = exec_result {
            tx.send(FlowEvent::Output(OutputLine::error(format!(
                "Failed on {file_name}: {e}"
            ))))
            .await
            .ok();
            // Continue to next file instead of failing completely
            continue;
        }

        // Success: Archive the completed todo file first, then upsert file reference
        let archived_path =
            match move_to_done(std::slice::from_ref(file_path), &paths.done_dir()).await {
                Ok(archived) => {
                    if let Some(path) = archived.into_iter().next() {
                        let archived_name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown");
                        tx.send(FlowEvent::Output(OutputLine::info(format!(
                            "Archived {file_name} -> {archived_name}"
                        ))))
                        .await
                        .ok();
                        Some(path)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    tx.send(FlowEvent::Output(OutputLine::warning(format!(
                        "Failed to archive todo file {file_name}: {e}"
                    ))))
                    .await
                    .ok();
                    None
                }
            };

        // Build file reference entry using archived path (or fallback to original path)
        let reference_path = archived_path.as_ref().unwrap_or(file_path);
        let reference_line = format!("- {}", reference_path.display());

        // Upsert the file reference into input_task_text
        *input_task_text = upsert_completed_task_summary(input_task_text, &reference_line);

        // Update the local completed_tasks_summary for subsequent tasks
        completed_tasks_summary = extract_completed_tasks_summary(input_task_text);

        // Persist the updated task text to task.md
        if let Err(e) = persist_task_text(input_task_text, &paths.task_file()).await {
            tx.send(FlowEvent::Output(OutputLine::warning(format!(
                "Failed to persist task.md: {e}"
            ))))
            .await
            .ok();
        } else {
            // Notify UI of task text update so the read-only Task Text panel stays in sync
            tx.send(FlowEvent::TaskTextUpdated(input_task_text.clone()))
                .await
                .ok();
        }

        tx.send(FlowEvent::Output(OutputLine::success(format!(
            "Completed: {file_name}"
        ))))
        .await
        .ok();
    }

    Ok(())
}

/// Persists task text to the task file.
///
/// Creates the parent directory if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
async fn persist_task_text(task_text: &str, task_file: &Path) -> Result<()> {
    // Ensure the parent directory exists
    if let Some(parent_dir) = task_file.parent() {
        async_fs::create_dir_all(parent_dir)
            .await
            .context("Failed to create parent directory")?;
    }

    async_fs::write(task_file, task_text)
        .await
        .context("Failed to write task file")?;

    Ok(())
}

/// Legacy cleanup: moves completed todo files to the done folder.
///
/// Note: This function is preserved for backward compatibility in tests only.
/// In the main flow, completed files are now summarized in task.md and removed
/// immediately after successful execution rather than being moved to a done folder.
///
/// # Arguments
///
/// * `todo_files` - List of todo files to move
/// * `done_dir` - Directory to move completed files into
/// * `tx` - Event sender for UI updates
#[cfg(test)]
async fn cleanup_phase(todo_files: &[PathBuf], done_dir: &Path, tx: &mpsc::Sender<FlowEvent>) {
    tx.send(FlowEvent::PhaseChanged(FlowPhase::MovingCompletedFiles))
        .await
        .ok();
    tx.send(FlowEvent::Output(OutputLine::running(
        "Archiving completed files...",
    )))
    .await
    .ok();

    if let Err(e) = crate::fs::move_to_done(todo_files, done_dir).await {
        tx.send(FlowEvent::Output(OutputLine::warning(format!(
            "Failed to archive files: {e}"
        ))))
        .await
        .ok();
    } else {
        tx.send(FlowEvent::Output(OutputLine::success(
            "Archived files to todo/done/",
        )))
        .await
        .ok();
    }
}

/// Generic retry wrapper for any AI CLI executor.
///
/// Executes the given input using the provided executor, with automatic
/// retry on failure. Reports phase changes and logs via the event channel.
async fn run_with_retry<F>(
    input_text: &str,
    executor: &dyn AiCliExecutor,
    phase_builder: F,
    config: &RetryConfig,
    tx: &mpsc::Sender<FlowEvent>,
    shutdown_rx: &watch::Receiver<bool>,
) -> Result<()>
where
    F: Fn(u32) -> FlowPhase,
{
    let executor_name = executor.name();

    for attempt in 1..=config.max_attempts {
        if *shutdown_rx.borrow() {
            anyhow::bail!("Shutdown signaled");
        }
        tx.send(FlowEvent::PhaseChanged(phase_builder(attempt)))
            .await
            .ok();

        // Create output channel for this attempt
        let (output_tx, mut output_rx) = mpsc::channel::<CliOutput>(1000);
        let tx_clone = tx.clone();

        // Spawn a task to forward CLI output to the UI, splitting by newlines
        let forward_handle = tokio::spawn(async move {
            while let Some(output) = output_rx.recv().await {
                let (text, is_stderr) = match output {
                    CliOutput::Stdout(s) => (s, false),
                    CliOutput::Stderr(s) => (s, true),
                };

                // Split by newlines and send each as a separate line
                for line_text in text.lines() {
                    let line = if is_stderr {
                        OutputLine::stderr(line_text)
                    } else {
                        OutputLine::stdout(line_text)
                    };
                    let _ = tx_clone.send(FlowEvent::Output(line)).await;
                }
            }
        });

        match executor
            .execute(input_text, output_tx, shutdown_rx.clone())
            .await
        {
            Ok(status) if status.success() => {
                let _ = forward_handle.await;
                return Ok(());
            }
            Ok(status) => {
                let _ = forward_handle.await;
                if *shutdown_rx.borrow() {
                    anyhow::bail!("Shutdown signaled");
                }
                let code = status.code().unwrap_or(-1);
                if attempt < config.max_attempts {
                    let wait_secs = config.wait_duration(attempt - 1).as_secs();
                    tx.send(FlowEvent::RetryWait(Some(wait_secs))).await.ok();
                    tx.send(FlowEvent::Output(OutputLine::warning(format!(
                        "{executor_name} exited with code {code}, retrying in {wait_secs}s..."
                    ))))
                    .await
                    .ok();
                    tokio::time::sleep(config.wait_duration(attempt - 1)).await;
                    tx.send(FlowEvent::RetryWait(None)).await.ok();
                    tx.send(FlowEvent::ClearOutput).await.ok();
                } else {
                    anyhow::bail!("{executor_name} exited with code {code}");
                }
            }
            Err(e) => {
                let _ = forward_handle.await;
                if *shutdown_rx.borrow() {
                    anyhow::bail!("Shutdown signaled");
                }
                if attempt < config.max_attempts {
                    let wait_secs = config.wait_duration(attempt - 1).as_secs();
                    tx.send(FlowEvent::RetryWait(Some(wait_secs))).await.ok();
                    tx.send(FlowEvent::Output(OutputLine::warning(format!(
                        "{executor_name} error: {e}, retrying in {wait_secs}s..."
                    ))))
                    .await
                    .ok();
                    tokio::time::sleep(config.wait_duration(attempt - 1)).await;
                    tx.send(FlowEvent::RetryWait(None)).await.ok();
                    tx.send(FlowEvent::ClearOutput).await.ok();
                } else {
                    return Err(e);
                }
            }
        }
    }

    anyhow::bail!("Max retries exceeded for {executor_name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CliOutput, FlowPhase, RetryConfig};
    use async_trait::async_trait;
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use tokio::fs;
    use tokio::sync::{mpsc, watch};

    /// Creates `McgravityPaths` from the given temp directory.
    fn test_paths(temp_dir: &TempDir) -> McgravityPaths {
        McgravityPaths::new(temp_dir.path())
    }

    // =========================================================================
    // Mock Executor
    // =========================================================================

    /// A mock executor for testing that records calls and returns predefined results.
    #[derive(Debug)]
    struct MockExecutor {
        /// Name of the mock executor.
        name: &'static str,
        /// Whether the executor should succeed.
        should_succeed: bool,
        /// Number of times execute was called.
        call_count: AtomicU32,
        /// Recorded inputs passed to execute.
        recorded_inputs: Arc<Mutex<Vec<String>>>,
        /// Optional output to send during execution.
        output_text: Option<String>,
    }

    impl MockExecutor {
        /// Creates a new mock executor that succeeds.
        fn new_success(name: &'static str) -> Self {
            Self {
                name,
                should_succeed: true,
                call_count: AtomicU32::new(0),
                recorded_inputs: Arc::new(Mutex::new(Vec::new())),
                output_text: None,
            }
        }

        /// Creates a new mock executor that fails.
        fn new_failure(name: &'static str) -> Self {
            Self {
                name,
                should_succeed: false,
                call_count: AtomicU32::new(0),
                recorded_inputs: Arc::new(Mutex::new(Vec::new())),
                output_text: None,
            }
        }

        /// Creates a mock executor with specific output.
        fn with_output(mut self, output: &str) -> Self {
            self.output_text = Some(output.to_string());
            self
        }

        /// Returns the number of times execute was called.
        fn get_call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }

        /// Returns the recorded inputs.
        fn get_recorded_inputs(&self) -> Vec<String> {
            // Test helper: panicking is acceptable if mutex is poisoned.
            #[allow(clippy::unwrap_used)]
            self.recorded_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AiCliExecutor for MockExecutor {
        async fn execute(
            &self,
            input: &str,
            output_tx: mpsc::Sender<CliOutput>,
            _shutdown_rx: watch::Receiver<bool>,
        ) -> Result<ExitStatus> {
            // Record the call
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // Mock executor: panicking is acceptable if mutex is poisoned.
            #[allow(clippy::unwrap_used)]
            self.recorded_inputs.lock().unwrap().push(input.to_string());

            // Send output if configured
            if let Some(ref text) = self.output_text {
                let _ = output_tx.send(CliOutput::Stdout(text.clone())).await;
            }

            if self.should_succeed {
                // Create a successful exit status (exit code 0)
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    Ok(ExitStatus::from_raw(0))
                }
                #[cfg(not(unix))]
                {
                    // On non-Unix systems, we need a different approach
                    // This is a workaround - in practice, tests run on Unix
                    Ok(std::process::Command::new("true")
                        .status()
                        .unwrap_or_else(|_| panic!("Cannot create exit status")))
                }
            } else {
                // Create a failed exit status (exit code 1)
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    Ok(ExitStatus::from_raw(256)) // Exit code 1 on Unix (256 >> 8 = 1)
                }
                #[cfg(not(unix))]
                {
                    Ok(std::process::Command::new("false")
                        .status()
                        .unwrap_or_else(|_| panic!("Cannot create exit status")))
                }
            }
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn command(&self) -> &'static str {
            "mock"
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    // =========================================================================
    // Shutdown Executor
    // =========================================================================

    #[derive(Debug)]
    struct ShutdownExecutor {
        name: &'static str,
        shutdown_tx: watch::Sender<bool>,
        call_count: AtomicU32,
    }

    impl ShutdownExecutor {
        fn new(name: &'static str, shutdown_tx: watch::Sender<bool>) -> Self {
            Self {
                name,
                shutdown_tx,
                call_count: AtomicU32::new(0),
            }
        }

        fn get_call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AiCliExecutor for ShutdownExecutor {
        async fn execute(
            &self,
            _input: &str,
            _output_tx: mpsc::Sender<CliOutput>,
            _shutdown_rx: watch::Receiver<bool>,
        ) -> Result<ExitStatus> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let _ = self.shutdown_tx.send(true);

            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                Ok(ExitStatus::from_raw(0))
            }
            #[cfg(not(unix))]
            {
                Ok(std::process::Command::new("true")
                    .status()
                    .unwrap_or_else(|_| panic!("Cannot create exit status")))
            }
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn command(&self) -> &'static str {
            "mock"
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    // =========================================================================
    // Helper Functions
    // =========================================================================

    /// Creates a temp directory with done files for testing.
    async fn create_done_files(
        dir: &TempDir,
        files: &[(&str, &str)],
    ) -> anyhow::Result<Vec<PathBuf>> {
        let done_dir = dir.path().join(".mcgravity").join("todo").join("done");
        fs::create_dir_all(&done_dir).await?;

        let mut paths = Vec::new();
        for (name, content) in files {
            let path = done_dir.join(name);
            // Add small delay to ensure different timestamps
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            fs::write(&path, content).await?;
            paths.push(path);
        }
        Ok(paths)
    }

    /// Creates a shutdown receiver that won't trigger.
    fn create_shutdown_rx() -> watch::Receiver<bool> {
        let (_, rx) = watch::channel(false);
        rx
    }

    /// Collects flow events from a receiver until it closes or a timeout.
    async fn collect_events(mut rx: mpsc::Receiver<FlowEvent>, timeout_ms: u64) -> Vec<FlowEvent> {
        let mut events = Vec::new();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(e) => events.push(e),
                        None => break,
                    }
                }
                () = tokio::time::sleep_until(deadline) => {
                    break;
                }
            }
        }
        events
    }

    // =========================================================================
    // scan_done_files_phase Tests
    // =========================================================================

    mod scan_done_files_phase_tests {
        use super::*;

        /// Tests scanning when done directory has files.
        #[tokio::test]
        async fn finds_done_files() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let paths = test_paths(&dir);
            let _files = create_done_files(
                &dir,
                &[("task-001.md", "Task 1"), ("task-002.md", "Task 2")],
            )
            .await?;

            let (tx, rx) = mpsc::channel(100);
            let result = scan_done_files_phase(&tx, &paths.done_dir()).await;

            assert!(result.is_ok());
            let files = result?;
            assert_eq!(files.len(), 2);

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit CheckingDoneFiles phase
            let has_checking_phase = events
                .iter()
                .any(|e| matches!(e, FlowEvent::PhaseChanged(FlowPhase::CheckingDoneFiles)));
            assert!(has_checking_phase);
            Ok(())
        }

        /// Tests scanning when done directory is empty.
        #[tokio::test]
        async fn handles_empty_directory() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let paths = test_paths(&dir);
            // Create the done directory
            fs::create_dir_all(paths.done_dir()).await?;

            let (tx, rx) = mpsc::channel(100);
            let result = scan_done_files_phase(&tx, &paths.done_dir()).await;

            assert!(result.is_ok());
            let files = result?;
            assert!(files.is_empty());

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit info about no legacy done files to migrate
            let has_info_output = events.iter().any(|e| {
                if let FlowEvent::Output(line) = e {
                    line.text.contains("No legacy done files")
                } else {
                    false
                }
            });
            assert!(has_info_output);
            Ok(())
        }

        /// Tests scanning when done directory doesn't exist.
        #[tokio::test]
        async fn handles_nonexistent_directory() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let paths = test_paths(&dir);

            let (tx, _rx) = mpsc::channel(100);
            let result = scan_done_files_phase(&tx, &paths.done_dir()).await;

            assert!(result.is_ok());
            let files = result?;
            assert!(files.is_empty());
            Ok(())
        }
    }

    // =========================================================================
    // run_with_retry Tests
    // =========================================================================

    mod run_with_retry_tests {
        use super::*;
        use std::borrow::Cow;

        /// Tests that successful execution returns Ok.
        #[tokio::test]
        async fn success_returns_ok() {
            let executor = MockExecutor::new_success("MockRunner");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            let result = run_with_retry(
                "test input",
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Mock"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(executor.get_call_count(), 1);
        }

        /// Tests that retry config with 1 attempt doesn't retry on failure.
        #[tokio::test]
        async fn no_retry_with_single_attempt() {
            let executor = MockExecutor::new_failure("MockRunner");
            let retry_config = RetryConfig {
                max_attempts: 1,
                ..Default::default()
            };
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            let result = run_with_retry(
                "test input",
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Mock"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await;

            assert!(result.is_err());
            assert_eq!(executor.get_call_count(), 1);
        }

        /// Tests that the correct input is passed to executor.
        #[tokio::test]
        async fn passes_correct_input() -> anyhow::Result<()> {
            let executor = MockExecutor::new_success("MockRunner");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            let input_text = "This is the test input for the executor";

            run_with_retry(
                input_text,
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Mock"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await?;

            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0], input_text);
            Ok(())
        }

        /// Tests that phase change events are emitted.
        #[tokio::test]
        async fn emits_phase_changes() -> anyhow::Result<()> {
            let executor = MockExecutor::new_success("MockRunner");
            let retry_config = RetryConfig::default();
            let (tx, rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            run_with_retry(
                "test input",
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("TestModel"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await?;

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Check for RunningPlanning phase with attempt 1
            let has_planning_phase = events.iter().any(|e| {
                matches!(
                    e,
                    FlowEvent::PhaseChanged(FlowPhase::RunningPlanning {
                        model_name,
                        attempt: 1
                    }) if model_name == "TestModel"
                )
            });
            assert!(has_planning_phase, "Should emit RunningPlanning phase");
            Ok(())
        }

        /// Tests that output from executor is forwarded.
        #[tokio::test]
        async fn forwards_executor_output() -> anyhow::Result<()> {
            let executor =
                MockExecutor::new_success("MockRunner").with_output("Hello from mock executor");
            let retry_config = RetryConfig::default();
            let (tx, rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            run_with_retry(
                "test input",
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Mock"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await?;

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Check for output containing the mock text
            let has_mock_output = events.iter().any(|e| {
                if let FlowEvent::Output(line) = e {
                    line.text.contains("Hello from mock executor")
                } else {
                    false
                }
            });
            assert!(has_mock_output, "Should forward executor output");
            Ok(())
        }

        /// Tests that shutdown skips execution attempts.
        #[tokio::test]
        async fn shutdown_before_attempt_skips_execution() {
            let executor = MockExecutor::new_success("MockRunner");
            let retry_config = RetryConfig::new(3, 0, 0);
            let (tx, _rx) = mpsc::channel(100);
            let (_shutdown_tx, shutdown_rx) = watch::channel(true);

            let result = run_with_retry(
                "test input",
                &executor,
                |attempt| FlowPhase::RunningPlanning {
                    model_name: Cow::Borrowed("Mock"),
                    attempt,
                },
                &retry_config,
                &tx,
                &shutdown_rx,
            )
            .await;

            assert!(result.is_err());
            assert_eq!(executor.get_call_count(), 0);
        }
    }

    // =========================================================================
    // read_input_phase Tests
    // =========================================================================

    mod read_input_phase_tests {
        use super::*;

        /// Tests reading input from direct text.
        #[tokio::test]
        async fn reads_direct_text() -> anyhow::Result<()> {
            let (tx, _rx) = mpsc::channel(100);
            let direct_text = "Direct input text for testing".to_string();

            let result = read_input_phase(None, direct_text.clone(), &tx).await?;

            assert_eq!(result, direct_text);
            Ok(())
        }

        /// Tests reading input from file.
        #[tokio::test]
        async fn reads_from_file() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("input.txt");
            let content = "File content for testing";
            fs::write(&file_path, content).await?;

            let (tx, _rx) = mpsc::channel(100);

            let result =
                read_input_phase(Some(file_path), "ignored direct text".to_string(), &tx).await?;

            assert_eq!(result, content);
            Ok(())
        }

        /// Tests that reading non-existent file returns error.
        #[tokio::test]
        async fn nonexistent_file_returns_error() {
            let (tx, _rx) = mpsc::channel(100);
            let fake_path = PathBuf::from("/nonexistent/path/to/input.txt");

            let result = read_input_phase(Some(fake_path), "direct text".to_string(), &tx).await;

            assert!(result.is_err());
        }

        /// Tests that `ReadingInput` phase is emitted.
        #[tokio::test]
        async fn emits_reading_input_phase() -> anyhow::Result<()> {
            let (tx, rx) = mpsc::channel(100);

            read_input_phase(None, "test".to_string(), &tx).await?;

            drop(tx);
            let events = collect_events(rx, 100).await;

            let has_reading_phase = events
                .iter()
                .any(|e| matches!(e, FlowEvent::PhaseChanged(FlowPhase::ReadingInput)));
            assert!(has_reading_phase, "Should emit ReadingInput phase");
            Ok(())
        }
    }

    // =========================================================================
    // check_todos_phase Tests
    // =========================================================================

    mod check_todos_phase_tests {
        use super::*;

        /// Tests that empty todo directory returns None.
        #[tokio::test]
        async fn empty_todo_returns_none() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join(".mcgravity").join("todo");
            fs::create_dir_all(&todo_dir).await?;

            let (tx, rx) = mpsc::channel(100);
            let result = check_todos_phase(&tx, &todo_dir).await?;

            assert!(result.is_none());

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit NoTodoFiles phase
            let has_no_todo_phase = events
                .iter()
                .any(|e| matches!(e, FlowEvent::PhaseChanged(FlowPhase::NoTodoFiles)));
            assert!(has_no_todo_phase);

            // Should emit Done event
            let has_done = events.iter().any(|e| matches!(e, FlowEvent::Done));
            assert!(has_done);
            Ok(())
        }

        /// Tests that todo files are returned when present.
        #[tokio::test]
        async fn returns_todo_files() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join(".mcgravity").join("todo");
            fs::create_dir_all(&todo_dir).await?;
            fs::write(todo_dir.join("task-001.md"), "Task 1").await?;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            fs::write(todo_dir.join("task-002.md"), "Task 2").await?;

            let (tx, rx) = mpsc::channel(100);
            let result = check_todos_phase(&tx, &todo_dir).await?;

            assert!(result.is_some());
            assert_eq!(result.map(|f| f.len()), Some(2));

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit CheckingTodoFiles phase
            let has_checking_phase = events
                .iter()
                .any(|e| matches!(e, FlowEvent::PhaseChanged(FlowPhase::CheckingTodoFiles)));
            assert!(has_checking_phase);

            // Should emit TodoFilesUpdated event
            let has_todo_update = events
                .iter()
                .any(|e| matches!(e, FlowEvent::TodoFilesUpdated(_)));
            assert!(has_todo_update);
            Ok(())
        }
    }

    // =========================================================================
    // process_todos_phase Tests
    // =========================================================================

    // Test modules below use unwrap/expect for test setup and assertions.
    // This is acceptable in test code where panicking on failure is the desired behavior.
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod process_todos_phase_tests {
        use super::*;

        /// Tests processing multiple todo files.
        #[tokio::test]
        async fn processes_multiple_files() -> anyhow::Result<()> {
            let dir = TempDir::new()?;
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await?;

            let task1_path = todo_dir.join("task-001.md");
            let task2_path = todo_dir.join("task-002.md");
            fs::write(&task1_path, "Task 1 content").await?;
            fs::write(&task2_path, "Task 2 content").await?;

            let todo_files = vec![task1_path, task2_path];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            let result = process_todos_phase(
                &todo_files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(executor.get_call_count(), 2);

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit ProcessingTodos phase
            let has_processing_phase = events.iter().any(|e| {
                matches!(
                    e,
                    FlowEvent::PhaseChanged(FlowPhase::ProcessingTodos { .. })
                )
            });
            assert!(has_processing_phase);
            Ok(())
        }

        /// Tests that execution wraps content with execution prompts.
        #[tokio::test]
        async fn wraps_content_for_execution() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let file = todo_dir.join("task-001.md");
            fs::write(&file, "Original task content").await.unwrap();

            let files = vec![file];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 1);
            assert!(
                inputs[0].contains("Original task content"),
                "Should contain original content"
            );
            // Check for execution prefix content
            assert!(
                inputs[0].contains("# Role"),
                "Should contain execution prefix"
            );
            // Check for completed tasks section
            assert!(
                inputs[0].contains("<COMPLETED_TASKS>"),
                "Should contain COMPLETED_TASKS section"
            );
        }

        /// Tests that execution includes completed tasks from `task_text`.
        #[tokio::test]
        async fn includes_completed_tasks_in_execution_context() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file
            let file = todo_dir.join("task-002.md");
            fs::write(&file, "New task content").await.unwrap();

            let files = vec![file];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            // Create task_text with existing completed tasks (file references only)
            let mut task_text = r"Initial task description

<COMPLETED_TASKS>
- .mcgravity/todo/done/task-001.md
</COMPLETED_TASKS>
"
            .to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 1);
            // Check that completed task file reference is included
            assert!(
                inputs[0].contains(".mcgravity/todo/done/task-001.md"),
                "Should contain completed task file reference in context"
            );
            // File references should NOT contain task titles or objectives
            assert!(
                !inputs[0].contains("Task 001: Setup Database"),
                "Should not contain task title - only file reference"
            );
        }

        /// Tests that processing continues after individual file failures.
        #[tokio::test]
        async fn continues_after_failure() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let task1_path = todo_dir.join("task-001.md");
            let task2_path = todo_dir.join("task-002.md");
            fs::write(&task1_path, "Task 1").await.unwrap();
            fs::write(&task2_path, "Task 2").await.unwrap();

            let todo_files = vec![task1_path, task2_path];

            let executor = MockExecutor::new_failure("MockExecutor");
            let retry_config = RetryConfig {
                max_attempts: 1,
                ..Default::default()
            };
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            // Should complete without error despite executor failures
            let result = process_todos_phase(
                &todo_files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await;

            assert!(result.is_ok());
            // Both files should be attempted
            assert_eq!(executor.get_call_count(), 2);
        }

        /// Tests processing with empty file list.
        #[tokio::test]
        async fn handles_empty_file_list() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            let result = process_todos_phase(
                &[],
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(executor.get_call_count(), 0);
        }

        /// Tests that processing stops when shutdown is signaled.
        #[tokio::test]
        async fn stops_on_shutdown_signal() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let task1_path = todo_dir.join("task-001.md");
            let task2_path = todo_dir.join("task-002.md");
            fs::write(&task1_path, "Task 1").await.unwrap();
            fs::write(&task2_path, "Task 2").await.unwrap();

            let todo_files = vec![task1_path, task2_path];

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let executor = ShutdownExecutor::new("MockExecutor", shutdown_tx);
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &todo_files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            assert_eq!(executor.get_call_count(), 1);
        }

        /// Tests that execution includes completed tasks from `task_text` in the execution prompt.
        /// This is an integration test verifying that `task_text` `COMPLETED_TASKS` block -> execution prompt.
        #[tokio::test]
        async fn execution_includes_completed_tasks_context() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file to execute
            let todo_file = todo_dir.join("task-003.md");
            fs::write(
                &todo_file,
                "# Task 003: Implement Auth\n\nImplement authentication.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            // Create task_text with pre-existing completed task file references
            let mut task_text = r"Initial task description

<COMPLETED_TASKS>
- .mcgravity/todo/done/task-001.md
- .mcgravity/todo/done/task-002.md
</COMPLETED_TASKS>
"
            .to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            // Verify executor received input with completed task file references
            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 1);

            let input = &inputs[0];

            // Verify COMPLETED_TASKS section exists
            assert!(
                input.contains("<COMPLETED_TASKS>"),
                "Input should contain COMPLETED_TASKS opening tag"
            );
            assert!(
                input.contains("</COMPLETED_TASKS>"),
                "Input should contain COMPLETED_TASKS closing tag"
            );

            // Verify completed task file references are included
            assert!(
                input.contains(".mcgravity/todo/done/task-001.md"),
                "Input should reference completed task task-001.md"
            );
            assert!(
                input.contains(".mcgravity/todo/done/task-002.md"),
                "Input should reference completed task task-002.md"
            );
            // File references should NOT contain task content (titles, objectives)
            assert!(
                !input.contains("Setup Database"),
                "File references should not contain task content"
            );
            assert!(
                !input.contains("Create Models"),
                "File references should not contain task content"
            );

            // Verify the task content is also present
            assert!(
                input.contains("# Task 003: Implement Auth"),
                "Input should contain the task being executed"
            );

            // Verify structure: completed tasks section comes before task content
            let completed_tasks_end = input
                .find("</COMPLETED_TASKS>")
                .expect("Should find closing tag");
            let task_pos = input
                .find("# Task 003: Implement Auth")
                .expect("Should find task content");
            assert!(
                completed_tasks_end < task_pos,
                "COMPLETED_TASKS section should appear before task content"
            );
        }

        /// Tests that execution with no completed tasks has empty `COMPLETED_TASKS` section.
        #[tokio::test]
        async fn execution_with_no_completed_tasks_has_empty_completed_tasks() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file to execute
            let todo_file = todo_dir.join("task-001.md");
            fs::write(&todo_file, "# Task 001: First Task\n\nDo something.")
                .await
                .unwrap();

            let files = vec![todo_file];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            // Task text with no COMPLETED_TASKS block
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 1);

            let input = &inputs[0];

            // Verify COMPLETED_TASKS section exists but is empty
            assert!(
                input.contains("<COMPLETED_TASKS>\n\n</COMPLETED_TASKS>"),
                "Input should contain empty COMPLETED_TASKS section"
            );

            // Verify task content is still present
            assert!(
                input.contains("# Task 001: First Task"),
                "Input should contain the task being executed"
            );
        }

        /// Tests that task.md is updated with completed task summary after successful execution.
        #[tokio::test]
        async fn updates_task_md_after_completion() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file
            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Setup Database\n\n## Objective\nConfigure PostgreSQL.",
            )
            .await
            .unwrap();

            let files = vec![todo_file.clone()];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            // Verify task_text was updated with the completed task file reference
            assert!(
                task_text.contains("<COMPLETED_TASKS>"),
                "task_text should contain COMPLETED_TASKS block"
            );
            assert!(
                task_text.contains(".mcgravity/todo/done/task-001.md"),
                "task_text should contain the archived file path reference"
            );
            // File references should NOT contain task content
            assert!(
                !task_text.contains("Task 001: Setup Database"),
                "task_text should not contain task title - only file reference"
            );

            // Verify task.md was persisted
            let task_md_path = paths.task_file();
            assert!(
                fs::try_exists(&task_md_path).await.unwrap(),
                "task.md should be created"
            );
            let saved_content = fs::read_to_string(&task_md_path).await.unwrap();
            assert!(
                saved_content.contains(".mcgravity/todo/done/task-001.md"),
                "Persisted task.md should contain the archived file reference"
            );
        }

        /// Tests that completed todo files are archived to done folder after successful execution.
        #[tokio::test]
        async fn archives_todo_file_after_completion() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            let done_dir = paths.done_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file
            let todo_file = todo_dir.join("task-001.md");
            fs::write(&todo_file, "# Task 001: Setup\n\nSetup something.")
                .await
                .unwrap();

            let files = vec![todo_file.clone()];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            // Verify the todo file is absent from the todo folder
            assert!(
                !fs::try_exists(&todo_file).await.unwrap(),
                "Todo file should be absent from todo folder after successful execution"
            );

            // Verify the todo file is present in the done folder
            let archived_file = done_dir.join("task-001.md");
            assert!(
                fs::try_exists(&archived_file).await.unwrap(),
                "Todo file should be archived to done folder"
            );

            // Verify content is preserved
            let content = fs::read_to_string(&archived_file).await.unwrap();
            assert!(
                content.contains("# Task 001: Setup"),
                "Archived file should preserve original content"
            );
        }

        /// Tests that subsequent tasks see updated completed tasks summary.
        #[tokio::test]
        async fn subsequent_tasks_see_updated_summary() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create two todo files
            let todo_file1 = todo_dir.join("task-001.md");
            let todo_file2 = todo_dir.join("task-002.md");
            fs::write(
                &todo_file1,
                "# Task 001: First\n\n## Objective\nDo first thing.",
            )
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            fs::write(
                &todo_file2,
                "# Task 002: Second\n\n## Objective\nDo second thing.",
            )
            .await
            .unwrap();

            let files = vec![todo_file1.clone(), todo_file2.clone()];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            // Verify executor was called twice
            assert_eq!(executor.get_call_count(), 2);

            let inputs = executor.get_recorded_inputs();
            assert_eq!(inputs.len(), 2);

            // First task should have empty completed tasks
            assert!(
                inputs[0].contains("<COMPLETED_TASKS>\n\n</COMPLETED_TASKS>"),
                "First task should have empty COMPLETED_TASKS"
            );

            // Second task should see the first task file reference as completed
            assert!(
                inputs[1].contains(".mcgravity/todo/done/task-001.md"),
                "Second task should see archived task-001.md path in COMPLETED_TASKS"
            );
            // File references should NOT contain task content
            assert!(
                !inputs[1].contains("Task 001: First"),
                "Second task should not see task title - only file reference"
            );

            // Both files should be absent from todo folder and present in done folder
            let done_dir = todo_dir.join("done");
            assert!(
                !fs::try_exists(&todo_file1).await.unwrap(),
                "First todo file should be absent from todo folder"
            );
            assert!(
                !fs::try_exists(&todo_file2).await.unwrap(),
                "Second todo file should be absent from todo folder"
            );
            assert!(
                fs::try_exists(done_dir.join("task-001.md")).await.unwrap(),
                "First todo file should be archived to done folder"
            );
            assert!(
                fs::try_exists(done_dir.join("task-002.md")).await.unwrap(),
                "Second todo file should be archived to done folder"
            );

            // Final task_text should contain both completed task file references
            assert!(
                task_text.contains(".mcgravity/todo/done/task-001.md"),
                "Final task_text should contain archived task-001.md path"
            );
            assert!(
                task_text.contains(".mcgravity/todo/done/task-002.md"),
                "Final task_text should contain archived task-002.md path"
            );
        }

        /// Tests that failed tasks are not added to completed tasks summary.
        #[tokio::test]
        async fn failed_tasks_not_added_to_summary() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a todo file
            let todo_file = todo_dir.join("task-001.md");
            fs::write(&todo_file, "# Task 001: Failed Task\n\nThis will fail.")
                .await
                .unwrap();

            let files = vec![todo_file.clone()];

            let executor = MockExecutor::new_failure("MockExecutor");
            let retry_config = RetryConfig {
                max_attempts: 1,
                ..Default::default()
            };
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            process_todos_phase(
                &files,
                &mut task_text,
                &executor,
                &retry_config,
                &tx,
                &shutdown_rx,
                &paths,
            )
            .await
            .unwrap();

            // Verify task_text was NOT updated (no COMPLETED_TASKS block should be added)
            assert!(
                !task_text.contains("<COMPLETED_TASKS>"),
                "task_text should not contain COMPLETED_TASKS block for failed task"
            );

            // Verify the todo file was NOT removed (failed tasks stay)
            assert!(
                fs::try_exists(&todo_file).await.unwrap(),
                "Todo file should remain after failed execution"
            );
        }
    }

    // =========================================================================
    // cleanup_phase Tests
    // =========================================================================

    // Test module uses unwrap/expect for test setup and assertions.
    // This is acceptable in test code where panicking on failure is the desired behavior.
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod cleanup_phase_tests {
        use super::*;

        /// Tests that cleanup phase emits correct events.
        #[tokio::test]
        async fn emits_cleanup_events() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            let done_dir = paths.done_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let file = todo_dir.join("task-001.md");
            fs::write(&file, "Task 1").await.unwrap();

            let files = vec![file];

            let (tx, rx) = mpsc::channel(100);
            cleanup_phase(&files, &done_dir, &tx).await;

            drop(tx);
            let events = collect_events(rx, 100).await;

            // Should emit MovingCompletedFiles phase
            let has_moving_phase = events
                .iter()
                .any(|e| matches!(e, FlowEvent::PhaseChanged(FlowPhase::MovingCompletedFiles)));
            assert!(has_moving_phase);

            // File should be moved to done
            assert!(fs::try_exists(done_dir.join("task-001.md")).await.unwrap());
        }
    }
}
