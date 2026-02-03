//! CLI command execution for AI coding assistants.
//!
//! Provides a trait-based abstraction for executing different AI CLI tools
//! (Codex, Claude, Gemini, etc.) with a unified interface.
//!
//! # Command Resolution
//!
//! This module uses the shell-aware resolution strategy from [`crate::core::cli_check`]:
//! 1. Fast PATH lookup via `which`/`where` with executability verification
//! 2. Shell-based resolution via `$SHELL -l -i -c "command -v <cmd>"` (Unix only)
//!
//! This ensures that commands installed via macOS aliases, shell functions, or
//! PATH modifications in shell profiles are properly detected and executed.
//!
//! See `docs/adding-executors.md` for the complete resolution strategy documentation.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::future::Future;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::{mpsc, watch};

/// Output line from CLI execution.
#[derive(Debug, Clone)]
pub enum CliOutput {
    /// Line from stdout.
    Stdout(String),
    /// Line from stderr.
    Stderr(String),
}

/// Parses a line from Claude's stream-json output and extracts displayable content.
///
/// Claude's `--output-format stream-json` emits newline-delimited JSON (JSONL).
/// Each line has a top-level `type` field indicating the message type:
/// - `assistant`: Contains assistant text in `message.content[].text`
/// - `user`: User messages (filtered out)
/// - `system`: Session initialization (shows init message)
/// - `result`: Final completion with `subtype` and `result` fields
///
/// Returns `Some(text)` if displayable content was found, `None` otherwise.
fn parse_claude_stream_json(line: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(line).ok()?;

    // Get the message type
    let msg_type = json.get("type")?.as_str()?;

    match msg_type {
        // Assistant messages contain the AI's responses
        // Structure: {"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
        "assistant" => {
            let message = json.get("message")?;
            let content = message.get("content")?.as_array()?;
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        item.get("text")?.as_str()
                    } else {
                        None
                    }
                })
                .collect();

            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        }
        // Result event indicates completion
        // Structure: {"type":"result","subtype":"success","result":"..."}
        "result" => {
            let subtype = json.get("subtype").and_then(serde_json::Value::as_str);
            match subtype {
                Some("error") => {
                    // Show error status
                    let is_error = json.get("is_error").and_then(serde_json::Value::as_bool);
                    Some(format!("[Error: is_error={is_error:?}]"))
                }
                Some("success") => {
                    // Show success result text so user sees final output
                    json.get("result")
                        .and_then(|r| r.as_str())
                        .map(str::to_string)
                }
                _ => None,
            }
        }
        // System events for session initialization
        // Structure: {"type":"system","subtype":"init","session_id":"..."}
        "system" => {
            let subtype = json.get("subtype").and_then(serde_json::Value::as_str);
            if subtype == Some("init") {
                Some("[Claude Code session started]".to_string())
            } else {
                None
            }
        }
        // Filter out other event types (user, etc.)
        _ => None,
    }
}

/// Trait for AI CLI executors.
///
/// Implement this trait to add support for new AI CLI tools.
/// Each implementation handles the specific CLI invocation for that tool.
#[async_trait]
pub trait AiCliExecutor: Send + Sync {
    /// Executes the CLI with the given input text, streaming output.
    ///
    /// # Arguments
    ///
    /// * `input` - The text/prompt to send to the CLI
    /// * `output_tx` - Channel sender for streaming CLI output
    /// * `shutdown_rx` - Receiver for shutdown signals
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to execute or if shutdown is signaled.
    async fn execute(
        &self,
        input: &str,
        output_tx: mpsc::Sender<CliOutput>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<ExitStatus>;

    /// Returns the display name for this executor.
    fn name(&self) -> &'static str;

    /// Returns the CLI command name used by this executor.
    fn command(&self) -> &'static str;

    /// Checks if this executor's CLI tool is available.
    ///
    /// Uses the shell-aware resolution strategy to detect commands available
    /// via PATH, shell aliases, functions, or shell profile modifications.
    fn is_available(&self) -> bool {
        check_cli_available(self.command())
    }
}

/// `OpenAI` Codex CLI executor.
///
/// Executes: `codex exec --dangerously-bypass-approvals-and-sandbox <text>`
#[derive(Debug, Clone, Copy, Default)]
pub struct CodexExecutor;

#[async_trait]
impl AiCliExecutor for CodexExecutor {
    async fn execute(
        &self,
        input: &str,
        output_tx: mpsc::Sender<CliOutput>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<ExitStatus> {
        run_cli_with_output(
            self.command(),
            &["exec", "--dangerously-bypass-approvals-and-sandbox", input],
            output_tx,
            shutdown_rx,
        )
        .await
    }

    fn name(&self) -> &'static str {
        "Codex"
    }

