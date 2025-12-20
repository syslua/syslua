//! Dependency graph building and resolution.
//!
//! This module handles:
//! - Building a dependency graph from input declarations
//! - Resolving `follows` declarations (with chain support)
//! - Topological sorting for resolution order
//! - Cycle detection and handling
//!
//! # Algorithm Overview
//!
//! 1. Parse root inputs into [`InputDecl`] values
//! 2. For each input with an init.lua, extract its declared inputs
//! 3. Build a directed graph of dependencies
//! 4. Apply `follows` overrides, resolving chains up to [`MAX_FOLLOWS_DEPTH`]
//! 5. Topologically sort nodes (handling cycles)
//! 6. Resolve each node in order, deduplicating identical URL+rev combinations

use std::collections::{BTreeMap, HashSet};

use thiserror::Error;
use tracing::{debug, trace, warn};

use super::types::{InputDecl, InputDecls, InputOverride, MAX_FOLLOWS_DEPTH};

/// A node in the dependency graph.
#[derive(Debug, Clone)]
pub struct GraphNode {
  /// The input name (as declared in the parent's inputs table).
  pub name: String,

  /// The input declaration.
  pub decl: InputDecl,

  /// Parent node path (e.g., "root" or "pkgs" or "pkgs/utils").
  /// Empty string for root-level inputs.
  pub parent_path: String,

  /// The full path to this node (e.g., "pkgs" or "pkgs/utils").
  pub full_path: String,

  /// Declared dependencies of this input (extracted from its init.lua).
  /// Only populated after the input is fetched and its init.lua is parsed.
  pub declared_inputs: Option<InputDecls>,

  /// Whether this node's dependencies have been extracted.
  pub deps_extracted: bool,
}

impl GraphNode {
  /// Create a new root-level graph node.
  pub fn root_input(name: String, decl: InputDecl) -> Self {
    Self {
      full_path: name.clone(),
      name,
      decl,
      parent_path: String::new(),
      declared_inputs: None,
      deps_extracted: false,
    }
  }

  /// Create a new transitive dependency node.
  pub fn transitive(name: String, decl: InputDecl, parent_path: &str) -> Self {
    let full_path = if parent_path.is_empty() {
      name.clone()
    } else {
      format!("{}/{}", parent_path, name)
    };

    Self {
      name,
      decl,
      parent_path: parent_path.to_string(),
      full_path,
      declared_inputs: None,
      deps_extracted: false,
    }
  }

  /// Check if this is a root-level input (declared directly in config).
  pub fn is_root_level(&self) -> bool {
    self.parent_path.is_empty()
  }
}

/// The dependency graph structure.
#[derive(Debug, Default)]
pub struct DependencyGraph {
  /// All nodes in the graph, keyed by their full path.
  pub nodes: BTreeMap<String, GraphNode>,

  /// Edges: from_path -> set of to_paths (dependencies).
  pub edges: BTreeMap<String, HashSet<String>>,

  /// Reverse edges: to_path -> set of from_paths (dependents).
  pub reverse_edges: BTreeMap<String, HashSet<String>>,

  /// Follows mappings after resolution: source_path -> target_path.
  pub follows_resolved: BTreeMap<String, String>,
}

/// Errors that can occur during graph operations.
#[derive(Debug, Error)]
pub enum GraphError {
  /// A follows target does not exist.
  #[error("follows target '{target}' not found (referenced from '{from}')")]
  FollowsTargetNotFound { from: String, target: String },

  /// Circular follows detected.
  #[error("circular follows detected: {chain}")]
  CircularFollows { chain: String },

  /// Follows chain exceeds maximum depth.
  #[error("follows chain too deep (maximum {max} hops): {chain}")]
  FollowsChainTooDeep { max: usize, chain: String },

  /// Invalid follows path format.
  #[error("invalid follows path '{path}': {reason}")]
  InvalidFollowsPath { path: String, reason: String },
}

impl DependencyGraph {
  /// Create a new empty dependency graph.
  pub fn new() -> Self {
    Self::default()
  }

  /// Add a root-level input to the graph.
  pub fn add_root_input(&mut self, name: &str, decl: InputDecl) {
    let node = GraphNode::root_input(name.to_string(), decl);
    self.nodes.insert(node.full_path.clone(), node);
  }

  /// Add a transitive dependency to the graph.
  pub fn add_transitive(&mut self, name: &str, decl: InputDecl, parent_path: &str) -> String {
    let node = GraphNode::transitive(name.to_string(), decl, parent_path);
    let full_path = node.full_path.clone();

    // Add edge from parent to this node
    self
      .edges
      .entry(parent_path.to_string())
      .or_default()
      .insert(full_path.clone());

    self
      .reverse_edges
      .entry(full_path.clone())
      .or_default()
      .insert(parent_path.to_string());

    self.nodes.insert(full_path.clone(), node);
    full_path
  }

