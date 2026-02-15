//! Application state structures.
//!
//! This module contains the state definitions for different parts of the application:
//!
//! - **`TextInputState`**: Multi-line text editing with cursor and @ mentions
//! - **`SettingsState`**: Model selection and settings configuration
//! - **`InitialSetupState`**: First-run model selection modal
//! - **`FlowUiState`**: Flow execution UI (logs, output, scrolling)
//! - **`LayoutState`**: Dynamic layout dimensions
//!
//! ## Settings Panel
//!
//! The settings panel uses `SettingsState` to track:
//! - Currently selected setting item
//! - Planning model selection
//! - Execution model selection
//! - Previous mode (for returning after close)
//!
//! ## Initial Setup Modal
//!
//! The initial setup modal (`InitialSetupState`) is displayed on first run when
//! no `.mcgravity/settings.json` exists. Unlike the settings panel:
//! - It only shows model selection (not enter behavior or max iterations)
//! - It cannot be dismissed with Esc (user must select models)
//! - It includes a welcome/introduction message

use std::path::PathBuf;
use std::time::Instant;

use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::app::input::RapidInputDetector;
use crate::app::slash_commands::SlashToken;
use crate::core::{FlowPhase, Model, ModelAvailability};
use crate::file_search::SearchResult;
use crate::tui::widgets::{CommandPopupState, OutputLine, PopupState};

/// Behavior of the Enter key in the text input area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnterBehavior {
    /// Enter submits the task, Shift+Enter inserts a newline.
    /// This is the default behavior (Chat style).
    #[default]
    Submit,
    /// Enter inserts a newline, Ctrl+Enter submits the task.
    /// This is the "Editor style" behavior.
    Newline,
}

impl EnterBehavior {
    /// Toggles between the two behaviors.
    #[must_use]
    pub fn next(&self) -> Self {
        match self {
            Self::Submit => Self::Newline,
            Self::Newline => Self::Submit,
        }
    }

    /// Returns the display name for this behavior.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Submit => "Submit",
            Self::Newline => "Newline",
        }
    }
}

/// Maximum number of iterations for the orchestration loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaxIterations {
    /// Single iteration only.
    One,
    /// Three iterations.
    Three,
    /// Five iterations (default).
    #[default]
    Five,
    /// Ten iterations.
    Ten,
    /// Unlimited iterations.
    Unlimited,
}

impl MaxIterations {
    /// Cycles to the next iteration option.
    #[must_use]
    pub fn next(&self) -> Self {
        match self {
            Self::One => Self::Three,
            Self::Three => Self::Five,
            Self::Five => Self::Ten,
            Self::Ten => Self::Unlimited,
            Self::Unlimited => Self::One,
        }
    }

    /// Returns the numeric value, or `None` for unlimited.
    #[must_use]
    pub const fn value(&self) -> Option<u32> {
        match self {
            Self::One => Some(1),
            Self::Three => Some(3),
            Self::Five => Some(5),
            Self::Ten => Some(10),
            Self::Unlimited => None,
        }
    }

    /// Returns the display name for this setting.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Three => "3",
            Self::Five => "5",
            Self::Ten => "10",
            Self::Unlimited => "Unlimited",
        }
    }
}

/// Whether to use a separate model call for task summary generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SummaryGeneration {
    /// Use inline `TASK_SUMMARY:` extraction only (faster, no extra model call).
    #[default]
    InlineOnly,
    /// Use inline extraction first, fall back to a separate model call.
    WithModelFallback,
}

impl SummaryGeneration {
    /// Cycles to the next option.
    #[must_use]
    pub fn next(&self) -> Self {
        match self {
            Self::InlineOnly => Self::WithModelFallback,
            Self::WithModelFallback => Self::InlineOnly,
        }
    }

    /// Returns the display name for this option.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::InlineOnly => "Inline Only",
            Self::WithModelFallback => "Model Fallback",
        }
    }

    /// Returns whether this setting enables the separate model call fallback.
    #[must_use]
    pub const fn uses_model_fallback(&self) -> bool {
        matches!(self, Self::WithModelFallback)
    }
}

