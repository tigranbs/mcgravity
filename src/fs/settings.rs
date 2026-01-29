//! Settings persistence module.
//!
//! This module provides functions to load and save application settings
//! as JSON in `.mcgravity/settings.json`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::app::state::{EnterBehavior, MaxIterations, SettingsState};
use crate::core::Model;

/// Directory for mcgravity configuration files.
pub const MCGRAVITY_DIR: &str = ".mcgravity";

/// Path to the settings file.
pub const SETTINGS_FILE: &str = ".mcgravity/settings.json";

/// Persisted settings that are saved between sessions.
///
/// This struct mirrors the relevant fields from `SettingsState` but uses
/// string representation for enum values to allow for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PersistedSettings {
    /// The planning model name (e.g., "Codex", "Claude Code", "Gemini").
    pub planning_model: String,
    /// The execution model name (e.g., "Codex", "Claude Code", "Gemini").
    pub execution_model: String,
    /// The enter key behavior ("Submit" or "Newline").
    pub enter_behavior: String,
    /// The maximum iterations setting ("1", "3", "5", "10", "Unlimited").
    pub max_iterations: String,
}

/// Parses a model from its string name.
///
/// Returns `Model::Codex` as the default for unrecognized values.
fn parse_model(s: &str) -> Model {
    match s {
        "Claude Code" => Model::Claude,
        "Gemini" => Model::Gemini,
        _ => Model::Codex, // Default
    }
}

/// Parses enter behavior from its string name.
///
/// Returns `EnterBehavior::Submit` as the default for unrecognized values.
fn parse_enter_behavior(s: &str) -> EnterBehavior {
    match s {
        "Newline" => EnterBehavior::Newline,
        _ => EnterBehavior::Submit, // Default
    }
}

/// Parses max iterations from its string name.
///
/// Returns `MaxIterations::Five` as the default for unrecognized values.
fn parse_max_iterations(s: &str) -> MaxIterations {
    match s {
        "1" => MaxIterations::One,
        "3" => MaxIterations::Three,
        "10" => MaxIterations::Ten,
        "Unlimited" => MaxIterations::Unlimited,
        _ => MaxIterations::Five, // Default
    }
}

impl From<&SettingsState> for PersistedSettings {
    fn from(state: &SettingsState) -> Self {
        Self {
            planning_model: state.planning_model.name().to_string(),
            execution_model: state.execution_model.name().to_string(),
            enter_behavior: state.enter_behavior.name().to_string(),
            max_iterations: state.max_iterations.name().to_string(),
        }
    }
}

impl PersistedSettings {
    /// Applies these persisted settings to a mutable `SettingsState`.
    ///
    /// This updates the planning model, execution model, enter behavior,
    /// and max iterations fields based on the persisted string values.
    /// Invalid or unrecognized values are replaced with sensible defaults.
    pub fn apply_to(&self, state: &mut SettingsState) {
        state.planning_model = parse_model(&self.planning_model);
        state.execution_model = parse_model(&self.execution_model);
        state.enter_behavior = parse_enter_behavior(&self.enter_behavior);
        state.max_iterations = parse_max_iterations(&self.max_iterations);
    }
}

/// Ensures the `.mcgravity/` directory exists.
///
/// Creates the directory if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn ensure_mcgravity_dir() -> Result<()> {
    let dir = Path::new(MCGRAVITY_DIR);
    if !dir.exists() {
        std::fs::create_dir_all(dir).context("Failed to create .mcgravity directory")?;
    }
    Ok(())
}

/// Checks whether this is a first run (no settings file exists).
///
/// Returns `true` if `.mcgravity/settings.json` does not exist, indicating
/// that the application has never been configured in this directory.
/// This is used to trigger the initial setup modal on first run.
#[must_use]
pub fn is_first_run() -> bool {
    !Path::new(SETTINGS_FILE).exists()
}

/// Loads settings from the specified settings file path.
///
/// If the file doesn't exist, returns default settings.
/// If the file exists but cannot be parsed, returns an error.
///
/// # Arguments
///
/// * `path` - Path to the settings file
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_settings(path: &Path) -> Result<PersistedSettings> {
    if !path.exists() {
        return Ok(PersistedSettings::default());
    }

    let content = std::fs::read_to_string(path).context("Failed to read settings file")?;

    serde_json::from_str(&content).context("Failed to parse settings file")
}

