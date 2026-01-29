# McGravity Architecture

This document describes the architecture and design patterns used in McGravity.

## High-Level Overview

McGravity is a TUI application that orchestrates AI CLI tools through a multi-phase workflow with verification:

```
┌───────────────────────────────────────────────────────────────────────┐
│                          McGravity Flow                               │
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐             │
│  │  Scan Done   │───▶│   Verify     │───▶│   Cleanup    │             │
│  │    Files     │    │  Completed   │    │  Done Files  │             │
│  └──────────────┘    └──────────────┘    └──────────────┘             │
│         │                                        │                    │
│         │ (no done files)                        ▼                    │
│         └──────────────────────────────▶┌──────────────┐              │
│                                         │   Planning   │              │
│                                         │    Phase     │              │
│                                         └──────────────┘              │
│                                                │                      │
│                                                ▼                      │
│                                         ┌──────────────┐              │
│                                         │  Todo Files  │              │
│                                         │   Created    │              │
│                                         └──────────────┘              │
│                                                │                      │
│                                                ▼                      │
│                                         ┌──────────────┐              │
│                                         │  Execution   │              │
│                                         │    Phase     │              │
│                                         └──────────────┘              │
│                                                │                      │
│                                                ▼                      │
│                                         ┌──────────────┐              │
│         ┌───────────────────────────────│  Move to     │              │
│         │                               │  todo/done/  │              │
│         │                               └──────────────┘              │
│         │                                                             │
│         └─────────────(Cycle until no todo files)────────────────────▶│
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

### Flow Sequence

1. **Scan Done Files**: Check `todo/done/` for completed tasks from previous cycles
2. **Verify Completed**: If done files exist, verify them against the original plan using the planning AI model
3. **Cleanup Done Files**: Remove verified done files to prevent re-verification
4. **Planning Phase**: AI breaks down the task into actionable steps (creates `todo/*.md` files)
5. **Execution Phase**: AI completes each todo file task
6. **Move Completed**: Completed tasks are moved to `todo/done/`
7. **Repeat**: Cycle continues until no todo files remain

## Module Structure

```
src/
├── main.rs              # Entry point, terminal setup, event loop
├── lib.rs               # Library exports for all modules
├── cli.rs               # CLI argument parsing (clap)
├── file_search.rs       # Fuzzy file path search for @ mentions
│
├── app/                 # Application state and UI logic
│   ├── mod.rs           # App struct, state management, file search
│   ├── events.rs        # Key event handling (Chat, Settings modes)
│   ├── input.rs         # Text input handling (cursor, editing, @ token detection)
│   ├── layout.rs        # Layout calculations (ChatLayout)
│   ├── render.rs        # UI rendering (chat mode, settings overlay)
│   ├── state.rs         # State structures (AppMode, FlowEvent, etc.)
│   └── tests.rs         # Application tests
│
├── core/                # Business logic (model-agnostic)
│   ├── mod.rs           # Model enum, public exports
│   ├── executor.rs      # AiCliExecutor trait and implementations
│   ├── flow.rs          # FlowPhase enum, FlowState struct
│   ├── prompts.rs       # Planning/execution prompt templates
│   ├── retry.rs         # RetryConfig for backoff logic
│   └── runner.rs        # Flow orchestration, generic retry wrapper
│
├── fs/                  # File system operations
│   ├── mod.rs           # Module exports
│   └── todo.rs          # Todo file scanning, reading, moving
│
└── tui/                 # TUI presentation layer
    ├── mod.rs           # Module exports
    ├── theme.rs         # Centralized color/style definitions
    └── widgets/         # Custom Ratatui widgets
        ├── mod.rs       # Widget exports
        ├── file_popup.rs # File suggestion popup for @ mentions
        ├── output.rs    # CLI output viewer widget
        └── status_indicator.rs  # Compact status indicator (2-line)
```

## Core Design Patterns

### 1. Trait-Based Executor Abstraction

All AI CLI tools implement a common trait:

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

Current implementations: `CodexExecutor`, `ClaudeExecutor`, `GeminiExecutor`

This enables:
- Adding new AI tools without changing flow logic
- Mixing different tools for planning vs execution
- Testing with mock executors

### 2. Immediate-Mode TUI

Following Ratatui patterns, the App owns all state and renders the entire frame each tick:

```rust
impl App {
    fn handle_key(&mut self, key: KeyEvent) { /* mutate state */ }
    fn render(&self, frame: &mut Frame)     { /* read-only render */ }
}
```

### 3. Event-Driven Flow Communication

The flow runs in a separate async task and communicates with the UI via events:

```rust
pub enum FlowEvent {
    PhaseChanged(FlowPhase),
    Output(OutputLine),            // CLI output and system messages
    TodoFilesUpdated(Vec<PathBuf>),
    CurrentFile(Option<String>),   // File being processed
    RetryWait(Option<u64>),        // Retry countdown in seconds
    ClearOutput,                   // Clear output buffer
    Done,                          // Flow completed
    SearchResult { generation, result },  // Async file search result
}
```

This decouples the UI from async execution.

### 4. Generic Retry Logic

A single `run_with_retry()` function handles all executors:

```rust
async fn run_with_retry<F>(
    input_text: &str,
    executor: &dyn AiCliExecutor,
    phase_builder: F,        // Closure to build FlowPhase
    config: &RetryConfig,
    tx: &Sender<FlowEvent>,
    shutdown_rx: &Receiver<bool>,
) -> Result<()>
```

### 5. Zero-Allocation Phase Names

`FlowPhase` uses `Cow<'static, str>` for model names to avoid unnecessary allocations:

