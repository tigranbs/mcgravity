//! Todo file scanning and management.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Default directory containing todo files.
pub const TODO_DIR: &str = ".mcgravity/todo";

/// Default directory for completed todo files.
pub const DONE_DIR: &str = ".mcgravity/todo/done";

/// Ensures the mcgravity todo directories exist.
/// Creates `.mcgravity/todo/` and `.mcgravity/todo/done/` if they don't exist.
///
/// # Errors
///
/// Returns an error if the directories cannot be created.
pub fn ensure_todo_dirs() -> Result<()> {
    std::fs::create_dir_all(TODO_DIR).context("Failed to create .mcgravity/todo directory")?;
    std::fs::create_dir_all(DONE_DIR).context("Failed to create .mcgravity/todo/done directory")?;
    Ok(())
}

/// Scans the specified directory for markdown files.
///
/// Returns files sorted by creation time (oldest first).
///
/// # Arguments
///
/// * `todo_dir` - Path to the directory containing todo files
///
/// # Errors
///
/// Returns an error if the directory cannot be read or file metadata is inaccessible.
pub async fn scan_todo_files(todo_dir: &Path) -> Result<Vec<PathBuf>> {
    // Check if directory exists using tokio::fs::try_exists
    if !fs::try_exists(todo_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    let mut read_dir = fs::read_dir(todo_dir)
        .await
        .context("Failed to read todo directory")?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();

        // Only include .md files that are not in subdirectories
        if let Ok(metadata) = fs::metadata(&path).await
            && metadata.is_file()
            && path.extension().is_some_and(|ext| ext == "md")
            && let Ok(created) = metadata.created().or_else(|_| metadata.modified())
        {
            files.push((path, created));
        }
    }

    // Sort by creation time (oldest first)
    files.sort_by(|a, b| a.1.cmp(&b.1));

    Ok(files.into_iter().map(|(path, _)| path).collect())
}

/// Moves completed todo files to the specified done directory.
///
/// Creates the done directory if it doesn't exist. Returns the final destination
/// paths for each moved file in the same order as the input, allowing callers to
/// observe the actual archived filename (which may differ due to conflict handling).
///
/// # Arguments
///
/// * `files` - List of file paths to move
/// * `done_dir` - Path to the directory where completed files should be moved
///
/// # Returns
///
/// A vector of destination paths in the same order as input files.
///
/// # Errors
///
/// Returns an error if files cannot be moved or directory cannot be created.
pub async fn move_to_done(files: &[PathBuf], done_dir: &Path) -> Result<Vec<PathBuf>> {
    // Create done directory if it doesn't exist
    if !fs::try_exists(done_dir).await.unwrap_or(false) {
        fs::create_dir_all(done_dir)
            .await
            .context("Failed to create done directory")?;
    }

    let mut archived_paths = Vec::with_capacity(files.len());

    for file in files {
        if let Some(file_name) = file.file_name() {
            let dest = done_dir.join(file_name);

            // Handle potential name conflicts by adding a suffix
            let final_dest = if fs::try_exists(&dest).await.unwrap_or(false) {
                let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                done_dir.join(format!("{stem}_{timestamp}.md"))
            } else {
                dest
            };

            fs::rename(file, &final_dest)
                .await
                .with_context(|| format!("Failed to move {} to done", file.display()))?;

            archived_paths.push(final_dest);
        }
    }

    Ok(archived_paths)
}

/// Reads the content of a file.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub async fn read_file_content(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file: {}", path.display()))
}

