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

## CLI Availability Resolution Strategy

This section documents the cross-shell strategy for resolving CLI commands (codex, claude, gemini) that accounts for PATH, shell aliases/functions, and executability without introducing shell injection risk.

### Problem Statement

The basic `which`/`where` approach only searches PATH and misses commands that are available via:
- Shell aliases (e.g., `alias claude='/usr/local/bin/claude-cli'`)
- Shell functions (e.g., `claude() { /path/to/claude "$@"; }`)
- PATH modifications in shell profiles (e.g., `~/.zshrc`, `~/.bashrc`)

This is particularly common on macOS where tools installed via Homebrew or npm may only be accessible after shell profile initialization.

### Resolution Algorithm

The CLI resolution follows this priority order:

1. **Direct PATH lookup** (fast path): Check if the command exists directly in PATH using `which`/`where`. If found and executable, use it.

2. **Shell-based resolution** (fallback): If not found in PATH, invoke the user's shell to resolve the command:
   ```
   $SHELL -l -i -c "command -v <cmd>"
   ```
   - `-l`: Login shell (loads `~/.profile`, `~/.bash_profile`, `~/.zprofile`)
   - `-i`: Interactive shell (loads `~/.bashrc`, `~/.zshrc`)
   - `command -v`: POSIX-compliant way to resolve commands (preferred over `type` or `which`)

3. **Output classification**: Parse the output of `command -v` to determine the command type:
   - **Path** (e.g., `/usr/local/bin/claude`): Direct executable in PATH
   - **Alias** (e.g., `alias claude='...'`): Shell alias definition
   - **Function** (e.g., `claude is a function`): Shell function
   - **Builtin** (e.g., `builtin`): Shell builtin command
   - **Not found** (empty output or error): Command unavailable

### Shell Invocation Behavior by Shell Type

| Shell | Login Profile | Interactive Profile | `command -v` Support |
|-------|---------------|---------------------|----------------------|
| bash  | `~/.bash_profile` or `~/.profile` | `~/.bashrc` | Yes (POSIX) |
| zsh   | `~/.zprofile` | `~/.zshrc` | Yes (POSIX) |
| sh    | `~/.profile` | - | Yes (POSIX) |
| fish  | `~/.config/fish/config.fish` | Same | `type -p` instead |

**Note**: For `fish` shell, use `type -p <cmd>` instead of `command -v` since fish doesn't support POSIX `command -v` syntax.

### Executability Verification

After resolving the command path, verify executability:

1. **For PATH executables**: Check that the resolved path:
   - Exists as a regular file (not directory or symlink to directory)
   - Has execute permission for the current user

2. **For aliases/functions**: The command is considered available but cannot be verified for executability until actual invocation. Mark as "available via shell".

3. **For builtins**: Shell builtins are always executable within their shell context.

### Classification Rules

Commands are classified into these categories for UI display and execution strategy:

| Classification | Resolution Output | Execution Method | UI Indicator |
|---------------|-------------------|------------------|--------------|
| `PathExecutable` | Absolute path | `Command::new(path)` | ✓ Available |
| `ShellAlias` | `alias name='...'` | Shell invocation required | ⚡ Available (alias) |
| `ShellFunction` | Function definition | Shell invocation required | ⚡ Available (function) |
| `ShellBuiltin` | `builtin` | Shell invocation required | ⚡ Available (builtin) |
| `NotFound` | Empty/error | N/A | ✗ Not found |

### Security Considerations

**Command Name Validation** (Critical):
- Command names MUST be validated before shell invocation
- Allow only: alphanumeric characters, hyphens (`-`), underscores (`_`)
- Reject any command containing: spaces, quotes, semicolons, pipes, redirects, backticks, `$`, etc.
- Maximum length: 64 characters

**Safe validation regex**: `^[a-zA-Z0-9_-]{1,64}$`

**Shell Injection Prevention**:
- Never interpolate untrusted input into shell commands
- Use the validated command name directly with `command -v`
- Do not use shell expansion or eval

**Example safe invocation**:
```rust
// SAFE: command_name is pre-validated to match ^[a-zA-Z0-9_-]{1,64}$
let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
Command::new(&shell)
    .args(["-l", "-i", "-c", &format!("command -v {}", command_name)])
    .output()
```

### Execution Strategy

Based on the classification, choose the execution method:

1. **PathExecutable**: Execute directly with `Command::new(resolved_path)`
   - Most reliable and fastest
   - Inherits environment from McGravity process

2. **ShellAlias/ShellFunction/ShellBuiltin**: Execute via shell wrapper
   ```rust
   Command::new(&shell)
       .args(["-l", "-i", "-c", &format!("{} {}", command_name, escaped_args)])
   ```
   - Required for alias/function resolution
   - Slower due to shell startup overhead
   - Args must be properly escaped for shell

### Alignment with Trait-Based Executor Abstraction

Per `CLAUDE.md` "Trait-Based Executor Abstraction" and `docs/architecture.md`:

- The `AiCliExecutor::is_available()` method should use the resolution algorithm above
- The `AiCliExecutor::command()` returns the command name (e.g., "claude")
- Execution via `execute()` should use the appropriate execution strategy based on classification
- The resolution result can be cached at startup to avoid repeated shell invocations

### CI Pipeline Compliance

Per `.github/workflows/ci.yaml`, the implementation must:
- Pass `cargo fmt --all -- --check`
- Pass `cargo clippy --all-targets --all-features -- -D warnings`
- Pass `cargo build --locked --all-features`
- Pass `cargo test --locked --all-features`

The shell-based resolution should be implemented behind a feature flag or as a fallback to maintain fast CI execution where shell environment is minimal.