/// Identifiers for settings items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    /// Planning model selection.
    PlanningModel,
    /// Execution model selection.
    ExecutionModel,
    /// Enter key behavior.
    EnterBehavior,
    /// Maximum iterations for the orchestration loop.
    MaxIterations,
    /// Summary generation strategy.
    SummaryGeneration,
}

impl SettingsItem {
    /// Returns all settings items in display order.
    #[must_use]
    pub fn all() -> &'static [SettingsItem] {
        &[
            SettingsItem::PlanningModel,
            SettingsItem::ExecutionModel,
            SettingsItem::EnterBehavior,
            SettingsItem::MaxIterations,
            SettingsItem::SummaryGeneration,
        ]
    }

    /// Returns the display label for this item.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::PlanningModel => "Planning Model",
            Self::ExecutionModel => "Execution Model",
            Self::EnterBehavior => "Enter Key",
            Self::MaxIterations => "Max Iterations",
            Self::SummaryGeneration => "Summary Mode",
        }
    }

    /// Returns a description for this item.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::PlanningModel => "AI CLI used for planning tasks",
            Self::ExecutionModel => "AI CLI used for executing tasks",
            Self::EnterBehavior => "Behavior of the Enter key (Submit vs Newline)",
            Self::MaxIterations => "Maximum cycles before stopping",
            Self::SummaryGeneration => "How task summaries are generated (Inline vs Model)",
        }
    }
}

/// Default visible height for output panel (used before first render).
pub const DEFAULT_OUTPUT_VISIBLE_HEIGHT: usize = 15;

/// Events sent from the flow execution to the UI.
#[derive(Debug, Clone)]
pub enum FlowEvent {
    /// Phase changed.
    PhaseChanged(FlowPhase),
    /// Output line added (includes both CLI output and system messages).
    Output(OutputLine),
    /// Todo files list updated.
    TodoFilesUpdated(Vec<PathBuf>),
    /// Current file being processed.
    CurrentFile(Option<String>),
    /// Retry wait countdown.
    RetryWait(Option<u64>),
    /// Clear output buffer.
    ClearOutput,
    /// Flow completed.
    Done,
    /// File search result received from background task.
    SearchResult {
        /// The generation of the search request (for cancellation).
        generation: u64,
        /// The search result containing matches.
        result: SearchResult,
    },
    /// Task text updated (propagates task.md changes to UI during execution).
    ///
    /// Emitted after successful `task.md` persistence so the read-only Task Text
    /// panel stays synchronized with the on-disk state.
    TaskTextUpdated(String),
}

/// Query sent to the background file search task.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The search query string (text after `@`).
    pub query: String,
    /// The working directory to search in.
    pub working_dir: PathBuf,
    /// Generation counter for debouncing/cancellation.
    pub generation: u64,
}

/// Application mode.
///
/// The application has four modes:
/// - **Chat**: The main unified interface with input field, output display,
///   and status bar. Users can submit tasks, view AI responses, and scroll
///   through output while maintaining access to the input field.
/// - **Settings**: Modal overlay for configuring models (Ctrl+S).
/// - **Finished**: Modal overlay shown after all tasks complete.
/// - **`InitialSetup`**: First-run modal for selecting default models when no
///   settings file exists. Unlike Settings, this mode cannot be dismissed
///   with Esc and only shows model selection (not enter behavior or max iterations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Chat-like unified interface (input + output + status in one view).
    /// This is the primary mode combining text input and flow output display.
    #[default]
    Chat,
    /// Settings panel overlay.
    Settings,
    /// Finished dialog overlay.
    Finished,
    /// Initial setup modal for first-run model selection.
    /// Displayed when no `.mcgravity/settings.json` exists.
    InitialSetup,
}

/// Information about an `@` token being typed.
///
/// This struct tracks the location and content of an `@`-prefixed token
/// that the cursor is currently within or immediately after. It's used
/// for triggering file/path completion suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtToken {
    /// The query text (without the `@` prefix).
    pub query: String,
    /// Byte position where the `@` starts in the current line.
    pub start_byte: usize,
    /// Byte position where the token ends (cursor position or whitespace).
    pub end_byte: usize,
    /// The row (line index) containing the token.
    pub row: usize,
}

// =============================================================================
// State Sub-Structs
// =============================================================================

