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
    extract_completed_tasks_summary, extract_task_summary_with_max_len, normalize_summary_entry,
    normalize_task_text_completed_section, summarize_task_files, truncate_summary,
    upsert_completed_task_summary,
};
use crate::core::{
    AiCliExecutor, CliOutput, FlowPhase, RetryConfig, wrap_for_execution, wrap_for_planning,
    wrap_for_task_summary,
};
use crate::fs::{McgravityPaths, move_to_done, read_file_content, scan_todo_files};
use crate::tui::widgets::OutputLine;

/// Maximum length for a completed-task summary entry stored in `<COMPLETED_TASKS>`.
const MAX_SUMMARY_ENTRY_LENGTH: usize = 500;

/// Maximum bytes of captured CLI output retained for summary prompt payloads.
/// Output beyond this limit is truncated (live UI forwarding is unaffected).
const MAX_CAPTURED_OUTPUT_BYTES: usize = 100_000;

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
    // and migrate their content summaries into task_text's COMPLETED_TASKS block (one-time migration)
    let done_files = scan_done_files_phase(&tx, &paths.done_dir()).await?;
    if !done_files.is_empty() {
        for done_file in &done_files {
            let summary_line = if let Ok(content) = read_file_content(done_file).await {
                // Use the full entry budget so legacy summaries are not
                // prematurely truncated to 100 chars.
                let summary = extract_task_summary_with_max_len(&content, MAX_SUMMARY_ENTRY_LENGTH);
                let entry = format!("- {summary}");
                truncate_summary(&entry, MAX_SUMMARY_ENTRY_LENGTH)
            } else {
                let file_name = done_file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                format!("- {file_name}: (content unavailable)")
            };
            task_text = upsert_completed_task_summary(&task_text, &summary_line);
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

    // Normalize any legacy path-based entries in the COMPLETED_TASKS block
    // before planning begins, so the planner never sees absolute paths.
    let normalized = normalize_task_text_completed_section(&task_text);
    if normalized != task_text {
        task_text = normalized;
        if let Err(e) = persist_task_text(&task_text, &paths.task_file()).await {
            tx.send(FlowEvent::Output(OutputLine::warning(format!(
                "Failed to persist normalized task.md: {e}"
            ))))
            .await
            .ok();
        } else {
            tx.send(FlowEvent::TaskTextUpdated(task_text.clone()))
                .await
                .ok();
        }
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
        let captured_output = match exec_result {
            Ok(output) => output,
            Err(e) => {
                tx.send(FlowEvent::Output(OutputLine::error(format!(
                    "Failed on {file_name}: {e}"
                ))))
                .await
                .ok();
                // Continue to next file instead of failing completely
                continue;
            }
        };

        // Success: Generate a summary of the completed task
        let summary_entry = generate_task_summary(
            &todo_task_content,
            &captured_output,
            execution_executor,
            tx,
            shutdown_rx,
        )
        .await;

        // Archive the completed todo file
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
                }
            }
            Err(e) => {
                tx.send(FlowEvent::Output(OutputLine::warning(format!(
                    "Failed to archive todo file {file_name}: {e}"
                ))))
                .await
                .ok();
            }
        }

        // Upsert the summary entry (not file path) into input_task_text
        *input_task_text = upsert_completed_task_summary(input_task_text, &summary_entry);

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

