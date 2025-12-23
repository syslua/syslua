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

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tracing::{debug, info, trace, warn};

use super::fetch::{FetchError, fetch_git, resolve_path};
use super::graph::{DependencyGraph, GraphError, build_initial_graph};
use super::lock::{LOCK_FILENAME, LockFile, LockedInput, load_input_lock};
use super::source::{InputSource, ParseError, parse, source_type};
use super::store::{InputStore, StoreError};
use super::types::{
  InputDecl, InputDecls, InputOverride, LuaNamespace, ResolvedInput as TypesResolvedInput,
  ResolvedInputs as TypesResolvedInputs,
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
  /// All Lua namespaces discovered during resolution.
  ///
  /// Maps namespace name to its metadata. Used for building `package.path`
  /// and detecting conflicts during evaluation.
  pub namespaces: Vec<LuaNamespace>,
}

/// Details of a namespace conflict between two inputs.
#[derive(Debug)]
pub struct NamespaceConflictError {
  pub namespace: String,
  pub provider1: String,
  pub url1: String,
  pub rev1: String,
  pub provider2: String,
  pub url2: String,
  pub rev2: String,
}

impl std::fmt::Display for NamespaceConflictError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "namespace conflict: '{}' provided by:\n  - '{}' ({}@{})\n  - '{}' ({}@{})\nAdd a follows override to resolve, or rename one of the directories.",
      self.namespace, self.provider1, self.url1, self.rev1, self.provider2, self.url2, self.rev2
    )
  }
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

  /// Namespace conflict detected between two inputs.
  #[error("{0}")]
  NamespaceConflict(Box<NamespaceConflictError>),

  /// Cyclic dependency detected.
  #[error("cyclic dependency detected: {cycle_path}")]
  CyclicDependency { cycle_path: String },
}

