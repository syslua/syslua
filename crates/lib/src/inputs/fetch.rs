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

  // NOTE: Git clone/fetch tests require network access and are better suited
  // for integration tests. The core logic is tested via the path resolution tests.
}