    fn command(&self) -> &'static str {
        "codex"
    }
}

/// Anthropic Claude Code CLI executor.
///
/// Executes: `claude -p <text> --dangerously-skip-permissions --output-format stream-json --verbose`
///
/// Uses `stream-json` format for real-time streaming output. The JSON is parsed
/// internally to extract text content only. The `--verbose` flag is required
/// when using `stream-json` with `--print` mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeExecutor;

#[async_trait]
impl AiCliExecutor for ClaudeExecutor {
    async fn execute(
        &self,
        input: &str,
        output_tx: mpsc::Sender<CliOutput>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<ExitStatus> {
        run_claude_cli_with_output(
            self.command(),
            &[
                "-p",
                input,
                "--dangerously-skip-permissions",
                "--output-format",
                "stream-json",
                "--verbose",
            ],
            output_tx,
            shutdown_rx,
        )
        .await
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn command(&self) -> &'static str {
        "claude"
    }
}

/// Google Gemini CLI executor.
///
/// Executes: `gemini -y <text>`
///
/// Uses `-y` (YOLO mode) to automatically accept all tool actions.
/// Output is plain text, streamed line-by-line to the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeminiExecutor;

#[async_trait]
impl AiCliExecutor for GeminiExecutor {
    async fn execute(
        &self,
        input: &str,
        output_tx: mpsc::Sender<CliOutput>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<ExitStatus> {
        run_cli_with_output(self.command(), &["-y", input], output_tx, shutdown_rx).await
    }

    fn name(&self) -> &'static str {
        "Gemini"
    }

    fn command(&self) -> &'static str {
        "gemini"
    }
}

/// A spawned CLI process with captured stdout and stderr.
struct SpawnedProcess {
    child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

/// Spawns a CLI process with stdout and stderr captured.
///
/// Uses the shell-aware resolution strategy to find the command:
/// 1. Resolves the command using [`crate::core::cli_check::resolve_cli_command`]
/// 2. For `PathExecutable`: spawns directly using the resolved path
/// 3. For shell-resolved commands (alias/function/builtin): spawns via shell wrapper
/// 4. For `NotFound`: returns a contextual error
///
/// On Linux, configures the child to be killed when the parent dies via `PR_SET_PDEATHSIG`.
///
/// # Arguments
///
/// * `command` - The command name to execute (e.g., "claude", "codex")
/// * `args` - Arguments to pass to the command
///
/// # Errors
///
/// Returns an error if the command cannot be resolved or if spawning fails.
fn spawn_cli_process(command: &str, args: &[&str]) -> Result<SpawnedProcess> {
    use crate::core::cli_check::{CommandResolution, resolve_cli_command};

    let resolution = resolve_cli_command(command);

    let mut cmd = match &resolution {
        CommandResolution::PathExecutable(path) => {
            // Direct execution with resolved path
            let mut c = Command::new(path);
            c.args(args);
            c
        }
        CommandResolution::ShellAlias(_)
        | CommandResolution::ShellFunction(_)
        | CommandResolution::ShellBuiltin => {
            // Shell wrapper required
            #[cfg(unix)]
            {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

                // Build the command string with properly escaped args
                let escaped_args: Vec<String> =
                    args.iter().map(|arg| shell_escape_arg(arg)).collect();
                let full_command = format!("{} {}", command, escaped_args.join(" "));

                let mut c = Command::new(&shell);
                c.args(["-l", "-i", "-c", &full_command]);
                c
            }
            #[cfg(windows)]
            {
                // On Windows, shell-resolved commands are less common
                // Fall back to direct execution attempt
                let mut c = Command::new(command);
                c.args(args);
                c
            }
        }
        CommandResolution::NotFound => {
            anyhow::bail!(
                "CLI command '{command}' not found. Ensure it is installed and available in PATH, \
                or via shell alias/function. Run `which {command}` or check your shell profile."
            );
        }
    };

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // On Linux, set up the child to be killed when the parent dies.
    // This ensures cleanup even if the parent is killed with SIGKILL.
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            // PR_SET_PDEATHSIG = 1, SIGKILL = 9
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {command} CLI"))?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    Ok(SpawnedProcess {
        child,
        stdout,
        stderr,
    })
}

