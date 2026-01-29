//! Main application state and logic.
//!
//! This module contains the core App struct and its implementation,
//! organized into submodules:
//! - `input` - Text input handling
//! - `render` - UI rendering
//! - `state` - Application state structures
//! - `events` - Event handling logic
//!
//! ## Application Modes
//!
//! The application operates in three modes:
//!
//! - **`Chat`**: Main unified interface combining task input and output display.
//!   Users can submit tasks, view AI responses, and scroll through output while
//!   maintaining access to the input field.
//! - **`Settings`**: Modal overlay panel for configuring models (Ctrl+S)
//! - **`Finished`**: Modal overlay after flow completion prompting next action
//!
//! ## Settings Panel
//!
//! The settings panel can be opened at any time via `Ctrl+S`. It allows
//! users to configure:
//! - Planning model (Codex, Claude, Gemini)
//! - Execution model (Codex, Claude, Gemini)
//!
//! Changes are applied immediately and persist for the session.

pub mod events;
mod input;
mod layout;
mod render;
pub mod slash_commands;
pub mod state;

#[cfg(test)]
mod tests;

pub use layout::{ChatLayout, calculate_chat_layout};

pub use input::{WrapResult, escape_file_path, wrap_lines_for_display};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::core::{CommandContext, CommandRegistry, CommandResult, FlowState, Model};
use crate::file_search::FileMatch;
use crate::fs::McgravityPaths;
use crate::tui::Theme;
use crate::tui::widgets::{CommandPopupState, OutputLine, PopupState};

pub use self::slash_commands::{SlashToken, detect_slash_token, parse_slash_command};
pub use self::state::{
    AppMode, AtToken, FlowEvent, FlowUiState, InitialSetupField, InitialSetupState, LayoutState,
    ScrollState, SearchQuery, SettingsItem, SettingsState, TextInputState,
};

/// Channel buffer size for flow events.
const EVENT_CHANNEL_SIZE: usize = 1000;

/// Minimum time between file searches (debounce) in milliseconds.
const FILE_SEARCH_DEBOUNCE_MS: u64 = 50;

/// Autosave debounce time in milliseconds (saves after 1 second of inactivity).
const AUTOSAVE_DEBOUNCE_MS: u64 = 1000;

/// Main application state.
///
/// Organized into component sub-structs for better separation of concerns:
/// - `text_input`: State for text input mode (lines, cursor, @ mentions)
/// - `settings`: State for settings panel (model selection)
/// - `flow_ui`: State for flow execution UI (logs, output, scrolling)
/// - `layout`: Dynamic layout dimensions updated each frame
pub struct App {
    // =========================================================================
    // Shared State
    // =========================================================================
    /// All mcgravity-related filesystem paths.
    pub(crate) paths: McgravityPaths,
    /// Flow orchestration state (phase, todo files, cycle count).
    pub(crate) flow: FlowState,
    /// Theme for styling.
    pub(crate) theme: Theme,
    /// Current application mode.
    pub(crate) mode: AppMode,
    /// Should quit flag.
    should_quit: bool,
    /// Is flow running.
    is_running: bool,

    // =========================================================================
    // Event Channels
    // =========================================================================
    /// Event receiver for flow events.
    event_rx: mpsc::Receiver<FlowEvent>,
    /// Event sender (for spawning flow task).
    event_tx: mpsc::Sender<FlowEvent>,
    /// Shutdown signal sender (to kill child processes on exit).
    shutdown_tx: watch::Sender<bool>,

    // =========================================================================
    // Component States
    // =========================================================================
    /// Text input mode state.
    pub(crate) text_input: TextInputState,
    /// Settings panel state.
    pub(crate) settings: SettingsState,
    /// Flow UI state (logs, output, scrolling).
    pub(crate) flow_ui: FlowUiState,
    /// Dynamic layout dimensions.
    pub(crate) layout: LayoutState,
    /// Initial setup state (only present during first-run model selection).
    /// This is `Some` when the application starts without a settings file and
    /// the user needs to select their default models. Set to `None` after
    /// the initial setup is complete.
    #[allow(dead_code)]
    // Used in first-run detection; rendering/events implemented in later tasks
    pub(crate) initial_setup: Option<InitialSetupState>,

    /// Registry of available slash commands.
    pub(crate) command_registry: CommandRegistry,
}