/// State for text input mode.
///
/// Contains all fields related to multi-line text editing, cursor position,
/// file search, and the @ mention popup.
///
/// Uses `tui-textarea`'s `TextArea` widget for text editing, which handles:
/// - Multi-line text input with proper cursor management
/// - Character insertion and deletion
/// - Line wrapping and navigation
pub struct TextInputState {
    /// The text area widget from `tui-textarea` crate.
    /// Handles multi-line text editing with cursor management.
    pub textarea: TextArea<'static>,
    /// Current `@` token being typed (if any).
    pub at_token: Option<AtToken>,
    /// File suggestion popup state.
    pub file_popup_state: PopupState,
    /// Last file search query (for debouncing).
    pub(crate) last_search_query: Option<String>,
    /// Last file search time (for debouncing).
    pub(crate) last_search_time: Option<Instant>,
    /// Channel sender for file search queries to background task.
    pub(crate) search_tx: mpsc::Sender<SearchQuery>,
    /// Current search generation (incremented for each new search).
    pub(crate) search_generation: u64,

    /// Rapid input detector for paste fallback when bracketed paste mode is unavailable.
    pub(crate) rapid_input: RapidInputDetector,

    // === Autosave State ===
    /// Timestamp of the last text edit (for autosave debouncing).
    pub last_edit_time: Option<Instant>,
    /// Whether there are unsaved changes in the text input.
    pub is_dirty: bool,

    // === Slash Command State ===
    /// Slash command suggestion popup state.
    pub command_popup_state: CommandPopupState,
    /// Current slash token being typed (if any).
    pub slash_token: Option<SlashToken>,
}

impl TextInputState {
    /// Creates a new text input state with default values.
    #[must_use]
    pub fn new(search_tx: mpsc::Sender<SearchQuery>) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type / for commands or describe a task...");
        Self {
            textarea,
            at_token: None,
            file_popup_state: PopupState::default(),
            last_search_query: None,
            last_search_time: None,
            search_tx,
            search_generation: 0,
            rapid_input: RapidInputDetector::new(),
            // Autosave state
            last_edit_time: None,
            is_dirty: false,
            // Slash command state
            command_popup_state: CommandPopupState::default(),
            slash_token: None,
        }
    }

    /// Resets the rapid input detection state.
    ///
    /// This should be called after a successful submit to clear
    /// the rapid input detection counters for the next input session.
    pub fn reset_rapid_input_state(&mut self) {
        self.rapid_input.reset();
    }

    /// Clears the text area content and resets cursor.
    ///
    /// This replaces the textarea with a fresh instance to clear all content.
    /// Also resets autosave state (dirty flag and edit time) and slash command state.
    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
        self.at_token = None;
        self.file_popup_state = PopupState::default();
        self.last_edit_time = None;
        self.is_dirty = false;
        self.command_popup_state = CommandPopupState::default();
        self.slash_token = None;
    }

    /// Returns the lines of text from the textarea.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.textarea.lines()
    }

    /// Returns the cursor position as (row, col) - both zero-indexed.
    /// Note: col is the character position, not byte position.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    /// Collects all input lines into a single string.
    #[must_use]
    pub fn collect_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Sets the textarea content from a list of lines (for testing).
    ///
    /// Creates a new `TextArea` with the given lines and replaces the current one.
    #[cfg(test)]
    pub fn set_lines(&mut self, lines: Vec<String>) {
        use tui_textarea::TextArea;
        let mut textarea = TextArea::new(lines);
        textarea.set_placeholder_text("Type / for commands or describe a task...");
        self.textarea = textarea;
    }
}

/// State for the settings panel.
///
/// Contains fields for navigating and selecting models and other settings.
/// Following the `OpenAI` Codex pattern, settings are displayed as a list
/// of items with checkboxes/toggles.
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Currently selected index in the settings list.
    pub selected_index: usize,
    /// Selected planning model.
    pub planning_model: Model,
    /// Selected execution model.
    pub execution_model: Model,
    /// Enter key behavior.
    pub enter_behavior: EnterBehavior,
    /// Maximum iterations for the orchestration loop.
    pub max_iterations: MaxIterations,
    /// Summary generation strategy.
    pub summary_generation: SummaryGeneration,
    /// Previous mode to return to when closing settings.
    pub previous_mode: Option<AppMode>,
    /// Cached availability status for all AI CLI tools.
    /// Checked once at startup to avoid repeated shell invocations.
    pub model_availability: ModelAvailability,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            selected_index: 0,
            planning_model: Model::default(),
            execution_model: Model::default(),
            enter_behavior: EnterBehavior::default(),
            max_iterations: MaxIterations::default(),
            summary_generation: SummaryGeneration::default(),
            previous_mode: None,
            model_availability: ModelAvailability::check_all(),
        }
    }
}

