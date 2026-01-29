//! Slash command system for `McGravity`.
//!
//! This module provides a trait-based command architecture that allows
//! for extensible slash commands like `/exit`, `/settings`, and `/clear`.
//!
//! ## Architecture
//!
//! - [`SlashCommand`] trait defines the interface for all commands
//! - [`CommandRegistry`] manages registration and lookup of commands
//! - [`CommandResult`] indicates how the app should respond to a command
//!
//! ## Adding New Commands
//!
//! 1. Create a struct implementing [`SlashCommand`]
//! 2. Register it in [`CommandRegistry::with_builtins()`]
//! 3. Handle any new [`CommandResult`] variants in `handle_command_result()`
//!
//! ## Example
//!
//! ```rust,ignore
//! pub struct MyCommand;
//!
//! impl SlashCommand for MyCommand {
//!     fn name(&self) -> &'static str { "mycommand" }
//!     fn description(&self) -> &'static str { "Does something" }
//!     fn execute(&self, _ctx: &CommandContext) -> CommandResult {
//!         CommandResult::Message("Done!".to_string())
//!     }
//! }
//! ```

use crate::app::state::AppMode;

/// Result of executing a slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// Command executed successfully, continue normal operation.
    Continue,
    /// Command requests application exit.
    Exit,
    /// Command requests opening settings panel.
    OpenSettings,
    /// Command requests clearing task, output, and todo files.
    Clear,
    /// Command executed with a message to display.
    Message(String),
}

/// Context provided to commands during execution.
#[derive(Debug)]
pub struct CommandContext<'a> {
    /// Whether the flow is currently running.
    pub is_running: bool,
    /// Current application mode.
    pub mode: &'a AppMode,
}

/// Trait for implementing slash commands.
///
/// Implement this trait to add new slash commands to `McGravity`.
/// Commands are registered with [`CommandRegistry`] and can be executed
/// by name (without the leading slash).
///
/// # Example
///
/// ```ignore
/// struct ExitCommand;
///
/// impl SlashCommand for ExitCommand {
///     fn name(&self) -> &'static str {
///         "exit"
///     }
///
///     fn description(&self) -> &'static str {
///         "Exit the application"
///     }
///
///     fn execute(&self, _ctx: &CommandContext) -> CommandResult {
///         CommandResult::Exit
///     }
/// }
/// ```
pub trait SlashCommand: Send + Sync {
    /// Returns the command name (without the leading slash).
    fn name(&self) -> &'static str;

    /// Returns a short description for help text.
    fn description(&self) -> &'static str;

    /// Executes the command and returns the result.
    fn execute(&self, ctx: &CommandContext) -> CommandResult;

    /// Returns true if this command can execute in the current context.
    ///
    /// Default implementation allows execution when the flow is not running.
    fn can_execute(&self, ctx: &CommandContext) -> bool {
        !ctx.is_running
    }
}

/// Registry of available slash commands.
///
/// The registry stores registered commands and provides lookup functionality
/// by exact name or prefix matching for autocomplete suggestions.
pub struct CommandRegistry {
    commands: Vec<Box<dyn SlashCommand>>,
}

impl CommandRegistry {
    /// Creates a new empty command registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Registers a new command with the registry.
    pub fn register(&mut self, cmd: Box<dyn SlashCommand>) {
        self.commands.push(cmd);
    }

    /// Finds a command by exact name match.
    ///
    /// Returns `None` if no command with the given name is registered.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&dyn SlashCommand> {
        self.commands
            .iter()
            .find(|cmd| cmd.name() == name)
            .map(AsRef::as_ref)
    }

    /// Returns all registered commands.
    #[must_use]
    pub fn all(&self) -> &[Box<dyn SlashCommand>] {
        &self.commands
    }

    /// Returns all commands whose names start with the given prefix.
    ///
    /// Useful for autocomplete functionality.
    #[must_use]
    pub fn matching(&self, prefix: &str) -> Vec<&dyn SlashCommand> {
        self.commands
            .iter()
            .filter(|cmd| cmd.name().starts_with(prefix))
            .map(AsRef::as_ref)
            .collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    /// Creates a registry with all built-in commands pre-registered.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ExitCommand));
        registry.register(Box::new(SettingsCommand));
        registry.register(Box::new(ClearCommand));
        registry
    }
}

