# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Documentation

- **[docs/architecture.md](docs/architecture.md)** - Complete system architecture, module structure, data flow, and design patterns.
- **[docs/adding-executors.md](docs/adding-executors.md)** - Step-by-step guide for adding new AI CLI tool support.

## Extended Guidelines

For comprehensive best practices, reference these files in `.cursor/rules/`:

- **`ratatui.mdc`** - Complete Ratatui 0.30.0 field guide covering widgets, layouts, styling, testing, and immediate-mode architecture. Read this when working on UI components, layouts, or widget implementations.

- **`rust.mdc`** - Comprehensive Rust 2024 edition best practices covering code organization, error handling, performance, security, and testing. Read this when making architectural decisions, handling errors, or optimizing code.

## Project Overview

**McGravity** is a TUI-based interface for orchestrating AI-assisted coding tools (Claude Code, OpenAI Codex CLI, Gemini CLI, etc.). It provides a unified workflow called "McGravity Flow" that helps software engineers:

- Compose better-structured tasks for AI coding assistants
- Execute tasks across multiple AI CLI tools with consistent patterns
- Track completed tasks and provide context for planning next steps
- Manage file system operations with proper sandboxing and approval flows
- Review and approve AI-generated changes before application

The project is inspired by OpenAI's Codex CLI architecture, which uses a modular Rust workspace design with separated concerns for core logic, TUI presentation, and file system operations.

## Build Commands

```bash
cargo build          # Build the project
cargo run            # Run the application
cargo test           # Run tests
cargo test <name>    # Run a specific test
cargo clippy         # Run linter
cargo fmt            # Format code
```

## Local Installation

To install mcgravity locally to `~/.cargo/bin/` (making it available system-wide):

```bash
cargo install --path . --force
```

- The `--force` (`-f`) flag ensures the binary is rebuilt and replaced even if already installed
- After installation, run from anywhere with: `mcgravity`
- To uninstall: `cargo uninstall mcgravity`

**When the user asks to "install mcgravity locally" or "install this package", run:**

```bash
cargo install --path . --force
```

## Mandatory Quality Checks

**IMPORTANT: After every code change, run the full validation pipeline:**

```bash
cargo fmt && cargo clippy && cargo build && cargo test
```

This ensures:

1. **Formatting** (`cargo fmt`) - Consistent code style
2. **Linting** (`cargo clippy`) - Catches common mistakes and enforces best practices
3. **Build** (`cargo build`) - Verifies compilation succeeds
4. **Tests** (`cargo test`) - Ensures no regressions

### CI Pipeline Compliance

**CRITICAL: All changes must pass the CI pipeline defined in [`.github/workflows/ci.yaml`](.github/workflows/ci.yaml).**

Before considering any change complete, verify it will pass CI by running the exact checks from the workflow:

```bash
# Format check (must pass with no changes needed)
cargo fmt --all -- --check

# Clippy with strict warnings-as-errors
cargo clippy --all-targets --all-features -- -D warnings

# Build and test with all features and locked dependencies
cargo build --locked --all-features
cargo test --locked --all-features
```

The CI runs on every PR and push to main. A change is not complete until it passes all CI checks.

### Clippy Guidelines

Run clippy with strict settings for maximum code quality:

```bash
# Standard check
cargo clippy

# Strict mode - treat warnings as errors
cargo clippy -- -D warnings

# With additional lints for pedantic checks
cargo clippy -- -W clippy::pedantic -W clippy::nursery
```

#### Clippy Best Practices

- **Fix all warnings**: Never ignore clippy warnings; they often indicate real issues
- **Use `#[allow(...)]` sparingly**: Only suppress warnings with a comment explaining why
- **Common lints to watch for**:
  - `clippy::unwrap_used` - Prefer `expect()` with context or proper error handling
  - `clippy::clone_on_ref_ptr` - Avoid unnecessary clones
  - `clippy::large_enum_variant` - Box large enum variants
  - `clippy::needless_pass_by_value` - Use references when ownership isn't needed
  - `clippy::missing_errors_doc` - Document error conditions in public APIs
  - `clippy::missing_panics_doc` - Document panic conditions