```rust
pub enum FlowPhase {
    RunningPlanning { model_name: Cow<'static, str>, attempt: u32 },
    // ...
}
```

### 6. @ File Tagging System

The text input supports `@` mentions for file references with fuzzy matching:

```
User types "@src/m" → Popup shows matching files → User selects → Path inserted
```

Components:
- **`file_search.rs`**: Fuzzy file matching using `nucleo-matcher`, respects `.gitignore`
- **`app/input.rs`**: `AtToken` detection with UTF-8-safe boundary handling
- **`tui/widgets/file_popup.rs`**: `PopupState` enum and `FileSuggestionPopup` widget

Key features:
- 50ms debouncing prevents excessive file system scans
- Email patterns like `user@domain.com` don't trigger the popup
- Paths with spaces are automatically quoted
- Up to 8 results shown, sorted by match score

## Data Flow

```
User Input
    │
    ▼
┌─────────────────┐
│    App::new()   │  ← CLI args parsed
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   TextInput     │  ← User enters task description
│     Mode        │  ← User can open settings (Ctrl+S) at any time
└────────┬────────┘
         │ (Submit)
         ▼
┌─────────────────┐     ┌─────────────────┐
│   run_flow()    │────▶│  FlowEvent::*   │
│  (async task)   │     │   (channel)     │
│  Uses models    │     └────────┬────────┘
│  from settings  │              │
└────────┬────────┘              ▼
         │              ┌─────────────────┐
         ▼              │ App::process_   │
┌─────────────────┐     │    events()     │
│  AiCliExecutor  │     └─────────────────┘
│   ::execute()   │
└─────────────────┘
```

## Key Types

### FlowState
Holds the current orchestration state:
- `phase: FlowPhase` - Current execution phase
- `input_text: String` - Task description
- `input_path: Option<PathBuf>` - Source file (if any)
- `todo_files: Vec<PathBuf>` - Pending task files
- `cycle_count: u32` - Current iteration

### FlowPhase
Enum representing all possible states:
- `Idle` - Initial state
- `ReadingInput` - Loading input file
- `CheckingDoneFiles` - Scanning `todo/done/` for completed tasks to verify
- `VerifyingDoneTask { file_name, attempt }` - Verifying a completed task
- `CleaningUpDoneFiles` - Removing verified done files
- `RunningPlanning { model_name, attempt }` - Executing planning model
- `CheckingTodoFiles` - Scanning todo directory
- `ProcessingTodos { current, total }` - Processing task queue
- `RunningExecution { model_name, file_index, attempt }` - Executing on a task
- `CycleComplete { iteration }` - One cycle done
- `MovingCompletedFiles` - Archiving completed tasks
- `Completed` / `Failed` / `NoTodoFiles` - Terminal states

### App
Main application struct containing:
- Flow state and theme
- UI state (scroll offsets, selection indices)
- Event channels for async communication
- Shutdown signal sender

