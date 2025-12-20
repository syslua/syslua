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
//!
//! # Transitive Resolution
//!
//! When an input has its own dependencies (declared in its init.lua), we:
//! 1. Fetch the input first
//! 2. Parse its init.lua to extract declared inputs
//! 3. Apply any `follows` overrides from the parent config
//! 4. Recursively resolve transitive dependencies
//! 5. Create `.inputs/` symlinks to connect dependencies

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tracing::{debug, info, trace, warn};

use super::fetch::{FetchError, fetch_git, resolve_path};
use super::graph::{DependencyGraph, GraphError, build_initial_graph};
use super::lock::{LOCK_FILENAME, LockFile, LockedInput};
use super::source::{InputSource, ParseError, parse, source_type};
use super::store::{InputStore, StoreError};
use super::types::{
  InputDecl, InputDecls, InputOverride, ResolvedInput as TypesResolvedInput, ResolvedInputs as TypesResolvedInputs,
};
use crate::lua::{loaders, runtime};
use crate::manifest::Manifest;
use crate::platform::paths::cache_dir;

/// Result of transitive input resolution.
#[derive(Debug)]
pub struct ResolutionResult {
  /// Resolved inputs with their transitive dependencies.
  pub inputs: TypesResolvedInputs,
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

  /// Graph resolution error.
  #[error("dependency graph error: {0}")]
  Graph(#[from] GraphError),

  /// Store operation error.
  #[error("store error: {0}")]
  Store(#[from] StoreError),

  /// Failed to extract inputs from an input's init.lua.
  #[error("failed to extract inputs from '{name}': {message}")]
  ExtractInputs { name: String, message: String },

  /// Input has no URL (pure follows without target).
  #[error("input '{name}' has no URL and follows target '{target}' could not be resolved")]
  NoUrl { name: String, target: String },
}

/// Resolve inputs with full transitive dependency support.
///
/// This function extends the basic resolution with:
/// - Parsing extended input declarations (table syntax with overrides)
/// - Recursive resolution of transitive dependencies
/// - Application of `follows` overrides
/// - Creation of `.inputs/` symlinks for dependency resolution
///
/// # Arguments
///
/// * `input_decls` - Input declarations from the config (supports extended syntax)
/// * `config_dir` - Directory containing the config file
/// * `force_update` - Optional set of input names to force update
///
/// # Returns
///
/// A [`ResolutionResult`] with fully resolved inputs including transitive deps.
pub fn resolve_inputs(
  input_decls: &InputDecls,
  config_dir: &Path,
  force_update: Option<&HashSet<String>>,
) -> Result<ResolutionResult, ResolveError> {
  let lock_path = config_dir.join(LOCK_FILENAME);

  // Load existing lock file (or create new)
  let mut lock_file = LockFile::load(&lock_path)
    .map_err(ResolveError::LoadLock)?
    .unwrap_or_default();

  let mut lock_changed = false;

  // Get cache directory and store
  let inputs_cache_dir = cache_dir().join("inputs");
  let store = InputStore::new();
  store.ensure_store_dir()?;

  // Build initial dependency graph from root declarations
  let mut graph = build_initial_graph(input_decls);

  // Track resolved inputs: full_path -> (path, rev, url)
  let mut resolved_cache: BTreeMap<String, (PathBuf, String, String)> = BTreeMap::new();

  // Track which inputs we've processed for transitive deps
  let mut processed_for_deps: HashSet<String> = HashSet::new();

  // Track URLs we've seen to avoid infinite loops with circular deps
  let mut seen_urls: HashSet<String> = HashSet::new();

  info!(
    count = input_decls.len(),
    "resolving inputs with transitive dependencies"
  );

  // Process inputs in waves until no new dependencies are discovered
  loop {
    let mut new_deps_found = false;

    // Get all unprocessed nodes that have URLs
    let nodes_to_process: Vec<(String, Option<String>)> = graph
      .nodes
      .iter()
      .filter(|(path, _)| !processed_for_deps.contains(*path))
      .filter_map(|(path, node)| {
        // Get effective URL (considering follows)
        let url = get_effective_url(&graph, path, node);
        url.map(|u| (path.clone(), Some(u)))
      })
      .collect();

    if nodes_to_process.is_empty() {
      break;
    }

    for (full_path, url_opt) in nodes_to_process {
      let Some(url) = url_opt else {
        continue;
      };

      // Resolve this input if not already cached
      if !resolved_cache.contains_key(&full_path) {
        let node = graph.get(&full_path);
        let name = node.map(|n| n.name.as_str()).unwrap_or(&full_path);

        // Determine the base directory for path resolution:
        // - Root-level inputs: use config_dir
        // - Transitive inputs: use the parent input's resolved path
        let base_dir = if let Some(node) = node {
          if node.is_root_level() {
            config_dir.to_path_buf()
          } else if let Some((parent_path, _, _)) = resolved_cache.get(&node.parent_path) {
            parent_path.clone()
          } else {
            // Parent not yet resolved; this shouldn't happen due to wave processing
            config_dir.to_path_buf()
          }
        } else {
          config_dir.to_path_buf()
        };

        let (path, rev) = resolve_single_input(
          name,
          &url,
          &full_path,
          &mut lock_file,
          &mut lock_changed,
          force_update,
          &base_dir,
          &inputs_cache_dir,
        )?;

        resolved_cache.insert(full_path.clone(), (path, rev, url.clone()));
      }

      // Extract transitive dependencies from this input's init.lua
      if let Some((path, _, _)) = resolved_cache.get(&full_path) {
        let init_path = path.join("init.lua");
        if init_path.exists()
          && !processed_for_deps.contains(&full_path)
          && let Ok(transitive_decls) = extract_input_decls_from_file(&init_path)
          && !transitive_decls.is_empty()
        {
          trace!(
            input = %full_path,
            count = transitive_decls.len(),
            "found transitive dependencies"
          );

          // Get overrides from parent's declaration
          let parent_overrides = graph
            .get(&full_path)
            .and_then(|n| n.decl.overrides())
            .cloned()
            .unwrap_or_default();

          // Add transitive deps to graph
          for (dep_name, mut dep_decl) in transitive_decls {
            // Apply override if present
            if let Some(override_) = parent_overrides.get(&dep_name) {
              dep_decl = apply_override(dep_decl, override_.clone());
            }

            // Check if this URL has already been seen (circular dep detection)
            let dep_url = dep_decl.url().map(|s| s.to_string());
            if let Some(ref url) = dep_url
              && seen_urls.contains(url)
            {
              trace!(parent = %full_path, dep = %dep_name, url = %url, "skipping already-seen URL (circular dep)");
              continue;
            }

            let dep_path = graph.add_transitive(&dep_name, dep_decl, &full_path);
            trace!(parent = %full_path, dep = %dep_path, "added transitive dependency");
            new_deps_found = true;
          }
        }
      }

      // Mark this URL as seen
      if let Some((_, _, url)) = resolved_cache.get(&full_path) {
        seen_urls.insert(url.clone());
      }

      processed_for_deps.insert(full_path);
    }

    if !new_deps_found {
      break;
    }
  }

  // Resolve follows declarations
  graph.resolve_follows()?;

  // Build the final resolved inputs structure
  let mut final_resolved: TypesResolvedInputs = BTreeMap::new();

  // Process root-level inputs
  for name in input_decls.keys() {
    if let Some((path, rev, _)) = resolved_cache.get(name) {
      // Build transitive inputs for this root input
      let transitive = build_transitive_inputs(&graph, &resolved_cache, name);

      // Create .inputs/ symlinks if there are transitive deps
      if !transitive.is_empty() {
        let dep_paths: BTreeMap<String, PathBuf> = transitive
          .iter()
          .map(|(dep_name, dep_resolved)| (dep_name.clone(), dep_resolved.path.clone()))
          .collect();

        store.link_dependencies(path, &dep_paths)?;
      }

      final_resolved.insert(
        name.clone(),
        TypesResolvedInput::with_inputs(path.clone(), rev.clone(), transitive),
      );
    }
  }

  // Clean up stale lock entries
  let _all_resolved_names: HashSet<&String> = resolved_cache.keys().collect();
  let locked_names = lock_file.input_names();

  for locked_name in locked_names {
    // Only clean up root-level entries (transitive deps are managed differently)
    if !input_decls.contains_key(&locked_name) && !locked_name.contains('/') {
      warn!(name = %locked_name, "removing stale input from lock file");
      lock_file.remove(&locked_name);
      lock_changed = true;
    }
  }

  Ok(ResolutionResult {
    inputs: final_resolved,
    lock_file,
    lock_changed,
  })
}

/// Get the effective URL for a node, considering follows overrides.
fn get_effective_url(graph: &DependencyGraph, path: &str, node: &super::graph::GraphNode) -> Option<String> {
  // Check if this path has a follows override
  if let Some(target) = graph.follows_resolved.get(path) {
    // Get the URL from the follows target
    if let Some(target_node) = graph.get(target) {
      return target_node.decl.url().map(|s| s.to_string());
    }
  }

  // Use the node's own URL
  node.decl.url().map(|s| s.to_string())
}

/// Apply an override to an input declaration.
fn apply_override(decl: InputDecl, override_: InputOverride) -> InputDecl {
  match override_ {
    InputOverride::Url(url) => InputDecl::Url(url),
    InputOverride::Follows(target) => {
      // Create an extended decl that marks this as following another input
      // The actual resolution happens via the graph's follows_resolved
      InputDecl::Extended {
        url: decl.url().map(|s| s.to_string()),
        inputs: {
          let mut m = BTreeMap::new();
          m.insert("__follows__".to_string(), InputOverride::Follows(target));
          m
        },
      }
    }
  }
}

/// Resolve a single input (git or path).
///
/// # Arguments
///
/// * `name` - The input name
/// * `url` - The input URL
/// * `full_path` - The full path in the dependency graph
/// * `lock_file` - The lock file to update
/// * `lock_changed` - Flag to track if lock file changed
/// * `force_update` - Optional set of inputs to force update
/// * `base_dir` - Base directory for resolving relative paths (parent input's path or config dir)
/// * `inputs_cache_dir` - Cache directory for git inputs
fn resolve_single_input(
  name: &str,
  url: &str,
  full_path: &str,
  lock_file: &mut LockFile,
  lock_changed: &mut bool,
  force_update: Option<&HashSet<String>>,
  base_dir: &Path,
  inputs_cache_dir: &Path,
) -> Result<(PathBuf, String), ResolveError> {
  debug!(name, url, path = full_path, "resolving input");

  let source = parse(url).map_err(|e| ResolveError::Parse {
    name: name.to_string(),
    source: e,
  })?;

  // Use the full path as the lock key for transitive deps
  let lock_key = full_path.to_string();
  let locked_entry = lock_file.get(&lock_key);

  // Determine if this input should be force-updated
  let should_force = force_update
    .map(|set| set.is_empty() || set.contains(name) || set.contains(full_path))
    .unwrap_or(false);

  // Verify URL hasn't changed (if locked and not force-updating)
  if !should_force
    && let Some(ref locked) = locked_entry
    && locked.url != url
  {
    return Err(ResolveError::LockMismatch {
      name: name.to_string(),
      locked_url: locked.url.clone(),
      config_url: url.to_string(),
    });
  }

  let (path, rev) = match source {
    InputSource::Git {
      url: git_url,
      rev: config_rev,
    } => {
      let target_rev = if should_force {
        config_rev.as_deref()
      } else {
        config_rev.as_deref().or(locked_entry.as_ref().map(|e| e.rev.as_str()))
      };

      let (path, actual_rev) =
        fetch_git(name, &git_url, target_rev, inputs_cache_dir).map_err(|e| ResolveError::Fetch {
          name: name.to_string(),
          source: e,
        })?;

      let should_update_lock = match &locked_entry {
        None => true,
        Some(locked) => should_force || (config_rev.is_some() && locked.rev != actual_rev),
      };

      if should_update_lock {
        info!(name, rev = %actual_rev, path = %full_path, "locking input");
        let timestamp = SystemTime::now()
          .duration_since(UNIX_EPOCH)
          .map(|d| d.as_secs())
          .unwrap_or(0);

        lock_file.insert(
          lock_key,
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
        *lock_changed = true;
      }

      (path, actual_rev)
    }
    InputSource::Path { path: path_str } => {
      let resolved_path = resolve_path(path_str.to_str().unwrap_or(""), base_dir).map_err(|e| ResolveError::Fetch {
        name: name.to_string(),
        source: e,
      })?;

      let rev = "local".to_string();

      if locked_entry.is_none() {
        info!(name, path = %resolved_path.display(), "locking new path input");
        lock_file.insert(lock_key, LockedInput::new("path", url, &rev));
        *lock_changed = true;
      }

      (resolved_path, rev)
    }
  };

  Ok((path, rev))
}

/// Extract input declarations from an input's init.lua file.
fn extract_input_decls_from_file(init_path: &Path) -> Result<InputDecls, ResolveError> {
  let manifest = Rc::new(RefCell::new(Manifest::default()));
  let lua = runtime::create_runtime(manifest).map_err(|e| ResolveError::ExtractInputs {
    name: init_path.display().to_string(),
    message: e.to_string(),
  })?;

  let result = loaders::load_file_with_dir(&lua, init_path).map_err(|e| ResolveError::ExtractInputs {
    name: init_path.display().to_string(),
    message: e.to_string(),
  })?;

  let table = match result {
    mlua::Value::Table(t) => t,
    _ => return Ok(BTreeMap::new()), // Not a table, no inputs
  };

  let inputs_value: mlua::Value = table.get("inputs").map_err(|e| ResolveError::ExtractInputs {
    name: init_path.display().to_string(),
    message: e.to_string(),
  })?;

  let inputs_table = match inputs_value {
    mlua::Value::Table(t) => t,
    mlua::Value::Nil => return Ok(BTreeMap::new()),
    _ => {
      return Err(ResolveError::ExtractInputs {
        name: init_path.display().to_string(),
        message: "inputs field is not a table".to_string(),
      });
    }
  };

  let mut decls = BTreeMap::new();

  for pair in inputs_table.pairs::<String, mlua::Value>() {
    let (name, value) = pair.map_err(|e| ResolveError::ExtractInputs {
      name: init_path.display().to_string(),
      message: e.to_string(),
    })?;

    let decl = parse_lua_input_decl(&name, value).map_err(|e| ResolveError::ExtractInputs {
      name: init_path.display().to_string(),
      message: e,
    })?;

    decls.insert(name, decl);
  }

  Ok(decls)
}

/// Parse a single input declaration from a Lua value.
fn parse_lua_input_decl(name: &str, value: mlua::Value) -> Result<InputDecl, String> {
  match value {
    mlua::Value::String(s) => {
      let url = s.to_str().map_err(|e| e.to_string())?.to_string();
      Ok(InputDecl::Url(url))
    }
    mlua::Value::Table(table) => {
      let url: Option<String> = table.get("url").map_err(|e| e.to_string())?;
      let inputs_value: mlua::Value = table.get("inputs").map_err(|e| e.to_string())?;

      let overrides = match inputs_value {
        mlua::Value::Nil => BTreeMap::new(),
        mlua::Value::Table(t) => {
          let mut m = BTreeMap::new();
          for pair in t.pairs::<String, mlua::Value>() {
            let (k, v) = pair.map_err(|e| e.to_string())?;
            let override_ = parse_lua_override(&k, v)?;
            m.insert(k, override_);
          }
          m
        }
        _ => return Err(format!("input '{}': inputs field must be a table", name)),
      };

      Ok(InputDecl::Extended { url, inputs: overrides })
    }
    _ => Err(format!("input '{}' must be a string or table", name)),
  }
}

/// Parse an input override from a Lua value.
fn parse_lua_override(name: &str, value: mlua::Value) -> Result<InputOverride, String> {
  match value {
    mlua::Value::String(s) => {
      let url = s.to_str().map_err(|e| e.to_string())?.to_string();
      Ok(InputOverride::Url(url))
    }
    mlua::Value::Table(table) => {
      let follows: Option<String> = table.get("follows").map_err(|e| e.to_string())?;
      if let Some(target) = follows {
        return Ok(InputOverride::Follows(target));
      }

      let url: Option<String> = table.get("url").map_err(|e| e.to_string())?;
      if let Some(u) = url {
        return Ok(InputOverride::Url(u));
      }

      Err(format!("override '{}' must have either 'url' or 'follows' field", name))
    }
    _ => Err(format!("override '{}' must be a string URL or a table", name)),
  }
}

/// Build the transitive inputs map for a given root input.
fn build_transitive_inputs(
  graph: &DependencyGraph,
  resolved_cache: &BTreeMap<String, (PathBuf, String, String)>,
  root_path: &str,
) -> TypesResolvedInputs {
  let mut transitive = BTreeMap::new();

  // Get direct dependencies
  let deps = graph.dependencies(root_path);

  for dep_path in deps {
    if let Some(node) = graph.get(dep_path) {
      // Check if this dep follows another input
      let dep_path_owned = dep_path.to_string();
      let effective_path = graph.follows_resolved.get(dep_path).unwrap_or(&dep_path_owned);

      if let Some((path, rev, _)) = resolved_cache.get(effective_path) {
        // Recursively get this dep's transitive deps
        let nested = build_transitive_inputs(graph, resolved_cache, dep_path);

        transitive.insert(
          node.name.clone(),
          TypesResolvedInput::with_inputs(path.clone(), rev.clone(), nested),
        );
      }
    }
  }

  transitive
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

  mod transitive_resolution_tests {
    use super::*;
    use std::fs;

    /// Helper to create a minimal Lua input with dependencies
    fn create_input_with_deps(dir: &Path, deps: &[(&str, &str)]) {
      fs::create_dir_all(dir).unwrap();

      if deps.is_empty() {
        // Simple input with no dependencies
        fs::write(
          dir.join("init.lua"),
          r#"
return {
  inputs = {},
  setup = function(inputs) end,
}
"#,
        )
        .unwrap();
      } else {
        // Input with dependencies
        let inputs_str: Vec<String> = deps
          .iter()
          .map(|(name, url)| format!("  {} = \"{}\",", name, url))
          .collect();

        let content = format!(
          r#"
return {{
  inputs = {{
{}
  }},
  setup = function(inputs) end,
}}
"#,
          inputs_str.join("\n")
        );
        fs::write(dir.join("init.lua"), content).unwrap();
      }
    }

    #[test]
    fn simple_path_input_with_transitive() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create lib_a which has no deps
      let lib_a = config_dir.join("lib_a");
      create_input_with_deps(&lib_a, &[]);

      // Create lib_b which depends on lib_a
      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_a = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          lib_a.display()
        ),
      )
      .unwrap();

      // Resolve lib_b from the config
      let mut decls = InputDecls::new();
      decls.insert("lib_b".to_string(), InputDecl::Url(format!("path:{}", lib_b.display())));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib_b should be resolved
      assert!(result.inputs.contains_key("lib_b"));
      let lib_b_resolved = result.inputs.get("lib_b").unwrap();

      // lib_b should have lib_a as a transitive dependency
      assert!(lib_b_resolved.inputs.contains_key("lib_a"));
    }

    #[test]
    fn transitive_path_input_creates_inputs_symlink() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create lib_a which has no deps
      let lib_a = config_dir.join("lib_a");
      create_input_with_deps(&lib_a, &[]);

      // Create lib_b which depends on lib_a
      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_a = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          lib_a.display()
        ),
      )
      .unwrap();

      // Resolve lib_b from the config
      let mut decls = InputDecls::new();
      decls.insert("lib_b".to_string(), InputDecl::Url(format!("path:{}", lib_b.display())));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // Check that .inputs/lib_a symlink exists in lib_b's store location
      // For path inputs, the store location is the canonical path
      let lib_b_resolved = result.inputs.get("lib_b").unwrap();
      let inputs_dir = lib_b_resolved.path.join(".inputs");

      // The .inputs dir should exist and contain lib_a
      assert!(inputs_dir.exists(), ".inputs directory should exist");
      assert!(inputs_dir.join("lib_a").exists(), ".inputs/lib_a should exist");
    }

    #[test]
    fn diamond_dependency_deduplication() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create shared dep (C)
      let lib_c = config_dir.join("lib_c");
      create_input_with_deps(&lib_c, &[]);

      // Create A which depends on C
      let lib_a = config_dir.join("lib_a");
      fs::create_dir_all(&lib_a).unwrap();
      fs::write(
        lib_a.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_c = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          lib_c.display()
        ),
      )
      .unwrap();

      // Create B which also depends on C
      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_c = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          lib_c.display()
        ),
      )
      .unwrap();

      // Resolve both A and B from the config
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(format!("path:{}", lib_a.display())));
      decls.insert("lib_b".to_string(), InputDecl::Url(format!("path:{}", lib_b.display())));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // Both A and B should be resolved
      assert!(result.inputs.contains_key("lib_a"));
      assert!(result.inputs.contains_key("lib_b"));

      // Both should have lib_c as a transitive dep
      let lib_a_resolved = result.inputs.get("lib_a").unwrap();
      let lib_b_resolved = result.inputs.get("lib_b").unwrap();

      assert!(lib_a_resolved.inputs.contains_key("lib_c"));
      assert!(lib_b_resolved.inputs.contains_key("lib_c"));

      // Both should reference the same path for lib_c (deduplication)
      let lib_a_c_path = &lib_a_resolved.inputs.get("lib_c").unwrap().path;
      let lib_b_c_path = &lib_b_resolved.inputs.get("lib_c").unwrap().path;
      assert_eq!(lib_a_c_path, lib_b_c_path);
    }

    #[test]
    fn input_without_init_lua_skips_transitive() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a plain directory (no init.lua)
      let lib_a = config_dir.join("lib_a");
      fs::create_dir_all(&lib_a).unwrap();
      fs::write(lib_a.join("some_file.txt"), "hello").unwrap();

      // Resolve it
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(format!("path:{}", lib_a.display())));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib_a should be resolved with no transitive deps
      assert!(result.inputs.contains_key("lib_a"));
      let lib_a_resolved = result.inputs.get("lib_a").unwrap();
      assert!(lib_a_resolved.inputs.is_empty());
    }

    #[test]
    fn follows_override_redirects_dependency() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create utils v1
      let utils_v1 = config_dir.join("utils_v1");
      fs::create_dir_all(&utils_v1).unwrap();
      fs::write(
        utils_v1.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      // Create utils v2 (what we want to use)
      let utils_v2 = config_dir.join("utils_v2");
      fs::create_dir_all(&utils_v2).unwrap();
      fs::write(
        utils_v2.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      // Create lib that depends on utils v1
      let lib = config_dir.join("lib");
      fs::create_dir_all(&lib).unwrap();
      fs::write(
        lib.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "path:{}",
  }},
  setup = function() end,
}}
"#,
          utils_v1.display()
        ),
      )
      .unwrap();

      // Resolve with follows override to redirect utils -> my_utils (v2)
      let mut decls = InputDecls::new();

      // Declare lib with override
      let mut overrides = std::collections::BTreeMap::new();
      overrides.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));
      decls.insert(
        "lib".to_string(),
        InputDecl::Extended {
          url: Some(format!("path:{}", lib.display())),
          inputs: overrides,
        },
      );

      // Also declare my_utils pointing to v2
      decls.insert(
        "my_utils".to_string(),
        InputDecl::Url(format!("path:{}", utils_v2.display())),
      );

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib should be resolved
      assert!(result.inputs.contains_key("lib"));
      let lib_resolved = result.inputs.get("lib").unwrap();

      // lib's utils should point to my_utils (v2), not v1
      assert!(lib_resolved.inputs.contains_key("utils"));
      let utils_resolved = lib_resolved.inputs.get("utils").unwrap();

      // The path should be the v2 path, not v1
      let utils_v2_canonical = utils_v2.canonicalize().unwrap();
      assert_eq!(
        utils_resolved.path, utils_v2_canonical,
        "follows override should redirect to my_utils (v2)"
      );
    }

    #[test]
    fn circular_dependency_is_handled() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create lib_a which depends on lib_b
      let lib_a = config_dir.join("lib_a");
      let lib_b = config_dir.join("lib_b");

      fs::create_dir_all(&lib_a).unwrap();
      fs::write(
        lib_a.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_b = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_b.display()
        ),
      )
      .unwrap();

      // Create lib_b which depends on lib_a (circular!)
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_a = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_a.display()
        ),
      )
      .unwrap();

      // Resolve lib_a
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(format!("path:{}", lib_a.display())));

      // Circular deps should be handled gracefully - resolution should succeed
      let result = resolve_inputs(&decls, config_dir, None);

      // Resolution should succeed (circular deps are supported for runtime)
      assert!(result.is_ok(), "circular deps should be handled: {:?}", result);

      let resolved = result.unwrap();
      assert!(resolved.inputs.contains_key("lib_a"));

      // lib_a should have lib_b as a transitive dep
      let lib_a_resolved = resolved.inputs.get("lib_a").unwrap();
      assert!(lib_a_resolved.inputs.contains_key("lib_b"));
    }

    #[test]
    fn deeply_nested_transitive_deps() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a chain: A -> B -> C -> D (3 levels deep)
      let lib_d = config_dir.join("lib_d");
      fs::create_dir_all(&lib_d).unwrap();
      fs::write(
        lib_d.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      let lib_c = config_dir.join("lib_c");
      fs::create_dir_all(&lib_c).unwrap();
      fs::write(
        lib_c.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_d = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_d.display()
        ),
      )
      .unwrap();

      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_c = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_c.display()
        ),
      )
      .unwrap();

      let lib_a = config_dir.join("lib_a");
      fs::create_dir_all(&lib_a).unwrap();
      fs::write(
        lib_a.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_b = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_b.display()
        ),
      )
      .unwrap();

      // Resolve lib_a
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(format!("path:{}", lib_a.display())));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // Verify the full chain is resolved
      assert!(result.inputs.contains_key("lib_a"));
      let lib_a_resolved = result.inputs.get("lib_a").unwrap();

      assert!(lib_a_resolved.inputs.contains_key("lib_b"));
      let lib_b_resolved = lib_a_resolved.inputs.get("lib_b").unwrap();

      assert!(lib_b_resolved.inputs.contains_key("lib_c"));
      let lib_c_resolved = lib_b_resolved.inputs.get("lib_c").unwrap();

      assert!(lib_c_resolved.inputs.contains_key("lib_d"));
      let lib_d_resolved = lib_c_resolved.inputs.get("lib_d").unwrap();

      // lib_d should have no further deps
      assert!(lib_d_resolved.inputs.is_empty());
    }
  }
}