## Project Structure

```
mcgravity/
├── CLAUDE.md                    # This file - AI assistant guidance
├── Cargo.toml                   # Project manifest
├── Cargo.lock                   # Dependency lock file
│
├── docs/                        # Project documentation
│   ├── architecture.md          # System architecture and design patterns
│   └── adding-executors.md      # Guide for adding new AI CLI tools
│
├── .cursor/rules/               # IDE-specific guidelines
│   ├── ratatui.mdc              # Ratatui 0.30.0 best practices
│   └── rust.mdc                 # Rust 2024 edition best practices
│
├── src/
│   ├── main.rs                  # Entry point, terminal setup, event loop
│   ├── lib.rs                   # Library exports for all modules
│   ├── cli.rs                   # CLI argument parsing (clap)
│   ├── file_search.rs           # Fuzzy file path search for @ mentions
│   │
│   ├── app/                     # Application state and UI logic
│   │   ├── mod.rs               # App struct, state management, file search
│   │   ├── events.rs            # Key event handling (Chat, Settings modes)
│   │   ├── input.rs             # Text input handling (cursor, editing, @ token detection)
│   │   ├── layout.rs            # Layout calculations (ChatLayout)
│   │   ├── render.rs            # UI rendering (chat mode, settings overlay)
│   │   ├── state.rs             # State structures (AppMode, FlowEvent, etc.)
│   │   └── tests.rs             # Application tests
│   │
│   ├── core/                    # Business logic (model-agnostic)
│   │   ├── mod.rs               # Model enum, public exports
│   │   ├── executor.rs          # AiCliExecutor trait and implementations
│   │   ├── flow.rs              # FlowPhase enum, FlowState struct
│   │   ├── prompts.rs           # Planning/execution prompt templates
│   │   ├── retry.rs             # RetryConfig for backoff logic
│   │   └── runner.rs            # Flow orchestration, generic retry wrapper
│   │
│   ├── fs/                      # File system operations
│   │   ├── mod.rs               # Module exports
│   │   ├── settings.rs          # Settings persistence to .mcgravity/settings.json
│   │   └── todo.rs              # Todo file scanning, reading, moving
│   │
│   └── tui/                     # TUI presentation layer
│       ├── mod.rs               # Module exports
│       ├── theme.rs             # Centralized color/style definitions
│       └── widgets/             # Custom Ratatui widgets
│           ├── mod.rs           # Widget exports
│           ├── file_popup.rs    # File suggestion popup for @ mentions
│           ├── output.rs        # CLI output viewer widget
│           └── status_indicator.rs  # Compact status indicator (2-line)
│
└── .mcgravity/                  # Runtime: mcgravity configuration and state
    ├── settings.json            # Persisted user settings
    ├── task.md                  # Current task description
    └── todo/                    # Task files created by planning phase
        └── done/                # Completed tasks (auto-archived)
```

## Core Design Principles

1. **Separation of Concerns**: Keep core business logic independent from TUI presentation. Core logic should be testable without terminal setup.

2. **State-Render Split**: Following immediate-mode patterns, the App owns all state. Render functions read state without mutation.

3. **Trait-Based Abstraction**: All AI CLIs implement `AiCliExecutor` trait for uniform execution. See [docs/adding-executors.md](docs/adding-executors.md).

4. **Generic Flow Logic**: The flow runner is model-agnostic - it works with any `AiCliExecutor` implementation without modification.

## Key Modules

### `core/executor.rs` - CLI Executor Trait

```rust
#[async_trait]
pub trait AiCliExecutor: Send + Sync {
    async fn execute(&self, input: &str, output_tx: Sender<CliOutput>,
                     shutdown_rx: Receiver<bool>) -> Result<ExitStatus>;
    fn name(&self) -> &'static str;
    fn command(&self) -> &'static str;
    async fn is_available(&self) -> bool;
}
```