/// Saves settings to the specified settings file path.
///
/// The parent directory must exist (caller should ensure this).
/// Serializes settings to pretty-printed JSON.
///
/// # Arguments
///
/// * `path` - Path to the settings file
/// * `settings` - The settings to save
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn save_settings(path: &Path, settings: &PersistedSettings) -> Result<()> {
    let json = serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;

    std::fs::write(path, json).context("Failed to write settings file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{EnterBehavior, MaxIterations, SettingsState};
    use crate::core::Model;
    use crate::fs::McgravityPaths;
    use anyhow::Result;
    use tempfile::TempDir;

    /// Tests that `PersistedSettings::default()` produces expected values.
    #[test]
    fn default_settings_are_empty_strings() {
        let settings = PersistedSettings::default();
        assert!(settings.planning_model.is_empty());
        assert!(settings.execution_model.is_empty());
        assert!(settings.enter_behavior.is_empty());
        assert!(settings.max_iterations.is_empty());
    }

    /// Tests serialization/deserialization roundtrip.
    #[test]
    fn serialization_roundtrip_works() -> Result<()> {
        let settings = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Codex".to_string(),
            enter_behavior: "Submit".to_string(),
            max_iterations: "5".to_string(),
        };

        let json = serde_json::to_string_pretty(&settings)?;
        let deserialized: PersistedSettings = serde_json::from_str(&json)?;

        assert_eq!(settings, deserialized);
        Ok(())
    }

    /// Tests that JSON output has expected format.
    #[test]
    fn json_format_is_readable() -> Result<()> {
        let settings = PersistedSettings {
            planning_model: "Codex".to_string(),
            execution_model: "Gemini".to_string(),
            enter_behavior: "Newline".to_string(),
            max_iterations: "10".to_string(),
        };

        let json = serde_json::to_string_pretty(&settings)?;

        // Verify JSON contains expected keys
        assert!(json.contains("\"planning_model\""));
        assert!(json.contains("\"execution_model\""));
        assert!(json.contains("\"enter_behavior\""));
        assert!(json.contains("\"max_iterations\""));
        assert!(json.contains("\"Codex\""));
        assert!(json.contains("\"Gemini\""));
        assert!(json.contains("\"Newline\""));
        assert!(json.contains("\"10\""));
        Ok(())
    }

    /// Tests loading settings from a non-existent file returns defaults.
    #[test]
    fn load_nonexistent_file_returns_defaults() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        let settings = paths.load_settings()?;
        assert_eq!(settings, PersistedSettings::default());
        Ok(())
    }

    /// Tests saving and loading settings roundtrip.
    #[test]
    fn save_and_load_roundtrip() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        let settings = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Codex".to_string(),
            enter_behavior: "Submit".to_string(),
            max_iterations: "Unlimited".to_string(),
        };

        paths.save_settings(&settings)?;
        let loaded = paths.load_settings()?;
        assert_eq!(settings, loaded);
        Ok(())
    }

    /// Tests that `save_settings` creates the .mcgravity directory.
    #[test]
    fn save_creates_mcgravity_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // Directory should not exist initially
        assert!(!paths.mcgravity_dir().exists());

        let settings = PersistedSettings::default();
        paths.save_settings(&settings)?;

        // Directory should now exist
        assert!(paths.mcgravity_dir().exists());
        assert!(paths.settings_file().exists());
        Ok(())
    }

    /// Tests `ensure_mcgravity_dir` creates directory when missing.
    #[test]
    fn ensure_mcgravity_dir_creates_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        assert!(!paths.mcgravity_dir().exists());

        paths.ensure_mcgravity_dir()?;
        assert!(paths.mcgravity_dir().exists());
        Ok(())
    }

    /// Tests `ensure_mcgravity_dir` succeeds when directory already exists.
    #[test]
    fn ensure_mcgravity_dir_succeeds_when_exists() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // Create directory first
        std::fs::create_dir_all(paths.mcgravity_dir())?;
        assert!(paths.mcgravity_dir().exists());

        // Should succeed even though directory exists
        paths.ensure_mcgravity_dir()?;
        Ok(())
    }

    // ==========================================================================
    // First-Run Detection Tests
    // ==========================================================================

    /// Tests `is_first_run` returns true when settings file doesn't exist.
    #[test]
    fn is_first_run_returns_true_when_no_settings_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // No settings file exists in fresh temp directory
        assert!(
            paths.is_first_run(),
            "Should be first run when settings file doesn't exist"
        );
        Ok(())
    }

    /// Tests `is_first_run` returns false when settings file exists.
    #[test]
    fn is_first_run_returns_false_when_settings_file_exists() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // Create settings file
        let settings = PersistedSettings::default();
        paths.save_settings(&settings)?;

        assert!(
            !paths.is_first_run(),
            "Should not be first run when settings file exists"
        );
        Ok(())
    }

    /// Tests `is_first_run` returns true even when .mcgravity directory exists but no settings.json.
    #[test]
    fn is_first_run_returns_true_when_only_directory_exists() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let paths = McgravityPaths::new(temp_dir.path());

        // Create just the directory, not the settings file
        paths.ensure_mcgravity_dir()?;
        assert!(paths.mcgravity_dir().exists());
        assert!(!paths.settings_file().exists());

        assert!(
            paths.is_first_run(),
            "Should be first run when only directory exists, not settings file"
        );
        Ok(())
    }

    // ==========================================================================
    // Conversion Tests
    // ==========================================================================

    /// Tests `From<&SettingsState> for PersistedSettings` with default settings.
    #[test]
    fn from_settings_state_default() {
        let state = SettingsState::default();
        let persisted = PersistedSettings::from(&state);

        assert_eq!(persisted.planning_model, "Codex");
        assert_eq!(persisted.execution_model, "Codex");
        assert_eq!(persisted.enter_behavior, "Submit");
        assert_eq!(persisted.max_iterations, "5");
    }

    /// Tests `From<&SettingsState> for PersistedSettings` with non-default settings.
    #[test]
    fn from_settings_state_custom() {
        let state = SettingsState {
            planning_model: Model::Claude,
            execution_model: Model::Gemini,
            enter_behavior: EnterBehavior::Newline,
            max_iterations: MaxIterations::Unlimited,
            ..Default::default()
        };

        let persisted = PersistedSettings::from(&state);

        assert_eq!(persisted.planning_model, "Claude Code");
        assert_eq!(persisted.execution_model, "Gemini");
        assert_eq!(persisted.enter_behavior, "Newline");
        assert_eq!(persisted.max_iterations, "Unlimited");
    }

    /// Tests `parse_model` for all valid values.
    #[test]
    fn parse_model_valid_values() {
        assert_eq!(super::parse_model("Codex"), Model::Codex);
        assert_eq!(super::parse_model("Claude Code"), Model::Claude);
        assert_eq!(super::parse_model("Gemini"), Model::Gemini);
    }

    /// Tests `parse_model` returns default for invalid values.
    #[test]
    fn parse_model_invalid_returns_default() {
        assert_eq!(super::parse_model(""), Model::Codex);
        assert_eq!(super::parse_model("Unknown"), Model::Codex);
        assert_eq!(super::parse_model("claude"), Model::Codex); // Case sensitive
    }

    /// Tests `parse_enter_behavior` for all valid values.
    #[test]
    fn parse_enter_behavior_valid_values() {
        assert_eq!(super::parse_enter_behavior("Submit"), EnterBehavior::Submit);
        assert_eq!(
            super::parse_enter_behavior("Newline"),
            EnterBehavior::Newline
        );
    }

    /// Tests `parse_enter_behavior` returns default for invalid values.
    #[test]
    fn parse_enter_behavior_invalid_returns_default() {
        assert_eq!(super::parse_enter_behavior(""), EnterBehavior::Submit);
        assert_eq!(
            super::parse_enter_behavior("Unknown"),
            EnterBehavior::Submit
        );
        assert_eq!(super::parse_enter_behavior("submit"), EnterBehavior::Submit); // Case sensitive
    }

    /// Tests `parse_max_iterations` for all valid values.
    #[test]
    fn parse_max_iterations_valid_values() {
        assert_eq!(super::parse_max_iterations("1"), MaxIterations::One);
        assert_eq!(super::parse_max_iterations("3"), MaxIterations::Three);
        assert_eq!(super::parse_max_iterations("5"), MaxIterations::Five);
        assert_eq!(super::parse_max_iterations("10"), MaxIterations::Ten);
        assert_eq!(
            super::parse_max_iterations("Unlimited"),
            MaxIterations::Unlimited
        );
    }

    /// Tests `parse_max_iterations` returns default for invalid values.
    #[test]
    fn parse_max_iterations_invalid_returns_default() {
        assert_eq!(super::parse_max_iterations(""), MaxIterations::Five);
        assert_eq!(super::parse_max_iterations("Unknown"), MaxIterations::Five);
        assert_eq!(super::parse_max_iterations("2"), MaxIterations::Five); // Not a valid option
        assert_eq!(
            super::parse_max_iterations("unlimited"),
            MaxIterations::Five
        ); // Case sensitive
    }

    /// Tests `apply_to` with valid values.
    #[test]
    fn apply_to_valid_values() {
        let persisted = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Gemini".to_string(),
            enter_behavior: "Newline".to_string(),
            max_iterations: "10".to_string(),
        };

        let mut state = SettingsState::default();
        persisted.apply_to(&mut state);

        assert_eq!(state.planning_model, Model::Claude);
        assert_eq!(state.execution_model, Model::Gemini);
        assert_eq!(state.enter_behavior, EnterBehavior::Newline);
        assert_eq!(state.max_iterations, MaxIterations::Ten);
    }

    /// Tests `apply_to` with invalid values uses defaults.
    #[test]
    fn apply_to_invalid_values_uses_defaults() {
        let persisted = PersistedSettings {
            planning_model: "Invalid".to_string(),
            execution_model: String::new(),
            enter_behavior: "invalid".to_string(),
            max_iterations: "99".to_string(),
        };

        let mut state = SettingsState {
            planning_model: Model::Claude,
            execution_model: Model::Gemini,
            enter_behavior: EnterBehavior::Newline,
            max_iterations: MaxIterations::Unlimited,
            ..Default::default()
        };

        persisted.apply_to(&mut state);

        // All should be reset to defaults due to invalid input
        assert_eq!(state.planning_model, Model::Codex);
        assert_eq!(state.execution_model, Model::Codex);
        assert_eq!(state.enter_behavior, EnterBehavior::Submit);
        assert_eq!(state.max_iterations, MaxIterations::Five);
    }

    /// Tests roundtrip: `SettingsState` -> `PersistedSettings` -> `apply_to` -> same values.
    #[test]
    fn roundtrip_conversion() {
        // Test with default settings
        let original = SettingsState::default();
        let persisted = PersistedSettings::from(&original);
        let mut restored = SettingsState {
            planning_model: Model::Claude, // Set different to verify it changes
            ..Default::default()
        };
        persisted.apply_to(&mut restored);

        assert_eq!(restored.planning_model, original.planning_model);
        assert_eq!(restored.execution_model, original.execution_model);
        assert_eq!(restored.enter_behavior, original.enter_behavior);
        assert_eq!(restored.max_iterations, original.max_iterations);
    }

    /// Tests roundtrip with custom settings.
    #[test]
    fn roundtrip_conversion_custom() {
        let original = SettingsState {
            planning_model: Model::Claude,
            execution_model: Model::Gemini,
            enter_behavior: EnterBehavior::Newline,
            max_iterations: MaxIterations::One,
            ..Default::default()
        };

        let persisted = PersistedSettings::from(&original);
        let mut restored = SettingsState::default();
        persisted.apply_to(&mut restored);

        assert_eq!(restored.planning_model, original.planning_model);
        assert_eq!(restored.execution_model, original.execution_model);
        assert_eq!(restored.enter_behavior, original.enter_behavior);
        assert_eq!(restored.max_iterations, original.max_iterations);
    }

    /// Tests roundtrip for all model variants.
    #[test]
    fn roundtrip_all_models() {
        for model in Model::all() {
            let original = SettingsState {
                planning_model: *model,
                execution_model: *model,
                ..Default::default()
            };

            let persisted = PersistedSettings::from(&original);
            let mut restored = SettingsState::default();
            persisted.apply_to(&mut restored);

            assert_eq!(restored.planning_model, *model);
            assert_eq!(restored.execution_model, *model);
        }
    }

    /// Tests roundtrip for all enter behavior variants.
    #[test]
    fn roundtrip_all_enter_behaviors() {
        for behavior in [EnterBehavior::Submit, EnterBehavior::Newline] {
            let original = SettingsState {
                enter_behavior: behavior,
                ..Default::default()
            };

            let persisted = PersistedSettings::from(&original);
            let mut restored = SettingsState::default();
            persisted.apply_to(&mut restored);

            assert_eq!(restored.enter_behavior, behavior);
        }
    }

    /// Tests roundtrip for all max iterations variants.
    #[test]
    fn roundtrip_all_max_iterations() {
        for iterations in [
            MaxIterations::One,
            MaxIterations::Three,
            MaxIterations::Five,
            MaxIterations::Ten,
            MaxIterations::Unlimited,
        ] {
            let original = SettingsState {
                max_iterations: iterations,
                ..Default::default()
            };

            let persisted = PersistedSettings::from(&original);
            let mut restored = SettingsState::default();
            persisted.apply_to(&mut restored);

            assert_eq!(restored.max_iterations, iterations);
        }
    }
}