impl SettingsState {
    /// Returns whether the given model's CLI tool is available on the system.
    #[must_use]
    pub fn is_model_available(&self, model: Model) -> bool {
        match model {
            Model::Codex => self.model_availability.codex,
            Model::Claude => self.model_availability.claude,
            Model::Gemini => self.model_availability.gemini,
        }
    }
}

/// Fields available for selection in the initial setup modal.
///
/// The initial setup modal only shows model selection (planning and execution),
/// unlike the full settings panel which also includes enter behavior and max iterations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialSetupField {
    /// Planning model selection field.
    #[default]
    PlanningModel,
    /// Execution model selection field.
    ExecutionModel,
}

impl InitialSetupField {
    /// Returns all fields in display order.
    #[must_use]
    pub fn all() -> &'static [InitialSetupField] {
        &[
            InitialSetupField::PlanningModel,
            InitialSetupField::ExecutionModel,
        ]
    }

    /// Cycles to the next field.
    #[must_use]
    pub fn next(&self) -> Self {
        match self {
            Self::PlanningModel => Self::ExecutionModel,
            Self::ExecutionModel => Self::PlanningModel,
        }
    }

    /// Returns the display label for this field.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::PlanningModel => "Planning Model",
            Self::ExecutionModel => "Execution Model",
        }
    }

    /// Returns a description for this field.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::PlanningModel => "AI CLI used for planning tasks",
            Self::ExecutionModel => "AI CLI used for executing tasks",
        }
    }
}

/// State for the initial setup modal.
///
/// This modal is displayed on first run when no `.mcgravity/settings.json` exists.
/// It prompts the user to select their default planning and execution models.
///
/// Unlike the regular settings panel:
/// - It cannot be dismissed with Esc (user must select models)
/// - It only shows model selection (not enter behavior or max iterations)
/// - It includes a welcome/introduction message
#[derive(Debug, Clone, Default)]
pub struct InitialSetupState {
    /// Currently selected field in the setup modal.
    pub selected_field: InitialSetupField,
    /// Selected planning model.
    pub planning_model: Model,
    /// Selected execution model.
    pub execution_model: Model,
}

/// State for flow execution UI.
///
/// Contains fields for output display, scrolling, and flow progress.
/// System messages and CLI output are now unified in the output buffer.
#[derive(Debug)]
pub struct FlowUiState {
    /// Output lines (includes both CLI output and system messages).
    pub output: Vec<OutputLine>,
    /// Output scroll state (position and auto-scroll behavior).
    pub(crate) output_scroll: ScrollState,
    /// Whether output has been truncated due to exceeding `MAX_OUTPUT_LINES`.
    pub output_truncated: bool,
    /// Current file being processed.
    pub current_file: Option<String>,
    /// Retry wait countdown in seconds.
    pub(crate) retry_wait: Option<u64>,
}

impl Default for FlowUiState {
    fn default() -> Self {
        Self {
            output: Vec::new(),
            output_scroll: ScrollState::new(),
            output_truncated: false,
            current_file: None,
            retry_wait: None,
        }
    }
}

/// Dynamic layout tracking state.
///
/// Stores the full [`ChatLayout`] calculated once per frame.
/// This ensures a single source of truth for layout dimensions,
/// used by both scroll calculations and rendering.
///
/// [`ChatLayout`]: crate::app::ChatLayout
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutState {
    /// The cached chat layout, calculated once per frame.
    pub chat: crate::app::ChatLayout,
}

impl LayoutState {
    /// Returns the visible height of the output panel (excluding borders).
    #[must_use]
    pub const fn output_visible_height(&self) -> usize {
        self.chat.output_visible_height
    }