## File System Layout

```
todo/                    # Active task files
├── task-001.md
├── task-002.md
└── done/                # Completed tasks (archived)
    ├── task-001_20240113_143052.md
    └── ...
```

## Error Handling

- Uses `anyhow` for application errors with context
- Errors propagate via `Result<T>` and `?` operator
- Failed CLI executions trigger retry logic
- Terminal errors display in UI log panel

## Constants

Key configuration values in `src/app/mod.rs`:

```rust
const EVENT_CHANNEL_SIZE: usize = 1000;    // Flow event buffer
const FILE_SEARCH_DEBOUNCE_MS: u64 = 50;   // Search debounce interval
```

Scroll configuration in `src/app/events.rs`:

```rust
const SCROLL_PAGE_SIZE: usize = 10;        // Page up/down amount
```

Output configuration in `src/tui/widgets/output.rs`:

```rust
const MAX_OUTPUT_LINES: usize = 5000;      // Maximum lines in output buffer
```

Retry configuration in `src/core/retry.rs`:

```rust
RetryConfig {
    max_attempts: 100,
    base_interval_secs: 10,
    interval_increment_secs: 10,  // Linear backoff
}
```

File search configuration in `src/file_search.rs`:

```rust
const MAX_FILE_MATCHES: usize = 8;         // Maximum results returned
```

Popup configuration in `src/tui/widgets/file_popup.rs`:

```rust
const MAX_POPUP_ROWS: usize = 8;           // Maximum visible popup rows
```

## File Search Module

The file search module (`src/file_search.rs`) provides fuzzy file path matching for the text input's `@` file tagging feature.

### Components

- **`search_files()`**: Main search function that traverses directories and matches paths
- **`FileMatch`**: Struct containing matched path and relevance score

### Dependencies

- `ignore`: Directory traversal respecting `.gitignore`
- `nucleo-matcher`: Fuzzy string matching with smart case handling

### @ Tagging Workflow

```
1. User types `@` followed by characters
      │
      ▼
2. detect_at_token() identifies the @ token
   - Scans current line for @-prefixed words
   - Validates @ is at word boundary (not email)
   - Extracts query text after @
      │
      ▼
3. update_file_search() triggers async search
   - Debounces rapid keystrokes (50ms)
   - Skips if query unchanged
   - Sets popup to Loading state
   - Sends query to background task via channel
      │
      ▼
4. Background task runs search_files()
   - Walks directory tree (respects .gitignore)
   - Scores each path with fuzzy matcher
   - Returns top 8 results by score
   - Sends results via FlowEvent::SearchResult
      │
      ▼
5. process_events() handles SearchResult
   - Validates generation counter (for cancellation)
   - Updates popup state with results
      │
      ▼
6. FileSuggestionPopup displays results
   - Renders bordered list with selection
   - Keyboard navigation (Up/Down/j/k)
      │
      ▼
7. User selects file (Tab/Enter)
   - Path replaces @ token in text
   - Spaces in path trigger quoting
   - Trailing space added for convenience
```

### Key Types

**AtToken** (in `app/state.rs`):
```rust
pub struct AtToken {
    pub query: String,      // Text after @
    pub start_byte: usize,  // Token start in line
    pub end_byte: usize,    // Token end in line
    pub row: usize,         // Which input line
}
```

**PopupState** (in `tui/widgets/file_popup.rs`):
```rust
pub enum PopupState {
    Hidden,                              // No popup
    Loading,                             // Async search in progress
    NoMatches,                           // Query returned no results
    Showing { matches, selected },       // Results with selection
}
```

### Design Decisions

1. **Async search**: File search runs in a background task to keep the UI responsive. A generation counter ensures stale results are ignored.

2. **Email rejection**: The pattern `email@domain.com` is explicitly rejected by checking if `@` is preceded by non-whitespace.

3. **Quote handling**: Paths containing spaces or special characters are automatically quoted when inserted (single quotes preferred, double quotes if path contains single quotes).

4. **Score-based sorting**: Results sorted by fuzzy match score (descending), then alphabetically for ties.

## Completed-Task Summary Flow

This section documents the design decisions for how completed-task summaries are sourced, formatted, and synchronized throughout the McGravity flow.