/// Removes all files from the specified done directory.
///
/// This function cleans up completed task files to prevent re-verification
/// in future runs. The directory itself is preserved but emptied.
///
/// # Arguments
///
/// * `done_dir` - Path to the directory containing completed task files
///
/// # Errors
///
/// Returns an error if the directory cannot be read or files cannot be deleted.
/// Individual file deletion errors are logged but don't stop the cleanup process.
pub async fn remove_done_files(done_dir: &Path) -> Result<()> {
    // Return early if directory doesn't exist
    if !fs::try_exists(done_dir).await.unwrap_or(false) {
        return Ok(());
    }

    let mut read_dir = fs::read_dir(done_dir)
        .await
        .with_context(|| format!("Failed to read done directory: {}", done_dir.display()))?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();

        // Only delete files, not subdirectories
        if let Ok(metadata) = fs::metadata(&path).await
            && metadata.is_file()
        {
            // Attempt to delete the file, continue on error
            if let Err(e) = fs::remove_file(&path).await {
                eprintln!("Warning: Failed to remove file {}: {}", path.display(), e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serial_test::serial;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Mutex to serialize tests that modify the current working directory.
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    /// Guard struct that restores the original directory when dropped.
    struct CwdGuard {
        original_dir: std::path::PathBuf,
        /// Held to keep the mutex locked until this guard is dropped.
        #[allow(dead_code)] // Field is held for RAII locking
        mutex_guard: std::sync::MutexGuard<'static, ()>,
    }

    impl CwdGuard {
        fn new() -> Result<Self> {
            let mutex_guard = CWD_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(Self {
                original_dir: std::env::current_dir()?,
                mutex_guard,
            })
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original_dir);
        }
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    mod constants {
        use super::*;

        /// Tests that `TODO_DIR` constant has expected value.
        #[test]
        fn todo_dir_is_todo() {
            assert_eq!(TODO_DIR, ".mcgravity/todo");
        }

        /// Tests that `DONE_DIR` constant has expected value.
        #[test]
        fn done_dir_is_todo_done() {
            assert_eq!(DONE_DIR, ".mcgravity/todo/done");
        }
    }

    // =========================================================================
    // ensure_todo_dirs Tests
    // =========================================================================

    mod ensure_todo_dirs_tests {
        use super::*;
        use std::path::Path;

        /// Tests that `ensure_todo_dirs` creates directories when they don't exist.
        #[test]
        #[serial]
        fn creates_directories_when_missing() -> Result<()> {
            let _guard = CwdGuard::new()?;
            let temp_dir = TempDir::new()?;
            std::env::set_current_dir(temp_dir.path())?;

            // Directories should not exist initially
            assert!(!Path::new(TODO_DIR).exists());
            assert!(!Path::new(DONE_DIR).exists());

            ensure_todo_dirs()?;

            // Both directories should now exist
            assert!(Path::new(TODO_DIR).exists());
            assert!(Path::new(DONE_DIR).exists());
            Ok(())
        }

        /// Tests that `ensure_todo_dirs` succeeds when directories already exist.
        #[test]
        #[serial]
        fn succeeds_when_directories_exist() -> Result<()> {
            let _guard = CwdGuard::new()?;
            let temp_dir = TempDir::new()?;
            std::env::set_current_dir(temp_dir.path())?;

            // Create directories first
            std::fs::create_dir_all(DONE_DIR)?;
            assert!(Path::new(TODO_DIR).exists());
            assert!(Path::new(DONE_DIR).exists());

            // Should succeed even though directories already exist
            ensure_todo_dirs()?;

            // Directories should still exist
            assert!(Path::new(TODO_DIR).exists());
            assert!(Path::new(DONE_DIR).exists());
            Ok(())
        }

        /// Tests that `ensure_todo_dirs` creates the full path hierarchy.
        #[test]
        #[serial]
        fn creates_full_path_hierarchy() -> Result<()> {
            let _guard = CwdGuard::new()?;
            let temp_dir = TempDir::new()?;
            std::env::set_current_dir(temp_dir.path())?;

            // Parent .mcgravity directory should not exist
            assert!(!Path::new(".mcgravity").exists());

            ensure_todo_dirs()?;

            // All directories in the hierarchy should exist
            assert!(Path::new(".mcgravity").exists());
            assert!(Path::new(".mcgravity/todo").exists());
            assert!(Path::new(".mcgravity/todo/done").exists());
            Ok(())
        }
    }

    // =========================================================================
    // read_file_content Tests
    // =========================================================================

    mod read_file_content_tests {
        use super::*;

        /// Tests reading an existing file.
        #[tokio::test]
        async fn reads_existing_file() -> Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("test.txt");
            fs::write(&file_path, "Hello, World!").await?;

            let content = read_file_content(&file_path).await?;
            assert_eq!(content, "Hello, World!");
            Ok(())
        }

        /// Tests reading a file with multiple lines.
        #[tokio::test]
        async fn reads_multiline_file() -> Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("multiline.txt");
            fs::write(&file_path, "Line 1\nLine 2\nLine 3").await?;

            let content = read_file_content(&file_path).await?;
            assert_eq!(content, "Line 1\nLine 2\nLine 3");
            Ok(())
        }

        /// Tests reading an empty file.
        #[tokio::test]
        async fn reads_empty_file() -> Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("empty.txt");
            fs::write(&file_path, "").await?;

            let content = read_file_content(&file_path).await?;
            assert!(content.is_empty());
            Ok(())
        }

        /// Tests reading a non-existent file returns error.
        #[tokio::test]
        async fn nonexistent_file_returns_error() {
            let path = Path::new("/nonexistent/path/to/file.txt");
            let result = read_file_content(path).await;

            assert!(result.is_err());
        }

        /// Tests error message contains file path.
        #[tokio::test]
        async fn error_contains_file_path() {
            let path = Path::new("/nonexistent/file.txt");
            let result = read_file_content(path).await;

            let err_msg = result.expect_err("Expected error").to_string();
            assert!(err_msg.contains("Failed to read file"));
        }

        /// Tests reading file with UTF-8 content.
        #[tokio::test]
        async fn reads_utf8_content() -> Result<()> {
            let dir = TempDir::new()?;
            let file_path = dir.path().join("utf8.txt");
            fs::write(&file_path, "こんにちは 🌍").await?;

            let content = read_file_content(&file_path).await?;
            assert_eq!(content, "こんにちは 🌍");
            Ok(())
        }
    }

    // =========================================================================
    // scan_todo_files Tests
    // =========================================================================

    mod scan_todo_files_tests {
        use super::*;

        /// Tests scanning an empty directory returns empty vec.
        #[tokio::test]
        async fn empty_directory_returns_empty_vec() -> Result<()> {
            let dir = TempDir::new()?;
            let files = scan_todo_files(dir.path()).await?;
            assert!(files.is_empty());
            Ok(())
        }

        /// Tests scanning a non-existent directory returns empty vec.
        #[tokio::test]
        async fn nonexistent_directory_returns_empty_vec() -> Result<()> {
            let dir = TempDir::new()?;
            let nonexistent = dir.path().join("does_not_exist");
            let files = scan_todo_files(&nonexistent).await?;
            assert!(files.is_empty());
            Ok(())
        }

        /// Tests that only .md files are returned.
        #[tokio::test]
        async fn only_returns_md_files() -> Result<()> {
            let dir = TempDir::new()?;

            // Create various files
            fs::write(dir.path().join("task1.md"), "Task 1").await?;
            fs::write(dir.path().join("task2.txt"), "Task 2").await?;
            fs::write(dir.path().join("task3.md"), "Task 3").await?;
            fs::write(dir.path().join("readme.rst"), "Readme").await?;

            let files = scan_todo_files(dir.path()).await?;

            assert_eq!(files.len(), 2);
            assert!(
                files
                    .iter()
                    .all(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
            );
            Ok(())
        }

        /// Tests that subdirectories are ignored.
        #[tokio::test]
        async fn ignores_subdirectories() -> Result<()> {
            let dir = TempDir::new()?;

            // Create a file and a subdirectory with a file
            fs::write(dir.path().join("task.md"), "Task").await?;
            let subdir = dir.path().join("subdir");
            fs::create_dir(&subdir).await?;
            fs::write(subdir.join("nested.md"), "Nested").await?;

            let files = scan_todo_files(dir.path()).await?;

            assert_eq!(files.len(), 1);
            assert!(files[0].file_name().is_some_and(|n| n == "task.md"));
            Ok(())
        }

        /// Tests that files are sorted by creation time (oldest first).
        #[tokio::test]
        async fn files_sorted_by_creation_time() -> Result<()> {
            let dir = TempDir::new()?;

            // Create files with small delays to ensure different timestamps
            fs::write(dir.path().join("first.md"), "First").await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            fs::write(dir.path().join("second.md"), "Second").await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            fs::write(dir.path().join("third.md"), "Third").await?;

            let files = scan_todo_files(dir.path()).await?;

            assert_eq!(files.len(), 3);
            // Files should be sorted oldest first
            assert!(files[0].file_name().is_some_and(|n| n == "first.md"));
            assert!(files[1].file_name().is_some_and(|n| n == "second.md"));
            assert!(files[2].file_name().is_some_and(|n| n == "third.md"));
            Ok(())
        }
    }

    // =========================================================================
    // move_to_done Tests
    // =========================================================================

    mod move_to_done_tests {
        use super::*;

        /// Tests moving a single file to done directory.
        #[tokio::test]
        async fn moves_single_file() -> Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join("todo");
            let done_dir = dir.path().join("done");

            fs::create_dir(&todo_dir).await?;
            let file_path = todo_dir.join("task.md");
            fs::write(&file_path, "Task content").await?;

            let result = move_to_done(std::slice::from_ref(&file_path), &done_dir).await?;

            // Verify returned path
            assert_eq!(result.len(), 1);
            assert_eq!(result[0], done_dir.join("task.md"));

            // Original file should be gone
            assert!(!fs::try_exists(&file_path).await?);
            // File should exist in done directory
            assert!(fs::try_exists(done_dir.join("task.md")).await?);
            Ok(())
        }

        /// Tests moving multiple files to done directory.
        #[tokio::test]
        async fn moves_multiple_files() -> Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join("todo");
            let done_dir = dir.path().join("done");

            fs::create_dir(&todo_dir).await?;

            let file1 = todo_dir.join("task1.md");
            let file2 = todo_dir.join("task2.md");
            fs::write(&file1, "Task 1").await?;
            fs::write(&file2, "Task 2").await?;

            let result = move_to_done(&[file1.clone(), file2.clone()], &done_dir).await?;

            // Verify returned paths are in input order
            assert_eq!(result.len(), 2);
            assert_eq!(result[0], done_dir.join("task1.md"));
            assert_eq!(result[1], done_dir.join("task2.md"));

            // Original files should be gone
            assert!(!fs::try_exists(&file1).await?);
            assert!(!fs::try_exists(&file2).await?);
            // Files should exist in done directory
            assert!(fs::try_exists(done_dir.join("task1.md")).await?);
            assert!(fs::try_exists(done_dir.join("task2.md")).await?);
            Ok(())
        }

        /// Tests that done directory is created if it doesn't exist.
        #[tokio::test]
        async fn creates_done_directory() -> Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join("todo");
            let done_dir = dir.path().join("done/nested/deep");

            fs::create_dir(&todo_dir).await?;
            let file_path = todo_dir.join("task.md");
            fs::write(&file_path, "Task").await?;

            // Done directory doesn't exist yet
            assert!(!fs::try_exists(&done_dir).await?);

            let result = move_to_done(&[file_path], &done_dir).await?;

            // Verify returned path
            assert_eq!(result.len(), 1);
            assert_eq!(result[0], done_dir.join("task.md"));

            // Done directory should now exist with the file
            assert!(fs::try_exists(done_dir.join("task.md")).await?);
            Ok(())
        }

        /// Tests handling of name conflicts by adding timestamp suffix.
        #[tokio::test]
        async fn handles_name_conflicts() -> Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join("todo");
            let done_dir = dir.path().join("done");

            fs::create_dir_all(&todo_dir).await?;
            fs::create_dir_all(&done_dir).await?;

            // Create a file with the same name in done directory
            fs::write(done_dir.join("task.md"), "Existing").await?;

            let file_path = todo_dir.join("task.md");
            fs::write(&file_path, "New task").await?;

            let result = move_to_done(std::slice::from_ref(&file_path), &done_dir).await?;

            // Verify returned path has timestamp suffix (conflict handling)
            assert_eq!(result.len(), 1);
            let archived_name = result[0]
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;
            assert!(
                archived_name.starts_with("task_")
                    && std::path::Path::new(archived_name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md")),
                "Archived filename should have timestamp suffix: {archived_name}"
            );

            // Original file should be gone
            assert!(!fs::try_exists(&file_path).await?);

            // Original done file should still exist
            let original_content = fs::read_to_string(done_dir.join("task.md")).await?;
            assert_eq!(original_content, "Existing");

            // New file should have timestamp suffix and correct content
            let archived_content = fs::read_to_string(&result[0]).await?;
            assert_eq!(archived_content, "New task");

            // Verify there are exactly 2 files in done directory
            let mut entries = fs::read_dir(&done_dir).await?;
            let mut count = 0;
            while entries.next_entry().await?.is_some() {
                count += 1;
            }
            assert_eq!(count, 2); // Original + new timestamped file
            Ok(())
        }

        /// Tests moving empty file list succeeds and returns empty vec.
        #[tokio::test]
        async fn empty_file_list_succeeds() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");

            let result = move_to_done(&[], &done_dir).await?;
            assert!(result.is_empty());
            Ok(())
        }

        /// Tests preserving file content during move.
        #[tokio::test]
        async fn preserves_file_content() -> Result<()> {
            let dir = TempDir::new()?;
            let todo_dir = dir.path().join("todo");
            let done_dir = dir.path().join("done");

            fs::create_dir(&todo_dir).await?;
            let file_path = todo_dir.join("task.md");
            let content = "# Task\n\nDetailed description with special chars: @#$%^&*()";
            fs::write(&file_path, content).await?;

            move_to_done(&[file_path], &done_dir).await?;

            let moved_content = fs::read_to_string(done_dir.join("task.md")).await?;
            assert_eq!(moved_content, content);
            Ok(())
        }
    }

    // =========================================================================
    // remove_done_files Tests
    // =========================================================================

    mod remove_done_files_tests {
        use super::*;

        /// Tests deleting files from a populated directory.
        #[tokio::test]
        async fn deletes_files_from_populated_directory() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");
            fs::create_dir(&done_dir).await?;

            // Create some files
            fs::write(done_dir.join("task1.md"), "Task 1").await?;
            fs::write(done_dir.join("task2.md"), "Task 2").await?;
            fs::write(done_dir.join("task3.txt"), "Task 3").await?;

            remove_done_files(&done_dir).await?;

            // Directory should still exist but be empty
            assert!(fs::try_exists(&done_dir).await?);

            let mut entries = fs::read_dir(&done_dir).await?;
            let mut count = 0;
            while entries.next_entry().await?.is_some() {
                count += 1;
            }
            assert_eq!(count, 0, "Directory should be empty after cleanup");
            Ok(())
        }

        /// Tests calling on an empty directory succeeds.
        #[tokio::test]
        async fn empty_directory_succeeds() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");
            fs::create_dir(&done_dir).await?;

            remove_done_files(&done_dir).await?;

            // Directory should still exist
            assert!(fs::try_exists(&done_dir).await?);
            Ok(())
        }

        /// Tests calling on a non-existent directory succeeds.
        #[tokio::test]
        async fn nonexistent_directory_succeeds() -> Result<()> {
            let dir = TempDir::new()?;
            let nonexistent = dir.path().join("does_not_exist");

            remove_done_files(&nonexistent).await?;
            Ok(())
        }

        /// Tests that files are actually removed.
        #[tokio::test]
        async fn verifies_files_are_gone() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");
            fs::create_dir(&done_dir).await?;

            let file1 = done_dir.join("task1.md");
            let file2 = done_dir.join("task2.md");

            fs::write(&file1, "Task 1").await?;
            fs::write(&file2, "Task 2").await?;

            // Verify files exist before cleanup
            assert!(fs::try_exists(&file1).await?);
            assert!(fs::try_exists(&file2).await?);

            remove_done_files(&done_dir).await?;

            // Verify files are gone
            assert!(!fs::try_exists(&file1).await?);
            assert!(!fs::try_exists(&file2).await?);
            Ok(())
        }

        /// Tests that subdirectories are preserved (not deleted).
        #[tokio::test]
        async fn preserves_subdirectories() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");
            let subdir = done_dir.join("subdir");

            fs::create_dir_all(&subdir).await?;
            fs::write(done_dir.join("task.md"), "Task").await?;
            fs::write(subdir.join("nested.md"), "Nested").await?;

            remove_done_files(&done_dir).await?;

            // Subdirectory should still exist with its contents
            assert!(fs::try_exists(&subdir).await?);
            assert!(fs::try_exists(subdir.join("nested.md")).await?);

            // But the file in done_dir should be gone
            assert!(!fs::try_exists(done_dir.join("task.md")).await?);
            Ok(())
        }

        /// Tests that directory itself is preserved after cleanup.
        #[tokio::test]
        async fn preserves_directory_structure() -> Result<()> {
            let dir = TempDir::new()?;
            let done_dir = dir.path().join("done");
            fs::create_dir(&done_dir).await?;

            fs::write(done_dir.join("task.md"), "Task").await?;

            remove_done_files(&done_dir).await?;

            // Directory should still exist
            assert!(fs::try_exists(&done_dir).await?);
            let metadata = fs::metadata(&done_dir).await?;
            assert!(metadata.is_dir());
            Ok(())
        }
    }
}
