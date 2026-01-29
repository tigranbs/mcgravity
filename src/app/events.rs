//! Event handling logic for the App.

use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use crate::app::input::RapidInputDetector;
use crate::app::state::{EnterBehavior, InitialSetupField, SettingsItem};
use crate::app::{AppMode, FlowEvent};
use crate::core::{FlowPhase, run_flow};
use crate::file_search::SearchResult;
use crate::fs::PersistedSettings;
use crate::tui::widgets::{MAX_OUTPUT_LINES, OutputLine, calculate_visual_line_count};

/// Scroll page size for navigation.
const SCROLL_PAGE_SIZE: usize = 10;

impl App {
    /// Handles pasted text from bracketed paste mode.
    ///
    /// When bracketed paste mode is enabled, multi-line pasted text is delivered
    /// as a single `Event::Paste(String)` event rather than individual key events.
    /// This prevents accidental submission of multi-line text (since Enter would
    /// otherwise be interpreted as submit).
    ///
    /// The pasted text is inserted at the current cursor position in Chat mode.
    /// In Settings mode, paste events are ignored.
    ///
    /// # Line Ending Normalization
    ///
    /// Windows-style line endings (`\r\n`) are normalized to Unix-style (`\n`)
    /// before processing. The `tui-textarea` crate handles `\n` and `\r\n`
    /// but not standalone `\r`, so we normalize those as well.
    ///
    /// # Control Character Filtering
    ///
    /// Control characters (except newlines and tabs) are filtered out to prevent
    /// insertion of non-printable characters that could corrupt the text display.
    pub fn handle_paste(&mut self, text: &str) {
        if self.mode != AppMode::Chat || self.is_running {
            return;
        }

        // Empty paste - do nothing
        if text.is_empty() {
            return;
        }

        // Normalize line endings:
        // 1. Convert \r\n to \n (Windows)
        // 2. Convert standalone \r to \n (old Mac)
        // tui-textarea handles \n and \r\n but not standalone \r
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

        // Filter control characters (except newlines)
        // This prevents insertion of non-printable characters
        // Note: tabs are filtered because they can cause display issues
        let filtered: String = normalized
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();

        // Use tui-textarea's insert_str which handles multi-line text correctly
        self.text_input.textarea.insert_str(&filtered);

        // Mark as dirty for autosave
        self.text_input.is_dirty = true;
        self.text_input.last_edit_time = Some(Instant::now());

        // Update @ token detection after paste
        self.update_at_token();

        // Update slash command popup
        self.update_slash_command_popup();
    }

    /// Handles a key event.
    ///
    /// The application operates in three modes:
    /// - **Chat**: Unified interface for task input and output display
    /// - **Settings**: Modal overlay for model configuration
    /// - **Finished**: Modal overlay after flow completion
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Debug logging for key events with timing info (enable with MCGRAVITY_DEBUG_KEYS=1)
        if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
            let elapsed_since_last = self
                .text_input
                .rapid_input
                .last_key_time()
                .map_or(0, |t| Instant::now().duration_since(t).as_millis());