/// Spawns a background task that handles file search queries.
///
/// This function creates an async task that:
/// 1. Listens for `SearchQuery` messages
/// 2. Runs `search_files` in a blocking task (since `ignore` crate is blocking)
/// 3. Sends results back via the event channel
/// 4. Uses generation counters for cancellation (stale results are ignored)
fn spawn_search_task(
    mut search_rx: mpsc::Receiver<SearchQuery>,
    event_tx: mpsc::Sender<FlowEvent>,
) {
    tokio::spawn(async move {
        while let Some(query) = search_rx.recv().await {
            let generation = query.generation;
            let query_str = query.query.clone();
            let working_dir = query.working_dir.clone();

            // Run the blocking search in a separate thread
            let search_result = tokio::task::spawn_blocking(move || {
                crate::file_search::search_files(&query_str, &working_dir)
            })
            .await;

            // Send result back to the UI thread
            if let Ok(result) = search_result {
                let _ = event_tx
                    .send(FlowEvent::SearchResult { generation, result })
                    .await;
            }
        }
    });
}

impl App {
    /// Creates a new application instance using the current working directory.
    ///
    /// If `input_path` is `Some`, validates the file exists and auto-starts the flow
    /// using default model settings.
    /// If `input_path` is `None`, starts in text input mode.
    ///
    /// The application always starts in Chat mode, which provides a unified interface
    /// for both text input and output display.
    ///
    /// # Errors
    ///
    /// Returns an error if the input file is provided but cannot be found.
    #[allow(clippy::needless_pass_by_value)] // Takes Option<PathBuf> for ergonomic API
    pub fn new(input_path: Option<PathBuf>) -> Result<Self> {
        Self::new_with_paths(input_path, McgravityPaths::from_cwd())
    }

    /// Creates a new application instance with custom paths.
    ///
    /// This constructor is primarily used for testing, allowing tests to use
    /// isolated temporary directories without affecting the real filesystem.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Optional path to an input file to process
    /// * `paths` - The mcgravity paths configuration (typically from a temp dir for tests)
    ///
    /// # Errors
    ///
    /// Returns an error if the input file is provided but cannot be found.
    #[allow(clippy::needless_pass_by_value)] // Takes Option<PathBuf> for ergonomic API
    pub fn new_with_paths(input_path: Option<PathBuf>, paths: McgravityPaths) -> Result<Self> {
        let has_input_file = input_path.is_some();

        // Always start in Chat mode - it's the only non-settings mode now
        let flow = match &input_path {
            Some(path) => {
                if !path.exists() {
                    anyhow::bail!("Input file not found: {}", path.display());
                }
                FlowState::new(path.clone())
            }
            None => FlowState::new_without_file(),
        };

        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);

        // Ensure .mcgravity directory structure exists
        if let Err(e) = paths.ensure_todo_dirs() {
            // Log warning but continue - directories might be created later
            eprintln!("Warning: Failed to create .mcgravity directories: {e}");
        }

        // Create search channel and spawn background search task
        let (search_tx, search_rx) = mpsc::channel(16);
        spawn_search_task(search_rx, event_tx.clone());

        // Detect first-run condition before loading settings
        let first_run = paths.is_first_run();

        // Determine initial mode and setup state based on first-run detection
        let (initial_mode, initial_setup_state) = if first_run {
            // First run: enter InitialSetup mode with default model selections
            let setup_state = InitialSetupState {
                selected_field: InitialSetupField::default(),
                planning_model: Model::default(),
                execution_model: Model::default(),
            };
            (AppMode::InitialSetup, Some(setup_state))
        } else {
            // Not first run: start in Chat mode
            (AppMode::Chat, None)
        };

        let mut app = Self {
            // Shared state
            paths,
            flow,
            theme: Theme::default(),
            mode: initial_mode,
            should_quit: false,
            is_running: false,
            // Event channels
            event_rx,
            event_tx,
            shutdown_tx,
            // Component states
            text_input: TextInputState::new(search_tx),
            settings: SettingsState::default(),
            flow_ui: FlowUiState::default(),
            layout: LayoutState::default(),
            initial_setup: initial_setup_state,
            command_registry: CommandRegistry::with_builtins(),
        };