    /// Returns the content width of the output panel (excluding borders and scrollbar).
    #[must_use]
    pub const fn output_content_width(&self) -> usize {
        self.chat.output_content_width
    }

    /// Returns the inner width of the input area (excluding borders).
    #[must_use]
    pub const fn input_visible_width(&self) -> usize {
        self.chat.input_inner_width
    }

    /// Returns the inner height of the input area (excluding borders).
    #[must_use]
    pub const fn input_visible_height(&self) -> usize {
        self.chat.input_inner_height
    }
}

/// Scroll state for a panel, combining position and auto-scroll behavior.
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    /// Current scroll offset (number of lines/visual lines from top).
    pub offset: usize,
    /// Whether to auto-scroll to bottom when new content is added.
    /// Set to false when user manually scrolls up, true when they scroll to bottom.
    pub auto_scroll: bool,
}

impl ScrollState {
    /// Creates a new scroll state with auto-scroll enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset: 0,
            auto_scroll: true,
        }
    }

    /// Resets scroll state to initial values (scroll to top, auto-scroll enabled).
    pub fn reset(&mut self) {
        self.offset = 0;
        self.auto_scroll = true;
    }

    /// Scrolls up by one line, disabling auto-scroll.
    pub fn scroll_up(&mut self) {
        self.offset = self.offset.saturating_sub(1);
        self.auto_scroll = false;
    }

    /// Scrolls down by one line, enabling auto-scroll if at the bottom.
    pub fn scroll_down(&mut self, content_len: usize, visible_height: usize) {
        let max_scroll = content_len.saturating_sub(visible_height);
        self.offset = (self.offset + 1).min(max_scroll);
        self.auto_scroll = self.offset >= max_scroll;
    }

    /// Scrolls up by a page, disabling auto-scroll.
    pub fn page_up(&mut self, page_size: usize) {
        self.offset = self.offset.saturating_sub(page_size);
        self.auto_scroll = false;
    }

    /// Scrolls down by a page, enabling auto-scroll if at the bottom.
    pub fn page_down(&mut self, content_len: usize, visible_height: usize, page_size: usize) {
        let max_scroll = content_len.saturating_sub(visible_height);
        self.offset = (self.offset + page_size).min(max_scroll);
        self.auto_scroll = self.offset >= max_scroll;
    }

    /// Scrolls to the top, disabling auto-scroll.
    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
        self.auto_scroll = false;
    }

    /// Scrolls to the bottom, enabling auto-scroll.
    pub fn scroll_to_bottom(&mut self, content_len: usize, visible_height: usize) {
        self.offset = content_len.saturating_sub(visible_height);
        self.auto_scroll = true;
    }

    /// Auto-scrolls to the bottom if auto-scroll is enabled.
    pub fn auto_scroll_if_enabled(&mut self, content_len: usize, visible_height: usize) {
        if self.auto_scroll {
            self.offset = content_len.saturating_sub(visible_height);
        }
    }
}

#[cfg(test)]
mod settings_state_tests {
    use super::*;
    use crate::core::Model;

    #[test]
    fn default_settings_use_codex() {
        let settings = SettingsState::default();
        assert_eq!(settings.planning_model, Model::Codex);
        assert_eq!(settings.execution_model, Model::Codex);
        assert_eq!(settings.selected_index, 0);
        assert!(settings.previous_mode.is_none());
    }