            eprintln!(
                "[DEBUG KEY] code={:?} modifiers={:?} kind={:?} elapsed_ms={elapsed_since_last}",
                key.code, key.modifiers, key.kind
            );
        }

        // Global hotkey: Ctrl+S opens settings from Chat mode
        if self.mode == AppMode::Chat
            && key.code == KeyCode::Char('s')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.open_settings();
            return;
        }

        match self.mode {
            AppMode::Chat => self.handle_chat_key(key),
            AppMode::Settings => self.handle_settings_key(key),
            AppMode::Finished => self.handle_finished_key(key),
            AppMode::InitialSetup => self.handle_initial_setup_key(key),
        }
    }

    /// Opens the settings panel.
    ///
    /// Settings can only be opened from Chat mode, so `previous_mode` is
    /// always set to `Chat`. The field is kept for API consistency.
    pub(crate) fn open_settings(&mut self) {
        // Don't nest settings - if already in settings, do nothing
        if self.mode == AppMode::Settings {
            return;
        }

        // Save the current mode to return to when closing (always Chat)
        self.settings.previous_mode = Some(self.mode);
        self.settings.selected_index = 0; // Reset selection
        self.mode = AppMode::Settings;
    }

    /// Closes the settings panel and returns to Chat mode.
    /// Settings are auto-saved to .mcgravity/settings.json.
    pub(crate) fn close_settings(&mut self) {
        // Save settings before closing
        let persisted = PersistedSettings::from(&self.settings);
        if let Err(e) = self.paths.save_settings(&persisted) {
            // Log warning but don't prevent closing
            self.flow_ui
                .output
                .push(OutputLine::warning(format!("Failed to save settings: {e}")));
        }

        // Always return to Chat mode (the only non-settings mode)
        self.settings.previous_mode = None;
        self.mode = AppMode::Chat;
    }

    /// Handles key events in unified chat mode.
    ///
    /// Key event priorities:
    /// 1. File popup handling (when popup is visible)
    /// 2. Command popup handling (when popup is visible)
    /// 3. Output scrolling (Ctrl+Arrow keys, PageUp/PageDown)
    /// 4. Quit shortcuts (Esc, Ctrl+C)
    /// 5. Text input handling (default)
    #[allow(clippy::too_many_lines)]
    fn handle_chat_key(&mut self, key: KeyEvent) {
        // Priority 1: File popup handling (when popup is visible)
        if !self.is_running && self.should_show_file_popup() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                    self.file_popup_up();
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                    self.file_popup_down();
                    return;
                }
                // Tab/Enter selects from popup ONLY when:
                // 1. There are actual matches to select (popup in Showing state)
                // 2. For Enter: no CONTROL modifier (Ctrl+Enter should submit, not select)
                KeyCode::Tab => {
                    if self.has_file_matches() {
                        self.select_file_from_popup();
                        return;
                    }
                    // No matches - fall through to normal handling
                }
                KeyCode::Enter if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.has_file_matches() {
                        self.select_file_from_popup();
                        return;
                    }
                    // No matches - fall through to normal handling (insert newline)
                }
                KeyCode::Esc => {
                    self.dismiss_file_popup();
                    return;
                }
                // Other keys fall through to normal handling
                _ => {}
            }
        }

        // Priority 1.5: Command popup handling (when popup is visible)
        if !self.is_running && self.should_show_command_popup() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                    self.command_popup_up();
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                    self.command_popup_down();
                    return;
                }
                // Tab selects from popup but does NOT submit (allows adding arguments)
                KeyCode::Tab => {
                    if self.has_command_matches() {
                        self.select_command_from_popup();
                        return;
                    }
                    // No matches - fall through to normal handling
                }
                // Enter selects from popup AND submits the command
                KeyCode::Enter if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.has_command_matches() {
                        self.select_command_from_popup();
                        self.submit_text_input();
                        return;
                    }
                    // No matches - fall through to normal handling
                }
                KeyCode::Esc => {
                    self.dismiss_command_popup();
                    return;
                }
                // Other keys fall through to normal handling
                _ => {}
            }
        }

        // Priority 2: Output scrolling with Ctrl modifier (doesn't conflict with text navigation)
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Up => {
                    self.scroll_output_up();
                    return;
                }
                KeyCode::Down => {
                    self.scroll_output_down();
                    return;
                }
                KeyCode::Home => {
                    self.scroll_output_to_top();
                    return;
                }
                KeyCode::End => {
                    self.scroll_output_to_bottom();
                    return;
                }
                _ => {}
            }
        }

        // Priority 3: Page scrolling (unambiguous keys that don't conflict with text editing)
        match key.code {
            KeyCode::PageUp => {
                self.page_up_output();
                return;
            }
            KeyCode::PageDown => {
                self.page_down_output();
                return;
            }
            _ => {}
        }

        // Priority 4: Quit shortcuts
        if self.is_running && key.code == KeyCode::Esc {
            self.trigger_shutdown();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                return;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.trigger_shutdown();
                self.should_quit = true;
                return;
            }
            _ => {}
        }

        // Priority 5: Text input handling (default)
        if self.is_running {
            return;
        }
        self.handle_text_input(key);
    }

    /// Handles text input key events.
    ///
    /// Key bindings:
    /// - `Enter` - Submit task (traditional chat behavior)
    /// - `Shift+Enter` - Insert newline
    /// - `Alt+Enter` - Insert newline (alternative for terminal compatibility)
    /// - `Ctrl+Enter` - Submit task (alternative)
    /// - `Ctrl+D` - Submit task (alternative)
    /// - Other keys - Delegated to `tui-textarea` for handling
    ///
    /// # Design Decision: Traditional Chat Behavior
    ///
    /// Like most chat applications, `McGravity` uses Enter to submit and
    /// `Shift+Enter` to create new lines. This matches user expectations from
    /// applications like Slack, Discord, and most messaging platforms.
    ///
    /// # Rapid Input Detection
    ///
    /// Rapid input detection is still tracked for potential future use cases.
    #[allow(clippy::match_same_arms)] // Separate arms for clarity and documentation
    #[allow(clippy::too_many_lines)] // Debug logging adds necessary lines; core logic is extracted to RapidInputDetector
    fn handle_text_input(&mut self, key: KeyEvent) {
        let now = Instant::now();

        // Process rapid input detection
        let rapid_result = self.text_input.rapid_input.process_key(&key);

        // Log state transitions (enable with MCGRAVITY_DEBUG_KEYS=1)
        if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
            if rapid_result.just_activated {
                eprintln!(
                    "[DEBUG RAPID] ACTIVATED: count {} (threshold={})",
                    self.text_input.rapid_input.key_count(),
                    RapidInputDetector::count_threshold()
                );
            } else if rapid_result.just_deactivated {
                eprintln!(
                    "[DEBUG RAPID] DEACTIVATED: elapsed_ms={} (reset after pause)",
                    rapid_result.elapsed_ms
                );
            }
        }

        // Enhanced debug logging for Enter key handling (enable with MCGRAVITY_DEBUG_KEYS=1)
        if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() && key.code == KeyCode::Enter {
            let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
            let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let has_alt = key.modifiers.contains(KeyModifiers::ALT);
            let input_text = self.text_input.collect_text();
            let input_not_empty = !input_text.trim().is_empty();

            let action = if has_ctrl && input_not_empty {
                "submit (Ctrl+Enter)"
            } else if has_ctrl {
                "no-op (Ctrl+Enter with empty input)"
            } else if has_shift || has_alt {
                "newline (Shift+Enter or Alt+Enter)"
            } else if rapid_result.is_rapid {
                "newline (rapid input - paste detected)"
            } else if input_not_empty {
                "submit (Enter)"
            } else {
                "no-op (Enter with empty input)"
            };

            eprintln!(
                "[DEBUG ENTER] modifiers={:?} shift={} ctrl={} alt={} is_rapid={} rapid_count={} rapid_has_chars={} elapsed_ms={} input_len={} input_not_empty={} action={}",
                key.modifiers,
                has_shift,
                has_ctrl,
                has_alt,
                rapid_result.is_rapid,
                self.text_input.rapid_input.key_count(),
                self.text_input.rapid_input.has_chars(),
                rapid_result.elapsed_ms,
                input_text.len(),
                input_not_empty,
                action
            );
        }

        match key.code {
            // Submit: Ctrl+Enter (alternative submit action, kept for compatibility)
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_text_input();
            }
            // Submit: Ctrl+D (alternative submit action)
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_text_input();
            }
            // Newline: Ctrl+J (universal - works on ALL terminals)
            // Ctrl+J = ASCII 10 (LF), the standard newline character.
            // This works reliably because it's a control character, not a modifier+key combo.
            // Critical for iPad keyboards and terminals that don't report Shift+Enter modifiers.
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.textarea.insert_newline();
                self.text_input.is_dirty = true;
                self.text_input.last_edit_time = Some(now);
                self.update_at_token();
                self.update_slash_command_popup();
            }
            // Newline: Shift+Enter or Alt+Enter (explicit newline action)
            // Alt+Enter provided as fallback for terminals with Shift+Enter issues.
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.text_input.textarea.insert_newline();
                self.text_input.is_dirty = true;
                self.text_input.last_edit_time = Some(now);
                self.update_at_token();
                self.update_slash_command_popup();
            }
            // Submit or Newline: Plain Enter
            // - Normal typing: Submit task (traditional chat apps style)
            // - Rapid input mode (paste fallback): Insert newline to avoid
            //   submitting each line of pasted text individually
            // - Backslash-Enter escape: Treat `\` + Enter as newline (works on ALL terminals)
            KeyCode::Enter => {
                // Check for backslash-Enter escape sequence (like Claude Code)
                // This provides a universal way to insert newlines that works on any terminal.
                let text = self.text_input.collect_text();
                if text.ends_with('\\') {
                    // Remove trailing backslash and insert newline
                    self.text_input.textarea.delete_char();
                    self.text_input.textarea.insert_newline();
                    self.text_input.is_dirty = true;
                    self.text_input.last_edit_time = Some(now);
                    self.update_at_token();
                    self.update_slash_command_popup();
                    return;
                }

                if rapid_result.is_rapid {
                    // During paste operation detected via rapid input, treat Enter
                    // as newline to prevent accidental submission of multi-line paste.
                    // Also activate rapid input mode if not already active, so subsequent
                    // Enter keys in this paste will also be treated as newlines.
                    if !self.text_input.rapid_input.is_active() {
                        self.text_input.rapid_input.activate();
                        if std::env::var("MCGRAVITY_DEBUG_KEYS").is_ok() {
                            eprintln!(
                                "[DEBUG RAPID] ACTIVATED via Enter: count={} (threshold={})",
                                self.text_input.rapid_input.key_count(),
                                RapidInputDetector::count_threshold()
                            );
                        }
                    }
                    self.text_input.textarea.insert_newline();
                    self.text_input.is_dirty = true;
                    self.text_input.last_edit_time = Some(now);
                    self.update_at_token();
                    self.update_slash_command_popup();
                } else {
                    match self.settings.enter_behavior {
                        EnterBehavior::Submit => self.submit_text_input(),
                        EnterBehavior::Newline => {
                            self.text_input.textarea.insert_newline();
                            self.text_input.is_dirty = true;
                            self.text_input.last_edit_time = Some(now);
                            self.update_at_token();
                            self.update_slash_command_popup();
                        }
                    }
                }
            }
            // Delegate all other keys to tui-textarea
            _ => {
                // tui-textarea handles: backspace, delete, navigation, character input, etc.
                // Track whether this is a text-modifying key for autosave
                let is_text_modifying = matches!(
                    key.code,
                    KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
                );

                self.text_input.textarea.input(key);

                if is_text_modifying {
                    self.text_input.is_dirty = true;
                    self.text_input.last_edit_time = Some(now);
                }

                self.update_at_token();
                self.update_slash_command_popup();
            }
        }
    }

    /// Handles key events in settings mode.
    fn handle_settings_key(&mut self, key: KeyEvent) {
        let items = SettingsItem::all();
        let max_index = items.len().saturating_sub(1);

        match key.code {
            // Navigation: Up / k
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings.selected_index = self.settings.selected_index.saturating_sub(1);
            }
            // Navigation: Down / j
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings.selected_index = (self.settings.selected_index + 1).min(max_index);
            }
            // Emacs-style navigation: Ctrl+P (up)
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.settings.selected_index = self.settings.selected_index.saturating_sub(1);
            }
            // Emacs-style navigation: Ctrl+N (down)
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.settings.selected_index = (self.settings.selected_index + 1).min(max_index);
            }
            // Toggle/Cycle: Enter or Space (common in checkboxes)
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.cycle_current_setting();
            }
            // Close settings with Esc or 'q' (saves automatically like Codex)
            KeyCode::Char('q') | KeyCode::Esc => {
                self.close_settings();
            }
            // Ctrl+C in settings closes settings instead of quitting
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_settings();
            }
            _ => {}
        }
    }

    /// Handles key events in finished mode.
    fn handle_finished_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.reset_session();
            }
            KeyCode::Char('q') => {
                self.trigger_shutdown();
                self.should_quit = true;
            }
            _ => {}
        }
    }

    /// Handles key events in initial setup mode.
    ///
    /// The initial setup modal cannot be dismissed with Esc - the user must
    /// select models and confirm to proceed. This ensures settings are always
    /// configured on first run.
    ///
    /// ## Key Bindings
    ///
    /// - `Up` / `k` - Navigate to previous field (wraps around)
    /// - `Down` / `j` - Navigate to next field (wraps around)
    /// - `Enter` / `Space` - Cycle through model options for selected field
    /// - `c` / `C` - Confirm selection and save settings, transition to Chat mode
    /// - `Ctrl+C` - Quit application
    /// - `Esc` - Does nothing (modal cannot be dismissed without confirming)
    #[allow(clippy::match_same_arms)] // Esc arm kept separate for documentation
    fn handle_initial_setup_key(&mut self, key: KeyEvent) {
        match key.code {
            // Navigation: Up / k - move to previous field (wraps around)
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(setup) = &mut self.initial_setup {
                    setup.selected_field = match setup.selected_field {
                        InitialSetupField::PlanningModel => InitialSetupField::ExecutionModel,
                        InitialSetupField::ExecutionModel => InitialSetupField::PlanningModel,
                    };
                }
            }
            // Navigation: Down / j - move to next field (wraps around)
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(setup) = &mut self.initial_setup {
                    setup.selected_field = setup.selected_field.next();
                }
            }
            // Cycle model: Enter or Space
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(setup) = &mut self.initial_setup {
                    match setup.selected_field {
                        InitialSetupField::PlanningModel => {
                            setup.planning_model = setup.planning_model.next();
                        }
                        InitialSetupField::ExecutionModel => {
                            setup.execution_model = setup.execution_model.next();
                        }
                    }
                }
            }
            // Confirm: 'c' or 'C' key - save settings and transition to Chat mode
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.confirm_initial_setup();
            }
            KeyCode::Char('C') => {
                self.confirm_initial_setup();
            }
            // Quit: Ctrl+C
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.trigger_shutdown();
                self.should_quit = true;
            }
            // Esc does nothing - modal cannot be dismissed without confirming
            KeyCode::Esc => {}
            // All other keys are ignored
            _ => {}
        }
    }

    /// Confirms the initial setup selection and transitions to Chat mode.
    ///
    /// This method:
    /// 1. Copies models from `initial_setup` state to `settings` state
    /// 2. Saves settings to `.mcgravity/settings.json`
    /// 3. Sets mode to `AppMode::Chat`
    /// 4. Clears `initial_setup` state (sets to `None`)
    ///
    /// If saving fails, a warning is logged to output but the transition still occurs.
    fn confirm_initial_setup(&mut self) {
        // Copy models from initial_setup to settings
        if let Some(setup) = &self.initial_setup {
            self.settings.planning_model = setup.planning_model;
            self.settings.execution_model = setup.execution_model;
        }

        // Save settings to file
        let persisted = PersistedSettings::from(&self.settings);
        if let Err(e) = self.paths.save_settings(&persisted) {
            // Log warning but continue with the transition
            self.flow_ui
                .output
                .push(OutputLine::warning(format!("Failed to save settings: {e}")));
        }

        // Transition to Chat mode
        self.mode = AppMode::Chat;

        // Clear initial_setup state
        self.initial_setup = None;
    }

    // =========================================================================
    // Output Scrolling Methods (for Chat mode)
    // =========================================================================

    /// Scrolls output up by one line.
    fn scroll_output_up(&mut self) {
        self.flow_ui.output_scroll.scroll_up();
    }

    /// Scrolls output down by one line.
    fn scroll_output_down(&mut self) {
        let content_len = self.output_visual_line_count();
        self.flow_ui
            .output_scroll
            .scroll_down(content_len, self.layout.output_visible_height());
    }

    /// Scrolls output to top.
    fn scroll_output_to_top(&mut self) {
        self.flow_ui.output_scroll.scroll_to_top();
    }

    /// Scrolls output to bottom (re-enables auto-scroll).
    fn scroll_output_to_bottom(&mut self) {
        let content_len = self.output_visual_line_count();
        self.flow_ui
            .output_scroll
            .scroll_to_bottom(content_len, self.layout.output_visible_height());
    }

    /// Scrolls output up by one page.
    fn page_up_output(&mut self) {
        self.flow_ui.output_scroll.page_up(SCROLL_PAGE_SIZE);
    }

    /// Scrolls output down by one page.
    fn page_down_output(&mut self) {
        let content_len = self.output_visual_line_count();
        self.flow_ui.output_scroll.page_down(
            content_len,
            self.layout.output_visible_height(),
            SCROLL_PAGE_SIZE,
        );
    }

    /// Cycles through options for the currently selected setting.
    fn cycle_current_setting(&mut self) {
        let items = SettingsItem::all();
        let Some(current_item) = items.get(self.settings.selected_index) else {
            return;
        };

        match current_item {
            SettingsItem::PlanningModel => {
                self.settings.planning_model = self.settings.planning_model.next();
            }
            SettingsItem::ExecutionModel => {
                self.settings.execution_model = self.settings.execution_model.next();
            }
            SettingsItem::EnterBehavior => {
                self.settings.enter_behavior = self.settings.enter_behavior.next();
            }
            SettingsItem::MaxIterations => {
                self.settings.max_iterations = self.settings.max_iterations.next();
            }
        }
    }

    /// Starts the orchestration flow with models from settings.
    ///
    /// Uses the models configured in `settings.planning_model` and
    /// `settings.execution_model`. If never configured, uses defaults.
    pub(super) fn start_flow(&mut self) {
        self.reset_shutdown();
        let tx = self.event_sender();
        let shutdown_rx = self.shutdown_receiver();
        let input_path = self.flow.input_path.clone();
        let input_text = self.flow.input_text.clone();
        let paths = self.paths.clone();

        // Get models from settings (they always have valid values)
        let planning_model = self.settings.planning_model;
        let execution_model = self.settings.execution_model;
        let max_iterations = self.settings.max_iterations.value();

        // Create executor instances for the selected models
        let planning_executor = planning_model.executor();
        let execution_executor = execution_model.executor();

        self.set_running(true);
        tokio::spawn(async move {
            let _ = run_flow(
                input_path,
                input_text,
                tx,
                shutdown_rx,
                planning_executor.as_ref(),
                execution_executor.as_ref(),
                max_iterations,
                paths,
            )
            .await;
        });
    }

    /// Processes pending flow events.
    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                FlowEvent::PhaseChanged(phase) => {
                    self.flow.phase = phase;
                }
                FlowEvent::Output(line) => {
                    self.flow_ui.output.push(line);
                    // Trim buffer if too large
                    if self.flow_ui.output.len() > MAX_OUTPUT_LINES {
                        let drain_count = self.flow_ui.output.len() - MAX_OUTPUT_LINES;
                        self.flow_ui.output.drain(0..drain_count);
                        self.flow_ui.output_scroll.offset = self
                            .flow_ui
                            .output_scroll
                            .offset
                            .saturating_sub(drain_count);
                        self.flow_ui.output_truncated = true;
                    }
                    self.auto_scroll_output_if_at_bottom();
                }
                FlowEvent::TodoFilesUpdated(files) => {
                    self.flow.todo_files = files;
                }
                FlowEvent::CurrentFile(file) => {
                    self.flow_ui.current_file = file;
                }
                FlowEvent::RetryWait(wait) => {
                    self.flow_ui.retry_wait = wait;
                }
                FlowEvent::ClearOutput => {
                    self.flow_ui.output.clear();
                    self.flow_ui.output_scroll.reset();
                    self.flow_ui.output_truncated = false;
                }
                FlowEvent::Done => {
                    match self.flow.phase {
                        FlowPhase::Completed | FlowPhase::NoTodoFiles => {
                            self.mode = AppMode::Finished;
                        }
                        _ => {
                            self.mode = AppMode::Chat;
                            // Restore task text from .mcgravity/task.md after cancellation or failure
                            // so the user can modify and retry their task
                            self.load_saved_task();
                        }
                    }
                    self.is_running = false;
                }
                FlowEvent::SearchResult { generation, result } => {
                    self.handle_search_result(generation, result);
                }
                FlowEvent::TaskTextUpdated(text) => {
                    // Update flow.input_text to keep the read-only Task Text panel
                    // synchronized with the on-disk task.md state.
                    // Note: We only update flow.input_text (used for rendering when running),
                    // not text_input.textarea (the editable input), to avoid clobbering
                    // any user edits in progress.
                    self.flow.input_text = text;
                }
            }
        }
    }

    /// Sets the running flag.
    pub fn set_running(&mut self, running: bool) {
        self.is_running = running;
    }

    /// Handles a search result from the background task.
    ///
    /// This method is called when a `FlowEvent::SearchResult` is received.
    /// It checks the generation to ensure we only process the latest result.
    fn handle_search_result(&mut self, generation: u64, result: SearchResult) {
        // Ignore stale results (from older searches)
        if generation != self.text_input.search_generation {
            return;
        }

        let matches = result.matches;
        let query = self.text_input.last_search_query.as_deref().unwrap_or("");

        // Update popup state
        self.text_input.file_popup_state = if matches.is_empty() {
            if query.is_empty() {
                // Empty query with no matches - should show NoMatches
                // (the search task would have searched with empty query)
                use crate::tui::widgets::PopupState;
                PopupState::NoMatches
            } else {
                use crate::tui::widgets::PopupState;
                PopupState::NoMatches
            }
        } else {
            // Keep selection in bounds
            use crate::tui::widgets::PopupState;
            let selected = if let PopupState::Showing {
                selected: prev_selected,
                ..
            } = &self.text_input.file_popup_state
            {
                (*prev_selected).min(matches.len().saturating_sub(1))
            } else {
                0
            };
            PopupState::Showing { matches, selected }
        };
    }

    /// Calculates the total visual line count for output after wrapping.
    fn output_visual_line_count(&self) -> usize {
        calculate_visual_line_count(&self.flow_ui.output, self.layout.output_content_width())
    }

    /// Auto-scrolls output panel if auto-scroll is enabled.
    fn auto_scroll_output_if_at_bottom(&mut self) {
        let content_len = self.output_visual_line_count();
        self.flow_ui
            .output_scroll
            .auto_scroll_if_enabled(content_len, self.layout.output_visible_height());
    }
}
