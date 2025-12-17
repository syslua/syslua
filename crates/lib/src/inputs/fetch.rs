//! Git fetch and path resolution for inputs.
//!
//! This module handles:
//! - Cloning/fetching git repositories to the cache directory
//! - Checking out specific revisions
//! - Resolving path inputs with tilde expansion
//!
//! # Cache Structure
//!
//! Git inputs are cached at `~/.cache/syslua/inputs/{name}/` with their `.git`
//! directories intact to enable incremental fetches.

use std::fs;
use std::path::{Path, PathBuf};

use gix::remote::Direction;
use thiserror::Error;
use tracing::{debug, info};

use crate::platform::paths::home_dir;

/// Errors that can occur during fetch operations.
#[derive(Debug, Error)]
pub enum FetchError {
  /// Failed to create the cache directory.
  #[error("failed to create cache directory '{0}': {1}")]
  CreateCacheDir(PathBuf, #[source] std::io::Error),

  /// Failed to clone a git repository.
  #[error("failed to clone repository '{url}': {source}")]
  Clone {
    url: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },

  /// Failed to open an existing git repository.
  #[error("failed to open repository at '{path}': {source}")]
  Open {
    path: PathBuf,
    #[source]
    source: Box<gix::open::Error>,
  },

  /// Failed to fetch from remote.
  #[error("failed to fetch from '{url}': {source}")]
  Fetch {
    url: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },

  /// Failed to find the specified revision.
  #[error("revision '{rev}' not found in repository")]
  RevisionNotFound { rev: String },

  /// Failed to checkout a revision.
  #[error("failed to checkout revision '{rev}': {source}")]
  Checkout {
    rev: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },

  /// Failed to resolve HEAD reference.
  #[error("failed to resolve HEAD: {0}")]
  ResolveHead(String),

  /// The path does not exist.
  #[error("path does not exist: {0}")]
  PathNotFound(PathBuf),

  /// Failed to canonicalize path.
  #[error("failed to resolve path '{path}': {source}")]
  CanonicalizePath {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },

  /// Failed to find remote.
  #[error("no remote configured for repository")]
  NoRemote,

  /// Failed to connect to remote.
  #[error("failed to connect to remote '{url}': {source}")]
  Connect {
    url: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },
}

/// Fetch a git input to the cache directory.
///
/// If the cache exists, fetches updates and checks out the target revision.
/// If the cache doesn't exist, clones and checks out.
/// If `rev` is `None`, uses HEAD and returns the resolved commit hash.
///
/// # Arguments
///
/// * `name` - The input name (used as the cache directory name)
/// * `url` - The git URL (without scheme prefix, e.g., "https://github.com/org/repo.git")
/// * `rev` - Optional revision to checkout (commit hash, tag, or branch)
/// * `cache_dir` - The base cache directory (e.g., `~/.cache/syslua/inputs`)
///
/// # Returns
///
/// A tuple of `(path, rev)` where:
/// - `path` is the full path to the checked-out repository
/// - `rev` is the actual commit hash that was checked out
pub fn fetch_git(name: &str, url: &str, rev: Option<&str>, cache_dir: &Path) -> Result<(PathBuf, String), FetchError> {
  let repo_path = cache_dir.join(name);

  // Ensure cache directory exists
  if !cache_dir.exists() {
    fs::create_dir_all(cache_dir).map_err(|e| FetchError::CreateCacheDir(cache_dir.to_path_buf(), e))?;
  }

  let repo = if repo_path.join(".git").exists() {
    // Repository exists, open and fetch
    debug!(name, path = %repo_path.display(), "opening existing repository");
    let repo = gix::open(&repo_path).map_err(|e| FetchError::Open {
      path: repo_path.clone(),
      source: Box::new(e),
    })?;

    // Fetch updates from origin
    fetch_updates(&repo, url)?;
    repo
  } else {
    // Clone the repository
    info!(name, url, path = %repo_path.display(), "cloning repository");
    clone_repo(url, &repo_path)?
  };

  // Resolve the target revision to a commit hash
  let commit_hash = resolve_revision(&repo, rev)?;

  debug!(name, rev = %commit_hash, "resolved revision");
  Ok((repo_path, commit_hash))
}

/// Clone a git repository to the specified path.
fn clone_repo(url: &str, dest: &Path) -> Result<gix::Repository, FetchError> {
  let mut prepared = gix::prepare_clone(url, dest).map_err(|e| FetchError::Clone {
    url: url.to_string(),
    source: Box::new(e),
  })?;

  let (mut checkout, _outcome) = prepared
    .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
    .map_err(|e| FetchError::Clone {
      url: url.to_string(),
      source: Box::new(e),
    })?;

  let (repo, _outcome) = checkout
    .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
    .map_err(|e| FetchError::Checkout {
      rev: "HEAD".to_string(),
      source: Box::new(e),
    })?;

  Ok(repo)
}

/// Fetch updates from the remote.
fn fetch_updates(repo: &gix::Repository, url: &str) -> Result<(), FetchError> {
  debug!(url, "fetching updates");

  let remote = repo
    .find_default_remote(Direction::Fetch)
    .ok_or(FetchError::NoRemote)?
    .map_err(|e| FetchError::Connect {
      url: url.to_string(),
      source: Box::new(e),
    })?;

  let connection = remote.connect(Direction::Fetch).map_err(|e| FetchError::Connect {
    url: url.to_string(),
    source: Box::new(e),
  })?;

  connection
    .prepare_fetch(gix::progress::Discard, Default::default())
    .map_err(|e| FetchError::Fetch {
      url: url.to_string(),
      source: Box::new(e),
    })?
    .receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
    .map_err(|e| FetchError::Fetch {
      url: url.to_string(),
      source: Box::new(e),
    })?;

  Ok(())
}

/// Resolve a revision spec to a commit hash.
///
/// If `rev` is `None`, resolves HEAD.
/// If `rev` is `Some`, tries to resolve it as a revision (commit, tag, branch).
fn resolve_revision(repo: &gix::Repository, rev: Option<&str>) -> Result<String, FetchError> {
  match rev {
    Some(rev_str) => {
      // Try to find the revision in the repository
      let spec = repo.rev_parse(rev_str).map_err(|_| FetchError::RevisionNotFound {
        rev: rev_str.to_string(),
      })?;

      let object_id = spec.single().ok_or_else(|| FetchError::RevisionNotFound {
        rev: format!("{} (ambiguous)", rev_str),
      })?;

      // Peel to commit to get the actual commit hash
      let commit = object_id.object().map_err(|e| FetchError::RevisionNotFound {
        rev: format!("{}: {}", rev_str, e),
      })?;

      Ok(commit.id.to_string())
    }
    None => {
      // Use HEAD
      let mut head = repo.head().map_err(|e| FetchError::ResolveHead(e.to_string()))?;

      let commit = head
        .peel_to_commit()
        .map_err(|e| FetchError::ResolveHead(e.to_string()))?;

      Ok(commit.id.to_string())
    }
  }
}

/// Resolve a path input.
///
/// Handles:
/// - Tilde expansion (`~` -> home directory)
/// - Relative paths (resolved against `config_dir`)
/// - Validates the path exists
///
/// # Arguments
///
/// * `path_str` - The path string (may contain `~` or be relative)
/// * `config_dir` - The directory containing the config file (for relative path resolution)
///
/// # Returns
///
/// The canonicalized absolute path.
pub fn resolve_path(path_str: &str, config_dir: &Path) -> Result<PathBuf, FetchError> {
  let expanded = if let Some(rest) = path_str.strip_prefix("~/") {
    // Tilde expansion
    home_dir().join(rest)
  } else if path_str == "~" {
    home_dir()
  } else if path_str.starts_with('/') {
    // Already absolute
    PathBuf::from(path_str)
  } else {
    // Relative path - resolve against config_dir
    config_dir.join(path_str)
  };

  // Canonicalize to get absolute path and verify existence
  let canonical = expanded.canonicalize().map_err(|e| {
    if e.kind() == std::io::ErrorKind::NotFound {
      FetchError::PathNotFound(expanded.clone())
    } else {
      FetchError::CanonicalizePath {
        path: expanded,
        source: e,
      }
    }
  })?;

  debug!(path = %canonical.display(), "resolved path input");
  Ok(canonical)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use tempfile::TempDir;

  mod resolve_path_tests {
    use super::*;

    #[test]
    #[serial]
    #[cfg(unix)]
    fn tilde_expansion() {
      let temp_dir = TempDir::new().unwrap();
      let home = temp_dir.path();

      // Create a directory in our fake home
      let dotfiles = home.join("dotfiles");
      fs::create_dir(&dotfiles).unwrap();

      temp_env::with_var("HOME", Some(home.to_str().unwrap()), || {
        let result = resolve_path("~/dotfiles", Path::new("/unused")).unwrap();
        assert_eq!(result, dotfiles.canonicalize().unwrap());
      });
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn bare_tilde() {
      let temp_dir = TempDir::new().unwrap();
      let home = temp_dir.path();

      temp_env::with_var("HOME", Some(home.to_str().unwrap()), || {
        let result = resolve_path("~", Path::new("/unused")).unwrap();
        assert_eq!(result, home.canonicalize().unwrap());
      });
    }

    #[test]
    fn relative_path() {
      let temp_dir = TempDir::new().unwrap();
      let config_dir = temp_dir.path();

      // Create a subdirectory
      let subdir = config_dir.join("local-config");
      fs::create_dir(&subdir).unwrap();

      let result = resolve_path("./local-config", config_dir).unwrap();
      assert_eq!(result, subdir.canonicalize().unwrap());
    }

    #[test]
    fn absolute_path() {
      let temp_dir = TempDir::new().unwrap();
      let abs_path = temp_dir.path();

      let result = resolve_path(abs_path.to_str().unwrap(), Path::new("/unused")).unwrap();
      assert_eq!(result, abs_path.canonicalize().unwrap());
    }

    #[test]
    fn nonexistent_path_returns_error() {
      let result = resolve_path("/nonexistent/path/12345", Path::new("/unused"));
      assert!(matches!(result, Err(FetchError::PathNotFound(_))));
    }
  }

  mod git_fetch_tests {
    use super::*;
    use std::process::Command;

    /// Create a local git repository with an initial commit.
    /// Returns the commit hash of the initial commit.
    fn create_local_repo(path: &Path) -> String {
      // Initialize repo
      let output = Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("git init failed");
      assert!(output.status.success(), "git init failed: {:?}", output);

      // Configure git for the test (avoid using system user config)
      Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .output()
        .expect("git config email failed");
      Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .expect("git config name failed");

      // Create initial commit
      fs::write(path.join("README.md"), "# Test Repo\n").unwrap();
      Command::new("git")
        .args(["add", "README.md"])
        .current_dir(path)
        .output()
        .expect("git add failed");
      let output = Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .expect("git commit failed");
      assert!(output.status.success(), "git commit failed: {:?}", output);

      // Get the commit hash
      let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .expect("git rev-parse failed");
      String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Create a tagged commit in a repo.
    /// Returns the commit hash of the tagged commit.
    fn create_tag(path: &Path, tag_name: &str) -> String {
      fs::write(path.join("CHANGELOG.md"), format!("# {}\n", tag_name)).unwrap();
      Command::new("git")
        .args(["add", "CHANGELOG.md"])
        .current_dir(path)
        .output()
        .expect("git add failed");
      Command::new("git")
        .args(["commit", "-m", &format!("Release {}", tag_name)])
        .current_dir(path)
        .output()
        .expect("git commit failed");
      Command::new("git")
        .args(["tag", tag_name])
        .current_dir(path)
        .output()
        .expect("git tag failed");

      let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .expect("git rev-parse failed");
      String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn fetch_git_clones_local_repo() {
      let temp = TempDir::new().unwrap();
      let source_repo = temp.path().join("source");
      let cache_dir = temp.path().join("cache");

      fs::create_dir(&source_repo).unwrap();
      let commit_hash = create_local_repo(&source_repo);

      // Fetch using file:// URL
      let url = format!("file://{}", source_repo.display());
      let (path, rev) = fetch_git("test-input", &url, None, &cache_dir).unwrap();

      // Verify the repo was cloned
      assert!(path.exists());
      assert!(path.join("README.md").exists());
      // The resolved revision should be the commit hash
      assert_eq!(rev, commit_hash);
    }

    #[test]
    fn fetch_git_checks_out_specific_tag() {
      let temp = TempDir::new().unwrap();
      let source_repo = temp.path().join("source");
      let cache_dir = temp.path().join("cache");

      fs::create_dir(&source_repo).unwrap();
      let _initial = create_local_repo(&source_repo);
      let v1_hash = create_tag(&source_repo, "v1.0.0");

      // Create more commits after the tag
      fs::write(source_repo.join("NEW.md"), "new content").unwrap();
      Command::new("git")
        .args(["add", "NEW.md"])
        .current_dir(&source_repo)
        .output()
        .unwrap();
      Command::new("git")
        .args(["commit", "-m", "Post-release commit"])
        .current_dir(&source_repo)
        .output()
        .unwrap();

      // Fetch the v1.0.0 tag specifically
      let url = format!("file://{}", source_repo.display());
      let (_path, rev) = fetch_git("test-input", &url, Some("v1.0.0"), &cache_dir).unwrap();

      // Should resolve to the v1.0.0 commit, not HEAD
      assert_eq!(rev, v1_hash);
    }

    #[test]
    fn fetch_git_resolves_branch_name() {
      let temp = TempDir::new().unwrap();
      let source_repo = temp.path().join("source");
      let cache_dir = temp.path().join("cache");

      fs::create_dir(&source_repo).unwrap();
      let _initial = create_local_repo(&source_repo);

      // Get the current branch name (usually "main" or "master")
      let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&source_repo)
        .output()
        .expect("git rev-parse failed");
      let branch_name = String::from_utf8(output.stdout).unwrap().trim().to_string();

      // Create another commit on the branch
      fs::write(source_repo.join("EXTRA.md"), "extra").unwrap();
      Command::new("git")
        .args(["add", "EXTRA.md"])
        .current_dir(&source_repo)
        .output()
        .unwrap();
      Command::new("git")
        .args(["commit", "-m", "Second commit"])
        .current_dir(&source_repo)
        .output()
        .unwrap();

      let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&source_repo)
        .output()
        .unwrap();
      let expected_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();

      // Fetch by branch name
      let url = format!("file://{}", source_repo.display());
      let (_path, rev) = fetch_git("test-input", &url, Some(&branch_name), &cache_dir).unwrap();

      assert_eq!(rev, expected_hash);
    }

    #[test]
    fn fetch_git_returns_error_for_invalid_revision() {
      let temp = TempDir::new().unwrap();
      let source_repo = temp.path().join("source");
      let cache_dir = temp.path().join("cache");

      fs::create_dir(&source_repo).unwrap();
      create_local_repo(&source_repo);

      let url = format!("file://{}", source_repo.display());
      let result = fetch_git("test-input", &url, Some("nonexistent-tag"), &cache_dir);

      assert!(matches!(result, Err(FetchError::RevisionNotFound { .. })));
    }

    #[test]
    fn fetch_git_returns_error_for_invalid_url() {
      let temp = TempDir::new().unwrap();
      let cache_dir = temp.path().join("cache");

      // Try to clone from a non-existent path
      let result = fetch_git("test-input", "file:///nonexistent/path/to/repo", None, &cache_dir);

      // Should fail with a clone error
      assert!(result.is_err());
    }
  }
}
