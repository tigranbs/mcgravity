//! File system operations.

use std::path::{Path, PathBuf};

pub mod settings;
pub mod todo;

pub use settings::{PersistedSettings, load_settings, save_settings};
pub use todo::{move_to_done, read_file_content, remove_done_files, scan_todo_files};

// Legacy constants for backward compatibility during migration
pub use settings::{MCGRAVITY_DIR, SETTINGS_FILE};
pub use todo::{DONE_DIR, TODO_DIR};

/// Path to the task file for persistence (legacy constant).
pub const TASK_FILE: &str = ".mcgravity/task.md";

/// Holds all mcgravity-related paths derived from a base directory.
///
/// This struct enables dependency injection of filesystem paths, allowing
/// tests to use isolated temporary directories instead of the actual
/// working directory. In production, the base is typically the current
/// working directory.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use mcgravity::fs::McgravityPaths;
///
/// // Production: use current directory
/// let paths = McgravityPaths::from_cwd();
///
/// // Tests: use a temp directory
/// let paths = McgravityPaths::new(Path::new("/tmp/test"));
/// assert_eq!(paths.task_file(), Path::new("/tmp/test/.mcgravity/task.md"));
/// ```
#[derive(Debug, Clone)]
pub struct McgravityPaths {
    base: PathBuf,
}

impl McgravityPaths {
    /// Creates paths rooted at the given base directory.
    ///
    /// All derived paths will be relative to this base.
    #[must_use]
    pub fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
        }
    }

    /// Creates paths rooted at the current working directory.
    ///
    /// This is the typical usage for production code.
    ///
    /// # Panics
    ///
    /// Panics if the current directory cannot be determined.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented panic - fundamental requirement for app startup.
    pub fn from_cwd() -> Self {
        Self {
            base: std::env::current_dir().expect("Failed to get current directory"),
        }
    }

    /// Returns the base directory.
    #[must_use]
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Returns the `.mcgravity` directory path.
    #[must_use]
    pub fn mcgravity_dir(&self) -> PathBuf {
        self.base.join(".mcgravity")
    }

    /// Returns the settings file path (`.mcgravity/settings.json`).
    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.base.join(".mcgravity/settings.json")
    }

    /// Returns the task file path (`.mcgravity/task.md`).
    #[must_use]
    pub fn task_file(&self) -> PathBuf {
        self.base.join(".mcgravity/task.md")
    }

    /// Returns the todo directory path (`.mcgravity/todo`).
    #[must_use]
    pub fn todo_dir(&self) -> PathBuf {
        self.base.join(".mcgravity/todo")
    }

    /// Returns the done directory path (`.mcgravity/todo/done`).
    #[must_use]
    pub fn done_dir(&self) -> PathBuf {
        self.base.join(".mcgravity/todo/done")
    }

    /// Ensures the `.mcgravity` directory exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn ensure_mcgravity_dir(&self) -> anyhow::Result<()> {
        let dir = self.mcgravity_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
        }
        Ok(())
    }

    /// Ensures the todo directories exist (`.mcgravity/todo` and `.mcgravity/todo/done`).
    ///
    /// # Errors
    ///
    /// Returns an error if the directories cannot be created.
    pub fn ensure_todo_dirs(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.todo_dir()).with_context(|| {
            format!(
                "Failed to create todo directory: {}",
                self.todo_dir().display()
            )
        })?;
        std::fs::create_dir_all(self.done_dir()).with_context(|| {
            format!(
                "Failed to create done directory: {}",
                self.done_dir().display()
            )
        })?;
        Ok(())
    }

    /// Checks whether this is a first run (no settings file exists).
    #[must_use]
    pub fn is_first_run(&self) -> bool {
        !self.settings_file().exists()
    }

    /// Loads settings from the settings file.
    ///
    /// If the file doesn't exist, returns default settings.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load_settings(&self) -> anyhow::Result<PersistedSettings> {
        load_settings(&self.settings_file())
    }

    /// Saves settings to the settings file.
    ///
    /// Creates the `.mcgravity` directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save_settings(&self, settings: &PersistedSettings) -> anyhow::Result<()> {
        self.ensure_mcgravity_dir()?;
        save_settings(&self.settings_file(), settings)
    }
}

impl Default for McgravityPaths {
    fn default() -> Self {
        Self::from_cwd()
    }
}

use anyhow::Context;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn paths_are_derived_from_base() {
        let base = Path::new("/test/base");
        let paths = McgravityPaths::new(base);

        assert_eq!(paths.base(), Path::new("/test/base"));
        assert_eq!(paths.mcgravity_dir(), Path::new("/test/base/.mcgravity"));
        assert_eq!(
            paths.settings_file(),
            Path::new("/test/base/.mcgravity/settings.json")
        );
        assert_eq!(
            paths.task_file(),
            Path::new("/test/base/.mcgravity/task.md")
        );
        assert_eq!(paths.todo_dir(), Path::new("/test/base/.mcgravity/todo"));
        assert_eq!(
            paths.done_dir(),
            Path::new("/test/base/.mcgravity/todo/done")
        );
    }

    #[test]
    fn ensure_mcgravity_dir_creates_directory() {
        let temp = TempDir::new().unwrap();
        let paths = McgravityPaths::new(temp.path());

        assert!(!paths.mcgravity_dir().exists());
        paths.ensure_mcgravity_dir().unwrap();
        assert!(paths.mcgravity_dir().exists());
    }

    #[test]
    fn ensure_todo_dirs_creates_directories() {
        let temp = TempDir::new().unwrap();
        let paths = McgravityPaths::new(temp.path());

        assert!(!paths.todo_dir().exists());
        assert!(!paths.done_dir().exists());
        paths.ensure_todo_dirs().unwrap();
        assert!(paths.todo_dir().exists());
        assert!(paths.done_dir().exists());
    }

    #[test]
    fn is_first_run_returns_true_when_no_settings() {
        let temp = TempDir::new().unwrap();
        let paths = McgravityPaths::new(temp.path());

        assert!(paths.is_first_run());
    }

    #[test]
    fn is_first_run_returns_false_when_settings_exist() {
        let temp = TempDir::new().unwrap();
        let paths = McgravityPaths::new(temp.path());

        paths.save_settings(&PersistedSettings::default()).unwrap();
        assert!(!paths.is_first_run());
    }

    #[test]
    fn save_and_load_settings_roundtrip() {
        let temp = TempDir::new().unwrap();
        let paths = McgravityPaths::new(temp.path());

        let settings = PersistedSettings {
            planning_model: "Claude Code".to_string(),
            execution_model: "Codex".to_string(),
            enter_behavior: "Submit".to_string(),
            max_iterations: "5".to_string(),
        };

        paths.save_settings(&settings).unwrap();
        let loaded = paths.load_settings().unwrap();
        assert_eq!(settings, loaded);
    }
}
