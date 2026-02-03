//! CLI availability checking utilities with shell-aware resolution.
//!
//! This module provides functions for checking whether AI CLI tools are available
//! in the system. It implements a two-stage resolution strategy:
//!
//! 1. **Fast PATH lookup**: Direct `which`/`where` check for executables
//! 2. **Shell-based resolution**: Fallback using `$SHELL -l -i -c "command -v <cmd>"`
//!    to detect commands available via aliases, functions, or PATH modifications
//!    in shell profiles
//!
//! # Resolution Order
//!
//! The resolution algorithm follows this priority:
//!
//! 1. Direct PATH scan via `which` (Unix) or `where` (Windows)
//! 2. If not found, shell-based resolution via the user's login shell
//! 3. Executability verification for PATH-resolved commands
//!
//! # Security
//!
//! Command names are validated against `^[a-zA-Z0-9_-]{1,64}$` before any shell
//! invocation to prevent command injection. See [`is_safe_command_name`] for details.
//!
//! # Classification
//!
//! Commands are classified into categories for appropriate execution strategy:
//! - [`CommandResolution::PathExecutable`]: Direct executable, can use `Command::new(path)`
//! - [`CommandResolution::ShellAlias`]: Requires shell wrapper for execution
//! - [`CommandResolution::ShellFunction`]: Requires shell wrapper for execution
//! - [`CommandResolution::ShellBuiltin`]: Requires shell wrapper for execution
//! - [`CommandResolution::NotFound`]: Command unavailable
//!
//! See `docs/adding-executors.md` for the complete resolution strategy documentation.

use std::path::PathBuf;
use std::process::Command;

/// Result of resolving a CLI command.
///
/// Indicates how a command was found and the appropriate execution strategy.
/// See `docs/adding-executors.md` for detailed classification rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResolution {
    /// Command found as executable in PATH.
    ///
    /// Can be executed directly via `Command::new(path)`.
    PathExecutable(PathBuf),
    /// Command available as shell alias.
    ///
    /// Requires shell wrapper: `$SHELL -l -i -c "<cmd> <args>"`.
    ShellAlias(String),
    /// Command available as shell function.
    ///
    /// Requires shell wrapper: `$SHELL -l -i -c "<cmd> <args>"`.
    ShellFunction(String),
    /// Command is a shell builtin.
    ///
    /// Requires shell wrapper: `$SHELL -l -i -c "<cmd> <args>"`.
    ShellBuiltin,
    /// Command not found by any resolution method.
    NotFound,
}

impl CommandResolution {
    /// Returns `true` if the command was found by any method.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        !matches!(self, Self::NotFound)
    }

    /// Returns the resolved path if this is a `PathExecutable`.
    #[must_use]
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::PathExecutable(path) => Some(path),
            _ => None,
        }
    }

    /// Returns `true` if execution requires a shell wrapper.
    #[must_use]
    pub const fn requires_shell(&self) -> bool {
        matches!(
            self,
            Self::ShellAlias(_) | Self::ShellFunction(_) | Self::ShellBuiltin
        )
    }
}