Implementations: `CodexExecutor`, `ClaudeExecutor`, `GeminiExecutor`

### `core/flow.rs` - Flow State Machine

```rust
pub enum FlowPhase {
    Idle,
    ReadingInput,
    CheckingDoneFiles,                                           // Scan for completed tasks (context for planning)
    RunningPlanning { model_name: Cow<'static, str>, attempt: u32 },
    CheckingTodoFiles,
    ProcessingTodos { current: usize, total: usize },
    RunningExecution { model_name: Cow<'static, str>, file_index: usize, attempt: u32 },
    CycleComplete { iteration: u32 },
    MovingCompletedFiles,
    Completed,
    Failed { reason: String },
    NoTodoFiles,
}
```

### `core/runner.rs` - Flow Orchestration

- `run_flow()` - Main orchestration loop (async task)
- `run_with_retry()` - Generic retry wrapper for any executor

### `app/mod.rs` - Application State

- `App` struct with UI state, event channels, shutdown signal
- `FlowEvent` enum for async communication
- Key event handling per mode (Chat, Settings)

## Adding New AI CLI Tools

To add support for a new AI CLI (e.g., Aider, Cursor):

1. Create executor struct in `src/core/executor.rs`
2. Implement `AiCliExecutor` trait
3. Add to `Model` enum in `src/core/mod.rs`
4. Add factory arm in `Model::executor()`

**See [docs/adding-executors.md](docs/adding-executors.md) for the complete guide.**

## Ratatui TUI Patterns

This project follows immediate-mode TUI patterns where the app owns all state and fully repaints the UI each frame.

### Key Patterns

- Use `ratatui::init()` / `ratatui::restore()` for terminal initialization
- Always use `frame.area()` for dimensions during render
- Store UI state explicitly (selection indices, scroll offsets, input buffers)
- Render the entire frame every draw call (diffing happens automatically)
- Wrap widgets in `Block::bordered()` for consistent framing
- Use `Clear` widget before rendering popups/modals

### Layout

- Use `Layout` with `Constraint`s (Length, Min, Max, Percentage, Ratio, Fill) to split areas
- Coordinates are `u16`, origin at top-left

### Styling

- Define styles in `Theme` struct (`src/tui/theme.rs`)
- Use the `Stylize` trait for fluent styling: `"text".red().bold()`

## Key Bindings

### Global

- `Ctrl+S` - Open settings panel
- `Ctrl+C` - Quit application
- `Esc` - Quit application

### Chat Mode (Unified Interface)

**Text Input:**