  /// Get a node by its full path.
  pub fn get(&self, path: &str) -> Option<&GraphNode> {
    self.nodes.get(path)
  }

  /// Get a mutable node by its full path.
  pub fn get_mut(&mut self, path: &str) -> Option<&mut GraphNode> {
    self.nodes.get_mut(path)
  }

  /// Get all root-level input names.
  pub fn root_inputs(&self) -> Vec<&str> {
    self
      .nodes
      .values()
      .filter(|n| n.is_root_level())
      .map(|n| n.name.as_str())
      .collect()
  }

  /// Get the dependencies of a node.
  pub fn dependencies(&self, path: &str) -> Vec<&str> {
    self
      .edges
      .get(path)
      .map(|deps| deps.iter().map(|s| s.as_str()).collect())
      .unwrap_or_default()
  }

  /// Get the dependents of a node (nodes that depend on it).
  pub fn dependents(&self, path: &str) -> Vec<&str> {
    self
      .reverse_edges
      .get(path)
      .map(|deps| deps.iter().map(|s| s.as_str()).collect())
      .unwrap_or_default()
  }

  /// Resolve all follows declarations in the graph.
  ///
  /// This processes all nodes with follows overrides and resolves them
  /// to their final targets, handling chains up to [`MAX_FOLLOWS_DEPTH`].
  pub fn resolve_follows(&mut self) -> Result<(), GraphError> {
    // Collect all follows declarations that need resolution
    let mut follows_to_resolve: Vec<(String, String)> = Vec::new();

    for node in self.nodes.values() {
      if let Some(overrides) = node.decl.overrides() {
        for (dep_name, override_) in overrides {
          if let InputOverride::Follows(target) = override_ {
            let source_path = if node.full_path.is_empty() {
              dep_name.clone()
            } else {
              format!("{}/{}", node.full_path, dep_name)
            };
            follows_to_resolve.push((source_path, target.clone()));
          }
        }
      }
    }

    // Resolve each follows declaration
    for (source_path, target) in follows_to_resolve {
      let resolved = self.resolve_follows_chain(&source_path, &target)?;
      debug!(source = %source_path, target = %resolved, "resolved follows");
      self.follows_resolved.insert(source_path, resolved);
    }

    Ok(())
  }

  /// Resolve a follows chain to its final target.
  fn resolve_follows_chain(&self, source: &str, initial_target: &str) -> Result<String, GraphError> {
    let mut visited = HashSet::new();
    let mut chain = vec![source.to_string()];
    let mut current_target = initial_target.to_string();

    for depth in 0..MAX_FOLLOWS_DEPTH {
      // Check for cycles
      if visited.contains(&current_target) {
        chain.push(current_target.clone());
        return Err(GraphError::CircularFollows {
          chain: chain.join(" -> "),
        });
      }

      visited.insert(current_target.clone());
      chain.push(current_target.clone());

      // Normalize the target path
      let normalized = self.normalize_follows_path(&current_target)?;

      // Check if the target itself has a follows
      if let Some(next_target) = self.get_follows_target(&normalized) {
        trace!(
          depth,
          current = %current_target,
          next = %next_target,
          "following chain"
        );
        current_target = next_target;
      } else {
        // Target doesn't follow anything else - verify it exists or will exist
        return Ok(normalized);
      }
    }

    Err(GraphError::FollowsChainTooDeep {
      max: MAX_FOLLOWS_DEPTH,
      chain: chain.join(" -> "),
    })
  }

  /// Normalize a follows path to a full node path.
  ///
  /// Handles:
  /// - Direct input names: "utils" -> "utils"
  /// - Nested paths: "pkgs/utils" -> "pkgs/utils"
  fn normalize_follows_path(&self, path: &str) -> Result<String, GraphError> {
    // Path is already in the correct format
    // Just validate it's not empty
    if path.is_empty() {
      return Err(GraphError::InvalidFollowsPath {
        path: path.to_string(),
        reason: "path cannot be empty".to_string(),
      });
    }

    Ok(path.to_string())
  }