/// Validates that a command name is safe for shell invocation.
///
/// This function prevents command injection by ensuring the command name
/// contains only safe characters. Per `rust.mdc` "Security Best Practices",
/// input validation is critical at system boundaries.
///
/// # Rules
/// - Only alphanumeric characters, hyphens (`-`), and underscores (`_`) allowed
/// - Maximum 64 characters
/// - Must not be empty
///
/// # Returns
/// `true` if the command name is safe, `false` otherwise.
///
/// # Examples
/// ```
/// use mcgravity::core::cli_check::is_safe_command_name;
///
/// assert!(is_safe_command_name("claude"));
/// assert!(is_safe_command_name("codex-cli"));
/// assert!(is_safe_command_name("my_tool"));
/// assert!(!is_safe_command_name(""));  // empty
/// assert!(!is_safe_command_name("cmd; rm -rf"));  // injection attempt
/// assert!(!is_safe_command_name("$(whoami)"));  // command substitution
/// ```
#[must_use]
pub fn is_safe_command_name(command: &str) -> bool {
    if command.is_empty() || command.len() > 64 {
        return false;
    }

    command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Resolves a CLI command using the shell-aware resolution strategy.
///
/// This function implements the resolution algorithm documented in
/// `docs/adding-executors.md` "CLI Availability Resolution Strategy".
///
/// # Resolution Order
/// 1. Fast PATH lookup via `which`/`where`
/// 2. Shell-based resolution via `$SHELL -l -i -c "command -v <cmd>"`
///
/// # Security
/// The command name is validated against `^[a-zA-Z0-9_-]{1,64}$` before
/// any shell invocation to prevent command injection.
///
/// # Arguments
/// * `command` - The command name to resolve (e.g., "claude", "codex")
///
/// # Returns
/// A [`CommandResolution`] indicating how the command can be executed.
///
/// # Examples
/// ```no_run
/// use mcgravity::core::cli_check::resolve_cli_command;
///
/// let resolution = resolve_cli_command("claude");
/// if resolution.is_available() {
///     println!("Claude CLI is available");
/// }
/// ```
#[must_use]
pub fn resolve_cli_command(command: &str) -> CommandResolution {
    // Security: validate command name first
    if !is_safe_command_name(command) {
        return CommandResolution::NotFound;
    }

    // Step 1: Fast PATH lookup
    if let Some(resolution) = try_path_lookup(command) {
        return resolution;
    }

    // Step 2: Shell-based resolution (Unix only)
    #[cfg(unix)]
    if let Some(resolution) = try_shell_resolution(command) {
        return resolution;
    }

    CommandResolution::NotFound
}

/// Attempts to find a command via direct PATH lookup using `which`/`where`.
///
/// Returns `Some(CommandResolution::PathExecutable)` if found and executable,
/// `None` otherwise.
fn try_path_lookup(command: &str) -> Option<CommandResolution> {
    #[cfg(windows)]
    let check_cmd = "where";
    #[cfg(not(windows))]
    let check_cmd = "which";

    let output = Command::new(check_cmd)
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Parse the path from output
    let path_str = String::from_utf8_lossy(&output.stdout);
    let path_str = path_str.trim();

    if path_str.is_empty() {
        return None;
    }

    // On Windows, `where` may return multiple paths; take the first
    let first_path = path_str.lines().next()?;
    let path = PathBuf::from(first_path);

    // Verify executability
    if is_executable(&path) {
        Some(CommandResolution::PathExecutable(path))
    } else {
        None
    }
}

/// Checks if a path points to an executable file.
#[cfg(unix)]
fn is_executable(path: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;

    match std::fs::metadata(path) {
        Ok(metadata) => {
            // Must be a regular file with execute permission
            metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0)
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_executable(path: &PathBuf) -> bool {
    // On Windows, check if the file exists and has an executable extension
    if !path.is_file() {
        return false;
    }

    // Windows executable extensions
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_lower = ext.to_lowercase();
            matches!(ext_lower.as_str(), "exe" | "cmd" | "bat" | "com" | "ps1")
        })
        .unwrap_or(false)
}

/// Attempts shell-based resolution using the user's login shell.
///
/// Invokes `$SHELL -l -i -c "command -v <cmd>"` and parses the output
/// to determine command type (path, alias, function, or builtin).
#[cfg(unix)]
fn try_shell_resolution(command: &str) -> Option<CommandResolution> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    // Security: command is already validated by resolve_cli_command
    let check_command = format!("command -v {command}");

    let output = Command::new(&shell)
        .args(["-l", "-i", "-c", &check_command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let result = String::from_utf8_lossy(&output.stdout);
    let result = result.trim();

    if result.is_empty() {
        return None;
    }

    // Classify the output
    Some(classify_command_v_output(result, command))
}

/// Classifies the output of `command -v` into a `CommandResolution`.
#[cfg(unix)]
fn classify_command_v_output(output: &str, command: &str) -> CommandResolution {
    // Check for alias definition (e.g., "alias claude='...'")
    if output.starts_with("alias ") {
        return CommandResolution::ShellAlias(output.to_string());
    }

    // Check for function (varies by shell, but often contains "function" or "()")
    if output.contains(" is a function")
        || output.contains("() {")
        || output.starts_with(&format!("{command} ()"))
    {
        return CommandResolution::ShellFunction(output.to_string());
    }

    // Check for builtin
    if output == command || output == "builtin" {
        // `command -v` returns just the name for builtins in some shells
        // Need to distinguish from a relative path
        if !output.contains('/') && !output.contains('\\') {
            // Could be a builtin; check explicitly
            return CommandResolution::ShellBuiltin;
        }
    }

    // Check if it's a path
    if output.starts_with('/') || output.contains('/') {
        let path = PathBuf::from(output);
        if is_executable(&path) {
            return CommandResolution::PathExecutable(path);
        }
    }

    // If we get here and have output, it's likely available but type unknown
    // Treat as alias (requires shell) for safety
    CommandResolution::ShellAlias(output.to_string())
}

/// Checks if a CLI command is available and executable.
///
/// Uses the shell-aware resolution strategy documented in `docs/adding-executors.md`:
/// 1. Fast PATH lookup via `which`/`where` with executability verification
/// 2. Shell-based resolution via `$SHELL -l -i -c "command -v <cmd>"` (Unix only)
///
/// This is a synchronous function suitable for use during UI initialization.
///
/// # Arguments
///
/// * `command` - The command name to check (e.g., "codex", "claude", "gemini")
///
/// # Returns
///
/// `true` if the command is found and executable, `false` otherwise.
/// Returns `false` for non-executable files in PATH.
///
/// # Security
///
/// The command name is validated before any shell invocation. See [`is_safe_command_name`].
#[must_use]
pub fn check_cli_in_path(command: &str) -> bool {
    resolve_cli_command(command).is_available()
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
    // is_safe_command_name Tests
    // =========================================================================

    mod is_safe_command_name_tests {
        use super::*;

        /// Tests valid command names.
        #[test]
        fn accepts_valid_command_names() {
            assert!(is_safe_command_name("claude"));
            assert!(is_safe_command_name("codex"));
            assert!(is_safe_command_name("gemini"));
            assert!(is_safe_command_name("codex-cli"));
            assert!(is_safe_command_name("my_tool"));
            assert!(is_safe_command_name("tool123"));
            assert!(is_safe_command_name("a")); // single char
        }

        /// Tests that empty string is rejected.
        #[test]
        fn rejects_empty_string() {
            assert!(!is_safe_command_name(""));
        }

        /// Tests that strings over 64 chars are rejected.
        #[test]
        fn rejects_too_long_names() {
            let long_name = "a".repeat(65);
            assert!(!is_safe_command_name(&long_name));

            // Exactly 64 chars should be accepted
            let max_name = "a".repeat(64);
            assert!(is_safe_command_name(&max_name));
        }

        /// Tests that shell injection attempts are rejected.
        #[test]
        fn rejects_injection_attempts() {
            assert!(!is_safe_command_name("cmd; rm -rf"));
            assert!(!is_safe_command_name("$(whoami)"));
            assert!(!is_safe_command_name("`whoami`"));
            assert!(!is_safe_command_name("cmd | cat"));
            assert!(!is_safe_command_name("cmd > file"));
            assert!(!is_safe_command_name("cmd < file"));
            assert!(!is_safe_command_name("cmd && bad"));
            assert!(!is_safe_command_name("cmd || bad"));
            assert!(!is_safe_command_name("cmd\nwhoami"));
            assert!(!is_safe_command_name("cmd'whoami"));
            assert!(!is_safe_command_name("cmd\"whoami"));
        }

        /// Tests that paths are rejected (slashes not allowed).
        #[test]
        fn rejects_paths() {
            assert!(!is_safe_command_name("/usr/bin/cmd"));
            assert!(!is_safe_command_name("../cmd"));
            assert!(!is_safe_command_name("./cmd"));
            assert!(!is_safe_command_name("dir/cmd"));
        }

        /// Tests that spaces are rejected.
        #[test]
        fn rejects_spaces() {
            assert!(!is_safe_command_name("cmd arg"));
            assert!(!is_safe_command_name(" cmd"));
            assert!(!is_safe_command_name("cmd "));
        }
    }

    // =========================================================================
    // CommandResolution Tests
    // =========================================================================

    mod command_resolution_tests {
        use super::*;

        /// Tests `is_available` method.
        #[test]
        fn is_available_returns_correct_value() {
            assert!(CommandResolution::PathExecutable(PathBuf::from("/bin/sh")).is_available());
            assert!(CommandResolution::ShellAlias("alias x='y'".to_string()).is_available());
            assert!(CommandResolution::ShellFunction("x () {}".to_string()).is_available());
            assert!(CommandResolution::ShellBuiltin.is_available());
            assert!(!CommandResolution::NotFound.is_available());
        }

        /// Tests `path` method.
        #[test]
        fn path_returns_correct_value() {
            let path = PathBuf::from("/bin/sh");
            assert_eq!(
                CommandResolution::PathExecutable(path.clone()).path(),
                Some(&path)
            );
            assert_eq!(CommandResolution::ShellAlias("x".to_string()).path(), None);
            assert_eq!(CommandResolution::NotFound.path(), None);
        }

        /// Tests `requires_shell` method.
        #[test]
        fn requires_shell_returns_correct_value() {
            assert!(!CommandResolution::PathExecutable(PathBuf::from("/bin/sh")).requires_shell());
            assert!(CommandResolution::ShellAlias("x".to_string()).requires_shell());
            assert!(CommandResolution::ShellFunction("x".to_string()).requires_shell());
            assert!(CommandResolution::ShellBuiltin.requires_shell());
            assert!(!CommandResolution::NotFound.requires_shell());
        }

        /// Tests Debug trait.
        #[test]
        fn debug_format() {
            let resolution = CommandResolution::PathExecutable(PathBuf::from("/bin/sh"));
            let debug_str = format!("{resolution:?}");
            assert!(debug_str.contains("PathExecutable"));
        }

        /// Tests Clone trait.
        #[test]
        fn clone_works() {
            let original = CommandResolution::PathExecutable(PathBuf::from("/bin/sh"));
            let cloned = original.clone();
            assert_eq!(original, cloned);
        }

        /// Tests Eq trait.
        #[test]
        fn equality() {
            let a = CommandResolution::PathExecutable(PathBuf::from("/bin/sh"));
            let b = CommandResolution::PathExecutable(PathBuf::from("/bin/sh"));
            let c = CommandResolution::PathExecutable(PathBuf::from("/bin/bash"));
            assert_eq!(a, b);
            assert_ne!(a, c);
        }
    }

    // =========================================================================
    // resolve_cli_command Tests
    // =========================================================================

    mod resolve_cli_command_tests {
        use super::*;

        /// Tests that an executable found via PATH scanning is reported as available.
        ///
        /// This test verifies that `resolve_cli_command` correctly identifies
        /// executables in PATH and returns `PathExecutable` with the resolved path.
        #[test]
        fn executable_in_path_is_available() {
            // 'sh' should exist on all Unix systems as an executable in PATH
            #[cfg(not(windows))]
            {
                let resolution = resolve_cli_command("sh");
                assert!(resolution.is_available(), "sh should be available via PATH");

                // Should be resolved as PathExecutable with a valid path
                if let CommandResolution::PathExecutable(path) = &resolution {
                    assert!(path.exists(), "Resolved path should exist");
                    assert!(is_executable(path), "Resolved path should be executable");
                } else {
                    // Could also be resolved via shell, which is acceptable
                    assert!(
                        resolution.is_available(),
                        "sh should be available by some method"
                    );
                }
            }

            // 'cmd' should exist on Windows
            #[cfg(windows)]
            {
                let resolution = resolve_cli_command("cmd");
                assert!(
                    resolution.is_available(),
                    "cmd should be available on Windows"
                );
            }
        }

        /// Tests that a nonexistent command returns `NotFound`.
        #[test]
        fn nonexistent_command_returns_not_found() {
            let resolution = resolve_cli_command("this_command_definitely_does_not_exist_xyz789");
            assert_eq!(resolution, CommandResolution::NotFound);
            assert!(!resolution.is_available());
        }

        /// Tests that invalid command names return `NotFound`.
        #[test]
        fn invalid_command_names_return_not_found() {
            // Empty
            assert_eq!(resolve_cli_command(""), CommandResolution::NotFound);

            // With path separators
            assert_eq!(
                resolve_cli_command("not/a/valid/command"),
                CommandResolution::NotFound
            );

            // Injection attempt
            assert_eq!(
                resolve_cli_command("cmd; rm -rf"),
                CommandResolution::NotFound
            );
        }

        /// Tests that `resolve_cli_command` validates command names before shell invocation.
        #[test]
        fn validates_command_before_resolution() {
            // These should all return NotFound due to validation failure,
            // not due to actual command resolution
            let dangerous_inputs = ["$(whoami)", "`id`", "x; y", "x | y", "x && y", "x || y"];

            for input in dangerous_inputs {
                let resolution = resolve_cli_command(input);
                assert_eq!(
                    resolution,
                    CommandResolution::NotFound,
                    "Dangerous input '{input}' should return NotFound"
                );
            }
        }
    }

    // =========================================================================
    // is_executable Tests (Unix-specific)
    // =========================================================================

    #[cfg(unix)]
    #[allow(clippy::expect_used)] // Test code uses expect for clear panic messages
    mod is_executable_tests {
        use super::*;
        use std::fs::{self, File};
        use std::os::unix::fs::PermissionsExt;

        /// Tests that a non-executable file in PATH returns false.
        ///
        /// This test creates a temporary file without execute permissions
        /// and verifies that `is_executable` correctly returns false.
        #[test]
        fn non_executable_file_returns_false() {
            // Create a temp file without execute permission
            let temp_dir = std::env::temp_dir();
            let test_file = temp_dir.join("mcgravity_test_non_exec_file");

            // Create the file
            File::create(&test_file).expect("Failed to create test file");

            // Ensure no execute permissions (read-only)
            let metadata = fs::metadata(&test_file).expect("Failed to get metadata");
            let mut perms = metadata.permissions();
            perms.set_mode(0o644); // rw-r--r--
            fs::set_permissions(&test_file, perms).expect("Failed to set permissions");

            // Verify it's not executable
            assert!(
                !is_executable(&test_file),
                "Non-executable file should return false"
            );

            // Clean up
            let _ = fs::remove_file(&test_file);
        }

        /// Tests that a file with execute permission returns true.
        #[test]
        fn executable_file_returns_true() {
            // Create a temp file with execute permission
            let temp_dir = std::env::temp_dir();
            let test_file = temp_dir.join("mcgravity_test_exec_file");

            // Create the file
            File::create(&test_file).expect("Failed to create test file");

            // Set execute permissions
            let metadata = fs::metadata(&test_file).expect("Failed to get metadata");
            let mut perms = metadata.permissions();
            perms.set_mode(0o755); // rwxr-xr-x
            fs::set_permissions(&test_file, perms).expect("Failed to set permissions");

            // Verify it's executable
            assert!(
                is_executable(&test_file),
                "Executable file should return true"
            );

            // Clean up
            let _ = fs::remove_file(&test_file);
        }

        /// Tests that a directory returns false (not executable as a command).
        #[test]
        fn directory_returns_false() {
            let temp_dir = std::env::temp_dir();
            assert!(!is_executable(&temp_dir), "Directory should return false");
        }

        /// Tests that a nonexistent path returns false.
        #[test]
        fn nonexistent_path_returns_false() {
            let fake_path = PathBuf::from("/this/path/does/not/exist/at/all");
            assert!(
                !is_executable(&fake_path),
                "Nonexistent path should return false"
            );
        }
    }

    // =========================================================================
    // Shell Resolution Tests (Unix-specific)
    // =========================================================================

    #[cfg(unix)]
    mod shell_resolution_tests {
        use super::*;

        /// Tests that shell-resolved commands succeed when the shell is available.
        ///
        /// This test verifies that the shell resolution fallback works correctly
        /// by resolving a known command that should be available via the shell.
        #[test]
        fn shell_resolves_known_command() {
            // First check if we have a working shell
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let shell_exists = PathBuf::from(&shell).exists();

            if !shell_exists {
                // Skip test if no shell available (unlikely in practice)
                eprintln!("Skipping shell resolution test: no shell available");
                return;
            }

            // `echo` is a builtin in most shells, should be resolvable
            // Note: We test with 'ls' which is typically a PATH executable
            // to ensure the shell can resolve commands
            let resolution = resolve_cli_command("ls");
            assert!(
                resolution.is_available(),
                "ls should be resolvable via shell or PATH"
            );
        }

        /// Tests classification of alias output.
        #[test]
        fn classifies_alias_output() {
            let output = "alias claude='/usr/local/bin/claude'";
            let result = classify_command_v_output(output, "claude");
            assert!(matches!(result, CommandResolution::ShellAlias(_)));
        }

        /// Tests classification of function output.
        #[test]
        fn classifies_function_output() {
            let output = "claude is a function";
            let result = classify_command_v_output(output, "claude");
            assert!(matches!(result, CommandResolution::ShellFunction(_)));

            let output2 = "claude () {\n  /usr/local/bin/claude \"$@\"\n}";
            let result2 = classify_command_v_output(output2, "claude");
            assert!(matches!(result2, CommandResolution::ShellFunction(_)));
        }

        /// Tests classification of path output.
        #[test]
        fn classifies_path_output() {
            // Use /bin/sh which should exist and be executable
            let output = "/bin/sh";
            let result = classify_command_v_output(output, "sh");
            if PathBuf::from(output).exists() && is_executable(&PathBuf::from(output)) {
                assert!(matches!(result, CommandResolution::PathExecutable(_)));
            }
        }
    }

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