/// Escapes a shell argument for safe inclusion in a shell command string.
///
/// Uses single quotes for most cases, with proper handling of embedded single quotes.
#[cfg(unix)]
fn shell_escape_arg(arg: &str) -> String {
    // If the arg contains no special characters, return as-is
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return arg.to_string();
    }

    // Use single quotes, escaping embedded single quotes as '\''
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Runs a CLI process with custom stdout processing.
///
/// This is the core execution function that handles:
/// - Spawning the process with captured I/O
/// - Running a custom stdout processor via `create_stdout_task`
/// - Streaming stderr to the output channel
/// - Handling shutdown signals gracefully
///
/// The `create_stdout_task` parameter is a function that takes stdout and the output sender,
/// and returns a future that processes stdout lines. This allows different executors to
/// customize how stdout is processed (e.g., raw lines vs JSON parsing).
async fn run_process_with_output<F, Fut>(
    command: &str,
    args: &[&str],
    output_tx: mpsc::Sender<CliOutput>,
    mut shutdown_rx: watch::Receiver<bool>,
    create_stdout_task: F,
) -> Result<ExitStatus>
where
    F: FnOnce(ChildStdout, mpsc::Sender<CliOutput>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let SpawnedProcess {
        mut child,
        stdout,
        stderr,
    } = spawn_cli_process(command, args)?;

    // Spawn stdout processor task using the provided factory
    let stdout_handle = tokio::spawn(create_stdout_task(stdout, output_tx.clone()));

    // Spawn stderr reader task (common for all executors)
    let tx_stderr = output_tx;
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_stderr.send(CliOutput::Stderr(line)).await;
        }
    });

    // Wait for either process completion or shutdown signal
    let status = tokio::select! {
        result = child.wait() => {
            result.with_context(|| format!("Failed to wait for {command} CLI"))?
        }
        () = wait_for_shutdown(&mut shutdown_rx) => {
            // Shutdown signaled - kill the child process
            let _ = child.kill().await;
            stdout_handle.abort();
            stderr_handle.abort();
            anyhow::bail!("Shutdown signaled - {command} process killed");
        }
    };

    // Wait for output readers to finish
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    Ok(status)
}

/// Runs a CLI command and streams its output.
///
/// If shutdown is signaled, the child process will be killed and an error returned.
async fn run_cli_with_output(
    command: &str,
    args: &[&str],
    output_tx: mpsc::Sender<CliOutput>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<ExitStatus> {
    run_process_with_output(
        command,
        args,
        output_tx,
        shutdown_rx,
        |stdout, tx| async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(CliOutput::Stdout(line)).await;
            }
        },
    )
    .await
}

/// Runs the Claude CLI and streams its parsed JSON output.
///
/// This function is specifically designed for Claude's `--output-format stream-json` mode.
/// It parses each JSONL line and extracts text content before forwarding to the output channel.
/// Non-text messages (tool usage, etc.) are silently filtered out.
///
/// If shutdown is signaled, the child process will be killed and an error returned.
async fn run_claude_cli_with_output(
    command: &str,
    args: &[&str],
    output_tx: mpsc::Sender<CliOutput>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<ExitStatus> {
    run_process_with_output(
        command,
        args,
        output_tx,
        shutdown_rx,
        |stdout, tx| async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Parse JSON and extract text content
                if let Some(text) = parse_claude_stream_json(&line) {
                    let _ = tx.send(CliOutput::Stdout(text)).await;
                } else if !line.trim().is_empty() {
                    // Forward unparseable non-empty lines as-is for debugging
                    let _ = tx.send(CliOutput::Stdout(line)).await;
                }
            }
        },
    )
    .await
}