### Canonical Source

The canonical source for completed-task summaries is the **done-folder files** (`.mcgravity/todo/done/*.md`). The done folder serves as the source of truth for what tasks have been completed in the current session.

**Rationale**: The done folder provides a clean, file-based record of completed work that:
- Is naturally created by the flow as tasks complete (via `move_to_done()`)
- Contains the full task specification for accurate summarization
- Is cleared on session reset, providing natural session boundaries
- Allows the flow runner to operate stateless between cycles

The `task.md` file remains a **persistence mechanism for the user's input text only**, not a source for completed-task context. This separation keeps concerns clean: input persistence vs. execution state.

### Summary Format

Completed-task summaries are generated by `summarize_task_files()` in `runner.rs` with the following format:

```
- task-NNN.md:
<first 5 lines of file content>
...
```

**Format rules**:
- One entry per completed task file
- Filename on first line (e.g., `task-001.md:`)
- Up to 5 lines (`MAX_SUMMARY_LINES`) of content as a snippet
- Ellipsis (`...`) appended if content was truncated
- Entries separated by double newlines for readability

**Length limits**:
- Per-file snippet: 5 lines maximum
- No explicit character limit per line (lines are taken as-is)
- Files are processed in creation-time order (oldest first)

**Deduplication**: By filename—each `.md` file in the done folder appears exactly once in the summary.

### Input-File Session Handling

When McGravity is started with an input file (`mcgravity path/to/input.md`):
1. The input file is read as the task text (not copied to `task.md`)
2. Flow executes using the file content directly
3. Done-folder context is built from `.mcgravity/todo/done/` as usual
4. The original input file is **not modified**

When started without an input file (interactive mode):
1. Task text is loaded from `task.md` if it exists (session restoration)
2. User edits are autosaved to `task.md` (1-second debounce)
3. On successful flow completion and session reset, `task.md` is cleared

**In-memory task text**: The task text loaded at flow start is used immutably throughout the flow. Any user edits in the input field during execution don't affect the running flow—they're saved to `task.md` for the next session.

### Sync Strategy

The completed-task context is **computed fresh** at each phase that needs it:
1. **Planning phase**: Scans done folder, generates summary, includes in prompt
2. **Execution phase**: Scans done folder again, generates summary for each task

This scan-on-demand approach:
- Ensures context reflects current state (including tasks completed in current cycle)
- Avoids complex synchronization between in-memory state and disk
- Has acceptable performance for typical task counts (<100 files)

### Session Lifecycle

```
New Session Start
      │
      ▼
┌──────────────────┐
│ Load task.md     │ (if exists, interactive mode only)
│ or input file    │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Flow Execution   │ ◄─── Done folder accumulates completed tasks
│ (multiple cycles)│
└────────┬─────────┘
         │
         ▼ (NoTodoFiles)
┌──────────────────┐
│ Session Reset    │ ──► Clear task.md + done folder
│ (user confirms)  │
└──────────────────┘
```

## Key Event Handling

### Event Priority Order

Key events in Chat mode are processed in this priority order (see `src/app/events.rs`):

1. **File popup handling** - When popup is visible, navigation keys are captured
2. **Output scrolling** - Ctrl+Arrow keys and PageUp/PageDown
3. **Quit shortcuts** - Esc and Ctrl+C
4. **Text input** - All other keys for editing

### Text Input Key Bindings

The text input follows a simple model:
- `Enter` → Insert newline (always)
- `Ctrl+Enter` → Submit task
- `Ctrl+D` → Submit task (alternative)

This design eliminates terminal compatibility issues with Shift/Alt modifiers.

### Paste Handling

Two mechanisms handle pasted text:

1. **Bracketed Paste Mode** (preferred): Terminal sends `Event::Paste(String)` for pasted text
2. **Rapid Input Detection** (fallback): Detects paste by timing when bracketed paste unavailable

Constants in `src/app/events.rs`:
```rust
const RAPID_INPUT_THRESHOLD_MS: u64 = 50;      // Max ms between keys for rapid sequence
const RAPID_INPUT_COUNT_THRESHOLD: usize = 3;  // Keys needed to trigger rapid mode
const RAPID_INPUT_RESET_MS: u64 = 200;         // Timeout to reset rapid state
```