- `Enter` - Submit task OR insert newline (configurable in Settings)
- `Shift+Enter` - Insert newline (standard behavior)
- `Alt+Enter` - Insert newline (alternative for terminal compatibility)
- `Ctrl+J` - Insert newline (universal - works on ALL terminals)
- `\` + `Enter` - Insert newline (backslash-Enter escape sequence)
- `Ctrl+Enter` - Submit task (always submits)
- `Ctrl+D` - Submit task (alternative)

**Navigation:**

- Arrow keys - Navigate cursor in input
- `Ctrl+Arrow` - Scroll output panel
- `PageUp/PageDown` - Page scroll output
- `@` - Trigger file path autocomplete
- `/` - Trigger slash command autocomplete (at line start)

**Enter Key Behavior:**

McGravity supports two input styles (configurable in Settings via Ctrl+S):

1. **Submit (Default)**: Enter submits, Shift+Enter inserts newline. Best for quick commands.
2. **Newline (Editor)**: Enter inserts newline, Ctrl+Enter submits. Best for writing longer prompts.

**Newline Input Methods:**

McGravity supports multiple ways to insert newlines to ensure compatibility across all terminals:

| Method          | How                               | Works On                               |
| --------------- | --------------------------------- | -------------------------------------- |
| **Ctrl+J**      | Press Ctrl+J                      | All terminals (recommended)            |
| **Alt+Enter**   | Press Alt+Enter                   | Most terminals                         |
| **Shift+Enter** | Press Shift+Enter                 | Terminals with modifier support        |
| **\\+Enter**    | Type backslash, then Enter        | All terminals                          |
| **Settings**    | Set "Enter Behavior" to "Newline" | All terminals (changes Enter behavior) |

**Note**: iPad keyboards and some terminal emulators (macOS Terminal.app, older Linux terminals)
don't report Shift+Enter correctly. Use `Ctrl+J` or the backslash method for guaranteed compatibility.

**Paste Behavior:**

Multi-line paste is handled correctly through bracketed paste mode. When supported by the
terminal, pasted text is received as a single event with newlines inserted directly.

When bracketed paste mode is NOT supported by the terminal:

- Pasted text arrives as individual key events
- Rapid input detection serves as a fallback (see Technical Notes below)
- The app detects paste operations by timing: 3+ keys within 150ms triggers rapid mode

### Settings Mode

- `Up/Down` or `j/k` - Navigate settings
- `Ctrl+P/Ctrl+N` - Navigate settings (Emacs-style)
- `Enter` or `Space` - Cycle current setting value
- `Esc` or `q` - Close settings (auto-saves)

### Slash Commands

Type `/` at the start of the input to see available commands:

- `/exit` - Exit the application gracefully
- `/settings` - Open the settings panel (equivalent to Ctrl+S)
- `/clear` - Clear task text, output, and todo files (does not reset settings)

When the command popup is visible:

- `Up/Down` or `j/k` - Navigate suggestions
- `Tab` - Insert selected command (allows adding arguments)
- `Enter` - Insert and execute selected command
- `Esc` - Dismiss popup without selecting

## @ File Tagging in Text Input

When entering task descriptions, you can reference files using `@` mentions:

- Type `@` followed by part of a filename to search for files
- Use **Up/Down** or **j/k** to navigate suggestions
- Press **Tab** or **Enter** to insert the file path
- Press **Esc** to dismiss without selecting

Features:

- Fuzzy matching powered by `nucleo-matcher`
- Respects `.gitignore` (won't suggest ignored files)
- Paths with spaces are automatically quoted
- Email patterns like `user@domain.com` don't trigger suggestions

## Error Handling

- Use `anyhow` for application errors with context
- Use `thiserror` for library-style error types
- Propagate errors with `?` operator
- Display user-friendly error messages in the TUI

## Testing

- Use `TestBackend::assert_buffer_lines()` with inline expected buffers for TUI regression tests
- Render UI tests via `render_app_to_terminal` in `src/app/tests/helpers.rs`, then assert lines on the backend
- This approach avoids committed snapshot files, keeps CI deterministic, and mirrors React/Jest-style expectations
- Test core logic independently from TUI layer
- Use integration tests for full workflow validation

```bash
cargo test                    # Run all tests
cargo test test_name          # Run specific test
cargo test -- --nocapture     # Show println! output
```

## Troubleshooting

### Text Input Issues

**Problem**: How do I submit my task?
**Solution**: Press `Enter` to submit. This works like most chat applications.

**Problem**: How do I create multi-line task descriptions?
**Solution**: Multiple methods available for inserting newlines:

- `Ctrl+J` - Works on ALL terminals (recommended)
- `Shift+Enter` - Standard method (may not work on iPad or some terminals)
- `Alt+Enter` - Alternative for terminals with Shift+Enter issues
- Type `\` then `Enter` - Backslash escape sequence (works everywhere)
  When done writing, press `Enter` to submit.

**Problem**: Special keys aren't working as expected
**Solution**: Some terminal emulators may not correctly report certain key combinations. Try:

- `MCGRAVITY_DEBUG_KEYS=1 mcgravity` to see what keys your terminal reports
- Use `j/k` as alternatives to Arrow keys for navigation

**Problem**: Paste is causing unexpected behavior
**Solution**: Verify bracketed paste mode is working:

```bash
MCGRAVITY_DEBUG_KEYS=1 mcgravity
```

Look for `[DEBUG PASTE]` events when pasting. If you see `[DEBUG KEY]` events instead,
your terminal may not support bracketed paste. The app falls back to rapid input detection.

### Slash Commands

**Problem**: Slash commands don't work
**Solution**: Ensure you type `/` at the very start of a line (not after other text).
Commands are only recognized when they're the entire input.

## Technical Notes

### Key Binding Design

McGravity supports configurable Enter key behavior to accommodate different workflows:

- **Chat Style (Default)**: `Enter` = submit, `Shift+Enter` = newline
- **Editor Style**: `Enter` = newline, `Ctrl+Enter` = submit

### Universal Newline Methods

Many terminals (macOS Terminal.app, iPad SSH clients, older Linux terminals) do NOT report
the Shift modifier when `Shift+Enter` is pressed. This is a fundamental limitation of
terminal emulators, documented in crossterm GitHub issues #861, #685, #460.

To ensure newline insertion works everywhere, McGravity provides:

1. **Ctrl+J** - ASCII 10 (LF). Works on ALL terminals because it's a control character,
   not a modifier+key combination. This is the recommended method for maximum compatibility.

2. **Backslash-Enter** - Type `\` followed by `Enter`. The backslash is removed and a
   newline is inserted. Provides visual feedback and works on any terminal.

3. **Alt+Enter** - Works on most terminals (more reliable than Shift+Enter).

4. **Settings toggle** - Set "Enter Behavior" to "Newline" mode for document-style editing.

**Note**: `Ctrl+J` may conflict with tmux default pane navigation (`Ctrl+B` then `j`).
Users can use backslash-Enter instead or remap their tmux bindings.

### Rapid Input Detection (Paste Fallback)

When bracketed paste mode is not supported by the terminal, McGravity uses rapid input
detection as a fallback to identify paste operations:

- **Threshold**: 3 keys arriving within 150ms of each other
- **Reset**: After 500ms without rapid keys
- **Effect**: Tracked for potential future use cases

This mechanism helps identify paste operations on terminals without bracketed paste support.
Rapid input detection is currently tracked but not used for Enter key behavior - the paste
handler bypasses key handling entirely by directly manipulating the text buffer.

### Debug Mode

Run with `MCGRAVITY_DEBUG_KEYS=1` to see detailed key event information:

```bash
MCGRAVITY_DEBUG_KEYS=1 mcgravity
```

Debug output includes:

- `[DEBUG INIT]` - Startup information (bracketed paste mode status)
- `[DEBUG KEY]` - Individual key events with timing
- `[DEBUG ENTER]` - Enter key handling decisions
- `[DEBUG PASTE]` - Bracketed paste events
- `[DEBUG RAPID]` - Rapid input detection state changes

Example debug output:

```
[DEBUG INIT] Bracketed paste mode ENABLED
[DEBUG KEY] code=Enter modifiers=SHIFT kind=Press elapsed_ms=150
[DEBUG ENTER] modifiers=SHIFT shift=true ctrl=false alt=false in_rapid=false input_len=10 input_not_empty=true action=newline (Shift+Enter or Alt+Enter)
[DEBUG KEY] code=Enter modifiers=NONE kind=Press elapsed_ms=200
[DEBUG ENTER] modifiers=NONE shift=false ctrl=false alt=false in_rapid=false input_len=12 input_not_empty=true action=submit (Enter)
[DEBUG PASTE] len=20 lines=3 trailing_newline=false first_chars="Line 1\nLine 2\nLin"
```

For paste operations without bracketed paste support:

```
[DEBUG KEY] code=Char('a') modifiers=NONE kind=Press elapsed_ms=5
[DEBUG RAPID] ACTIVATED: count 2 -> 3 (threshold=3)
[DEBUG KEY] code=Enter modifiers=NONE kind=Press elapsed_ms=3
[DEBUG ENTER] modifiers=NONE shift=false ctrl=false alt=false in_rapid=true input_len=15 input_not_empty=true action=newline (rapid input - paste detected)
```
