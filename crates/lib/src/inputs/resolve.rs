//! Input resolution orchestration.
//!
//! This module coordinates the full input resolution flow:
//! 1. Parse input URLs from the raw `M.inputs` table
//! 2. Check lock file for pinned revisions
//! 3. Fetch/resolve each input (git clone/fetch or path resolution)
//! 4. Update lock file with new entries
//!
//! # Resolution Algorithm
//!
//! For each input in the config:
//! - If config specifies a rev (`#v1.0.0`): use that rev, verify lock matches if present
//! - If locked and URL matches: use locked revision
//! - If locked but URL differs: error (requires `sys update`)
//! - If not locked: fetch latest and add to lock file

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tracing::{debug, info, warn};

use super::fetch::{FetchError, fetch_git, resolve_path};
use super::lock::{LOCK_FILENAME, LockFile, LockedInput};
use super::source::{InputSource, ParseError, parse, source_type};
use crate::platform::paths::cache_dir;

/// A resolved input ready for use.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
  /// Path to the input's root directory.
  pub path: PathBuf,
  /// The resolved revision (git commit hash or "local" for paths).
  pub rev: String,
}

/// Map of input names to their resolved state.
pub type ResolvedInputs = BTreeMap<String, ResolvedInput>;

/// Result of input resolution.
#[derive(Debug)]
pub struct ResolutionResult {
  /// Resolved inputs ready for use.
  pub inputs: ResolvedInputs,
  /// Updated lock file (may have new entries).
  pub lock_file: LockFile,
  /// Whether the lock file changed and should be written.
  pub lock_changed: bool,
}