  /// Get the follows target for a path, if it has one.
  fn get_follows_target(&self, path: &str) -> Option<String> {
    // First check if this path has already been resolved
    if let Some(resolved) = self.follows_resolved.get(path) {
      return Some(resolved.clone());
    }

    // Check if this is a pending follows (from node overrides)
    let parts: Vec<&str> = path.rsplitn(2, '/').collect();
    if parts.len() == 2 {
      let (dep_name, parent_path) = (parts[0], parts[1]);
      if let Some(node) = self.nodes.get(parent_path)
        && let Some(overrides) = node.decl.overrides()
        && let Some(InputOverride::Follows(target)) = overrides.get(dep_name)
      {
        return Some(target.clone());
      }
    } else if let Some(node) = self.nodes.get(path) {
      // Root-level input - check if it's a follows-only declaration
      if let InputDecl::Extended { url: None, inputs } = &node.decl
        && inputs.is_empty()
      {
        // This shouldn't happen, but handle gracefully
        return None;
      }
    }

    None
  }

  /// Perform a topological sort of the graph.
  ///
  /// Returns nodes in an order where dependencies come before dependents.
  /// Handles cycles by including all nodes in cycles together.
  pub fn topological_sort(&self) -> Vec<String> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    // Process all nodes
    for path in self.nodes.keys() {
      if !visited.contains(path) {
        self.topo_visit(path, &mut visited, &mut in_stack, &mut result);
      }
    }

    result
  }

  /// DFS visit for topological sort.
  fn topo_visit(
    &self,
    path: &str,
    visited: &mut HashSet<String>,
    in_stack: &mut HashSet<String>,
    result: &mut Vec<String>,
  ) {
    if visited.contains(path) {
      return;
    }

    // Detect cycle (node already in current DFS stack)
    if in_stack.contains(path) {
      // Cycle detected - we still need to include this node
      // It will be added when we finish processing the cycle
      warn!(path, "cycle detected in dependency graph");
      return;
    }

    in_stack.insert(path.to_string());

    // Visit dependencies first
    if let Some(deps) = self.edges.get(path) {
      for dep in deps {
        self.topo_visit(dep, visited, in_stack, result);
      }
    }

    in_stack.remove(path);
    visited.insert(path.to_string());
    result.push(path.to_string());
  }

  /// Find all nodes that are part of cycles.
  pub fn find_cycles(&self) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let mut on_stack = HashSet::new();

    for path in self.nodes.keys() {
      if !visited.contains(path) {
        self.find_cycles_dfs(path, &mut visited, &mut stack, &mut on_stack, &mut cycles);
      }
    }

    cycles
  }

  /// DFS helper for cycle detection.
  fn find_cycles_dfs(
    &self,
    path: &str,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    on_stack: &mut HashSet<String>,
    cycles: &mut Vec<Vec<String>>,
  ) {
    visited.insert(path.to_string());
    stack.push(path.to_string());
    on_stack.insert(path.to_string());

    if let Some(deps) = self.edges.get(path) {
      for dep in deps {
        if !visited.contains(dep) {
          self.find_cycles_dfs(dep, visited, stack, on_stack, cycles);
        } else if on_stack.contains(dep) {
          // Found a cycle - extract it from the stack
          let cycle_start = stack.iter().position(|p| p == dep).unwrap();
          let cycle: Vec<String> = stack[cycle_start..].to_vec();
          cycles.push(cycle);
        }
      }
    }

    stack.pop();
    on_stack.remove(path);
  }
}

/// Build a dependency graph from root input declarations.
///
/// This creates the initial graph structure from the root config's inputs.
/// Transitive dependencies are added later as inputs are fetched and parsed.
pub fn build_initial_graph(root_inputs: &InputDecls) -> DependencyGraph {
  let mut graph = DependencyGraph::new();

  for (name, decl) in root_inputs {
    graph.add_root_input(name, decl.clone());
  }

  graph
}

#[cfg(test)]
mod tests {
  use super::*;

  mod graph_node {
    use super::*;

    #[test]
    fn root_input_node() {
      let node = GraphNode::root_input(
        "pkgs".to_string(),
        InputDecl::Url("git:https://example.com".to_string()),
      );

      assert!(node.is_root_level());
      assert_eq!(node.name, "pkgs");
      assert_eq!(node.full_path, "pkgs");
      assert!(node.parent_path.is_empty());
    }

    #[test]
    fn transitive_node() {
      let node = GraphNode::transitive(
        "utils".to_string(),
        InputDecl::Url("git:https://example.com/utils".to_string()),
        "pkgs",
      );

      assert!(!node.is_root_level());
      assert_eq!(node.name, "utils");
      assert_eq!(node.full_path, "pkgs/utils");
      assert_eq!(node.parent_path, "pkgs");
    }