/// Resolve inputs with full transitive dependency support.
///
/// This function extends the basic resolution with:
/// - Parsing extended input declarations (table syntax with overrides)
/// - Recursive resolution of transitive dependencies
/// - Application of `follows` overrides
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

        let mut ctx = ResolveContext {
          lock_file: &mut lock_file,
          lock_changed: &mut lock_changed,
          force_update,
          inputs_cache_dir: &inputs_cache_dir,
        };

        let (path, rev) = resolve_single_input(name, &url, &full_path, &base_dir, &mut ctx)?;

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

          // Load the input's own lock file (if it has one)
          // This is used to pin transitive dependencies to specific revisions
          let input_lock = load_input_lock(path);
          if input_lock.is_some() {
            trace!(input = %full_path, "loaded per-input lock file");
          }

          // Add transitive deps to graph
          for (dep_name, mut dep_decl) in transitive_decls {
            // Lock file precedence order:
            // 1. `follows` directive (explicit override) - highest priority
            // 2. Input's own `syslua.lock` - input controls its transitive deps
            // 3. Input's `init.lua` declaration (floating) - if no lock exists

            // Apply override if present (follows takes highest precedence)
            if let Some(override_) = parent_overrides.get(&dep_name) {
              dep_decl = apply_override(dep_decl, override_.clone());
            } else if let Some(ref lock) = input_lock {
              // No override - check input's lock file for a pinned revision
              dep_decl = apply_input_lock_to_decl(dep_decl, &dep_name, lock);
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

  // Scan for Lua namespaces in all resolved inputs and the config directory
  let namespaces = scan_all_lua_namespaces(config_dir, &resolved_cache, &graph)?;

  Ok(ResolutionResult {
    inputs: final_resolved,
    lock_file,
    lock_changed,
    namespaces,
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

/// Apply a locked revision from an input's lock file to a dependency declaration.
///
/// If the dependency is locked in the input's lock file, this function modifies
/// the URL to include the locked revision. This ensures transitive dependencies
/// are pinned to the versions specified by the input author.
///
/// # Arguments
///
/// * `decl` - The original input declaration from the input's init.lua
/// * `dep_name` - The name of the dependency (for lookup in lock file)
/// * `lock` - The input's lock file
///
/// # Returns
///
/// The modified declaration with locked revision, or the original if not locked.
fn apply_input_lock_to_decl(decl: InputDecl, dep_name: &str, lock: &LockFile) -> InputDecl {
  // Look up the dependency in the lock file
  let Some(locked) = lock.get(dep_name) else {
    // Not locked, return original declaration
    return decl;
  };

  // Get the original URL from the declaration
  let Some(_original_url) = decl.url() else {
    // No URL in declaration, can't apply lock
    return decl;
  };

  // For path inputs, use the locked URL directly (it contains the correct path)
  // For git inputs, we inject the locked revision into the original URL
  // This ensures we use the exact pinned location/revision from the lock file
  let new_url = if locked.type_ == "path" {
    // Use the locked URL directly for path inputs
    locked.url.clone()
  } else {
    // For git inputs, inject the locked revision
    // We use the locked URL base with the locked revision to ensure we get
    // the exact same source that was locked
    inject_revision_into_url(&locked.url, &locked.rev)
  };

  trace!(
    dep = dep_name,
    locked_url = locked.url,
    locked_rev = locked.rev,
    new_url = new_url,
    "applied input lock file"
  );

  // Preserve any overrides from the original declaration
  match decl {
    InputDecl::Url(_) => InputDecl::Url(new_url),
    InputDecl::Extended { inputs, .. } => InputDecl::Extended {
      url: Some(new_url),
      inputs,
    },
  }
}

/// Inject a revision into a URL, replacing any existing revision.
///
/// For git URLs, this appends `#<rev>` or replaces an existing `#<ref>`.
/// For path URLs, this is a no-op (path inputs don't have revisions).
fn inject_revision_into_url(url: &str, rev: &str) -> String {
  if let Some(base) = url.strip_prefix("git:") {
    // Strip any existing revision
    let base_without_rev = base.split('#').next().unwrap_or(base);
    format!("git:{}#{}", base_without_rev, rev)
  } else {
    // Path or other URL type - don't modify
    url.to_string()
  }
}

/// Context for resolving a single input.
///
/// Groups together the shared state needed for input resolution to reduce
/// the number of function parameters.
struct ResolveContext<'a> {
  /// The lock file to update.
  lock_file: &'a mut LockFile,
  /// Flag to track if lock file changed.
  lock_changed: &'a mut bool,
  /// Optional set of inputs to force update.
  force_update: Option<&'a HashSet<String>>,
  /// Cache directory for git inputs.
  inputs_cache_dir: &'a Path,
}

/// Resolve a single input (git or path).
///
/// # Arguments
///
/// * `name` - The input name
/// * `url` - The input URL
/// * `full_path` - The full path in the dependency graph
/// * `base_dir` - Base directory for resolving relative paths (parent input's path or config dir)
/// * `ctx` - Resolution context with shared state
fn resolve_single_input(
  name: &str,
  url: &str,
  full_path: &str,
  base_dir: &Path,
  ctx: &mut ResolveContext<'_>,
) -> Result<(PathBuf, String), ResolveError> {
  debug!(name, url, path = full_path, "resolving input");

  let source = parse(url).map_err(|e| ResolveError::Parse {
    name: name.to_string(),
    source: e,
  })?;

  // Use the full path as the lock key for transitive deps
  let lock_key = full_path.to_string();
  let locked_entry = ctx.lock_file.get(&lock_key);

  // Determine if this input should be force-updated
  let should_force = ctx
    .force_update
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
        fetch_git(name, &git_url, target_rev, ctx.inputs_cache_dir).map_err(|e| ResolveError::Fetch {
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

        ctx.lock_file.insert(
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
        *ctx.lock_changed = true;
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
        ctx.lock_file.insert(lock_key, LockedInput::new("path", url, &rev));
        *ctx.lock_changed = true;
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

/// Scan for Lua namespaces in an input's `lua/` directory.
///
/// Returns a list of namespaces found (subdirectories of `lua/`).
fn scan_lua_namespaces(input_path: &Path, provider_input: &str, url: &str, rev: &str) -> Vec<LuaNamespace> {
  let lua_dir = input_path.join("lua");
  if !lua_dir.exists() || !lua_dir.is_dir() {
    return Vec::new();
  }

  let mut namespaces = Vec::new();

  if let Ok(entries) = std::fs::read_dir(&lua_dir) {
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir()
        && let Some(name) = path.file_name().and_then(|n| n.to_str())
      {
        let name = name.to_string();
        namespaces.push(LuaNamespace::new(name, provider_input, url, rev, path));
      }
    }
  }

  namespaces
}

/// Scan all Lua namespaces from resolved inputs and the config directory.
///
/// This function:
/// 1. Scans the config directory's `lua/` directory (if present)
/// 2. Scans each resolved input's `lua/` directory
/// 3. Detects namespace conflicts (same namespace from different sources)
/// 4. Returns deduplicated namespaces (diamond deps with same URL+rev are merged)
fn scan_all_lua_namespaces(
  config_dir: &Path,
  resolved_cache: &BTreeMap<String, (PathBuf, String, String)>,
  graph: &DependencyGraph,
) -> Result<Vec<LuaNamespace>, ResolveError> {
  // Map namespace name -> LuaNamespace (for conflict detection)
  let mut namespace_map: BTreeMap<String, LuaNamespace> = BTreeMap::new();

  // 1. Scan config directory's lua/ first (highest priority)
  let config_lua_dir = config_dir.join("lua");
  if config_lua_dir.exists() && config_lua_dir.is_dir() {
    let config_namespaces = scan_lua_namespaces(
      config_dir,
      "<config>",
      &format!("path:{}", config_dir.display()),
      "local",
    );

    for ns in config_namespaces {
      trace!(namespace = %ns.name, provider = %ns.provider_input, "found config namespace");
      namespace_map.insert(ns.name.clone(), ns);
    }
  }

  // 2. Scan all resolved inputs
  for (full_path, (input_path, rev, url)) in resolved_cache {
    // Skip inputs that are followed (they're replaced by their target)
    if graph.follows_resolved.contains_key(full_path) {
      continue;
    }

    let input_namespaces = scan_lua_namespaces(input_path, full_path, url, rev);

    for ns in input_namespaces {
      trace!(namespace = %ns.name, provider = %ns.provider_input, "found input namespace");

      if let Some(existing) = namespace_map.get(&ns.name) {
        // Check for conflict
        if existing.same_source(&ns) {
          // Same source (URL + rev), no conflict - deduplicate
          trace!(
            namespace = %ns.name,
            provider1 = %existing.provider_input,
            provider2 = %ns.provider_input,
            "deduplicating diamond dependency"
          );
          continue;
        }

        // Genuine conflict: different source or version
        return Err(ResolveError::NamespaceConflict(Box::new(NamespaceConflictError {
          namespace: ns.name,
          provider1: existing.provider_input.clone(),
          url1: existing.url.clone(),
          rev1: existing.rev.clone(),
          provider2: ns.provider_input,
          url2: ns.url,
          rev2: ns.rev,
        })));
      }

      namespace_map.insert(ns.name.clone(), ns);
    }
  }

  // Return namespaces in a deterministic order
  Ok(namespace_map.into_values().collect())
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

    use crate::util::testutil::path_to_lua_url;

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
    lib_a = "{}",
  }},
  setup = function(inputs) end,
}}
"#,
          path_to_lua_url(&lib_a)
        ),
      )
      .unwrap();

      // Resolve lib_b from the config
      let mut decls = InputDecls::new();
      decls.insert("lib_b".to_string(), InputDecl::Url(path_to_lua_url(&lib_b)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib_b should be resolved
      assert!(result.inputs.contains_key("lib_b"));
      let lib_b_resolved = result.inputs.get("lib_b").unwrap();

      // lib_b should have lib_a as a transitive dependency
      assert!(lib_b_resolved.inputs.contains_key("lib_a"));
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
    lib_c = "{}",
  }},
  setup = function(inputs) end,
}}
"#,
          path_to_lua_url(&lib_c)
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
    lib_c = "{}",
  }},
  setup = function(inputs) end,
}}
"#,
          path_to_lua_url(&lib_c)
        ),
      )
      .unwrap();

      // Resolve both A and B from the config
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));
      decls.insert("lib_b".to_string(), InputDecl::Url(path_to_lua_url(&lib_b)));

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
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));

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
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils_v1)
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
          url: Some(path_to_lua_url(&lib)),
          inputs: overrides,
        },
      );

      // Also declare my_utils pointing to v2
      decls.insert("my_utils".to_string(), InputDecl::Url(path_to_lua_url(&utils_v2)));

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
    lib_b = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&lib_b)
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
    lib_a = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&lib_a)
        ),
      )
      .unwrap();

      // Resolve lib_a
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));

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
    lib_d = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&lib_d)
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
    lib_c = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&lib_c)
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
    lib_b = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&lib_b)
        ),
      )
      .unwrap();

      // Resolve lib_a
      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));

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

  mod namespace_tests {
    use super::*;
    use std::fs;

    use crate::util::testutil::path_to_lua_url;

    /// Helper to create an input with a lua/ namespace directory
    fn create_input_with_namespace(dir: &Path, namespace: &str) {
      fs::create_dir_all(dir).unwrap();
      let lua_dir = dir.join("lua").join(namespace);
      fs::create_dir_all(&lua_dir).unwrap();
      fs::write(lua_dir.join("init.lua"), "return {}").unwrap();
      fs::write(
        dir.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();
    }

    #[test]
    fn namespace_is_discovered() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a library with a lua/my_lib/ namespace
      let lib = config_dir.join("lib");
      create_input_with_namespace(&lib, "my_lib");

      let mut decls = InputDecls::new();
      decls.insert("lib".to_string(), InputDecl::Url(path_to_lua_url(&lib)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // The namespace should be discovered
      assert_eq!(result.namespaces.len(), 1);
      assert_eq!(result.namespaces[0].name, "my_lib");
      assert_eq!(result.namespaces[0].provider_input, "lib");
    }

    #[test]
    fn config_lua_namespace_is_discovered() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a lua/ directory in config with a namespace
      let lua_dir = config_dir.join("lua").join("my_config");
      fs::create_dir_all(&lua_dir).unwrap();
      fs::write(lua_dir.join("init.lua"), "return {}").unwrap();

      // No inputs
      let decls = InputDecls::new();
      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // The config's namespace should be discovered
      assert_eq!(result.namespaces.len(), 1);
      assert_eq!(result.namespaces[0].name, "my_config");
      assert_eq!(result.namespaces[0].provider_input, "<config>");
    }

    #[test]
    fn diamond_dependency_same_version_no_conflict() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create shared utils lib
      let utils = config_dir.join("utils");
      create_input_with_namespace(&utils, "utils");

      // Create lib_a that depends on utils
      let lib_a = config_dir.join("lib_a");
      fs::create_dir_all(&lib_a).unwrap();
      fs::create_dir_all(lib_a.join("lua/lib_a")).unwrap();
      fs::write(lib_a.join("lua/lib_a/init.lua"), "return {}").unwrap();
      fs::write(
        lib_a.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils)
        ),
      )
      .unwrap();

      // Create lib_b that also depends on utils (same path = same version)
      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::create_dir_all(lib_b.join("lua/lib_b")).unwrap();
      fs::write(lib_b.join("lua/lib_b/init.lua"), "return {}").unwrap();
      fs::write(
        lib_b.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils)
        ),
      )
      .unwrap();

      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));
      decls.insert("lib_b".to_string(), InputDecl::Url(path_to_lua_url(&lib_b)));

      // Should succeed - same utils version from both paths
      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // Should have: lib_a, lib_b, utils (deduplicated)
      let namespace_names: Vec<_> = result.namespaces.iter().map(|ns| ns.name.as_str()).collect();
      assert!(namespace_names.contains(&"lib_a"));
      assert!(namespace_names.contains(&"lib_b"));
      assert!(namespace_names.contains(&"utils"));
      // utils should only appear once (deduplicated)
      assert_eq!(namespace_names.iter().filter(|&&n| n == "utils").count(), 1);
    }

    #[test]
    fn namespace_conflict_different_sources() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create two different libs that both provide "utils" namespace
      let lib_a = config_dir.join("lib_a");
      create_input_with_namespace(&lib_a, "utils"); // lib_a provides lua/utils/

      let lib_b = config_dir.join("lib_b");
      create_input_with_namespace(&lib_b, "utils"); // lib_b also provides lua/utils/

      let mut decls = InputDecls::new();
      decls.insert("lib_a".to_string(), InputDecl::Url(path_to_lua_url(&lib_a)));
      decls.insert("lib_b".to_string(), InputDecl::Url(path_to_lua_url(&lib_b)));

      // Should fail with namespace conflict
      let result = resolve_inputs(&decls, config_dir, None);
      assert!(result.is_err());

      let err = result.unwrap_err();
      match err {
        ResolveError::NamespaceConflict(ref conflict) => {
          assert_eq!(conflict.namespace, "utils");
        }
        _ => panic!("expected NamespaceConflict error, got: {:?}", err),
      }
    }

    #[test]
    fn config_namespace_conflicts_with_input() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a lua/my_lib/ in config
      let config_lua = config_dir.join("lua").join("my_lib");
      fs::create_dir_all(&config_lua).unwrap();
      fs::write(config_lua.join("init.lua"), "return {}").unwrap();

      // Create an input that also provides lua/my_lib/
      let lib = config_dir.join("lib");
      create_input_with_namespace(&lib, "my_lib");

      let mut decls = InputDecls::new();
      decls.insert("lib".to_string(), InputDecl::Url(path_to_lua_url(&lib)));

      // Should fail - config's my_lib conflicts with input's my_lib
      let result = resolve_inputs(&decls, config_dir, None);
      assert!(result.is_err());

      let err = result.unwrap_err();
      match err {
        ResolveError::NamespaceConflict(ref conflict) => {
          assert_eq!(conflict.namespace, "my_lib");
          assert_eq!(conflict.provider1, "<config>");
        }
        _ => panic!("expected NamespaceConflict error, got: {:?}", err),
      }
    }

    #[test]
    fn input_with_multiple_namespaces() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create an input with multiple namespaces
      let lib = config_dir.join("lib");
      fs::create_dir_all(&lib).unwrap();
      fs::create_dir_all(lib.join("lua/ns_one")).unwrap();
      fs::create_dir_all(lib.join("lua/ns_two")).unwrap();
      fs::write(lib.join("lua/ns_one/init.lua"), "return {}").unwrap();
      fs::write(lib.join("lua/ns_two/init.lua"), "return {}").unwrap();
      fs::write(
        lib.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      let mut decls = InputDecls::new();
      decls.insert("lib".to_string(), InputDecl::Url(path_to_lua_url(&lib)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // Both namespaces should be discovered
      let namespace_names: Vec<_> = result.namespaces.iter().map(|ns| ns.name.as_str()).collect();
      assert!(namespace_names.contains(&"ns_one"));
      assert!(namespace_names.contains(&"ns_two"));
    }
  }

  mod per_input_lock_tests {
    use super::*;
    use std::fs;

    use crate::util::testutil::path_to_lua_url;

    #[test]
    fn inject_revision_into_url_git() {
      // Test URL without existing revision
      let url = "git:https://github.com/org/repo.git";
      let result = inject_revision_into_url(url, "abc123");
      assert_eq!(result, "git:https://github.com/org/repo.git#abc123");

      // Test URL with existing revision (should be replaced)
      let url_with_rev = "git:https://github.com/org/repo.git#main";
      let result = inject_revision_into_url(url_with_rev, "abc123");
      assert_eq!(result, "git:https://github.com/org/repo.git#abc123");
    }

    #[test]
    fn inject_revision_into_url_path() {
      // Path URLs should not be modified
      let url = "path:./local/dir";
      let result = inject_revision_into_url(url, "abc123");
      assert_eq!(result, "path:./local/dir");
    }

    #[test]
    fn apply_input_lock_to_decl_not_locked() {
      let lock = LockFile::new();
      let decl = InputDecl::Url("git:https://github.com/org/utils.git".to_string());

      let result = apply_input_lock_to_decl(decl.clone(), "utils", &lock);
      assert_eq!(result, decl); // Unchanged
    }

    #[test]
    fn apply_input_lock_to_decl_locked() {
      use crate::inputs::lock::LockedInput;

      let mut lock = LockFile::new();
      lock.insert(
        "utils".to_string(),
        LockedInput::new("git", "git:https://github.com/org/utils.git", "locked123"),
      );

      let decl = InputDecl::Url("git:https://github.com/org/utils.git".to_string());
      let result = apply_input_lock_to_decl(decl, "utils", &lock);

      match result {
        InputDecl::Url(url) => {
          assert!(url.ends_with("#locked123"), "expected locked revision, got: {}", url);
        }
        _ => panic!("expected Url variant"),
      }
    }

    #[test]
    fn apply_input_lock_to_decl_preserves_overrides() {
      use crate::inputs::lock::LockedInput;

      let mut lock = LockFile::new();
      lock.insert(
        "utils".to_string(),
        LockedInput::new("git", "git:https://github.com/org/utils.git", "locked123"),
      );

      let mut overrides = BTreeMap::new();
      overrides.insert("nested".to_string(), InputOverride::Follows("other".to_string()));

      let decl = InputDecl::Extended {
        url: Some("git:https://github.com/org/utils.git".to_string()),
        inputs: overrides.clone(),
      };

      let result = apply_input_lock_to_decl(decl, "utils", &lock);

      match result {
        InputDecl::Extended { url, inputs } => {
          assert!(
            url.as_ref().unwrap().ends_with("#locked123"),
            "expected locked revision, got: {:?}",
            url
          );
          assert_eq!(inputs, overrides); // Overrides preserved
        }
        _ => panic!("expected Extended variant"),
      }
    }

    #[test]
    fn transitive_dep_uses_input_lock_file() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create utils v1 (will be declared by lib but overridden by lock)
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

      // Create utils v2 (what the lock file pins to)
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

      // Create lib that depends on utils (pointing to v1 in init.lua)
      let lib = config_dir.join("lib");
      fs::create_dir_all(&lib).unwrap();
      fs::write(
        lib.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils_v1)
        ),
      )
      .unwrap();

      // Create lib's lock file that pins utils to v2
      let lib_lock_content = format!(
        r#"{{
  "version": 1,
  "root": "root",
  "nodes": {{
    "root": {{
      "inputs": {{
        "utils": "utils-locked"
      }}
    }},
    "utils-locked": {{
      "type": "path",
      "url": "{}",
      "rev": "local",
      "inputs": {{}}
    }}
  }}
}}"#,
        path_to_lua_url(&utils_v2)
      );
      fs::write(lib.join("syslua.lock"), lib_lock_content).unwrap();

      // Resolve lib from the config
      let mut decls = InputDecls::new();
      decls.insert("lib".to_string(), InputDecl::Url(path_to_lua_url(&lib)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib should be resolved
      assert!(result.inputs.contains_key("lib"));
      let lib_resolved = result.inputs.get("lib").unwrap();

      // lib's utils should be resolved (from the lock file)
      assert!(lib_resolved.inputs.contains_key("utils"));
      let utils_resolved = lib_resolved.inputs.get("utils").unwrap();

      // The utils path should be v2 (from lock file), not v1 (from init.lua)
      let utils_v2_canonical = utils_v2.canonicalize().unwrap();
      assert_eq!(
        utils_resolved.path, utils_v2_canonical,
        "input lock file should pin utils to v2"
      );
    }

    #[test]
    fn follows_overrides_input_lock() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create three versions of utils
      let utils_v1 = config_dir.join("utils_v1");
      fs::create_dir_all(&utils_v1).unwrap();
      fs::write(
        utils_v1.join("init.lua"),
        r#"return { inputs = {}, setup = function() end }"#,
      )
      .unwrap();

      let utils_v2 = config_dir.join("utils_v2");
      fs::create_dir_all(&utils_v2).unwrap();
      fs::write(
        utils_v2.join("init.lua"),
        r#"return { inputs = {}, setup = function() end }"#,
      )
      .unwrap();

      let utils_v3 = config_dir.join("utils_v3");
      fs::create_dir_all(&utils_v3).unwrap();
      fs::write(
        utils_v3.join("init.lua"),
        r#"return { inputs = {}, setup = function() end }"#,
      )
      .unwrap();

      // Create lib that depends on utils (pointing to v1 in init.lua)
      let lib = config_dir.join("lib");
      fs::create_dir_all(&lib).unwrap();
      fs::write(
        lib.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils_v1)
        ),
      )
      .unwrap();

      // Create lib's lock file that pins utils to v2
      let lib_lock_content = format!(
        r#"{{
  "version": 1,
  "root": "root",
  "nodes": {{
    "root": {{
      "inputs": {{
        "utils": "utils-locked"
      }}
    }},
    "utils-locked": {{
      "type": "path",
      "url": "{}",
      "rev": "local",
      "inputs": {{}}
    }}
  }}
}}"#,
        path_to_lua_url(&utils_v2)
      );
      fs::write(lib.join("syslua.lock"), lib_lock_content).unwrap();

      // Resolve lib from the config with a follows override to v3
      let mut decls = InputDecls::new();

      // Declare lib with follows override for utils -> my_utils
      let mut overrides = BTreeMap::new();
      overrides.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));
      decls.insert(
        "lib".to_string(),
        InputDecl::Extended {
          url: Some(path_to_lua_url(&lib)),
          inputs: overrides,
        },
      );

      // Declare my_utils pointing to v3
      decls.insert("my_utils".to_string(), InputDecl::Url(path_to_lua_url(&utils_v3)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib should be resolved
      assert!(result.inputs.contains_key("lib"));
      let lib_resolved = result.inputs.get("lib").unwrap();

      // lib's utils should follow my_utils (v3), not the locked v2
      assert!(lib_resolved.inputs.contains_key("utils"));
      let utils_resolved = lib_resolved.inputs.get("utils").unwrap();

      let utils_v3_canonical = utils_v3.canonicalize().unwrap();
      assert_eq!(
        utils_resolved.path, utils_v3_canonical,
        "follows should override input lock file - expected v3"
      );
    }

    #[test]
    fn missing_input_lock_uses_floating() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create utils
      let utils = config_dir.join("utils");
      fs::create_dir_all(&utils).unwrap();
      fs::write(
        utils.join("init.lua"),
        r#"return { inputs = {}, setup = function() end }"#,
      )
      .unwrap();

      // Create lib that depends on utils (no lock file)
      let lib = config_dir.join("lib");
      fs::create_dir_all(&lib).unwrap();
      fs::write(
        lib.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    utils = "{}",
  }},
  setup = function() end,
}}
"#,
          path_to_lua_url(&utils)
        ),
      )
      .unwrap();
      // Note: No syslua.lock file in lib

      // Resolve lib from the config
      let mut decls = InputDecls::new();
      decls.insert("lib".to_string(), InputDecl::Url(path_to_lua_url(&lib)));

      let result = resolve_inputs(&decls, config_dir, None).unwrap();

      // lib should be resolved
      assert!(result.inputs.contains_key("lib"));
      let lib_resolved = result.inputs.get("lib").unwrap();

      // utils should be resolved (using floating declaration from init.lua)
      assert!(lib_resolved.inputs.contains_key("utils"));
      let utils_resolved = lib_resolved.inputs.get("utils").unwrap();

      let utils_canonical = utils.canonicalize().unwrap();
      assert_eq!(
        utils_resolved.path, utils_canonical,
        "without lock file, should use floating declaration"
      );
    }
  }
}