/// Checks if a CLI tool is available using the shell-aware resolution strategy.
///
/// This function delegates to [`crate::core::cli_check::resolve_cli_command`]
/// which implements the resolution algorithm documented in `docs/adding-executors.md`:
/// 1. Fast PATH lookup via `which`/`where` with executability verification
/// 2. Shell-based resolution via `$SHELL -l -i -c "command -v <cmd>"` (Unix only)
///
/// Returns `false` for non-executable files in PATH.
///
/// # Arguments
///
/// * `name` - The command name to check (e.g., "codex", "claude", "gemini")
///
/// # Returns
///
/// `true` if the command is available, `false` otherwise.
#[must_use]
pub fn check_cli_available(name: &str) -> bool {
    crate::core::cli_check::resolve_cli_command(name).is_available()
}

/// Waits for a shutdown signal on the watch channel.
///
/// This function loops until the shutdown value becomes true, checking after each
/// change notification. It's designed to be used with `tokio::select!`.
async fn wait_for_shutdown(rx: &mut watch::Receiver<bool>) {
    loop {
        // Check current value without holding the lock
        if *rx.borrow() {
            return;
        }
        // Wait for a change, exit if channel closed
        if rx.changed().await.is_err() {
            // Channel closed, treat as shutdown
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // parse_claude_stream_json Tests
    // =========================================================================

    mod parse_claude_stream_json_tests {
        use super::*;

        /// Tests parsing an assistant message with text content.
        #[test]
        fn parses_assistant_message_with_text() {
            let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world!"}]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("Hello world!".to_string()));
        }

        /// Tests parsing an assistant message with multiple text items.
        #[test]
        fn parses_assistant_message_with_multiple_texts() {
            let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"First"},{"type":"text","text":"Second"}]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("FirstSecond".to_string()));
        }

        /// Tests that system/init events show session started message.
        #[test]
        fn shows_system_init_message() {
            let line = r#"{"type":"system","subtype":"init","session_id":"abc123"}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("[Claude Code session started]".to_string()));
        }

        /// Tests that non-init system events are filtered out.
        #[test]
        fn filters_non_init_system_events() {
            let line = r#"{"type":"system","subtype":"other","session_id":"abc123"}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests that user events are filtered out.
        #[test]
        fn filters_user_events() {
            let line = r#"{"type":"user","message":{"content":[{"type":"text","text":"hello"}]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests that success result events return the result text.
        #[test]
        fn shows_success_result_text() {
            let line =
                r#"{"type":"result","subtype":"success","result":"1 + 1 = 2","duration_ms":1234}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("1 + 1 = 2".to_string()));
        }

        /// Tests that success result events without result field return None.
        #[test]
        fn filters_success_result_without_text() {
            let line = r#"{"type":"result","subtype":"success","duration_ms":1234}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests that error result events show error info.
        #[test]
        fn shows_error_result_status() {
            let line = r#"{"type":"result","subtype":"error","is_error":true}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("[Error: is_error=Some(true)]".to_string()));
        }

        /// Tests that invalid JSON returns None.
        #[test]
        fn returns_none_for_invalid_json() {
            let line = "not valid json";
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests that JSON without type field returns None.
        #[test]
        fn returns_none_for_missing_type() {
            let line = r#"{"message":{"content":[{"type":"text","text":"Hello"}]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests assistant message with empty content array.
        #[test]
        fn returns_none_for_empty_content() {
            let line = r#"{"type":"assistant","message":{"content":[]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests assistant message with only `tool_use` in content (no text).
        #[test]
        fn returns_none_for_assistant_without_text() {
            let line =
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, None);
        }

        /// Tests parsing real Claude output format.
        #[test]
        fn parses_real_claude_output() {
            let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-5-20251101","id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"1 + 1 = 2"}],"stop_reason":null}}"#;
            let result = parse_claude_stream_json(line);
            assert_eq!(result, Some("1 + 1 = 2".to_string()));
        }
    }

    // =========================================================================
    // CliOutput Tests
    // =========================================================================

    mod cli_output {
        use super::*;

        /// Tests creating stdout output.
        #[test]
        fn stdout_contains_text() {
            let output = CliOutput::Stdout("Hello, world!".to_string());

            if let CliOutput::Stdout(text) = output {
                assert_eq!(text, "Hello, world!");
            } else {
                panic!("Expected Stdout variant");
            }
        }

        /// Tests creating stderr output.
        #[test]
        fn stderr_contains_text() {
            let output = CliOutput::Stderr("Error occurred".to_string());

            if let CliOutput::Stderr(text) = output {
                assert_eq!(text, "Error occurred");
            } else {
                panic!("Expected Stderr variant");
            }
        }

        /// Tests that `CliOutput` can be cloned.
        #[test]
        fn clone_preserves_variant_and_content() {
            let original = CliOutput::Stdout("test".to_string());
            let cloned = original.clone();

            if let (CliOutput::Stdout(orig_text), CliOutput::Stdout(clone_text)) =
                (&original, &cloned)
            {
                assert_eq!(orig_text, clone_text);
            } else {
                panic!("Clone changed variant");
            }
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_shows_variant() {
            let stdout = CliOutput::Stdout("msg".to_string());
            let stderr = CliOutput::Stderr("err".to_string());

            assert!(format!("{stdout:?}").contains("Stdout"));
            assert!(format!("{stderr:?}").contains("Stderr"));
        }

        /// Tests stdout with empty string.
        #[test]
        fn stdout_empty_string() {
            let output = CliOutput::Stdout(String::new());

            if let CliOutput::Stdout(text) = output {
                assert!(text.is_empty());
            } else {
                panic!("Expected Stdout variant");
            }
        }

        /// Tests stderr with multiline content.
        #[test]
        fn stderr_multiline_content() {
            let multiline = "Line 1\nLine 2\nLine 3".to_string();
            let output = CliOutput::Stderr(multiline.clone());

            if let CliOutput::Stderr(text) = output {
                assert_eq!(text, multiline);
                assert!(text.contains('\n'));
            } else {
                panic!("Expected Stderr variant");
            }
        }
    }

    // =========================================================================
    // CodexExecutor Tests
    // =========================================================================

    mod codex_executor {
        use super::*;

        /// Tests that `CodexExecutor` returns correct name.
        #[test]
        fn name_returns_codex() {
            let executor = CodexExecutor;
            assert_eq!(executor.name(), "Codex");
        }

        /// Tests that `CodexExecutor` returns correct command.
        #[test]
        fn command_returns_codex() {
            let executor = CodexExecutor;
            assert_eq!(executor.command(), "codex");
        }

        /// Tests that `CodexExecutor` implements `Default`.
        #[test]
        fn default_creates_instance() {
            let executor = CodexExecutor;
            assert_eq!(executor.name(), "Codex");
        }

        /// Tests that `CodexExecutor` can be cloned.
        #[test]
        fn clone_creates_copy() {
            let original = CodexExecutor;
            let cloned = original; // Copy, since it's Copy

            assert_eq!(original.name(), cloned.name());
            assert_eq!(original.command(), cloned.command());
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let executor = CodexExecutor;
            let debug_str = format!("{executor:?}");

            assert!(debug_str.contains("CodexExecutor"));
        }
    }

    // =========================================================================
    // ClaudeExecutor Tests
    // =========================================================================

    mod claude_executor {
        use super::*;

        /// Tests that `ClaudeExecutor` returns correct name.
        #[test]
        fn name_returns_claude_code() {
            let executor = ClaudeExecutor;
            assert_eq!(executor.name(), "Claude Code");
        }

        /// Tests that `ClaudeExecutor` returns correct command.
        #[test]
        fn command_returns_claude() {
            let executor = ClaudeExecutor;
            assert_eq!(executor.command(), "claude");
        }

        /// Tests that `ClaudeExecutor` implements `Default`.
        #[test]
        fn default_creates_instance() {
            let executor = ClaudeExecutor;
            assert_eq!(executor.name(), "Claude Code");
        }

        /// Tests that `ClaudeExecutor` can be copied.
        #[test]
        fn copy_creates_identical_instance() {
            let original = ClaudeExecutor;
            let copied = original;

            assert_eq!(original.name(), copied.name());
            assert_eq!(original.command(), copied.command());
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let executor = ClaudeExecutor;
            let debug_str = format!("{executor:?}");

            assert!(debug_str.contains("ClaudeExecutor"));
        }
    }

    // =========================================================================
    // GeminiExecutor Tests
    // =========================================================================

    mod gemini_executor {
        use super::*;

        /// Tests that `GeminiExecutor` returns correct name.
        #[test]
        fn name_returns_gemini() {
            let executor = GeminiExecutor;
            assert_eq!(executor.name(), "Gemini");
        }

        /// Tests that `GeminiExecutor` returns correct command.
        #[test]
        fn command_returns_gemini() {
            let executor = GeminiExecutor;
            assert_eq!(executor.command(), "gemini");
        }

        /// Tests that `GeminiExecutor` implements `Default`.
        #[test]
        fn default_creates_instance() {
            let executor = GeminiExecutor;
            assert_eq!(executor.name(), "Gemini");
        }

        /// Tests that `GeminiExecutor` can be copied.
        #[test]
        fn copy_creates_identical_instance() {
            let original = GeminiExecutor;
            let copied = original;

            assert_eq!(original.name(), copied.name());
            assert_eq!(original.command(), copied.command());
        }

        /// Tests Debug trait implementation.
        #[test]
        fn debug_format_is_readable() {
            let executor = GeminiExecutor;
            let debug_str = format!("{executor:?}");

            assert!(debug_str.contains("GeminiExecutor"));
        }
    }

    // =========================================================================
    // Trait Object Tests
    // =========================================================================

    mod trait_object {
        use super::*;

        /// Tests that executors can be used as trait objects.
        #[test]
        fn executors_work_as_trait_objects() {
            let executors: Vec<Box<dyn AiCliExecutor>> = vec![
                Box::new(CodexExecutor),
                Box::new(ClaudeExecutor),
                Box::new(GeminiExecutor),
            ];

            assert_eq!(executors[0].name(), "Codex");
            assert_eq!(executors[0].command(), "codex");
            assert_eq!(executors[1].name(), "Claude Code");
            assert_eq!(executors[1].command(), "claude");
            assert_eq!(executors[2].name(), "Gemini");
            assert_eq!(executors[2].command(), "gemini");
        }

        /// Helper to get executor name via trait object.
        fn get_name(executor: &dyn AiCliExecutor) -> &'static str {
            executor.name()
        }

        /// Tests that trait object references work correctly.
        #[test]
        fn trait_object_references() {
            let codex = CodexExecutor;
            let claude = ClaudeExecutor;
            let gemini = GeminiExecutor;

            assert_eq!(get_name(&codex), "Codex");
            assert_eq!(get_name(&claude), "Claude Code");
            assert_eq!(get_name(&gemini), "Gemini");
        }
    }

    // =========================================================================
    // Async Tests (using tokio::test)
    // =========================================================================

    mod async_tests {
        use super::*;

        /// Tests `check_cli_available` with a command that should exist (sh).
        #[test]
        fn check_cli_available_finds_sh() {
            // 'sh' should exist on all Unix systems
            let result = check_cli_available("sh");
            assert!(result);
        }

        /// Tests `check_cli_available` with a non-existent command.
        #[test]
        fn check_cli_available_nonexistent_returns_false() {
            let result = check_cli_available("this_command_definitely_does_not_exist_12345");
            assert!(!result);
        }

        /// Tests `wait_for_shutdown` returns immediately when already signaled.
        #[tokio::test]
        async fn wait_for_shutdown_returns_when_already_true() -> anyhow::Result<()> {
            let (tx, mut rx) = watch::channel(true); // Start with true

            // Should return immediately since already signaled
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                wait_for_shutdown(&mut rx),
            )
            .await
            .map_err(|_| anyhow::anyhow!("wait_for_shutdown should have returned immediately"))?;

            drop(tx); // Prevent unused warning
            Ok(())
        }

        /// Tests `wait_for_shutdown` returns when channel is closed.
        #[tokio::test]
        async fn wait_for_shutdown_returns_on_channel_close() -> anyhow::Result<()> {
            let (tx, mut rx) = watch::channel(false);

            // Drop the sender to close the channel
            drop(tx);

            // Should return because channel closed
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                wait_for_shutdown(&mut rx),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!("wait_for_shutdown should have returned on channel close")
            })?;
            Ok(())
        }

        /// Tests `wait_for_shutdown` returns when signal is sent.
        #[tokio::test]
        async fn wait_for_shutdown_returns_on_signal() -> anyhow::Result<()> {
            let (tx, mut rx) = watch::channel(false);

            // Spawn a task to send the signal after a short delay
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let _ = tx.send(true);
            });

            // Should return when signal is sent
            tokio::time::timeout(
                std::time::Duration::from_millis(200),
                wait_for_shutdown(&mut rx),
            )
            .await
            .map_err(|_| anyhow::anyhow!("wait_for_shutdown should have returned on signal"))?;
            Ok(())
        }
    }
}
