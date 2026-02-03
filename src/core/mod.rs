//! Core business logic for orchestration.

pub mod cli_check;
pub mod commands;
pub mod executor;
pub mod flow;
pub mod prompts;
pub mod retry;
pub mod runner;
pub mod task_utils;

pub use cli_check::{
    CommandResolution, ModelAvailability, check_cli_in_path, is_safe_command_name,
    resolve_cli_command,
};
pub use commands::{
    ClearCommand, CommandContext, CommandRegistry, CommandResult, ExitCommand, SettingsCommand,
    SlashCommand,
};
pub use executor::{AiCliExecutor, ClaudeExecutor, CliOutput, CodexExecutor, GeminiExecutor};
pub use flow::{FlowPhase, FlowState};
pub use prompts::{wrap_for_execution, wrap_for_planning};
pub use retry::RetryConfig;
pub use runner::run_flow;

/// Available AI CLI models for orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Model {
    /// `OpenAI` Codex CLI.
    #[default]
    Codex,
    /// Anthropic Claude Code CLI.
    Claude,
    /// Google Gemini CLI.
    Gemini,
}

impl Model {
    /// Returns the display name for the model.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Gemini => "Gemini",
        }
    }

    /// Returns a short description of the model.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Codex => "OpenAI Codex CLI",
            Self::Claude => "Anthropic Claude CLI",
            Self::Gemini => "Google Gemini CLI",
        }
    }

    /// Returns the CLI command name for this model.
    ///
    /// This is the actual binary name that should be in PATH.
    #[must_use]
    pub const fn command(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }

    /// Creates an executor instance for this model.
    ///
    /// This is a factory method that returns a boxed trait object,
    /// allowing the flow runner to work with any model uniformly.
    #[must_use]
    pub fn executor(&self) -> Box<dyn AiCliExecutor> {
        match self {
            Self::Codex => Box::new(CodexExecutor),
            Self::Claude => Box::new(ClaudeExecutor),
            Self::Gemini => Box::new(GeminiExecutor),
        }
    }

    /// Returns the next model in the cycle.
    ///
    /// Cycles: Codex -> Claude -> Gemini -> Codex
    #[must_use]
    pub const fn next(&self) -> Self {
        match self {
            Self::Codex => Self::Claude,
            Self::Claude => Self::Gemini,
            Self::Gemini => Self::Codex,
        }
    }

    /// Returns the previous model in the cycle.
    ///
    /// Cycles: Codex -> Gemini -> Claude -> Codex
    #[must_use]
    pub const fn prev(&self) -> Self {
        match self {
            Self::Codex => Self::Gemini,
            Self::Claude => Self::Codex,
            Self::Gemini => Self::Claude,
        }
    }

    /// Returns all available models.
    #[must_use]
    pub const fn all() -> &'static [Model] {
        &[Model::Codex, Model::Claude, Model::Gemini]
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn model_next_cycles_correctly() {
        assert_eq!(Model::Codex.next(), Model::Claude);
        assert_eq!(Model::Claude.next(), Model::Gemini);
        assert_eq!(Model::Gemini.next(), Model::Codex);
    }

    #[test]
    fn model_prev_cycles_correctly() {
        assert_eq!(Model::Codex.prev(), Model::Gemini);
        assert_eq!(Model::Claude.prev(), Model::Codex);
        assert_eq!(Model::Gemini.prev(), Model::Claude);
    }

    #[test]
    fn model_next_and_prev_are_inverse() {
        for model in Model::all() {
            assert_eq!(model.next().prev(), *model);
            assert_eq!(model.prev().next(), *model);
        }
    }

    #[test]
    fn model_all_returns_all_models() {
        let models = Model::all();
        assert_eq!(models.len(), 3);
        assert!(models.contains(&Model::Codex));
        assert!(models.contains(&Model::Claude));
        assert!(models.contains(&Model::Gemini));
    }

    #[test]
    fn model_default_is_codex() {
        assert_eq!(Model::default(), Model::Codex);
    }

    #[test]
    fn model_names_are_not_empty() {
        for model in Model::all() {
            assert!(!model.name().is_empty());
            assert!(!model.description().is_empty());
        }
    }

    #[test]
    fn model_name_codex() {
        assert_eq!(Model::Codex.name(), "Codex");
        assert_eq!(Model::Codex.description(), "OpenAI Codex CLI");
    }

    #[test]
    fn model_name_claude() {
        assert_eq!(Model::Claude.name(), "Claude Code");
        assert_eq!(Model::Claude.description(), "Anthropic Claude CLI");
    }

    #[test]
    fn model_name_gemini() {
        assert_eq!(Model::Gemini.name(), "Gemini");
        assert_eq!(Model::Gemini.description(), "Google Gemini CLI");
    }

    #[test]
    fn model_full_cycle_returns_to_start() {
        let start = Model::Codex;
        let after_one = start.next();
        let after_two = after_one.next();
        let after_three = after_two.next();
        assert_eq!(after_three, start);
    }

    #[test]
    fn model_equality() {
        assert_eq!(Model::Codex, Model::Codex);
        assert_eq!(Model::Claude, Model::Claude);
        assert_eq!(Model::Gemini, Model::Gemini);
        assert_ne!(Model::Codex, Model::Claude);
        assert_ne!(Model::Claude, Model::Gemini);
        assert_ne!(Model::Gemini, Model::Codex);
    }

    #[test]
    fn model_clone() {
        let model = Model::Claude;
        let cloned = model;
        assert_eq!(model, cloned);
    }
}
