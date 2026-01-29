//! Rendering methods for the App.
//!
//! This module contains all UI rendering logic including:
//! - **Chat mode**: Unified interface with header, output, status, progress, input, and footer
//! - **Settings panel**: Modal overlay for model configuration
//! - **Finished dialog**: Modal overlay after flow completion
//! - **Initial setup**: First-run modal for model selection

mod chat;
mod finished;
mod initial_setup;
mod settings;

use ratatui::Frame;

use super::{App, AppMode};

impl App {
    /// Renders the application UI.
    ///
    /// The application has four modes:
    /// - **Chat**: Main unified interface with input, output, and status
    /// - **Settings**: Modal overlay for model configuration
    /// - **Finished**: Modal overlay prompting for next action
    /// - **`InitialSetup`**: First-run modal for selecting default models
    pub fn render(&self, frame: &mut Frame) {
        match self.mode {
            AppMode::Chat => self.render_chat(frame),
            AppMode::Settings => {
                // Render chat as background, then overlay settings
                self.render_chat(frame);
                self.render_settings(frame);
            }
            AppMode::Finished => {
                // Render chat as background, then overlay completion dialog
                self.render_chat(frame);
                self.render_finished_dialog(frame);
            }
            AppMode::InitialSetup => {
                // Render chat as background, then overlay initial setup modal
                self.render_chat(frame);
                self.render_initial_setup(frame);
            }
        }
    }
}
