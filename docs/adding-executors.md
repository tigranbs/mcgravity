# Adding New AI CLI Executors

This guide explains how to add support for new AI CLI tools (e.g., Aider, Cursor, etc.) to McGravity.

## Currently Supported Executors

McGravity currently supports three AI CLI tools:

- **Codex** (`codex`) - OpenAI Codex CLI
- **Claude Code** (`claude`) - Anthropic Claude CLI
- **Gemini** (`gemini`) - Google Gemini CLI

## Overview

McGravity uses a trait-based abstraction for AI CLI executors. Each executor implements the `AiCliExecutor` trait, which provides a uniform interface for:

- Executing CLI commands with streaming output
- Identifying the executor by name
- Checking availability in PATH

## Quick Start

Adding a new executor requires changes to only **2 files**:

1. `src/core/executor.rs` - Define the executor struct and implement the trait
2. `src/core/mod.rs` - Add the model to the `Model` enum

## Step-by-Step Guide

### Step 1: Define the Executor Struct

In `src/core/executor.rs`, add your new executor struct:

```rust
/// Aider CLI executor.
///
/// Executes: `aider --yes <text>`
#[derive(Debug, Clone, Copy, Default)]
pub struct AiderExecutor;
```

### Step 2: Implement the `AiCliExecutor` Trait

```rust
#[async_trait]
impl AiCliExecutor for AiderExecutor {
    async fn execute(
        &self,
        input: &str,
        output_tx: mpsc::Sender<CliOutput>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<ExitStatus> {
        run_cli_with_output(
            self.command(),
            &["--yes", input],  // Adjust args for your CLI
            output_tx,
            shutdown_rx,
        )
        .await
    }

    fn name(&self) -> &'static str {
        "Aider"  // Display name shown in UI
    }

    fn command(&self) -> &'static str {
        "aider"  // Actual CLI command name
    }

    // Optional: Override is_available() if you need custom availability check
    // The default implementation uses `which <command>` on Linux
}
```

### Step 3: Add to the Model Enum

In `src/core/mod.rs`:

```rust
/// Available AI CLI models for orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Model {
    #[default]
    Codex,
    Claude,
    Gemini,
    Aider,  // Add new variant
}

impl Model {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Gemini => "Gemini",
            Self::Aider => "Aider",  // Add display name
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            Self::Codex => "OpenAI Codex CLI",
            Self::Claude => "Anthropic Claude CLI",
            Self::Gemini => "Google Gemini CLI",
            Self::Aider => "Aider CLI",  // Add description
        }
    }

    pub fn executor(&self) -> Box<dyn AiCliExecutor> {
        match self {
            Self::Codex => Box::new(CodexExecutor),
            Self::Claude => Box::new(ClaudeExecutor),
            Self::Gemini => Box::new(GeminiExecutor),
            Self::Aider => Box::new(AiderExecutor),  // Add factory
        }
    }
}
```

### Step 4: Export the Executor (Optional)

If you want the executor to be publicly accessible:

```rust
// In src/core/mod.rs
pub use executor::{AiCliExecutor, AiderExecutor, ClaudeExecutor, CliOutput, CodexExecutor, GeminiExecutor};
```

## Trait Reference

```rust
#[async_trait]
pub trait AiCliExecutor: Send + Sync {
    /// Executes the CLI with the given input text, streaming output.
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

    /// Checks if this executor's CLI tool is available in PATH.
    /// Default implementation uses `which <command>`.
    async fn is_available(&self) -> bool {
        check_cli_available(self.command()).await.unwrap_or(false)
    }
}
```

## CLI Output Handling

The `output_tx` channel streams CLI output to the UI:

```rust
pub enum CliOutput {
    Stdout(String),  // Standard output line
    Stderr(String),  // Standard error line
}
```

Lines are automatically split and displayed in the TUI with appropriate styling (stderr appears in yellow/warning color).

## Shutdown Handling

The `shutdown_rx` channel signals when the user wants to quit. Your executor should pass this to `run_cli_with_output()`, which will:

1. Kill the child process if shutdown is signaled
2. Clean up stdout/stderr reader tasks
3. Return an error indicating shutdown

## Testing Your Executor

1. Ensure the CLI tool is installed and in PATH
2. Run the application: `cargo run`
3. Select your new model in the model selection screen
4. Verify output streaming works correctly

## Reference: Implemented Executors

The following executors are already implemented in `src/core/executor.rs`:

### GeminiExecutor (Actual Implementation)

```rust
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
```

### CodexExecutor

```rust
/// OpenAI Codex CLI executor.
///
/// Executes: `codex exec --dangerously-bypass-approvals-and-sandbox <text>`
#[derive(Debug, Clone, Copy, Default)]
pub struct CodexExecutor;
```

### ClaudeExecutor

```rust
/// Anthropic Claude Code CLI executor.
///
/// Executes: `claude -p <text> --dangerously-skip-permissions --output-format stream-json --verbose`
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeExecutor;
```

## Notes

- The `#[derive(Debug, Clone, Copy, Default)]` is recommended for zero-sized executor structs
- Use `&'static str` for `name()` and `command()` to avoid allocations
- The `run_cli_with_output()` helper handles process spawning, output streaming, and cleanup
- On Linux, child processes are automatically killed when the parent dies (via `PR_SET_PDEATHSIG`)