        // Load persisted settings if available (only when not first run)
        if !first_run {
            match app.paths.load_settings() {
                Ok(persisted) => {
                    persisted.apply_to(&mut app.settings);
                }
                Err(e) => {
                    // Log warning but continue with defaults
                    app.flow_ui
                        .output
                        .push(crate::tui::widgets::OutputLine::warning(format!(
                            "Failed to load settings: {e}"
                        )));
                }
            }
        }

        // Load task.md content if starting without an input file
        if input_path.is_none() && app.load_saved_task() {
            app.flow_ui
                .output
                .push(crate::tui::widgets::OutputLine::info(
                    "Restored previous task text from .mcgravity/task.md",
                ));
        }

        // Auto-start flow if input file was provided
        if has_input_file {
            app.start_flow();
        }

        Ok(app)
    }

    /// Returns true if the application should quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Returns true if the flow is still running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    /// Gets the event sender for spawning flow tasks.
    #[must_use]
    pub fn event_sender(&self) -> mpsc::Sender<FlowEvent> {
        self.event_tx.clone()
    }

    /// Gets the input path (None if text was entered directly).
    #[must_use]
    pub fn input_path(&self) -> Option<&PathBuf> {
        self.flow.input_path.as_ref()
    }

    /// Gets a shutdown receiver for the flow task.
    #[must_use]
    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    /// Triggers shutdown to kill any running child processes.
    ///
    /// Uses `send_modify` to update the value even when no receivers exist,
    /// ensuring consistent state regardless of flow lifecycle.
    pub fn trigger_shutdown(&self) {
        self.shutdown_tx.send_modify(|v| *v = true);
    }

    /// Resets the shutdown signal to `false`, even when no receivers exist.
    ///
    /// This must be called after a flow is cancelled or completes to ensure the
    /// next flow can start cleanly. Uses `send_modify` to update the value
    /// regardless of whether any receivers are currently subscribed.
    ///
    /// # Why not `send(false)`?
    ///
    /// `watch::Sender::send()` returns an error and does NOT update the stored
    /// value when there are no receivers. After ESC cancellation, the flow task
    /// drops its receiver, so `send(false)` would silently fail and leave the
    /// shutdown flag stuck at `true`. The next `subscribe()` call would then
    /// immediately observe `true` and exit.
    pub fn reset_shutdown(&self) {
        self.shutdown_tx.send_modify(|v| *v = false);
    }

    /// Resets state for a fresh session after flow completion.
    ///
    /// This method is called when the user starts a new session after successful
    /// flow completion (by pressing Enter in Finished mode). It clears all state
    /// and also removes the `.mcgravity/task.md` file and the done folder to
    /// provide a clean slate.
    ///
    /// The task.md file and done folder are NOT cleared when:
    /// - The user cancels with ESC (task text should persist for retry)
    /// - The flow fails (task text should persist for retry)
    pub(crate) fn reset_session(&mut self) {
        // Clear task.md file for fresh session
        // Do this first so any error can be logged to output before we clear it
        if let Err(e) = std::fs::remove_file(self.paths.task_file()) {
            // Only warn if it's not a "file not found" error (file may not exist yet)
            if e.kind() != std::io::ErrorKind::NotFound {
                self.flow_ui
                    .output
                    .push(crate::tui::widgets::OutputLine::warning(format!(
                        "Failed to clear task.md: {e}"
                    )));
            }
        }

        // Clear done folder for fresh session
        if let Err(e) = std::fs::remove_dir_all(self.paths.done_dir()) {
            // Only warn if it's not a "not found" error
            if e.kind() != std::io::ErrorKind::NotFound {
                self.flow_ui
                    .output
                    .push(crate::tui::widgets::OutputLine::warning(format!(
                        "Failed to clear done folder: {e}"
                    )));
            }
        }
        // Recreate the done directory
        if let Err(e) = std::fs::create_dir_all(self.paths.done_dir()) {
            self.flow_ui
                .output
                .push(crate::tui::widgets::OutputLine::warning(format!(
                    "Failed to recreate done folder: {e}"
                )));
        }

        self.flow_ui = FlowUiState::default();
        self.flow = FlowState::new_without_file();
        self.reset_shutdown();
        self.mode = AppMode::Chat;
        self.is_running = false;

        let search_tx = self.text_input.search_tx.clone();
        self.text_input = TextInputState::new(search_tx);
    }

    /// Calculates and caches the layout based on terminal dimensions.
    ///
    /// Uses the centralized layout helpers from [`layout`] module.
    /// The calculated layout is stored in `self.layout` and used by both
    /// scroll calculations and rendering to ensure consistency.
    ///
    /// Should be called once per frame before rendering.
    pub fn update_layout(&mut self, terminal_area: Rect) {
        // Always calculate chat layout - it's the primary layout and is
        // used even when Settings overlay is shown
        self.layout.chat = calculate_chat_layout(terminal_area, self.is_running);
    }

    // =========================================================================
    // File Search Integration
    // =========================================================================

    /// Updates file search based on current @ token.
    ///
    /// This method:
    /// 1. Checks if there's an active @ token
    /// 2. Debounces rapid typing
    /// 3. Performs the file search
    /// 4. Updates the popup state with results
    pub(crate) fn update_file_search(&mut self) {
        if let Some(token) = &self.text_input.at_token {
            // Debounce check
            let should_search = match (
                &self.text_input.last_search_query,
                self.text_input.last_search_time,
            ) {
                (Some(last_query), Some(last_time)) => {
                    // Search if query changed or debounce period elapsed
                    last_query != &token.query
                        || last_time.elapsed() >= Duration::from_millis(FILE_SEARCH_DEBOUNCE_MS)
                }
                _ => true, // First search
            };

            if should_search {
                let query = token.query.clone();
                self.perform_file_search(&query);
            }
        } else {
            // No @ token, hide popup
            self.text_input.file_popup_state = PopupState::Hidden;
            self.text_input.last_search_query = None;
        }
    }

    /// Initiates an async file search by sending a query to the background task.
    ///
    /// This method:
    /// 1. Increments the search generation (for cancellation tracking)
    /// 2. Sets the popup state to Loading
    /// 3. Sends the query to the background search task
    ///
    /// Results arrive via `FlowEvent::SearchResult` and are processed in `process_events`.
    pub(crate) fn perform_file_search(&mut self, query: &str) {
        let working_dir = std::env::current_dir().unwrap_or_default();

        // Reset selection when query changes
        if self.text_input.last_search_query.as_deref() != Some(query)
            && let PopupState::Showing { selected, .. } = &mut self.text_input.file_popup_state
        {
            *selected = 0;
        }

        // Update tracking
        self.text_input.last_search_query = Some(query.to_string());
        self.text_input.last_search_time = Some(Instant::now());

        // Increment generation for this search
        self.text_input.search_generation = self.text_input.search_generation.wrapping_add(1);

        // Set popup to loading state
        self.text_input.file_popup_state = PopupState::Loading;

        // Send query to background task
        let search_query = SearchQuery {
            query: query.to_string(),
            working_dir,
            generation: self.text_input.search_generation,
        };

        // Use try_send to avoid blocking; if channel is full, the oldest query
        // will be processed and newer ones will arrive shortly
        let _ = self.text_input.search_tx.try_send(search_query);
    }

    /// Returns true if the file suggestion popup should be shown.
    #[must_use]
    pub fn should_show_file_popup(&self) -> bool {
        match &self.text_input.file_popup_state {
            PopupState::Hidden => false,
            PopupState::Loading | PopupState::NoMatches => true,
            PopupState::Showing { matches, .. } => !matches.is_empty(),
        }
    }

    /// Gets the current @ token query, if any.
    #[must_use]
    pub fn current_at_query(&self) -> &str {
        self.text_input
            .at_token
            .as_ref()
            .map_or("", |t| t.query.as_str())
    }

    // =========================================================================
    // File Popup Navigation
    // =========================================================================

    /// Moves selection up in the file popup.
    pub(crate) fn file_popup_up(&mut self) {
        if let PopupState::Showing { selected, .. } = &mut self.text_input.file_popup_state {
            *selected = selected.saturating_sub(1);
        }
    }

    /// Moves selection down in the file popup.
    pub(crate) fn file_popup_down(&mut self) {
        if let PopupState::Showing { matches, selected } = &mut self.text_input.file_popup_state {
            let max_index = matches.len().saturating_sub(1);
            *selected = (*selected + 1).min(max_index);
        }
    }

    /// Returns true if the file popup is currently visible.
    #[must_use]
    pub fn file_popup_visible(&self) -> bool {
        self.text_input.file_popup_state.is_visible()
    }

    /// Returns the currently selected file match, if any.
    #[must_use]
    pub fn selected_file_match(&self) -> Option<&FileMatch> {
        if let PopupState::Showing { matches, selected } = &self.text_input.file_popup_state {
            matches.get(*selected)
        } else {
            None
        }
    }

    /// Returns true if the file popup has matches that can be selected.
    ///
    /// This returns true only when the popup is in `Showing` state with at least
    /// one match. It returns false for `Hidden`, `Loading`, or `NoMatches` states.
    #[must_use]
    pub fn has_file_matches(&self) -> bool {
        matches!(
            &self.text_input.file_popup_state,
            PopupState::Showing { matches, .. } if !matches.is_empty()
        )
    }

    // =========================================================================
    // File Selection
    // =========================================================================

    /// Selects the currently highlighted file from the popup.
    ///
    /// This replaces the @ token with the full file path and dismisses the popup.
    /// For directories, a trailing slash is appended to the path.
    pub(crate) fn select_file_from_popup(&mut self) {
        // Get the selected file path and is_dir flag
        let selected_info = match &self.text_input.file_popup_state {
            PopupState::Showing { matches, selected } => {
                matches.get(*selected).map(|m| (m.path.clone(), m.is_dir))
            }
            _ => None,
        };

        let Some((path, is_dir)) = selected_info else {
            return;
        };

        // Get the @ token info
        let Some(token) = self.text_input.at_token.take() else {
            return;
        };

        // For directories, append a trailing slash to the path string
        let path_with_slash = if is_dir {
            PathBuf::from(format!("{}/", path.display()))
        } else {
            path
        };

        // Replace the @ token with the file path
        self.replace_at_token_with_path(&token, &path_with_slash);

        // Dismiss the popup
        self.dismiss_file_popup();
    }

    /// Replaces an @ token with the given file path.
    ///
    /// Handles paths with spaces and special characters by escaping them properly.
    /// Adds a trailing space after the path for convenient continued typing.
    ///
    /// # Arguments
    ///
    /// * `token` - The @ token to replace
    /// * `path` - The file path to insert
    pub(crate) fn replace_at_token_with_path(&mut self, token: &AtToken, path: &std::path::Path) {
        use tui_textarea::{CursorMove, TextArea};

        // Get the lines from the textarea
        let lines = self.text_input.textarea.lines();

        // Defensive bounds check for row - return early on invalid state
        let Some(line) = lines.get(token.row) else {
            return;
        };

        // Convert path to string and escape properly
        let path_str = path.display().to_string();
        let insert_str = escape_file_path(&path_str);

        // The token includes everything from start_byte to end_byte (the @ is at start_byte)
        let start = token.start_byte;
        let end = token.end_byte;

        // Defensive bounds check for byte range - return early on invalid state
        if start > end || end > line.len() {
            return;
        }

        // Create a modified line with the replacement
        let mut new_line = line.clone();
        new_line.replace_range(start..end, &insert_str);

        // Calculate the new cursor position (character-wise)
        // Count characters up to start_byte, then add insert_str length
        let start_char_idx = line[..start].chars().count();
        let new_cursor_col = start_char_idx + insert_str.chars().count();

        // Rebuild the lines with the modified one
        let mut new_lines: Vec<String> = lines.to_vec();
        new_lines[token.row] = new_line;

        // Create a new textarea with the updated content
        let mut new_textarea = TextArea::new(new_lines);
        new_textarea.set_placeholder_text("Type / for commands or describe a task...");

        // Position cursor at the end of the inserted text
        new_textarea.move_cursor(CursorMove::Top);
        for _ in 0..token.row {
            new_textarea.move_cursor(CursorMove::Down);
        }
        new_textarea.move_cursor(CursorMove::Head);
        for _ in 0..new_cursor_col {
            new_textarea.move_cursor(CursorMove::Forward);
        }

        self.text_input.textarea = new_textarea;
    }

    /// Dismisses the file suggestion popup without selecting.
    pub(crate) fn dismiss_file_popup(&mut self) {
        self.text_input.file_popup_state = PopupState::Hidden;
        self.text_input.at_token = None;
        self.text_input.last_search_query = None;
    }

    // =========================================================================
    // Command Popup Navigation
    // =========================================================================

    /// Returns true if the command popup should be shown.
    #[must_use]
    pub fn should_show_command_popup(&self) -> bool {
        self.text_input.command_popup_state.is_visible()
    }

    /// Returns true if the command popup has matches that can be selected.
    #[must_use]
    pub fn has_command_matches(&self) -> bool {
        matches!(
            &self.text_input.command_popup_state,
            CommandPopupState::Showing { matches, .. } if !matches.is_empty()
        )
    }

    /// Moves selection up in the command popup.
    pub(crate) fn command_popup_up(&mut self) {
        self.text_input.command_popup_state.select_up();
    }

    /// Moves selection down in the command popup.
    pub(crate) fn command_popup_down(&mut self) {
        self.text_input.command_popup_state.select_down();
    }

    /// Selects the currently highlighted command from the popup.
    ///
    /// This replaces the current input with the full command (e.g., "/exit")
    /// and dismisses the popup. The cursor is positioned at the end of the
    /// command to allow adding arguments.
    pub(crate) fn select_command_from_popup(&mut self) {
        use tui_textarea::CursorMove;

        let Some(cmd_name) = self.text_input.command_popup_state.selected_command() else {
            return;
        };

        // Replace the current input with the full command
        let full_command = format!("/{cmd_name}");

        // Clear and set new text
        let mut new_textarea = tui_textarea::TextArea::new(vec![full_command]);
        new_textarea.set_placeholder_text("Type / for commands or describe a task...");
        self.text_input.textarea = new_textarea;

        // Move cursor to end
        self.text_input.textarea.move_cursor(CursorMove::End);

        // Dismiss popup
        self.dismiss_command_popup();
    }

    /// Dismisses the command popup without selecting.
    pub(crate) fn dismiss_command_popup(&mut self) {
        self.text_input.command_popup_state = CommandPopupState::Hidden;
        self.text_input.slash_token = None;
    }

    // =========================================================================
    // Tick / Autosave
    // =========================================================================

    /// Processes periodic tasks like autosaving.
    ///
    /// This method should be called regularly (e.g., on each event loop tick).
    /// It checks if there are unsaved changes and if sufficient time has passed
    /// since the last edit, then triggers an autosave.
    ///
    /// Autosave is debounced to avoid excessive disk writes during rapid typing.
    /// The save only occurs after `AUTOSAVE_DEBOUNCE_MS` milliseconds of inactivity.
    pub fn tick(&mut self) {
        // Check if there are unsaved changes
        if !self.text_input.is_dirty {
            return;
        }

        // Check if enough time has passed since the last edit
        let Some(last_edit) = self.text_input.last_edit_time else {
            return;
        };

        if last_edit.elapsed() < Duration::from_millis(AUTOSAVE_DEBOUNCE_MS) {
            return;
        }

        // Perform autosave
        if let Err(e) = self.save_current_task() {
            self.flow_ui
                .output
                .push(crate::tui::widgets::OutputLine::warning(format!(
                    "Autosave failed: {e}"
                )));
        }

        // Reset dirty flag after successful save (or attempted save)
        self.text_input.is_dirty = false;
    }

    // =========================================================================
    // Task Persistence
    // =========================================================================

    /// Loads saved task text from `.mcgravity/task.md` into the text input.
    ///
    /// This method reads the `.mcgravity/task.md` file and populates the text
    /// input buffer. The cursor is positioned at the end of the loaded content.
    ///
    /// # Behavior
    ///
    /// - If the file doesn't exist or is empty, returns `false`
    /// - Uses `split('\n')` to preserve trailing newlines as empty lines
    /// - Cursor is placed at the end of the last line
    /// - Returns `true` if content was successfully loaded
    ///
    /// # Returns
    ///
    /// `true` if `.mcgravity/task.md` was successfully read and content was loaded,
    /// `false` if the file doesn't exist, is empty, or couldn't be read.
    fn load_saved_task(&mut self) -> bool {
        use tui_textarea::{CursorMove, TextArea};

        let Ok(content) = std::fs::read_to_string(self.paths.task_file()) else {
            return false;
        };

        if content.is_empty() {
            return false;
        }

        // Use split('\n') to preserve trailing newline as empty line
        let lines: Vec<String> = content.split('\n').map(String::from).collect();
        if lines.is_empty() {
            return false;
        }

        // Create a new TextArea with the loaded content
        let mut textarea = TextArea::new(lines);
        textarea.set_placeholder_text("Type / for commands or describe a task...");

        // Move cursor to the end of the content
        textarea.move_cursor(CursorMove::Bottom);
        textarea.move_cursor(CursorMove::End);

        self.text_input.textarea = textarea;

        true
    }

    /// Saves the current task text to `.mcgravity/task.md`.
    ///
    /// This method writes the current text input content to `.mcgravity/task.md`
    /// for future reference. The `.mcgravity/` directory is created if it doesn't
    /// exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the write failed (permissions, disk full, etc.).
    pub fn save_current_task(&self) -> std::io::Result<()> {
        // Ensure the .mcgravity directory exists
        std::fs::create_dir_all(self.paths.mcgravity_dir())?;
        let text = self.text_input.collect_text();
        std::fs::write(self.paths.task_file(), &text)
    }

    // =========================================================================
    // Slash Command Execution
    // =========================================================================

    /// Attempts to parse and execute a slash command from the input.
    ///
    /// Returns `true` if a command was executed, `false` if input is not a command.
    pub(crate) fn try_execute_slash_command(&mut self) -> bool {
        let input = self.text_input.collect_text();

        // Parse the input to see if it's a slash command
        let Some((name, _args)) = parse_slash_command(&input) else {
            return false;
        };

        // Look up the command in the registry
        let Some(cmd) = self.command_registry.find(name) else {
            // Unknown command - show error message
            self.flow_ui
                .output
                .push(OutputLine::warning(format!("Unknown command: /{name}")));
            self.text_input.clear();
            return true;
        };

        // Build context and check if command can execute
        let ctx = CommandContext {
            is_running: self.is_running,
            mode: &self.mode,
        };

        if !cmd.can_execute(&ctx) {
            self.flow_ui.output.push(OutputLine::warning(format!(
                "Cannot execute /{name} while flow is running"
            )));
            return true;
        }

        // Execute the command
        let result = cmd.execute(&ctx);

        // Handle the result
        self.handle_command_result(result);

        // Clear input after command execution
        self.text_input.clear();

        true
    }

    /// Handles the result of a slash command execution.
    fn handle_command_result(&mut self, result: CommandResult) {
        match result {
            CommandResult::Continue => {}
            CommandResult::Exit => {
                self.trigger_shutdown();
                self.should_quit = true;
            }
            CommandResult::OpenSettings => {
                self.open_settings();
            }
            CommandResult::Clear => {
                self.execute_clear_command();
            }
            CommandResult::Message(msg) => {
                self.flow_ui.output.push(OutputLine::info(msg));
            }
        }
    }

    /// Executes the `/clear` command: clears task.md, output, and todo files.
    ///
    /// Does NOT reset settings.
    fn execute_clear_command(&mut self) {
        // Clear task.md
        if let Err(e) = std::fs::remove_file(self.paths.task_file())
            && e.kind() != std::io::ErrorKind::NotFound
        {
            self.flow_ui
                .output
                .push(OutputLine::warning(format!("Failed to clear task.md: {e}")));
        }

        // Clear todo folder (including done subfolder)
        self.clear_todo_folder();

        // Clear output buffer
        self.flow_ui.output.clear();
        self.flow_ui.output_scroll.reset();
        self.flow_ui.output_truncated = false;

        // Clear the text input
        let search_tx = self.text_input.search_tx.clone();
        self.text_input = TextInputState::new(search_tx);

        // Also clear flow.input_text to reset the Task Text panel
        self.flow.input_text = String::new();

        // Show confirmation
        self.flow_ui
            .output
            .push(OutputLine::info("Cleared task, output, and todo files"));
    }

    /// Clears all files in the todo folder and done subfolder.
    fn clear_todo_folder(&mut self) {
        // Clear todo/*.md files
        if let Ok(entries) = std::fs::read_dir(self.paths.todo_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        // Clear done folder contents
        if let Err(e) = std::fs::remove_dir_all(self.paths.done_dir())
            && e.kind() != std::io::ErrorKind::NotFound
        {
            self.flow_ui.output.push(OutputLine::warning(format!(
                "Failed to clear done folder: {e}"
            )));
        }
        // Recreate done directory
        let _ = std::fs::create_dir_all(self.paths.done_dir());
    }

    /// Gets the paths configuration for this app instance.
    #[must_use]
    pub fn paths(&self) -> &McgravityPaths {
        &self.paths
    }
}