    #[test]
    fn deeply_nested_node() {
      let node = GraphNode::transitive(
        "helpers".to_string(),
        InputDecl::Url("git:https://example.com/helpers".to_string()),
        "pkgs/utils",
      );

      assert_eq!(node.full_path, "pkgs/utils/helpers");
      assert_eq!(node.parent_path, "pkgs/utils");
    }
  }

  mod dependency_graph {
    use super::*;

    #[test]
    fn add_root_inputs() {
      let mut graph = DependencyGraph::new();
      graph.add_root_input("pkgs", InputDecl::Url("git:https://example.com/pkgs".to_string()));
      graph.add_root_input("utils", InputDecl::Url("git:https://example.com/utils".to_string()));

      assert_eq!(graph.nodes.len(), 2);
      assert!(graph.get("pkgs").is_some());
      assert!(graph.get("utils").is_some());

      let roots = graph.root_inputs();
      assert_eq!(roots.len(), 2);
    }

    #[test]
    fn add_transitive_dependency() {
      let mut graph = DependencyGraph::new();
      graph.add_root_input("pkgs", InputDecl::Url("git:https://example.com/pkgs".to_string()));
      graph.add_transitive(
        "utils",
        InputDecl::Url("git:https://example.com/utils".to_string()),
        "pkgs",
      );

      assert_eq!(graph.nodes.len(), 2);
      assert!(graph.get("pkgs/utils").is_some());

      let deps = graph.dependencies("pkgs");
      assert_eq!(deps.len(), 1);
      assert_eq!(deps[0], "pkgs/utils");

      let dependents = graph.dependents("pkgs/utils");
      assert_eq!(dependents.len(), 1);
      assert_eq!(dependents[0], "pkgs");
    }

    #[test]
    fn topological_sort_simple() {
      let mut graph = DependencyGraph::new();
      graph.add_root_input("pkgs", InputDecl::Url("git:a".to_string()));
      graph.add_transitive("utils", InputDecl::Url("git:b".to_string()), "pkgs");

      let order = graph.topological_sort();

      // utils should come before pkgs (dependency before dependent)
      let utils_idx = order.iter().position(|p| p == "pkgs/utils").unwrap();
      let pkgs_idx = order.iter().position(|p| p == "pkgs").unwrap();
      assert!(utils_idx < pkgs_idx);
    }

    #[test]
    fn topological_sort_diamond() {
      // Diamond: A depends on B and C, B and C both depend on D
      let mut graph = DependencyGraph::new();
      graph.add_root_input("a", InputDecl::Url("git:a".to_string()));
      graph.add_transitive("b", InputDecl::Url("git:b".to_string()), "a");
      graph.add_transitive("c", InputDecl::Url("git:c".to_string()), "a");
      graph.add_transitive("d", InputDecl::Url("git:d".to_string()), "a/b");

      // Also add edge from c to d (shared dependency)
      graph
        .edges
        .entry("a/c".to_string())
        .or_default()
        .insert("a/b/d".to_string());
      graph
        .reverse_edges
        .entry("a/b/d".to_string())
        .or_default()
        .insert("a/c".to_string());

      let order = graph.topological_sort();

      // D should come before both B and C
      let d_idx = order.iter().position(|p| p == "a/b/d").unwrap();
      let b_idx = order.iter().position(|p| p == "a/b").unwrap();
      let c_idx = order.iter().position(|p| p == "a/c").unwrap();
      let a_idx = order.iter().position(|p| p == "a").unwrap();

      assert!(d_idx < b_idx);
      assert!(d_idx < c_idx);
      assert!(b_idx < a_idx);
      assert!(c_idx < a_idx);
    }

    #[test]
    fn find_cycles_none() {
      let mut graph = DependencyGraph::new();
      graph.add_root_input("a", InputDecl::Url("git:a".to_string()));
      graph.add_transitive("b", InputDecl::Url("git:b".to_string()), "a");

      let cycles = graph.find_cycles();
      assert!(cycles.is_empty());
    }

    #[test]
    fn find_cycles_simple() {
      let mut graph = DependencyGraph::new();
      graph.add_root_input("a", InputDecl::Url("git:a".to_string()));
      graph.add_root_input("b", InputDecl::Url("git:b".to_string()));

      // Create cycle: a -> b -> a
      graph.edges.entry("a".to_string()).or_default().insert("b".to_string());
      graph.edges.entry("b".to_string()).or_default().insert("a".to_string());
      graph
        .reverse_edges
        .entry("b".to_string())
        .or_default()
        .insert("a".to_string());
      graph
        .reverse_edges
        .entry("a".to_string())
        .or_default()
        .insert("b".to_string());

      let cycles = graph.find_cycles();
      assert!(!cycles.is_empty());
    }
  }