    #[test]
    fn settings_item_all_returns_expected_items() {
        let items = SettingsItem::all();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0], SettingsItem::PlanningModel);
        assert_eq!(items[1], SettingsItem::ExecutionModel);
        assert_eq!(items[2], SettingsItem::EnterBehavior);
        assert_eq!(items[3], SettingsItem::MaxIterations);
        assert_eq!(items[4], SettingsItem::SummaryGeneration);
    }

    #[test]
    fn settings_item_labels_are_not_empty() {
        for item in SettingsItem::all() {
            assert!(!item.label().is_empty());
            assert!(!item.description().is_empty());
        }
    }

    #[test]
    fn settings_item_planning_model_label() {
        assert_eq!(SettingsItem::PlanningModel.label(), "Planning Model");
        assert_eq!(
            SettingsItem::PlanningModel.description(),
            "AI CLI used for planning tasks"
        );
    }

    #[test]
    fn settings_item_execution_model_label() {
        assert_eq!(SettingsItem::ExecutionModel.label(), "Execution Model");
        assert_eq!(
            SettingsItem::ExecutionModel.description(),
            "AI CLI used for executing tasks"
        );
    }

    #[test]
    fn settings_item_enter_behavior_label() {
        assert_eq!(SettingsItem::EnterBehavior.label(), "Enter Key");
        assert_eq!(
            SettingsItem::EnterBehavior.description(),
            "Behavior of the Enter key (Submit vs Newline)"
        );
    }

    #[test]
    fn enter_behavior_toggles() {
        let behavior = EnterBehavior::Submit;
        assert_eq!(behavior.next(), EnterBehavior::Newline);
        assert_eq!(behavior.next().next(), EnterBehavior::Submit);
    }

    #[test]
    fn settings_item_max_iterations_label() {
        assert_eq!(SettingsItem::MaxIterations.label(), "Max Iterations");
        assert_eq!(
            SettingsItem::MaxIterations.description(),
            "Maximum cycles before stopping"
        );
    }

    #[test]
    fn max_iterations_cycles() {
        let iter = MaxIterations::One;
        assert_eq!(iter.next(), MaxIterations::Three);
        assert_eq!(iter.next().next(), MaxIterations::Five);
        assert_eq!(iter.next().next().next(), MaxIterations::Ten);
        assert_eq!(iter.next().next().next().next(), MaxIterations::Unlimited);
        assert_eq!(iter.next().next().next().next().next(), MaxIterations::One);
    }

    #[test]
    fn max_iterations_values() {
        assert_eq!(MaxIterations::One.value(), Some(1));
        assert_eq!(MaxIterations::Three.value(), Some(3));
        assert_eq!(MaxIterations::Five.value(), Some(5));
        assert_eq!(MaxIterations::Ten.value(), Some(10));
        assert_eq!(MaxIterations::Unlimited.value(), None);
    }

    #[test]
    fn max_iterations_names() {
        assert_eq!(MaxIterations::One.name(), "1");
        assert_eq!(MaxIterations::Three.name(), "3");
        assert_eq!(MaxIterations::Five.name(), "5");
        assert_eq!(MaxIterations::Ten.name(), "10");
        assert_eq!(MaxIterations::Unlimited.name(), "Unlimited");
    }

    #[test]
    fn max_iterations_default_is_five() {
        assert_eq!(MaxIterations::default(), MaxIterations::Five);
    }

    #[test]
    fn settings_state_preserves_models_when_modified() {
        let settings = SettingsState {
            planning_model: Model::Claude,
            execution_model: Model::Gemini,
            selected_index: 1,
            previous_mode: Some(AppMode::Chat),
            ..SettingsState::default()
        };

        assert_eq!(settings.planning_model, Model::Claude);
        assert_eq!(settings.execution_model, Model::Gemini);
        assert_eq!(settings.selected_index, 1);
        assert_eq!(settings.previous_mode, Some(AppMode::Chat));
    }

    #[test]
    fn settings_state_clone_works() {
        let settings = SettingsState {
            planning_model: Model::Claude,
            selected_index: 1,
            ..SettingsState::default()
        };

        let cloned = settings.clone();
        assert_eq!(cloned.planning_model, Model::Claude);
        assert_eq!(cloned.selected_index, 1);
    }

    #[test]
    fn settings_state_has_model_availability() {
        let settings = SettingsState::default();
        // model_availability is populated by check_all() during default init
        // We can't assert specific values since they depend on the system,
        // but we can verify the field exists and is accessible
        // Verify the fields exist and are accessible by using them in assertions
        // These are trivially true - the point is verifying field access compiles
        let _ = format!(
            "codex={}, claude={}, gemini={}",
            settings.model_availability.codex,
            settings.model_availability.claude,
            settings.model_availability.gemini
        );
    }

    #[test]
    fn is_model_available_returns_correct_values() {
        // Override with known values for testing
        let settings = SettingsState {
            model_availability: ModelAvailability {
                codex: true,
                claude: false,
                gemini: true,
            },
            ..SettingsState::default()
        };

        assert!(settings.is_model_available(Model::Codex));
        assert!(!settings.is_model_available(Model::Claude));
        assert!(settings.is_model_available(Model::Gemini));
    }

    #[test]
    fn is_model_available_all_false() {
        let settings = SettingsState {
            model_availability: ModelAvailability {
                codex: false,
                claude: false,
                gemini: false,
            },
            ..SettingsState::default()
        };

        assert!(!settings.is_model_available(Model::Codex));
        assert!(!settings.is_model_available(Model::Claude));
        assert!(!settings.is_model_available(Model::Gemini));
    }

    #[test]
    fn is_model_available_all_true() {
        let settings = SettingsState {
            model_availability: ModelAvailability {
                codex: true,
                claude: true,
                gemini: true,
            },
            ..SettingsState::default()
        };

        assert!(settings.is_model_available(Model::Codex));
        assert!(settings.is_model_available(Model::Claude));
        assert!(settings.is_model_available(Model::Gemini));
    }
}