// =============================================================================
// Built-in Commands
// =============================================================================

/// Command to exit the application gracefully.
pub struct ExitCommand;

impl SlashCommand for ExitCommand {
    fn name(&self) -> &'static str {
        "exit"
    }

    fn description(&self) -> &'static str {
        "Exit the application"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Exit
    }

    /// Exit can execute even while the flow is running (user wants to quit).
    fn can_execute(&self, _ctx: &CommandContext) -> bool {
        true
    }
}

/// Command to open the settings panel.
pub struct SettingsCommand;

impl SlashCommand for SettingsCommand {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn description(&self) -> &'static str {
        "Open settings panel (Ctrl+S)"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        CommandResult::OpenSettings
    }
}

/// Command to clear task, output, and todo files.
///
/// Note: Does NOT reset settings.
pub struct ClearCommand;

impl SlashCommand for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }

    fn description(&self) -> &'static str {
        "Clear task, output, and todo files"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // The actual clearing logic will be handled by App
        // This just signals the intent
        CommandResult::Clear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple test command for unit tests.
    struct TestCommand {
        name: &'static str,
        description: &'static str,
        result: CommandResult,
    }

    impl SlashCommand for TestCommand {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> &'static str {
            self.description
        }

        fn execute(&self, _ctx: &CommandContext) -> CommandResult {
            self.result.clone()
        }
    }

    /// A command that can only execute when running.
    struct RunningOnlyCommand;

    impl SlashCommand for RunningOnlyCommand {
        fn name(&self) -> &'static str {
            "running-only"
        }

        fn description(&self) -> &'static str {
            "Only runs when flow is active"
        }

        fn execute(&self, _ctx: &CommandContext) -> CommandResult {
            CommandResult::Continue
        }

        fn can_execute(&self, ctx: &CommandContext) -> bool {
            ctx.is_running
        }
    }

    fn make_context(is_running: bool) -> CommandContext<'static> {
        static CHAT_MODE: AppMode = AppMode::Chat;
        CommandContext {
            is_running,
            mode: &CHAT_MODE,
        }
    }

    // =========================================================================
    // CommandResult Tests
    // =========================================================================

    #[test]
    fn command_result_continue_variant() {
        let result = CommandResult::Continue;
        assert_eq!(result, CommandResult::Continue);
    }

    #[test]
    fn command_result_exit_variant() {
        let result = CommandResult::Exit;
        assert_eq!(result, CommandResult::Exit);
    }

    #[test]
    fn command_result_open_settings_variant() {
        let result = CommandResult::OpenSettings;
        assert_eq!(result, CommandResult::OpenSettings);
    }

    #[test]
    fn command_result_message_variant() {
        let result = CommandResult::Message("Hello".to_string());
        assert_eq!(result, CommandResult::Message("Hello".to_string()));
    }

    #[test]
    fn command_result_message_equality() {
        let a = CommandResult::Message("test".to_string());
        let b = CommandResult::Message("test".to_string());
        let c = CommandResult::Message("other".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn command_result_clone() {
        let original = CommandResult::Message("test".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn command_result_debug() {
        let result = CommandResult::Continue;
        let debug_str = format!("{result:?}");
        assert!(debug_str.contains("Continue"));
    }

    // =========================================================================
    // CommandContext Tests
    // =========================================================================

    #[test]
    fn command_context_is_running_true() {
        let ctx = make_context(true);
        assert!(ctx.is_running);
    }

    #[test]
    fn command_context_is_running_false() {
        let ctx = make_context(false);
        assert!(!ctx.is_running);
    }

    #[test]
    fn command_context_mode_chat() {
        let ctx = make_context(false);
        assert_eq!(*ctx.mode, AppMode::Chat);
    }

    #[test]
    fn command_context_debug() {
        let ctx = make_context(false);
        let debug_str = format!("{ctx:?}");
        assert!(debug_str.contains("is_running"));
        assert!(debug_str.contains("mode"));
    }

    // =========================================================================
    // SlashCommand Trait Tests
    // =========================================================================

    #[test]
    fn slash_command_name_returns_correct_value() {
        let cmd = TestCommand {
            name: "test",
            description: "A test command",
            result: CommandResult::Continue,
        };
        assert_eq!(cmd.name(), "test");
    }

    #[test]
    fn slash_command_description_returns_correct_value() {
        let cmd = TestCommand {
            name: "test",
            description: "A test command",
            result: CommandResult::Continue,
        };
        assert_eq!(cmd.description(), "A test command");
    }

    #[test]
    fn slash_command_execute_returns_result() {
        let cmd = TestCommand {
            name: "exit",
            description: "Exit the app",
            result: CommandResult::Exit,
        };
        let ctx = make_context(false);
        assert_eq!(cmd.execute(&ctx), CommandResult::Exit);
    }

    #[test]
    fn slash_command_default_can_execute_when_not_running() {
        let cmd = TestCommand {
            name: "test",
            description: "Test",
            result: CommandResult::Continue,
        };
        let ctx = make_context(false);
        assert!(cmd.can_execute(&ctx));
    }

    #[test]
    fn slash_command_default_cannot_execute_when_running() {
        let cmd = TestCommand {
            name: "test",
            description: "Test",
            result: CommandResult::Continue,
        };
        let ctx = make_context(true);
        assert!(!cmd.can_execute(&ctx));
    }

    #[test]
    fn slash_command_custom_can_execute() {
        let cmd = RunningOnlyCommand;

        let ctx_running = make_context(true);
        let ctx_idle = make_context(false);

        assert!(cmd.can_execute(&ctx_running));
        assert!(!cmd.can_execute(&ctx_idle));
    }

    // =========================================================================
    // CommandRegistry Tests
    // =========================================================================

    #[test]
    fn registry_new_is_empty() {
        let registry = CommandRegistry::new();
        assert!(registry.all().is_empty());
    }

    #[test]
    fn registry_default_is_empty() {
        let registry = CommandRegistry::default();
        assert!(registry.all().is_empty());
    }

    #[test]
    fn registry_register_adds_command() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "test",
            description: "Test",
            result: CommandResult::Continue,
        }));
        assert_eq!(registry.all().len(), 1);
    }

    #[test]
    fn registry_register_multiple_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "one",
            description: "First",
            result: CommandResult::Continue,
        }));
        registry.register(Box::new(TestCommand {
            name: "two",
            description: "Second",
            result: CommandResult::Exit,
        }));
        registry.register(Box::new(TestCommand {
            name: "three",
            description: "Third",
            result: CommandResult::OpenSettings,
        }));
        assert_eq!(registry.all().len(), 3);
    }

    #[test]
    fn registry_find_existing_command() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "test",
            description: "Test",
            result: CommandResult::Continue,
        }));
        let found = registry.find("test");
        assert!(found.is_some(), "test command should be found");
        assert_eq!(found.map(super::SlashCommand::name), Some("test"));
    }

    #[test]
    fn registry_find_nonexistent_command() {
        let registry = CommandRegistry::new();
        assert!(registry.find("nonexistent").is_none());
    }

    #[test]
    fn registry_find_with_multiple_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit",
            result: CommandResult::Exit,
        }));
        registry.register(Box::new(TestCommand {
            name: "settings",
            description: "Settings",
            result: CommandResult::OpenSettings,
        }));
        registry.register(Box::new(TestCommand {
            name: "clear",
            description: "Clear",
            result: CommandResult::Continue,
        }));

        assert_eq!(
            registry.find("exit").map(super::SlashCommand::name),
            Some("exit")
        );
        assert_eq!(
            registry.find("settings").map(super::SlashCommand::name),
            Some("settings")
        );
        assert_eq!(
            registry.find("clear").map(super::SlashCommand::name),
            Some("clear")
        );
        assert!(registry.find("unknown").is_none());
    }

    #[test]
    fn registry_all_returns_all_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "a",
            description: "A",
            result: CommandResult::Continue,
        }));
        registry.register(Box::new(TestCommand {
            name: "b",
            description: "B",
            result: CommandResult::Continue,
        }));

        let all = registry.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn registry_matching_with_prefix() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "settings",
            description: "Settings",
            result: CommandResult::OpenSettings,
        }));
        registry.register(Box::new(TestCommand {
            name: "status",
            description: "Status",
            result: CommandResult::Continue,
        }));
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit",
            result: CommandResult::Exit,
        }));

        let matches = registry.matching("s");
        assert_eq!(matches.len(), 2);

        let matches = registry.matching("set");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name(), "settings");

        let matches = registry.matching("e");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name(), "exit");
    }

    #[test]
    fn registry_matching_empty_prefix() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit",
            result: CommandResult::Exit,
        }));
        registry.register(Box::new(TestCommand {
            name: "clear",
            description: "Clear",
            result: CommandResult::Continue,
        }));

        let matches = registry.matching("");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn registry_matching_no_matches() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit",
            result: CommandResult::Exit,
        }));

        let matches = registry.matching("xyz");
        assert!(matches.is_empty());
    }

    #[test]
    fn registry_matching_exact_name() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit",
            result: CommandResult::Exit,
        }));

        let matches = registry.matching("exit");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name(), "exit");
    }

    // =========================================================================
    // Built-in Command Tests (ExitCommand, SettingsCommand, ClearCommand)
    // =========================================================================

    #[test]
    fn exit_command_name() {
        let cmd = ExitCommand;
        assert_eq!(cmd.name(), "exit");
    }

    #[test]
    fn exit_command_description() {
        let cmd = ExitCommand;
        assert_eq!(cmd.description(), "Exit the application");
    }

    #[test]
    fn exit_command_executes_to_exit() {
        let cmd = ExitCommand;
        let ctx = make_context(false);
        assert_eq!(cmd.execute(&ctx), CommandResult::Exit);
    }

    #[test]
    fn exit_command_can_execute_while_running() {
        let cmd = ExitCommand;
        let ctx = make_context(true);
        assert!(cmd.can_execute(&ctx));
    }

    #[test]
    fn exit_command_can_execute_when_idle() {
        let cmd = ExitCommand;
        let ctx = make_context(false);
        assert!(cmd.can_execute(&ctx));
    }

    #[test]
    fn settings_command_name() {
        let cmd = SettingsCommand;
        assert_eq!(cmd.name(), "settings");
    }

    #[test]
    fn settings_command_description() {
        let cmd = SettingsCommand;
        assert_eq!(cmd.description(), "Open settings panel (Ctrl+S)");
    }

    #[test]
    fn settings_command_opens_settings() {
        let cmd = SettingsCommand;
        let ctx = make_context(false);
        assert_eq!(cmd.execute(&ctx), CommandResult::OpenSettings);
    }

    #[test]
    fn settings_command_cannot_execute_while_running() {
        let cmd = SettingsCommand;
        let ctx = make_context(true);
        // Uses default can_execute which returns false when running
        assert!(!cmd.can_execute(&ctx));
    }

    #[test]
    fn settings_command_can_execute_when_idle() {
        let cmd = SettingsCommand;
        let ctx = make_context(false);
        assert!(cmd.can_execute(&ctx));
    }

    #[test]
    fn clear_command_name() {
        let cmd = ClearCommand;
        assert_eq!(cmd.name(), "clear");
    }

    #[test]
    fn clear_command_description() {
        let cmd = ClearCommand;
        assert_eq!(cmd.description(), "Clear task, output, and todo files");
    }

    #[test]
    fn clear_command_returns_clear() {
        let cmd = ClearCommand;
        let ctx = make_context(false);
        assert_eq!(cmd.execute(&ctx), CommandResult::Clear);
    }

    #[test]
    fn clear_command_cannot_execute_while_running() {
        let cmd = ClearCommand;
        let ctx = make_context(true);
        // Uses default can_execute which returns false when running
        assert!(!cmd.can_execute(&ctx));
    }

    #[test]
    fn clear_command_can_execute_when_idle() {
        let cmd = ClearCommand;
        let ctx = make_context(false);
        assert!(cmd.can_execute(&ctx));
    }

    // =========================================================================
    // CommandRegistry::with_builtins() Tests
    // =========================================================================

    #[test]
    fn registry_with_builtins_has_exit() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.find("exit").is_some());
        assert_eq!(
            registry.find("exit").map(super::SlashCommand::name),
            Some("exit")
        );
    }

    #[test]
    fn registry_with_builtins_has_settings() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.find("settings").is_some());
        assert_eq!(
            registry.find("settings").map(super::SlashCommand::name),
            Some("settings")
        );
    }

    #[test]
    fn registry_with_builtins_has_clear() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.find("clear").is_some());
        assert_eq!(
            registry.find("clear").map(super::SlashCommand::name),
            Some("clear")
        );
    }

    #[test]
    fn registry_with_builtins_has_three_commands() {
        let registry = CommandRegistry::with_builtins();
        assert_eq!(registry.all().len(), 3);
    }

    #[test]
    fn registry_with_builtins_find_unknown_returns_none() {
        let registry = CommandRegistry::with_builtins();
        assert!(registry.find("nonexistent").is_none());
    }

    #[test]
    fn registry_with_builtins_matching_e_prefix() {
        let registry = CommandRegistry::with_builtins();
        let matches = registry.matching("e");
        assert!(matches.iter().any(|c| c.name() == "exit"));
    }

    #[test]
    fn registry_with_builtins_matching_s_prefix() {
        let registry = CommandRegistry::with_builtins();
        let matches = registry.matching("s");
        assert!(matches.iter().any(|c| c.name() == "settings"));
    }

    #[test]
    fn registry_with_builtins_matching_c_prefix() {
        let registry = CommandRegistry::with_builtins();
        let matches = registry.matching("c");
        assert!(matches.iter().any(|c| c.name() == "clear"));
    }

    #[test]
    fn registry_with_builtins_matching_empty_returns_all() {
        let registry = CommandRegistry::with_builtins();
        let matches = registry.matching("");
        assert_eq!(matches.len(), 3);
    }

    // =========================================================================
    // Integration Tests
    // =========================================================================

    #[test]
    fn registry_find_and_execute() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "exit",
            description: "Exit the application",
            result: CommandResult::Exit,
        }));

        let ctx = make_context(false);
        let cmd = registry.find("exit");
        assert!(cmd.is_some(), "exit command should exist");
        if let Some(cmd) = cmd {
            assert!(cmd.can_execute(&ctx));
            assert_eq!(cmd.execute(&ctx), CommandResult::Exit);
        }
    }

    #[test]
    fn registry_find_and_check_can_execute_when_running() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand {
            name: "clear",
            description: "Clear output",
            result: CommandResult::Continue,
        }));

        let ctx = make_context(true);
        let cmd = registry.find("clear");
        assert!(cmd.is_some(), "clear command should exist");
        if let Some(cmd) = cmd {
            // Default can_execute returns false when running
            assert!(!cmd.can_execute(&ctx));
        }
    }
}
