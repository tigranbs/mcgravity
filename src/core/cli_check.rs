//! Synchronous CLI availability checking utilities.
//!
//! This module provides synchronous functions for checking whether AI CLI tools
//! are available in the system's PATH. Unlike the async version in `executor.rs`,
//! these functions can be called during UI initialization before the async runtime
//! context is fully established.

use std::process::Command;

/// Checks if a CLI command is available in the system's PATH.
///
/// Uses `which` on Unix systems and `where` on Windows to check for command availability.
/// This is a synchronous function suitable for use during UI initialization.
///
/// # Arguments
///
/// * `command` - The command name to check (e.g., "codex", "claude", "gemini")
///
/// # Returns
///
/// `true` if the command is found in PATH, `false` otherwise.
#[must_use]
pub fn check_cli_in_path(command: &str) -> bool {
    #[cfg(windows)]
    let check_cmd = "where";
    #[cfg(not(windows))]
    let check_cmd = "which";

    Command::new(check_cmd)
        .arg(command)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Stores CLI availability status for all supported AI models.
///
/// This struct caches the results of CLI availability checks, allowing
/// the UI to display proper error messages or disable unavailable models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelAvailability {
    /// Whether the Codex CLI (`codex`) is available.
    pub codex: bool,
    /// Whether the Claude CLI (`claude`) is available.
    pub claude: bool,
    /// Whether the Gemini CLI (`gemini`) is available.
    pub gemini: bool,
}

impl ModelAvailability {
    /// Checks availability of all supported CLI tools synchronously.
    ///
    /// This performs three separate `which`/`where` commands to check for
    /// each CLI tool. Suitable for use during application startup.
    #[must_use]
    pub fn check_all() -> Self {
        Self {
            codex: check_cli_in_path("codex"),
            claude: check_cli_in_path("claude"),
            gemini: check_cli_in_path("gemini"),
        }
    }

    /// Returns `true` if at least one model is available.
    #[must_use]
    pub const fn any_available(&self) -> bool {
        self.codex || self.claude || self.gemini
    }

    /// Returns `true` if all models are available.
    #[must_use]
    pub const fn all_available(&self) -> bool {
        self.codex && self.claude && self.gemini
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // check_cli_in_path Tests
    // =========================================================================

    mod check_cli_in_path_tests {
        use super::*;

        /// Tests that `sh` (or equivalent shell) is found on Unix systems.
        #[test]
        fn finds_existing_command() {
            // 'sh' should exist on all Unix systems
            #[cfg(not(windows))]
            {
                assert!(check_cli_in_path("sh"));
            }
            // 'cmd' should exist on Windows
            #[cfg(windows)]
            {
                assert!(check_cli_in_path("cmd"));
            }
        }

        /// Tests that a nonexistent command returns false.
        #[test]
        fn returns_false_for_nonexistent_command() {
            assert!(!check_cli_in_path(
                "this_command_definitely_does_not_exist_xyz789"
            ));
        }

        /// Tests that empty string returns false.
        #[test]
        fn returns_false_for_empty_command() {
            assert!(!check_cli_in_path(""));
        }

        /// Tests that a command with special characters returns false.
        #[test]
        fn returns_false_for_invalid_command_name() {
            assert!(!check_cli_in_path("not/a/valid/command"));
        }
    }

    // =========================================================================
    // ModelAvailability Tests
    // =========================================================================

    mod model_availability_tests {
        use super::*;

        /// Tests default values are all false.
        #[test]
        fn default_all_false() {
            let availability = ModelAvailability::default();
            assert!(!availability.codex);
            assert!(!availability.claude);
            assert!(!availability.gemini);
        }

        /// Tests `any_available` when none are available.
        #[test]
        fn any_available_none() {
            let availability = ModelAvailability::default();
            assert!(!availability.any_available());
        }

        /// Tests `any_available` when one is available.
        #[test]
        fn any_available_one() {
            let availability = ModelAvailability {
                codex: true,
                claude: false,
                gemini: false,
            };
            assert!(availability.any_available());
        }

        /// Tests `any_available` when all are available.
        #[test]
        fn any_available_all() {
            let availability = ModelAvailability {
                codex: true,
                claude: true,
                gemini: true,
            };
            assert!(availability.any_available());
        }

        /// Tests `all_available` when none are available.
        #[test]
        fn all_available_none() {
            let availability = ModelAvailability::default();
            assert!(!availability.all_available());
        }

        /// Tests `all_available` when some are available.
        #[test]
        fn all_available_some() {
            let availability = ModelAvailability {
                codex: true,
                claude: true,
                gemini: false,
            };
            assert!(!availability.all_available());
        }

        /// Tests `all_available` when all are available.
        #[test]
        fn all_available_all() {
            let availability = ModelAvailability {
                codex: true,
                claude: true,
                gemini: true,
            };
            assert!(availability.all_available());
        }

        /// Tests that `check_all` returns a valid struct (doesn't panic).
        #[test]
        fn check_all_does_not_panic() {
            // This test just verifies the function completes without panicking.
            // The actual availability depends on the system.
            let _availability = ModelAvailability::check_all();
        }

        /// Tests Clone implementation.
        #[test]
        fn clone_works() {
            let original = ModelAvailability {
                codex: true,
                claude: false,
                gemini: true,
            };
            let cloned = original;
            assert_eq!(original, cloned);
        }

        /// Tests Debug implementation.
        #[test]
        fn debug_format() {
            let availability = ModelAvailability {
                codex: true,
                claude: false,
                gemini: true,
            };
            let debug_str = format!("{availability:?}");
            assert!(debug_str.contains("ModelAvailability"));
            assert!(debug_str.contains("codex: true"));
            assert!(debug_str.contains("claude: false"));
            assert!(debug_str.contains("gemini: true"));
        }

        /// Tests equality.
        #[test]
        fn equality() {
            let a = ModelAvailability {
                codex: true,
                claude: false,
                gemini: true,
            };
            let b = ModelAvailability {
                codex: true,
                claude: false,
                gemini: true,
            };
            let c = ModelAvailability {
                codex: false,
                claude: false,
                gemini: true,
            };
            assert_eq!(a, b);
            assert_ne!(a, c);
        }
    }
}
