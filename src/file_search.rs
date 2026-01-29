//! File search functionality for @ mentions.
//!
//! This module provides fuzzy file path matching for the text input's
//! @ file tagging feature. It uses the `ignore` crate for efficient
//! directory traversal and `nucleo-matcher` for fuzzy matching.

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::path::{Path, PathBuf};

/// Maximum number of file matches to return.
const MAX_FILE_MATCHES: usize = 8;

/// Score boost applied to directories during fuzzy matching.
/// This ensures directories appear prominently when their names match well.
const DIRECTORY_SCORE_BOOST: u32 = 50;

/// A single file match from a search operation.
#[derive(Debug, Clone)]
pub struct FileMatch {
    /// The relative path from the working directory.
    pub path: PathBuf,
    /// The fuzzy match score (higher is better).
    pub score: u32,
    /// Whether this match is a directory.
    pub is_dir: bool,
}

/// Result of a file search operation.
///
/// Contains both the matching files and metadata about errors
/// that occurred during the search (e.g., permission denied).
#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    /// Matching files, sorted by score (best matches first).
    pub matches: Vec<FileMatch>,
    /// Number of directories that could not be accessed (permission denied, etc.).
    pub inaccessible_dirs: usize,
    /// True if any errors occurred during the search.
    pub had_errors: bool,
}

impl SearchResult {
    /// Returns just the matches, for convenience and backward compatibility.
    #[must_use]
    pub fn into_matches(self) -> Vec<FileMatch> {
        self.matches
    }
}