  mod build_initial_graph {
    use super::*;

    #[test]
    fn builds_from_declarations() {
      let mut decls = InputDecls::new();
      decls.insert(
        "pkgs".to_string(),
        InputDecl::Url("git:https://example.com/pkgs".to_string()),
      );
      decls.insert(
        "utils".to_string(),
        InputDecl::Url("git:https://example.com/utils".to_string()),
      );

      let graph = build_initial_graph(&decls);

      assert_eq!(graph.nodes.len(), 2);
      assert!(graph.get("pkgs").is_some());
      assert!(graph.get("utils").is_some());
    }
  }

  mod follows_resolution {
    use super::*;

    #[test]
    fn simple_follows() {
      let mut decls = InputDecls::new();
      decls.insert(
        "my_utils".to_string(),
        InputDecl::Url("git:https://example.com/utils".to_string()),
      );

      let mut overrides = BTreeMap::new();
      overrides.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));

      decls.insert(
        "pkgs".to_string(),
        InputDecl::Extended {
          url: Some("git:https://example.com/pkgs".to_string()),
          inputs: overrides,
        },
      );

      let mut graph = build_initial_graph(&decls);
      graph.resolve_follows().unwrap();

      assert!(graph.follows_resolved.contains_key("pkgs/utils"));
      assert_eq!(graph.follows_resolved.get("pkgs/utils").unwrap(), "my_utils");
    }

    #[test]
    fn follows_chain_resolves() {
      // Test: A/utils follows B/utils, B/utils follows my_utils
      let mut decls = InputDecls::new();

      // Declare my_utils as the final target
      decls.insert(
        "my_utils".to_string(),
        InputDecl::Url("git:https://example.com/utils".to_string()),
      );

      // Declare input_b with utils following my_utils
      let mut b_overrides = BTreeMap::new();
      b_overrides.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));
      decls.insert(
        "input_b".to_string(),
        InputDecl::Extended {
          url: Some("git:https://example.com/b".to_string()),
          inputs: b_overrides,
        },
      );

      // Declare input_a with utils following input_b/utils
      let mut a_overrides = BTreeMap::new();
      a_overrides.insert("utils".to_string(), InputOverride::Follows("input_b/utils".to_string()));
      decls.insert(
        "input_a".to_string(),
        InputDecl::Extended {
          url: Some("git:https://example.com/a".to_string()),
          inputs: a_overrides,
        },
      );

      let mut graph = build_initial_graph(&decls);

      // Manually add the transitive nodes that would be created during resolution
      graph.add_transitive("utils", InputDecl::Url("git:placeholder".to_string()), "input_a");
      graph.add_transitive("utils", InputDecl::Url("git:placeholder".to_string()), "input_b");

      graph.resolve_follows().unwrap();

      // input_a/utils should ultimately resolve to my_utils (through the chain)
      assert!(graph.follows_resolved.contains_key("input_a/utils"));
      // The chain should be resolved: input_a/utils -> input_b/utils -> my_utils
      assert_eq!(graph.follows_resolved.get("input_a/utils").unwrap(), "my_utils");
    }

    #[test]
    fn circular_follows_returns_error() {
      let mut decls = InputDecls::new();

      // Create circular follows: a/utils follows b/utils, b/utils follows a/utils
      let mut a_overrides = BTreeMap::new();
      a_overrides.insert("utils".to_string(), InputOverride::Follows("input_b/utils".to_string()));
      decls.insert(
        "input_a".to_string(),
        InputDecl::Extended {
          url: Some("git:https://example.com/a".to_string()),
          inputs: a_overrides,
        },
      );

      let mut b_overrides = BTreeMap::new();
      b_overrides.insert("utils".to_string(), InputOverride::Follows("input_a/utils".to_string()));
      decls.insert(
        "input_b".to_string(),
        InputDecl::Extended {
          url: Some("git:https://example.com/b".to_string()),
          inputs: b_overrides,
        },
      );

      let mut graph = build_initial_graph(&decls);

      // Add the transitive nodes
      graph.add_transitive("utils", InputDecl::Url("git:placeholder".to_string()), "input_a");
      graph.add_transitive("utils", InputDecl::Url("git:placeholder".to_string()), "input_b");

      let result = graph.resolve_follows();

      assert!(result.is_err());
      assert!(matches!(result.unwrap_err(), GraphError::CircularFollows { .. }));
    }

    // Note: FollowsTargetNotFound is validated during full transitive resolution,
    // not during graph.resolve_follows(). The graph allows unresolved targets
    // because they may be discovered during transitive dependency resolution.
  }
}