#[cfg(test)]
mod scroll_state_tests {
    use super::*;

    #[test]
    fn scroll_state_new_has_defaults() {
        let state = ScrollState::new();
        assert_eq!(state.offset, 0, "Initial offset should be 0");
        assert!(
            state.auto_scroll,
            "Auto-scroll should be enabled by default"
        );
    }

    #[test]
    fn scroll_up_at_boundary_does_nothing() {
        let mut state = ScrollState::new();
        state.offset = 0;
        state.scroll_up();
        assert_eq!(
            state.offset, 0,
            "Offset should stay at 0 when scrolling up from top"
        );
        assert!(
            !state.auto_scroll,
            "Auto-scroll should be disabled after manual scroll"
        );
    }

    #[test]
    fn scroll_up_decrements_offset() {
        let mut state = ScrollState::new();
        state.offset = 10;
        state.scroll_up();
        assert_eq!(state.offset, 9, "Offset should decrement by 1");
        assert!(
            !state.auto_scroll,
            "Auto-scroll should be disabled after manual scroll"
        );
    }

    #[test]
    fn scroll_down_increments_offset() {
        let mut state = ScrollState::new();
        state.offset = 5;
        let content_len = 100;
        let visible_height = 20;
        state.scroll_down(content_len, visible_height);
        assert_eq!(state.offset, 6, "Offset should increment by 1");
    }

    #[test]
    fn scroll_down_at_bottom_boundary_stays_at_max() {
        let mut state = ScrollState::new();
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = content_len - visible_height; // 80
        state.offset = max_scroll;
        state.scroll_down(content_len, visible_height);
        assert_eq!(
            state.offset, max_scroll,
            "Offset should not exceed max_scroll"
        );
        assert!(state.auto_scroll, "Auto-scroll should be enabled at bottom");
    }

    #[test]
    fn scroll_down_enables_auto_scroll_at_bottom() {
        let mut state = ScrollState::new();
        state.auto_scroll = false;
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = content_len - visible_height; // 80
        state.offset = max_scroll - 1;
        state.scroll_down(content_len, visible_height);
        assert_eq!(state.offset, max_scroll);
        assert!(
            state.auto_scroll,
            "Auto-scroll should be re-enabled when reaching bottom"
        );
    }

    #[test]
    fn page_up_scrolls_by_page_size() {
        let mut state = ScrollState::new();
        state.offset = 50;
        let page_size = 10;
        state.page_up(page_size);
        assert_eq!(state.offset, 40, "Offset should decrease by page_size");
        assert!(
            !state.auto_scroll,
            "Auto-scroll should be disabled after manual scroll"
        );
    }

    #[test]
    fn page_up_at_top_saturates_to_zero() {
        let mut state = ScrollState::new();
        state.offset = 5;
        let page_size = 10;
        state.page_up(page_size);
        assert_eq!(state.offset, 0, "Offset should saturate at 0");
        assert!(
            !state.auto_scroll,
            "Auto-scroll should be disabled after manual scroll"
        );
    }

    #[test]
    fn page_down_scrolls_by_page_size() {
        let mut state = ScrollState::new();
        state.offset = 10;
        let content_len = 100;
        let visible_height = 20;
        let page_size = 10;
        state.page_down(content_len, visible_height, page_size);
        assert_eq!(state.offset, 20, "Offset should increase by page_size");
    }