/// Generates a short summary for a completed task by running the execution executor
/// with the task-summary prompt.
///
/// The summary is capped at `MAX_SUMMARY_ENTRY_LENGTH` characters and formatted as
/// a list item (prefixed with `"- "`).
///
/// Output is consumed concurrently via a spawned receiver task to prevent
/// backpressure deadlocks when executor output exceeds the channel buffer.
///
/// Falls back to a local content-based summary if the executor call fails.
async fn generate_task_summary(
    task_content: &str,
    execution_output: &str,
    executor: &dyn AiCliExecutor,
    tx: &mpsc::Sender<FlowEvent>,
    shutdown_rx: &watch::Receiver<bool>,
) -> String {
    tx.send(FlowEvent::Output(OutputLine::running(
        "Generating task summary...",
    )))
    .await
    .ok();

    let summary_prompt = wrap_for_task_summary(task_content, execution_output);

    // Create a channel to capture the summary output
    let (output_tx, mut output_rx) = mpsc::channel::<CliOutput>(100);

    // Spawn a receiver task to consume output concurrently, preventing
    // backpressure deadlocks when output exceeds channel capacity.
    // Capture is bounded at `MAX_CAPTURED_OUTPUT_BYTES` to avoid unbounded
    // in-memory accumulation for summary payload construction.
    let receiver_handle = tokio::spawn(async move {
        let mut captured = String::new();
        let mut capture_full = true;
        while let Some(output) = output_rx.recv().await {
            match output {
                CliOutput::Stdout(s) | CliOutput::Stderr(s) => {
                    if capture_full {
                        if !captured.is_empty() {
                            captured.push('\n');
                        }
                        captured.push_str(&s);
                        if captured.len() > MAX_CAPTURED_OUTPUT_BYTES {
                            captured.truncate(MAX_CAPTURED_OUTPUT_BYTES);
                            while !captured.is_char_boundary(captured.len()) {
                                captured.pop();
                            }
                            capture_full = false;
                        }
                    }
                    // Always consume from channel to prevent backpressure
                }
            }
        }
        captured
    });

    let exec_result = executor
        .execute(&summary_prompt, output_tx, shutdown_rx.clone())
        .await;

    // Wait for the receiver task to finish collecting output.
    // The channel closes when `output_tx` is dropped by the executor,
    // which causes the receiver loop to exit.
    let captured_output = receiver_handle.await.unwrap_or_default();

    let raw_summary =
        if exec_result.is_ok_and(|s| s.success()) && !captured_output.trim().is_empty() {
            captured_output.trim().to_string()
        } else {
            // Fallback: use local extraction with the full entry budget so the
            // summary is not prematurely truncated to 100 chars.
            extract_task_summary_with_max_len(task_content, MAX_SUMMARY_ENTRY_LENGTH)
        };

    // Normalize the summary: strip any path references the model may have included.
    // If normalization empties the text, fall back to local extraction, then to a
    // deterministic placeholder to guarantee we never produce a bare/empty list entry.
    let summary_text = normalize_summary_entry(&raw_summary)
        .or_else(|| {
            let fallback =
                extract_task_summary_with_max_len(task_content, MAX_SUMMARY_ENTRY_LENGTH);
            normalize_summary_entry(&fallback)
        })
        .unwrap_or_else(|| "Completed task".to_string());

    let entry = format!("- {summary_text}");
    truncate_summary(&entry, MAX_SUMMARY_ENTRY_LENGTH)
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
///
/// On success, returns the captured CLI output text from the successful attempt.
/// The output is also forwarded to the UI in real-time via `FlowEvent::Output`.
async fn run_with_retry<F>(
    input_text: &str,
    executor: &dyn AiCliExecutor,
    phase_builder: F,
    config: &RetryConfig,
    tx: &mpsc::Sender<FlowEvent>,
    shutdown_rx: &watch::Receiver<bool>,
) -> Result<String>
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

        // Spawn a task to forward CLI output to the UI and capture bounded text.
        // Capture is capped at `MAX_CAPTURED_OUTPUT_BYTES` so summary payloads
        // cannot grow without bound; live UI forwarding is always performed.
        let forward_handle = tokio::spawn(async move {
            let mut captured = String::new();
            let mut capture_full = true;
            while let Some(output) = output_rx.recv().await {
                let (text, is_stderr) = match output {
                    CliOutput::Stdout(s) => (s, false),
                    CliOutput::Stderr(s) => (s, true),
                };

                // Capture output text (bounded)
                if capture_full {
                    if !captured.is_empty() {
                        captured.push('\n');
                    }
                    captured.push_str(&text);
                    if captured.len() > MAX_CAPTURED_OUTPUT_BYTES {
                        captured.truncate(MAX_CAPTURED_OUTPUT_BYTES);
                        // Re-align to a char boundary after truncation
                        while !captured.is_char_boundary(captured.len()) {
                            captured.pop();
                        }
                        capture_full = false;
                    }
                }

                // Split by newlines and send each as a separate line (always)
                for line_text in text.lines() {
                    let line = if is_stderr {
                        OutputLine::stderr(line_text)
                    } else {
                        OutputLine::stdout(line_text)
                    };
                    let _ = tx_clone.send(FlowEvent::Output(line)).await;
                }
            }
            captured
        });

        match executor
            .execute(input_text, output_tx, shutdown_rx.clone())
            .await
        {
            Ok(status) if status.success() => {
                let captured = forward_handle.await.unwrap_or_default();
                return Ok(captured);
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
            // 2 tasks × 2 calls each (execution + summary generation)
            assert_eq!(executor.get_call_count(), 4);

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

        /// Tests that execution wraps content with execution prompts and passes
        /// captured execution output into the summary prompt's `EXECUTION_OUTPUT` section.
        #[tokio::test]
        async fn wraps_content_for_execution() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let file = todo_dir.join("task-001.md");
            fs::write(&file, "Original task content").await.unwrap();

            let files = vec![file];

            let executor = MockExecutor::new_success("MockExecutor")
                .with_output("Mock execution completed successfully");
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
            // 2 calls: execution + summary generation
            assert_eq!(inputs.len(), 2);
            // First call is the execution prompt
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
            // Second call is the summary prompt
            assert!(
                inputs[1].contains("TASK_SPECIFICATION"),
                "Second call should be the summary prompt"
            );
            // Verify the summary prompt contains non-empty EXECUTION_OUTPUT
            // from the preceding execution call
            assert!(
                inputs[1].contains("<EXECUTION_OUTPUT>"),
                "Summary prompt should contain EXECUTION_OUTPUT section"
            );
            assert!(
                inputs[1].contains("Mock execution completed successfully"),
                "Summary prompt should contain the captured execution output"
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

            // Create task_text with existing completed task summaries (inline summaries)
            let mut task_text = r"Initial task description

<COMPLETED_TASKS>
- Added retry logic with exponential backoff for CLI executor
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
            // 2 calls: execution + summary generation
            assert_eq!(inputs.len(), 2);
            // Check that completed task summary is included in the execution prompt
            assert!(
                inputs[0].contains("Added retry logic with exponential backoff"),
                "Should contain completed task summary in context"
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

            // Create task_text with pre-existing completed task summaries (inline summaries)
            let mut task_text = r"Initial task description

<COMPLETED_TASKS>
- Set up PostgreSQL database with connection pooling and initial schema
- Created data models and ORM mappings for user and product entities
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

            // Verify executor received input with completed task summaries
            let inputs = executor.get_recorded_inputs();
            // 2 calls: execution + summary generation
            assert_eq!(inputs.len(), 2);

            // First call is the execution prompt
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

            // Verify completed task summaries are included
            assert!(
                input.contains("Set up PostgreSQL database"),
                "Input should contain first completed task summary"
            );
            assert!(
                input.contains("Created data models"),
                "Input should contain second completed task summary"
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

            // Verify no file paths leak into COMPLETED_TASKS
            assert!(
                !input.contains(".mcgravity/todo/done/"),
                "COMPLETED_TASKS should not contain done-file paths"
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
            // 2 calls: execution + summary generation
            assert_eq!(inputs.len(), 2);

            // First call is the execution prompt
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

            // Verify task_text was updated with a summary (not a file path)
            assert!(
                task_text.contains("<COMPLETED_TASKS>"),
                "task_text should contain COMPLETED_TASKS block"
            );
            // The summary should contain task content derived from extract_task_summary fallback
            // (since MockExecutor doesn't produce summary output, it falls back to local extraction)
            assert!(
                task_text.contains("Task 001: Setup Database"),
                "task_text should contain task summary derived from content"
            );
            // Archived file paths should NOT appear in COMPLETED_TASKS
            assert!(
                !task_text.contains(".mcgravity/todo/done/"),
                "task_text should not contain archived file path references"
            );

            // Verify task.md was persisted
            let task_md_path = paths.task_file();
            assert!(
                fs::try_exists(&task_md_path).await.unwrap(),
                "task.md should be created"
            );
            let saved_content = fs::read_to_string(&task_md_path).await.unwrap();
            assert!(
                !saved_content.contains(".mcgravity/todo/done/"),
                "Persisted task.md should not contain archived file path references"
            );
            assert!(
                saved_content.contains("<COMPLETED_TASKS>"),
                "Persisted task.md should contain COMPLETED_TASKS block"
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

            // 2 tasks × 2 calls each (execution + summary generation)
            assert_eq!(executor.get_call_count(), 4);

            let inputs = executor.get_recorded_inputs();
            // 4 inputs: exec1, summary1, exec2, summary2
            assert_eq!(inputs.len(), 4);

            // First execution should have empty completed tasks
            assert!(
                inputs[0].contains("<COMPLETED_TASKS>\n\n</COMPLETED_TASKS>"),
                "First task execution should have empty COMPLETED_TASKS"
            );

            // Second execution (index 2) should see the first task summary as completed
            assert!(
                inputs[2].contains("Task 001: First"),
                "Second task execution should see first task summary in COMPLETED_TASKS"
            );

            // No file paths should appear in any execution input's COMPLETED_TASKS
            assert!(
                !inputs[2].contains(".mcgravity/todo/done/"),
                "Second task execution should not see done-file paths in COMPLETED_TASKS"
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

            // Final task_text should contain both completed task summaries (not file paths)
            assert!(
                task_text.contains("Task 001: First"),
                "Final task_text should contain first task summary"
            );
            assert!(
                task_text.contains("Task 002: Second"),
                "Final task_text should contain second task summary"
            );
            // No file paths in the final task_text
            assert!(
                !task_text.contains(".mcgravity/todo/done/"),
                "Final task_text should not contain done-file paths"
            );

            // Each summary entry should be capped below 500 characters
            let summary = extract_completed_tasks_summary(&task_text);
            for line in summary.lines() {
                assert!(
                    line.len() < MAX_SUMMARY_ENTRY_LENGTH,
                    "Each summary entry should be under {MAX_SUMMARY_ENTRY_LENGTH} chars, got {}",
                    line.len()
                );
            }
        }

        /// Tests that legacy path entries in `task_text` are cleaned before planning.
        /// This verifies the normalization applied in `run_flow` before the main loop.
        #[tokio::test]
        async fn legacy_path_entries_cleaned_from_task_text() {
            use crate::core::task_utils::normalize_task_text_completed_section;

            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-003.md");
            fs::write(&todo_file, "# Task 003: New Work\n\nDo something.")
                .await
                .unwrap();

            let files = vec![todo_file];

            let executor = MockExecutor::new_success("MockExecutor");
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(100);
            let shutdown_rx = create_shutdown_rx();

            // Create task_text with legacy path-based COMPLETED_TASKS entries
            let mut task_text = r"Initial task description

<COMPLETED_TASKS>
- /home/tigran/projects/ungravity/mcgravity/.mcgravity/todo/done/task-001.md
- /home/tigran/projects/ungravity/mcgravity/.mcgravity/todo/done/task-002.md
- Added retry logic with exponential backoff
</COMPLETED_TASKS>
"
            .to_string();

            // Apply normalization as run_flow does before planning
            task_text = normalize_task_text_completed_section(&task_text);

            // Verify legacy paths were removed
            assert!(
                !task_text.contains("/home/"),
                "Legacy /home/ paths should be removed after normalization"
            );
            assert!(
                !task_text.contains(".mcgravity/todo/done/"),
                "Legacy done-file paths should be removed after normalization"
            );
            assert!(
                task_text.contains("Added retry logic"),
                "Clean summaries should be preserved after normalization"
            );

            // Now run process_todos_phase with the cleaned task_text
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
            // 2 calls: execution + summary generation
            assert_eq!(inputs.len(), 2);

            // Verify execution input contains clean summary, not paths
            assert!(
                inputs[0].contains("Added retry logic"),
                "Execution input should contain clean summary text"
            );
            assert!(
                !inputs[0].contains("/home/tigran"),
                "Execution input should not contain legacy absolute paths"
            );
            assert!(
                !inputs[0].contains(".mcgravity/todo/done/task-001"),
                "Execution input should not contain done-file path references"
            );
        }

        /// Tests that model-generated summary with paths is normalized.
        #[tokio::test]
        async fn model_summary_with_paths_is_normalized() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Setup Database\n\n## Objective\nConfigure PostgreSQL.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];

            // Mock executor that returns summary text containing paths
            let executor = MockExecutor::new_success("MockExecutor")
                .with_output("Updated /home/user/project/src/core/executor.rs to add retry logic");
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

            // Verify the summary in task_text does not contain paths
            assert!(
                !task_text.contains("/home/"),
                "task_text should not contain absolute paths from model output"
            );
            // It should have fallen back to local extraction
            assert!(
                task_text.contains("Task 001: Setup Database"),
                "task_text should contain fallback summary from local extraction"
            );
        }

        /// Regression test: summary generation with output exceeding channel capacity
        /// completes without deadlocking.
        ///
        /// Before the fix, `generate_task_summary()` would deadlock when the executor
        /// produced more output than the channel buffer (100) because output was only
        /// drained after `execute()` returned, but `execute()` blocked on a full channel.
        #[tokio::test]
        async fn summary_generation_does_not_deadlock_on_verbose_output() {
            /// A mock executor that sends many individual output messages to exceed
            /// the channel buffer capacity and trigger backpressure.
            #[derive(Debug)]
            struct VerboseExecutor {
                line_count: usize,
            }

            #[async_trait]
            impl AiCliExecutor for VerboseExecutor {
                async fn execute(
                    &self,
                    _input: &str,
                    output_tx: mpsc::Sender<CliOutput>,
                    _shutdown_rx: watch::Receiver<bool>,
                ) -> Result<ExitStatus> {
                    // Send more lines than the channel capacity (100)
                    for i in 0..self.line_count {
                        output_tx
                            .send(CliOutput::Stdout(format!("output line {i}")))
                            .await
                            .ok();
                    }
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
                    "Verbose"
                }
                fn command(&self) -> &'static str {
                    "mock"
                }
                fn is_available(&self) -> bool {
                    true
                }
            }

            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Verbose\n\n## Objective\nGenerate verbose output.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];
            // 200 lines exceeds the channel buffer of 100
            let executor = VerboseExecutor { line_count: 200 };
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(1000);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            // This must complete within a timeout; a deadlock would hang forever.
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                process_todos_phase(
                    &files,
                    &mut task_text,
                    &executor,
                    &retry_config,
                    &tx,
                    &shutdown_rx,
                    &paths,
                ),
            )
            .await;

            assert!(
                result.is_ok(),
                "process_todos_phase should complete without deadlocking"
            );
            assert!(
                result.unwrap().is_ok(),
                "process_todos_phase should succeed"
            );
        }

        /// Regression: captured execution output passed to the summary prompt is bounded.
        ///
        /// When the executor produces output much larger than `MAX_CAPTURED_OUTPUT_BYTES`,
        /// the captured output used for summary generation must be deterministically
        /// truncated (live UI forwarding is unaffected).
        #[tokio::test]
        async fn summary_generation_uses_bounded_captured_output() {
            /// A mock executor whose execution call produces oversized output and
            /// whose summary call succeeds with clean text.
            #[derive(Debug)]
            struct OversizedOutputExecutor {
                call_count: AtomicU32,
            }

            #[async_trait]
            impl AiCliExecutor for OversizedOutputExecutor {
                async fn execute(
                    &self,
                    _input: &str,
                    output_tx: mpsc::Sender<CliOutput>,
                    _shutdown_rx: watch::Receiver<bool>,
                ) -> Result<ExitStatus> {
                    let call = self.call_count.fetch_add(1, Ordering::SeqCst);
                    if call == 0 {
                        // First call is execution: send oversized output
                        // Send 200KB worth of output in chunks
                        for i in 0..2000 {
                            output_tx
                                .send(CliOutput::Stdout(format!("line {i}: {}", "X".repeat(100))))
                                .await
                                .ok();
                        }
                    } else {
                        // Second call is summary generation: return clean text
                        output_tx
                            .send(CliOutput::Stdout(
                                "Added bounded output capture for summary generation".to_string(),
                            ))
                            .await
                            .ok();
                    }
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
                    "OversizedOutput"
                }
                fn command(&self) -> &'static str {
                    "mock"
                }
                fn is_available(&self) -> bool {
                    true
                }
            }

            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Bounded\n\n## Objective\nTest bounded capture.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];
            let executor = OversizedOutputExecutor {
                call_count: AtomicU32::new(0),
            };
            let retry_config = RetryConfig::default();
            let (tx, _rx) = mpsc::channel(10_000);
            let shutdown_rx = create_shutdown_rx();
            let mut task_text = "Initial task description".to_string();

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                process_todos_phase(
                    &files,
                    &mut task_text,
                    &executor,
                    &retry_config,
                    &tx,
                    &shutdown_rx,
                    &paths,
                ),
            )
            .await;

            assert!(
                result.is_ok(),
                "process_todos_phase should complete without deadlocking"
            );
            assert!(
                result.unwrap().is_ok(),
                "process_todos_phase should succeed"
            );
            // Verify the summary was added (from bounded capture)
            assert!(
                task_text.contains("<COMPLETED_TASKS>"),
                "task_text should have COMPLETED_TASKS block after bounded capture"
            );
        }

        /// Regression: `process_todos_phase` never writes bare/empty completed-task list entries.
        ///
        /// When both the model output and the local extraction produce only path
        /// references (which get stripped), the fallback must produce a non-empty
        /// deterministic summary entry.
        #[tokio::test]
        async fn process_todos_phase_never_writes_bare_entries() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Create a minimal todo file with no title/objective (worst case for extraction)
            let todo_file = todo_dir.join("task-001.md");
            fs::write(&todo_file, "").await.unwrap();

            let files = vec![todo_file];

            // Executor succeeds but produces no output (empty summary scenario)
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

            // Verify COMPLETED_TASKS block was added
            assert!(
                task_text.contains("<COMPLETED_TASKS>"),
                "task_text should have COMPLETED_TASKS block"
            );

            // Extract the block content and ensure no bare entries
            let open = task_text.find("<COMPLETED_TASKS>").unwrap();
            let close = task_text.find("</COMPLETED_TASKS>").unwrap();
            let block = &task_text[open + "<COMPLETED_TASKS>".len()..close];
            for line in block.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // No bare "- " entries
                let without_marker = trimmed.strip_prefix('-').unwrap_or(trimmed).trim();
                assert!(
                    !without_marker.is_empty(),
                    "COMPLETED_TASKS must not contain bare list markers, found: {trimmed:?}"
                );
            }
        }

        /// Regression: completed-task summaries in `task_text` contain no path leakage.
        #[tokio::test]
        async fn process_todos_phase_no_path_leakage() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Setup\n\n## Objective\nConfigure the system.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];

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

            // Verify no path leakage in the completed tasks block
            assert!(
                !task_text.contains("src/"),
                "task_text must not contain repo-style src/ paths"
            );
            assert!(
                !task_text.contains("./"),
                "task_text must not contain ./ relative paths"
            );
            assert!(
                !task_text.contains("../"),
                "task_text must not contain ../ relative paths"
            );
            assert!(
                !task_text.contains(".mcgravity/todo/done/"),
                "task_text must not contain done-file paths"
            );
            assert!(
                !task_text.contains("/home/"),
                "task_text must not contain absolute paths"
            );
        }

        /// Regression: multiline model summary output is collapsed into a single-line
        /// entry in `<COMPLETED_TASKS>` with no embedded newlines.
        #[tokio::test]
        async fn process_todos_phase_multiline_summary_collapsed() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            let todo_file = todo_dir.join("task-001.md");
            fs::write(
                &todo_file,
                "# Task 001: Setup Database\n\n## Objective\nConfigure PostgreSQL.",
            )
            .await
            .unwrap();

            let files = vec![todo_file];

            // Executor returns multiline summary output (simulating model that embeds newlines)
            let executor = MockExecutor::new_success("MockExecutor")
                .with_output("Added retry logic\nwith exponential backoff\nfor CLI executor");
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

            // Verify COMPLETED_TASKS block exists
            assert!(
                task_text.contains("<COMPLETED_TASKS>"),
                "task_text should contain COMPLETED_TASKS block"
            );

            // Extract the block content and verify each entry is a single line
            let open = task_text.find("<COMPLETED_TASKS>").unwrap();
            let close = task_text.find("</COMPLETED_TASKS>").unwrap();
            let block = &task_text[open + "<COMPLETED_TASKS>".len()..close];
            for line in block.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Each entry must start with "- " and be a single line
                assert!(
                    trimmed.starts_with("- "),
                    "Entry should start with '- ', got: {trimmed:?}"
                );
                // Verify the entry text (after "- ") contains no newlines
                // (this is inherently true because we iterate `.lines()`, but
                // we also verify the overall block has the expected shape)
                let content = trimmed.strip_prefix("- ").unwrap();
                assert!(
                    !content.is_empty(),
                    "Entry content must not be empty after '- '"
                );
            }

            // Verify the persisted task.md also has single-line entries
            let task_md_path = paths.task_file();
            let saved_content = fs::read_to_string(&task_md_path).await.unwrap();
            let saved_open = saved_content.find("<COMPLETED_TASKS>").unwrap();
            let saved_close = saved_content.find("</COMPLETED_TASKS>").unwrap();
            let saved_block = &saved_content[saved_open + "<COMPLETED_TASKS>".len()..saved_close];
            let entry_lines: Vec<&str> = saved_block
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            assert!(
                !entry_lines.is_empty(),
                "Should have at least one completed task entry"
            );
            for entry in &entry_lines {
                assert!(
                    entry.starts_with("- "),
                    "Persisted entry should start with '- ', got: {entry:?}"
                );
            }
        }

        /// Regression: when the model output is empty/invalid and the fallback summary
        /// is used, the completed-task entry stored in `<COMPLETED_TASKS>` must not be
        /// prematurely truncated to ~100 characters with `...`.
        ///
        /// The fallback path uses `extract_task_summary` (capped at 100 chars), but the
        /// entry budget is 500 chars (`MAX_SUMMARY_ENTRY_LENGTH`). A long task objective
        /// should be preserved up to the 500-char cap, not cut short by the intermediate
        /// 100-char extraction limit.
        ///
        /// This test follows the CLAUDE.md "Testing" guideline:
        ///   "Test core logic independently from TUI layer"
        /// and rust.mdc "5.1. Unit Testing" for focused regression coverage.
        #[tokio::test]
        async fn summary_fallback_not_prematurely_truncated() {
            let dir = TempDir::new().unwrap();
            let paths = test_paths(&dir);
            let todo_dir = paths.todo_dir();
            fs::create_dir_all(&todo_dir).await.unwrap();

            // Build a task file with a long objective (200+ chars).
            // When fallback fires, the stored entry should retain more than 100 chars.
            let long_objective = "A".repeat(250);
            let todo_content =
                format!("# Task 001: Long Objective Task\n\n## Objective\n{long_objective}\n");
            let todo_file = todo_dir.join("task-001.md");
            fs::write(&todo_file, &todo_content).await.unwrap();

            let files = vec![todo_file];

            // Executor succeeds but produces no output → triggers fallback path
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

            // Extract the completed-tasks block
            let summary = extract_completed_tasks_summary(&task_text);
            assert!(
                !summary.is_empty(),
                "Should have a completed task summary entry"
            );

            // The entry must NOT be prematurely truncated to ~100 chars.
            // With a 250-char objective + ~30-char title collapsed into a single
            // line, the entry (with "- " prefix) should be ~280+ chars before the
            // 500-char cap. The premature truncation bug would cut this to ~102 chars.
            let entry_line = summary.lines().next().unwrap();
            assert!(
                entry_line.len() > 200,
                "Fallback summary should preserve most of the 280-char content, \
                 not be prematurely truncated to ~100 chars; got {} chars: {:?}",
                entry_line.len(),
                entry_line
            );

            // The entry must still be bounded at MAX_SUMMARY_ENTRY_LENGTH (500 chars)
            assert!(
                entry_line.len() <= MAX_SUMMARY_ENTRY_LENGTH,
                "Summary entry must be capped at {MAX_SUMMARY_ENTRY_LENGTH} chars, got {}",
                entry_line.len()
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