/// Errors that can occur during input resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
  /// Failed to parse an input URL.
  #[error("failed to parse input '{name}': {source}")]
  Parse {
    name: String,
    #[source]
    source: ParseError,
  },

  /// Lock file URL doesn't match config URL.
  #[error("input '{name}' URL changed from '{locked_url}' to '{config_url}'. Run 'sys update {name}' to update.")]
  LockMismatch {
    name: String,
    locked_url: String,
    config_url: String,
  },

  /// Failed to fetch a git input.
  #[error("failed to fetch input '{name}': {source}")]
  Fetch {
    name: String,
    #[source]
    source: FetchError,
  },

  /// Failed to load lock file.
  #[error("failed to load lock file: {0}")]
  LoadLock(#[source] super::lock::LockError),

  /// Failed to save lock file.
  #[error("failed to save lock file: {0}")]
  SaveLock(#[source] super::lock::LockError),
}

/// Resolve all inputs from the raw `M.inputs` table.
///
/// # Arguments
///
/// * `raw_inputs` - Map of input names to URL strings from config
/// * `config_dir` - Directory containing the config file (for lock file and relative paths)
/// * `force_update` - Optional set of input names to force update (ignore lock file).
///   - `None`: use lock file revisions when available (normal behavior)
///   - `Some(empty set)`: force update all inputs
///   - `Some(non-empty set)`: force update only the named inputs
///
/// # Returns
///
/// A [`ResolutionResult`] containing:
/// - Resolved inputs with paths and revisions
/// - Updated lock file
/// - Whether the lock file changed
///
/// # Errors
///
/// Returns [`ResolveError`] if:
/// - An input URL cannot be parsed
/// - A locked input's URL doesn't match the config (requires `sys update`)
/// - Fetching a git input fails
/// - A path input doesn't exist
pub fn resolve_inputs(
  raw_inputs: &HashMap<String, String>,
  config_dir: &Path,
  force_update: Option<&HashSet<String>>,
) -> Result<ResolutionResult, ResolveError> {
  let lock_path = config_dir.join(LOCK_FILENAME);

  // Load existing lock file (or create new)
  let mut lock_file = LockFile::load(&lock_path)
    .map_err(ResolveError::LoadLock)?
    .unwrap_or_default();

  let mut resolved = BTreeMap::new();
  let mut lock_changed = false;

  // Get cache directory for git inputs
  let inputs_cache_dir = cache_dir().join("inputs");

  info!(count = raw_inputs.len(), "resolving inputs");

  for (name, url) in raw_inputs {
    debug!(name, url, "resolving input");

    let source = parse(url).map_err(|e| ResolveError::Parse {
      name: name.clone(),
      source: e,
    })?;

    // Check lock file for existing entry
    let locked_entry = lock_file.get(name);

    // Determine if this input should be force-updated (ignore lock)
    let should_force = force_update
      .map(|set| set.is_empty() || set.contains(name))
      .unwrap_or(false);

    // Verify URL hasn't changed (if locked and not force-updating)
    if !should_force
      && let Some(locked) = locked_entry
      && locked.url != *url
    {
      return Err(ResolveError::LockMismatch {
        name: name.clone(),
        locked_url: locked.url.clone(),
        config_url: url.clone(),
      });
    }

    let (path, rev) = match source {
      InputSource::Git {
        url: git_url,
        rev: config_rev,
      } => {
        // Determine which rev to use:
        // 1. If force-updating: only use config-specified rev (or fetch HEAD)
        // 2. Otherwise: config rev takes precedence, then lock file rev
        let target_rev = if should_force {
          config_rev.as_deref()
        } else {
          config_rev.as_deref().or(locked_entry.map(|e| e.rev.as_str()))
        };

        let (path, actual_rev) =
          fetch_git(name, &git_url, target_rev, &inputs_cache_dir).map_err(|e| ResolveError::Fetch {
            name: name.clone(),
            source: e,
          })?;

        // Add/update lock entry if:
        // - Not already locked, OR
        // - Force updating this input, OR
        // - Config specifies a rev that differs from locked rev
        let should_update_lock = match locked_entry {
          None => true,
          Some(locked) => {
            // Update if force-updating or if config specifies a new rev
            should_force || (config_rev.is_some() && locked.rev != actual_rev)
          }
        };

        if should_update_lock {
          info!(name, rev = %actual_rev, "locking input");
          let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

          lock_file.insert(
            name.clone(),
            LockedInput::new(
              source_type(&InputSource::Git {
                url: git_url,
                rev: config_rev,
              }),
              url,
              &actual_rev,
            )
            .with_last_modified(timestamp),
          );
          lock_changed = true;
        }

        (path, actual_rev)
      }
      InputSource::Path { path: path_str } => {
        let resolved_path =
          resolve_path(path_str.to_str().unwrap_or(""), config_dir).map_err(|e| ResolveError::Fetch {
            name: name.clone(),
            source: e,
          })?;

        let rev = "local".to_string();

        // Add to lock file if not present (paths use "local" as rev)
        if locked_entry.is_none() {
          info!(name, path = %resolved_path.display(), "locking new path input");
          lock_file.insert(name.clone(), LockedInput::new("path", url, &rev));
          lock_changed = true;
        }

        (resolved_path, rev)
      }
    };

    resolved.insert(name.clone(), ResolvedInput { path, rev });
  }

  // Clean up lock entries for inputs that were removed from config
  let config_names: std::collections::HashSet<_> = raw_inputs.keys().collect();
  let locked_names: Vec<_> = lock_file.inputs.keys().cloned().collect();

  for locked_name in locked_names {
    if !config_names.contains(&locked_name) {
      warn!(name = %locked_name, "removing stale input from lock file");
      lock_file.inputs.remove(&locked_name);
      lock_changed = true;
    }
  }

  Ok(ResolutionResult {
    inputs: resolved,
    lock_file,
    lock_changed,
  })
}

/// Save the lock file if it changed.
pub fn save_lock_file_if_changed(result: &ResolutionResult, config_dir: &Path) -> Result<(), ResolveError> {
  if result.lock_changed {
    let lock_path = config_dir.join(LOCK_FILENAME);
    info!(path = %lock_path.display(), "writing lock file");
    result.lock_file.save(&lock_path).map_err(ResolveError::SaveLock)?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  mod resolve_inputs_tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_input_resolves_correctly() {
      let temp_dir = TempDir::new().unwrap();
      let config_dir = temp_dir.path();

      // Create a local input directory
      let local_input = config_dir.join("my-input");
      fs::create_dir(&local_input).unwrap();

      let mut raw_inputs = HashMap::new();
      raw_inputs.insert("local".to_string(), "path:./my-input".to_string());

      let result = resolve_inputs(&raw_inputs, config_dir, None).unwrap();

      assert_eq!(result.inputs.len(), 1);
      let resolved = result.inputs.get("local").unwrap();
      assert_eq!(resolved.path, local_input.canonicalize().unwrap());
      assert_eq!(resolved.rev, "local");
      assert!(result.lock_changed);
    }

    #[test]
    fn lock_mismatch_returns_error() {
      let temp_dir = TempDir::new().unwrap();
      let config_dir = temp_dir.path();

      // Create existing lock file with different URL
      let mut lock = LockFile::new();
      lock.insert(
        "myinput".to_string(),
        LockedInput::new("git", "git:https://old-url.com/repo.git", "abc123"),
      );
      lock.save(&config_dir.join(LOCK_FILENAME)).unwrap();

      // Try to resolve with different URL
      let mut raw_inputs = HashMap::new();
      raw_inputs.insert("myinput".to_string(), "git:https://new-url.com/repo.git".to_string());

      let result = resolve_inputs(&raw_inputs, config_dir, None);

      assert!(matches!(result, Err(ResolveError::LockMismatch { .. })));
    }

    #[test]
    fn invalid_url_returns_parse_error() {
      let temp_dir = TempDir::new().unwrap();

      let mut raw_inputs = HashMap::new();
      raw_inputs.insert("bad".to_string(), "invalid-url".to_string());

      let result = resolve_inputs(&raw_inputs, temp_dir.path(), None);

      assert!(matches!(result, Err(ResolveError::Parse { .. })));
    }

    #[test]
    fn nonexistent_path_returns_error() {
      let temp_dir = TempDir::new().unwrap();

      let mut raw_inputs = HashMap::new();
      raw_inputs.insert("missing".to_string(), "path:./does-not-exist".to_string());

      let result = resolve_inputs(&raw_inputs, temp_dir.path(), None);

      assert!(matches!(result, Err(ResolveError::Fetch { .. })));
    }
  }
}