    #[test]
    fn page_down_at_bottom_caps_at_max() {
        let mut state = ScrollState::new();
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = content_len - visible_height; // 80
        let page_size = 10;
        state.offset = 75;
        state.page_down(content_len, visible_height, page_size);
        assert_eq!(state.offset, max_scroll, "Offset should cap at max_scroll");
        assert!(state.auto_scroll, "Auto-scroll should be enabled at bottom");
    }

    #[test]
    fn scroll_to_top_sets_offset_to_zero() {
        let mut state = ScrollState::new();
        state.offset = 50;
        state.auto_scroll = true;
        state.scroll_to_top();
        assert_eq!(state.offset, 0, "Offset should be set to 0");
        assert!(
            !state.auto_scroll,
            "Auto-scroll should be disabled after jump to top"
        );
    }

    #[test]
    fn scroll_to_bottom_sets_offset_to_max() {
        let mut state = ScrollState::new();
        state.offset = 10;
        state.auto_scroll = false;
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = content_len - visible_height; // 80
        state.scroll_to_bottom(content_len, visible_height);
        assert_eq!(
            state.offset, max_scroll,
            "Offset should be set to max_scroll"
        );
        assert!(
            state.auto_scroll,
            "Auto-scroll should be enabled after jump to bottom"
        );
    }

    #[test]
    fn scroll_to_bottom_from_various_positions() {
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = 80;

        // From top
        let mut state = ScrollState::new();
        state.offset = 0;
        state.scroll_to_bottom(content_len, visible_height);
        assert_eq!(state.offset, max_scroll);
        assert!(state.auto_scroll);

        // From middle
        let mut state = ScrollState::new();
        state.offset = 50;
        state.scroll_to_bottom(content_len, visible_height);
        assert_eq!(state.offset, max_scroll);
        assert!(state.auto_scroll);

        // Already at bottom
        let mut state = ScrollState::new();
        state.offset = max_scroll;
        state.scroll_to_bottom(content_len, visible_height);
        assert_eq!(state.offset, max_scroll);
        assert!(state.auto_scroll);
    }

    #[test]
    fn auto_scroll_if_enabled_scrolls_when_enabled() {
        let mut state = ScrollState::new();
        state.offset = 10;
        state.auto_scroll = true;
        let content_len = 100;
        let visible_height = 20;
        let max_scroll = 80;
        state.auto_scroll_if_enabled(content_len, visible_height);
        assert_eq!(
            state.offset, max_scroll,
            "Offset should jump to bottom when auto_scroll is true"
        );
    }

    #[test]
    fn auto_scroll_if_enabled_does_nothing_when_disabled() {
        let mut state = ScrollState::new();
        state.offset = 10;
        state.auto_scroll = false;
        let content_len = 100;
        let visible_height = 20;
        state.auto_scroll_if_enabled(content_len, visible_height);
        assert_eq!(
            state.offset, 10,
            "Offset should not change when auto_scroll is false"
        );
    }

    #[test]
    fn scroll_offset_calculation_when_content_shorter_than_visible() {
        let mut state = ScrollState::new();
        let content_len = 10;
        let visible_height = 20;
        // max_scroll = 10 - 20 = saturates to 0
        state.offset = 50; // Invalid, but let's see what happens
        state.scroll_to_bottom(content_len, visible_height);
        assert_eq!(
            state.offset, 0,
            "When content is shorter than visible, offset should be 0"
        );
    }

    #[test]
    fn reset_returns_to_initial_state() {
        let mut state = ScrollState::new();
        state.offset = 50;
        state.auto_scroll = false;
        state.reset();
        assert_eq!(state.offset, 0, "Reset should set offset to 0");
        assert!(state.auto_scroll, "Reset should enable auto-scroll");
    }

    #[test]
    fn scroll_down_with_content_equal_to_visible() {
        let mut state = ScrollState::new();
        let content_len = 20;
        let visible_height = 20;
        // max_scroll = 0
        state.offset = 0;
        state.scroll_down(content_len, visible_height);
        assert_eq!(
            state.offset, 0,
            "When content equals visible, offset should stay 0"
        );
        assert!(state.auto_scroll, "Should be considered at bottom");
    }
}