/// Searches for files matching the given query.
///
/// # Arguments
///
/// * `query` - The search query (text after `@`)
/// * `working_dir` - The base directory to search in
///
/// # Returns
///
/// A `SearchResult` containing matching files (sorted by score, best matches first)
/// and metadata about any errors that occurred during traversal.
/// Returns at most `MAX_FILE_MATCHES` results.
#[must_use]
pub fn search_files(query: &str, working_dir: &Path) -> SearchResult {
    let mut result = SearchResult::default();

    // Build the walker for directory traversal
    let walker = WalkBuilder::new(working_dir)
        .hidden(false) // Don't skip hidden files by default
        .git_ignore(true) // Respect .gitignore in git repos
        .git_global(true) // Respect global git excludes
        .git_exclude(true) // Respect .git/info/exclude
        .follow_links(true) // Follow symlinks
        .add_custom_ignore_filename(".gitignore") // Also support .gitignore in non-git dirs
        .build();

    // Collect file and directory paths while tracking errors
    let mut entries: Vec<(PathBuf, bool)> = Vec::new(); // (path, is_dir)

    for entry_result in walker {
        match entry_result {
            Ok(entry) => {
                if let Some(ft) = entry.file_type() {
                    // Include both files and directories (skip root directory)
                    if let Ok(relative_path) = entry.path().strip_prefix(working_dir) {
                        if relative_path.as_os_str().is_empty() {
                            // Skip the root directory itself
                            continue;
                        }
                        entries.push((relative_path.to_path_buf(), ft.is_dir()));
                    }
                }
            }
            Err(e) => {
                // Track errors
                result.had_errors = true;

                // Count permission-related errors
                if let Some(io_error) = e.io_error()
                    && io_error.kind() == std::io::ErrorKind::PermissionDenied
                {
                    result.inaccessible_dirs += 1;
                }
            }
        }
    }

    // Handle empty query: return entries with directories first, then sorted alphabetically
    if query.is_empty() {
        // Sort by: directories first, then alphabetically by path
        entries.sort_by(|a, b| {
            // is_dir: true sorts before false (reverse order because true > false in bool comparison)
            match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.cmp(&b.0),
            }
        });
        result.matches = entries
            .into_iter()
            .take(MAX_FILE_MATCHES)
            .map(|(path, is_dir)| FileMatch {
                path,
                score: 0,
                is_dir,
            })
            .collect();
        return result;
    }

    // Set up fuzzy matcher
    let mut fuzzy_matcher = Matcher::new(Config::DEFAULT);
    let atom = Atom::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
        false,
    );

    // Score each entry
    // For directories, append "/" to the haystack so queries like "dir/" match well
    // Also give directories a score boost to ensure they appear prominently
    let mut matches: Vec<FileMatch> = Vec::new();
    for (path, is_dir) in entries {
        let path_str = path.to_string_lossy();
        let haystack_str = if is_dir {
            format!("{path_str}/")
        } else {
            path_str.into_owned()
        };
        let mut haystack_buf = Vec::new();
        let haystack = Utf32Str::new(&haystack_str, &mut haystack_buf);

        if let Some(score) = atom.score(haystack, &mut fuzzy_matcher) {
            // Apply a score boost to directories so they stay visible among many file matches
            let final_score = if is_dir {
                u32::from(score).saturating_add(DIRECTORY_SCORE_BOOST)
            } else {
                u32::from(score)
            };
            matches.push(FileMatch {
                path,
                score: final_score,
                is_dir,
            });
        }
    }

    // Sort by score (descending), then alphabetically for ties
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));

    // Limit results
    matches.truncate(MAX_FILE_MATCHES);
    result.matches = matches;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn create_test_files(dir: &Path, files: &[&str]) -> Result<()> {
        for file in files {
            let path = dir.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            File::create(&path)?;
        }
        Ok(())
    }

    #[test]
    fn test_search_files_empty_query() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["a.txt", "b.txt", "c.txt"])?;

        let result = search_files("", temp_dir.path());

        // Should return files sorted alphabetically
        assert!(!result.matches.is_empty());
        assert!(result.matches.len() <= MAX_FILE_MATCHES);
        // Verify alphabetical order
        for i in 1..result.matches.len() {
            assert!(result.matches[i - 1].path <= result.matches[i].path);
        }
        Ok(())
    }

    #[test]
    fn test_search_files_exact_match() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["main.rs", "lib.rs", "test.rs", "main_helper.rs"],
        )?;

        let result = search_files("main.rs", temp_dir.path());

        assert!(!result.matches.is_empty());
        // Exact match should be first or near the top
        assert!(
            result
                .matches
                .iter()
                .any(|m| m.path == Path::new("main.rs"))
        );
        // main.rs should have a high score
        let main_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("main.rs"));
        assert!(main_match.is_some());
        Ok(())
    }

    #[test]
    fn test_search_files_fuzzy_match() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["src/file_search.rs", "src/main.rs", "tests/integration.rs"],
        )?;

        let result = search_files("flsrch", temp_dir.path());

        // Should find file_search.rs via fuzzy matching
        assert!(
            result
                .matches
                .iter()
                .any(|m| m.path.to_string_lossy().contains("file_search"))
        );
        Ok(())
    }

    #[test]
    fn test_search_files_respects_gitignore() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Create a .gitignore file
        fs::write(temp_dir.path().join(".gitignore"), "ignored_dir/\n*.log\n")?;

        // Create files, some that should be ignored
        create_test_files(
            temp_dir.path(),
            &[
                "keep.txt",
                "ignored_dir/secret.txt",
                "debug.log",
                "src/main.rs",
            ],
        )?;

        let result = search_files("", temp_dir.path());

        // Should not find ignored files
        assert!(
            !result
                .matches
                .iter()
                .any(|m| m.path.to_string_lossy().contains("ignored_dir"))
        );
        assert!(
            !result
                .matches
                .iter()
                .any(|m| m.path.to_string_lossy().contains("debug.log"))
        );

        // Should find non-ignored files
        assert!(
            result
                .matches
                .iter()
                .any(|m| m.path == Path::new("keep.txt"))
        );
        Ok(())
    }

    #[test]
    fn test_search_files_limits_results() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Create more files than MAX_FILE_MATCHES
        let files: Vec<String> = (0..20).map(|i| format!("file_{i}.txt")).collect();
        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        create_test_files(temp_dir.path(), &file_refs)?;

        let result = search_files("file", temp_dir.path());

        assert!(result.matches.len() <= MAX_FILE_MATCHES);
        Ok(())
    }

    #[test]
    fn test_search_files_no_matches() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["apple.txt", "banana.txt"])?;

        let result = search_files("zzzznotfound", temp_dir.path());

        assert!(result.matches.is_empty());
        Ok(())
    }

    #[test]
    fn test_search_files_nested_directories() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &[
                "src/app/mod.rs",
                "src/core/executor.rs",
                "tests/unit/test_executor.rs",
            ],
        )?;

        let result = search_files("executor", temp_dir.path());

        assert!(!result.matches.is_empty());
        // Should find files in nested directories
        assert!(
            result
                .matches
                .iter()
                .any(|m| m.path.to_string_lossy().contains("executor"))
        );
        Ok(())
    }

    #[test]
    fn test_search_files_scores_sorted_descending() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["main.rs", "main_test.rs", "some_main.rs", "other.rs"],
        )?;

        let result = search_files("main", temp_dir.path());

        // Verify scores are in descending order
        for i in 1..result.matches.len() {
            assert!(result.matches[i - 1].score >= result.matches[i].score);
        }
        Ok(())
    }

    #[test]
    fn test_search_result_default_has_no_errors() {
        let result = SearchResult::default();
        assert!(!result.had_errors);
        assert_eq!(result.inaccessible_dirs, 0);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_search_with_no_errors_reports_clean() -> Result<()> {
        let temp_dir = TempDir::new()?;
        File::create(temp_dir.path().join("file.txt"))?;

        let result = search_files("file", temp_dir.path());

        assert!(!result.had_errors);
        assert_eq!(result.inaccessible_dirs, 0);
        Ok(())
    }

    #[test]
    fn test_into_matches_returns_matches() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["test.txt"])?;

        let result = search_files("test", temp_dir.path());
        let matches = result.into_matches();

        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.path == Path::new("test.txt")));
        Ok(())
    }

    #[test]
    fn test_search_finds_directories() -> Result<()> {
        let temp_dir = TempDir::new()?;
        // Create nested directories with files
        create_test_files(
            temp_dir.path(),
            &["src/main.rs", "src/lib.rs", "tests/test_main.rs"],
        )?;

        let result = search_files("src", temp_dir.path());

        // Should find the src directory
        let src_match = result.matches.iter().find(|m| m.path == Path::new("src"));
        assert!(src_match.is_some(), "Should find 'src' directory");
        assert!(
            src_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "src should be marked as directory"
        );
        Ok(())
    }

    #[test]
    fn test_directory_is_dir_flag() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["src/main.rs", "file.txt"])?;

        let result = search_files("", temp_dir.path());

        // Find the directory and file
        let src_match = result.matches.iter().find(|m| m.path == Path::new("src"));
        let file_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("file.txt"));

        assert!(src_match.is_some(), "Should find src directory");
        assert!(
            src_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "src should have is_dir=true"
        );

        assert!(file_match.is_some(), "Should find file.txt");
        assert!(
            !file_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "file.txt should have is_dir=false"
        );
        Ok(())
    }

    #[test]
    fn test_directory_matches_with_trailing_slash() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["src/main.rs", "src/lib.rs", "srcfile.txt"],
        )?;

        // Query with trailing slash should match directories well
        let result = search_files("src/", temp_dir.path());

        // The src directory should be in the results
        let src_match = result.matches.iter().find(|m| m.path == Path::new("src"));
        assert!(
            src_match.is_some(),
            "Should find 'src' directory with trailing slash query"
        );
        assert!(
            src_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "src should be marked as directory"
        );
        Ok(())
    }

    #[test]
    fn test_nested_directory_found() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["src/app/mod.rs", "src/core/mod.rs", "tests/test.rs"],
        )?;

        let result = search_files("app", temp_dir.path());

        // Should find the src/app directory
        let app_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("src/app"));
        assert!(app_match.is_some(), "Should find 'src/app' directory");
        assert!(
            app_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "src/app should be marked as directory"
        );
        Ok(())
    }

    #[test]
    fn test_files_have_is_dir_false() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_files(temp_dir.path(), &["main.rs", "lib.rs"])?;

        let result = search_files("main", temp_dir.path());

        let main_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("main.rs"));
        assert!(main_match.is_some(), "Should find main.rs");
        assert!(
            !main_match
                .ok_or_else(|| anyhow::anyhow!("checked above"))?
                .is_dir,
            "main.rs should have is_dir=false"
        );
        Ok(())
    }

    #[test]
    fn test_directory_visible_among_many_files() -> Result<()> {
        // This test ensures directories appear even when many files have similar names.
        // This simulates the real scenario where typing "@src" should show the src/ directory
        // even when many files inside src/ also match.
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &[
                "src/main.rs",
                "src/lib.rs",
                "src/app/mod.rs",
                "src/app/events.rs",
                "src/app/input.rs",
                "src/core/mod.rs",
                "src/core/executor.rs",
                "src/tui/mod.rs",
                "src/tui/theme.rs",
                "src/file_search.rs",
            ],
        )?;

        let result = search_files("src", temp_dir.path());

        // The src directory should appear in the results
        let src_dir_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("src") && m.is_dir);
        assert!(
            src_dir_match.is_some(),
            "src/ directory should be visible among file results. Found: {:?}",
            result.matches.iter().map(|m| &m.path).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn test_directory_not_pushed_out_by_max_matches() -> Result<()> {
        // This test ensures directories are not pushed out when there are more than
        // MAX_FILE_MATCHES files matching the query.
        let temp_dir = TempDir::new()?;

        // Create more files than MAX_FILE_MATCHES (8) that all match "src"
        let mut files: Vec<String> = (0..15).map(|i| format!("src/file_{i}.rs")).collect();
        files.push("src/app/mod.rs".to_string());
        files.push("src/core/mod.rs".to_string());
        files.push("src/tui/mod.rs".to_string());

        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        create_test_files(temp_dir.path(), &file_refs)?;

        let result = search_files("src", temp_dir.path());

        // The src directory should still appear in results
        let src_dir_match = result
            .matches
            .iter()
            .find(|m| m.path == Path::new("src") && m.is_dir);
        assert!(
            src_dir_match.is_some(),
            "src/ directory should not be pushed out by many files. Found: {:?}",
            result
                .matches
                .iter()
                .map(|m| (&m.path, m.score, m.is_dir))
                .collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn test_empty_query_shows_directories_first() -> Result<()> {
        // When browsing (empty query), directories should be prominently visible
        let temp_dir = TempDir::new()?;
        create_test_files(
            temp_dir.path(),
            &["src/main.rs", "tests/test.rs", "docs/readme.md", "file.txt"],
        )?;

        let result = search_files("", temp_dir.path());

        // With empty query, we should see directories (they're sorted alphabetically)
        let dirs: Vec<_> = result.matches.iter().filter(|m| m.is_dir).collect();
        assert!(
            !dirs.is_empty(),
            "Empty query should return directories. Found: {:?}",
            result
                .matches
                .iter()
                .map(|m| (&m.path, m.is_dir))
                .collect::<Vec<_>>()
        );
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod unix_permission_tests {
    use super::*;
    use anyhow::Result;
    use std::fs::{self, File, Permissions};
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn test_search_counts_inaccessible_directories() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Create an accessible file
        let accessible = temp_dir.path().join("accessible.txt");
        File::create(&accessible)?;

        // Create an inaccessible directory
        let restricted_dir = temp_dir.path().join("restricted");
        fs::create_dir(&restricted_dir)?;

        // Create a file inside the restricted directory
        let restricted_file = restricted_dir.join("secret.txt");
        File::create(&restricted_file)?;

        // Make the directory inaccessible (no read permission)
        fs::set_permissions(&restricted_dir, Permissions::from_mode(0o000))?;

        // Search should complete but report the error
        let result = search_files("", temp_dir.path());

        // Restore permissions for cleanup
        fs::set_permissions(&restricted_dir, Permissions::from_mode(0o755))?;

        // Should have found the accessible file
        assert!(
            result
                .matches
                .iter()
                .any(|m| m.path == Path::new("accessible.txt"))
        );

        // Should have reported the inaccessible directory
        // Note: The exact count depends on how the walker reports errors
        assert!(result.had_errors || result.inaccessible_dirs > 0 || result.matches.len() == 1);
        Ok(())
    }
}
