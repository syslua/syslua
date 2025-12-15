//! Execution DAG for build and bind dependency management.
//!
//! This module provides a directed acyclic graph (DAG) for managing build and bind
//! dependencies and computing parallel execution waves.

use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::bind::{BindDef, BindHash};
use crate::build::BuildHash;
use crate::inputs::InputsRef;
use crate::manifest::Manifest;

use super::types::ExecuteError;

/// A node in the execution DAG.
///
/// Represents either a build or bind that needs to be executed.
/// Used for unified wave computation where builds and binds are
/// interleaved based on their dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DagNode {
  /// A build to be realized.
  Build(BuildHash),
  /// A bind to be applied.
  Bind(BindHash),
}

/// A DAG representing build and bind dependencies for execution planning.
///
/// The DAG is constructed from a manifest and provides:
/// - Topological ordering of builds and binds
/// - Parallel execution waves (groups of independent nodes)
/// - Dependency queries for both builds and binds
pub struct ExecutionDag {
  /// The underlying graph.
  graph: DiGraph<DagNode, ()>,

  /// Map from build hash to node index.
  build_nodes: HashMap<BuildHash, NodeIndex>,

  /// Map from bind hash to node index.
  bind_nodes: HashMap<BindHash, NodeIndex>,
}

impl ExecutionDag {
  /// Build an execution DAG from a manifest.
  ///
  /// This extracts all builds and their dependencies, creating edges
  /// from dependencies to dependents.
  ///
  /// # Errors
  ///
  /// Returns `InvalidManifest` if any build has bind references in its inputs.
  /// Builds cannot depend on binds (binds are side-effectful and cannot be
  /// inputs to immutable builds).
  pub fn from_manifest(manifest: &Manifest) -> Result<Self, ExecuteError> {
    let mut graph = DiGraph::new();
    let mut build_nodes = HashMap::new();
    let mut bind_nodes = HashMap::new();

    // Validate: builds cannot have bind references in inputs
    for (hash, build_def) in &manifest.builds {
      if let Some(ref inputs) = build_def.inputs
        && Self::inputs_contain_bind_ref(inputs)
      {
        return Err(ExecuteError::InvalidManifest(format!(
          "build {} has bind references in inputs: builds cannot depend on binds",
          hash.0
        )));
      }
    }

    // First pass: create nodes for all builds
    for hash in manifest.builds.keys() {
      let idx = graph.add_node(DagNode::Build(hash.clone()));
      build_nodes.insert(hash.clone(), idx);
    }

    // Create nodes for all binds (they can be dependencies)
    for hash in manifest.bindings.keys() {
      let idx = graph.add_node(DagNode::Bind(hash.clone()));
      bind_nodes.insert(hash.clone(), idx);
    }

    // Second pass: add edges for build dependencies (builds can only depend on builds)
    for (hash, build_def) in &manifest.builds {
      let dependent_idx = build_nodes[hash];

      if let Some(inputs) = &build_def.inputs {
        let deps = extract_dependencies(inputs);
        for dep in deps {
          if let DagNode::Build(dep_hash) = dep
            && let Some(&dep_idx) = build_nodes.get(&dep_hash)
          {
            // Edge from dependency to dependent
            graph.add_edge(dep_idx, dependent_idx, ());
          }
          // Note: Bind deps are not allowed for builds (validated above)
          // If dependency not found, it might be external - ignore for now
        }
      }
    }

    // Also process bind dependencies (binds can depend on builds and other binds)
    for (hash, bind_def) in &manifest.bindings {
      let dependent_idx = bind_nodes[hash];

      if let Some(inputs) = &bind_def.inputs {
        let deps = extract_dependencies(inputs);
        for dep in deps {
          match dep {
            DagNode::Build(dep_hash) => {
              if let Some(&dep_idx) = build_nodes.get(&dep_hash) {
                graph.add_edge(dep_idx, dependent_idx, ());
              }
            }
            DagNode::Bind(dep_hash) => {
              if let Some(&dep_idx) = bind_nodes.get(&dep_hash) {
                graph.add_edge(dep_idx, dependent_idx, ());
              }
            }
          }
        }
      }
    }

    let dag = Self {
      graph,
      build_nodes,
      bind_nodes,
    };

    // Verify no cycles
    dag.verify_acyclic()?;

    Ok(dag)
  }

  /// Verify that the graph is acyclic.
  fn verify_acyclic(&self) -> Result<(), ExecuteError> {
    toposort(&self.graph, None).map_err(|_| ExecuteError::CycleDetected)?;
    Ok(())
  }

  /// Check if inputs contain any bind references (recursive).
  ///
  /// Used for validating that builds don't depend on binds.
  fn inputs_contain_bind_ref(inputs: &InputsRef) -> bool {
    match inputs {
      InputsRef::Bind(_) => true,
      InputsRef::Table(map) => map.values().any(Self::inputs_contain_bind_ref),
      InputsRef::Array(arr) => arr.iter().any(Self::inputs_contain_bind_ref),
      InputsRef::String(_) | InputsRef::Number(_) | InputsRef::Boolean(_) | InputsRef::Build(_) => false,
    }
  }

  /// Get builds in topological order.
  ///
  /// Returns build hashes in an order where dependencies come before dependents.
  pub fn topological_builds(&self) -> Result<Vec<BuildHash>, ExecuteError> {
    let sorted = toposort(&self.graph, None).map_err(|_| ExecuteError::CycleDetected)?;

    Ok(
      sorted
        .into_iter()
        .filter_map(|idx| {
          if let DagNode::Build(hash) = &self.graph[idx] {
            Some(hash.clone())
          } else {
            None
          }
        })
        .collect(),
    )
  }

  /// Get builds organized into parallel execution waves.
  ///
  /// Each wave contains builds that can be executed in parallel because
  /// all their dependencies are in previous waves.
  pub fn build_waves(&self) -> Result<Vec<Vec<BuildHash>>, ExecuteError> {
    // Use Kahn's algorithm variant to compute levels
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut node_level: HashMap<NodeIndex, usize> = HashMap::new();

    // Initialize in-degrees
    for idx in self.graph.node_indices() {
      in_degree.insert(idx, self.graph.neighbors_directed(idx, Direction::Incoming).count());
    }

    // Process nodes level by level
    let mut current_level = 0;
    let mut remaining: HashSet<NodeIndex> = self.graph.node_indices().collect();

    while !remaining.is_empty() {
      // Find nodes with no remaining dependencies
      let ready: Vec<NodeIndex> = remaining.iter().filter(|&&idx| in_degree[&idx] == 0).copied().collect();

      if ready.is_empty() && !remaining.is_empty() {
        return Err(ExecuteError::CycleDetected);
      }

      // Assign level to ready nodes
      for &idx in &ready {
        node_level.insert(idx, current_level);
        remaining.remove(&idx);

        // Decrement in-degree of dependents
        for neighbor in self.graph.neighbors_directed(idx, Direction::Outgoing) {
          if let Some(deg) = in_degree.get_mut(&neighbor) {
            *deg = deg.saturating_sub(1);
          }
        }
      }

      current_level += 1;
    }

    // Group builds by level
    let max_level = node_level.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<BuildHash>> = vec![Vec::new(); max_level + 1];

    for (hash, &idx) in &self.build_nodes {
      if let Some(&level) = node_level.get(&idx) {
        waves[level].push(hash.clone());
      }
    }

    // Remove empty waves (can happen if a level only has binds)
    waves.retain(|w| !w.is_empty());

    Ok(waves)
  }

  /// Get the direct build dependencies of a build.
  pub fn build_dependencies(&self, hash: &BuildHash) -> Vec<BuildHash> {
    let Some(&idx) = self.build_nodes.get(hash) else {
      return Vec::new();
    };

    self
      .graph
      .neighbors_directed(idx, Direction::Incoming)
      .filter_map(|dep_idx| {
        if let DagNode::Build(dep_hash) = &self.graph[dep_idx] {
          Some(dep_hash.clone())
        } else {
          None
        }
      })
      .collect()
  }

  /// Get the direct bind dependencies of a build.
  pub fn bind_dependencies(&self, hash: &BuildHash) -> Vec<BindHash> {
    let Some(&idx) = self.build_nodes.get(hash) else {
      return Vec::new();
    };

    self
      .graph
      .neighbors_directed(idx, Direction::Incoming)
      .filter_map(|dep_idx| {
        if let DagNode::Bind(dep_hash) = &self.graph[dep_idx] {
          Some(dep_hash.clone())
        } else {
          None
        }
      })
      .collect()
  }

  /// Check if a build has any dependencies.
  pub fn has_dependencies(&self, hash: &BuildHash) -> bool {
    let Some(&idx) = self.build_nodes.get(hash) else {
      return false;
    };

    self.graph.neighbors_directed(idx, Direction::Incoming).next().is_some()
  }

  /// Get all build hashes in the DAG.
  pub fn all_builds(&self) -> Vec<BuildHash> {
    self.build_nodes.keys().cloned().collect()
  }

  /// Get the number of builds in the DAG.
  pub fn build_count(&self) -> usize {
    self.build_nodes.len()
  }

  /// Get the number of binds in the DAG.
  pub fn bind_count(&self) -> usize {
    self.bind_nodes.len()
  }

  /// Get all bind hashes in the DAG.
  pub fn all_binds(&self) -> impl Iterator<Item = &BindHash> {
    self.bind_nodes.keys()
  }

  /// Get a bind definition by hash.
  pub fn get_bind<'a>(&self, hash: &BindHash, manifest: &'a Manifest) -> Option<&'a BindDef> {
    if self.bind_nodes.contains_key(hash) {
      manifest.bindings.get(hash)
    } else {
      None
    }
  }

  /// Get the direct build dependencies of a bind.
  pub fn bind_build_dependencies(&self, hash: &BindHash) -> Vec<BuildHash> {
    let Some(&idx) = self.bind_nodes.get(hash) else {
      return Vec::new();
    };

    self
      .graph
      .neighbors_directed(idx, Direction::Incoming)
      .filter_map(|dep_idx| {
        if let DagNode::Build(dep_hash) = &self.graph[dep_idx] {
          Some(dep_hash.clone())
        } else {
          None
        }
      })
      .collect()
  }

  /// Get the direct bind dependencies of a bind.
  pub fn bind_bind_dependencies(&self, hash: &BindHash) -> Vec<BindHash> {
    let Some(&idx) = self.bind_nodes.get(hash) else {
      return Vec::new();
    };

    self
      .graph
      .neighbors_directed(idx, Direction::Incoming)
      .filter_map(|dep_idx| {
        if let DagNode::Bind(dep_hash) = &self.graph[dep_idx] {
          Some(dep_hash.clone())
        } else {
          None
        }
      })
      .collect()
  }

  /// Get unified execution waves containing both builds and binds.
  ///
  /// Each wave contains nodes (builds and binds) that can be executed in parallel
  /// because all their dependencies are in previous waves. This interleaves
  /// builds and binds based on their actual dependencies.
  ///
  /// Note: Builds can only depend on other builds, while binds can depend on
  /// both builds and other binds.
  ///
  /// # Example
  ///
  /// If you have:
  /// - Build A (no deps)
  /// - Build B (depends on Build A)
  /// - Bind X (depends on Build A)
  /// - Bind Y (depends on Bind X)
  ///
  /// The waves would be:
  /// - Wave 0: [Build(A)]
  /// - Wave 1: [Build(B), Bind(X)]
  /// - Wave 2: [Bind(Y)]
  pub fn execution_waves(&self) -> Result<Vec<Vec<DagNode>>, ExecuteError> {
    // Use Kahn's algorithm variant to compute levels
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut node_level: HashMap<NodeIndex, usize> = HashMap::new();

    // Initialize in-degrees
    for idx in self.graph.node_indices() {
      in_degree.insert(idx, self.graph.neighbors_directed(idx, Direction::Incoming).count());
    }

    // Process nodes level by level
    let mut current_level = 0;
    let mut remaining: HashSet<NodeIndex> = self.graph.node_indices().collect();

    while !remaining.is_empty() {
      // Find nodes with no remaining dependencies
      let ready: Vec<NodeIndex> = remaining.iter().filter(|&&idx| in_degree[&idx] == 0).copied().collect();

      if ready.is_empty() && !remaining.is_empty() {
        return Err(ExecuteError::CycleDetected);
      }

      // Assign level to ready nodes
      for &idx in &ready {
        node_level.insert(idx, current_level);
        remaining.remove(&idx);

        // Decrement in-degree of dependents
        for neighbor in self.graph.neighbors_directed(idx, Direction::Outgoing) {
          if let Some(deg) = in_degree.get_mut(&neighbor) {
            *deg = deg.saturating_sub(1);
          }
        }
      }

      current_level += 1;
    }

    // Group nodes by level
    let max_level = node_level.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<DagNode>> = vec![Vec::new(); max_level + 1];

    for idx in self.graph.node_indices() {
      if let Some(&level) = node_level.get(&idx) {
        waves[level].push(self.graph[idx].clone());
      }
    }

    // Remove empty waves (shouldn't happen, but be safe)
    waves.retain(|w| !w.is_empty());

    Ok(waves)
  }
}

/// Extract build and bind dependencies from an InputsRef.
fn extract_dependencies(inputs: &InputsRef) -> Vec<DagNode> {
  let mut deps = Vec::new();
  collect_dependencies(inputs, &mut deps);
  deps
}

/// Recursively collect dependencies from nested InputsRef.
fn collect_dependencies(inputs: &InputsRef, deps: &mut Vec<DagNode>) {
  match inputs {
    InputsRef::Build(hash) => {
      deps.push(DagNode::Build(hash.clone()));
    }
    InputsRef::Bind(hash) => {
      deps.push(DagNode::Bind(hash.clone()));
    }
    InputsRef::Table(map) => {
      for value in map.values() {
        collect_dependencies(value, deps);
      }
    }
    InputsRef::Array(arr) => {
      for value in arr {
        collect_dependencies(value, deps);
      }
    }
    InputsRef::String(_) | InputsRef::Number(_) | InputsRef::Boolean(_) => {}
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::*;
  use crate::bind::BindDef;
  use crate::build::{BuildAction, BuildDef};

  fn make_build(name: &str, inputs: Option<InputsRef>) -> BuildDef {
    BuildDef {
      name: name.to_string(),
      version: None,
      inputs,
      apply_actions: vec![BuildAction::Cmd {
        cmd: "echo test".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
    }
  }

  fn make_bind(inputs: Option<InputsRef>) -> BindDef {
    use crate::bind::BindAction;
    BindDef {
      inputs,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo test".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
      destroy_actions: None,
    }
  }

  #[test]
  fn empty_manifest() {
    let manifest = Manifest::default();
    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    assert_eq!(dag.build_count(), 0);
    assert!(dag.topological_builds().unwrap().is_empty());
    assert!(dag.build_waves().unwrap().is_empty());
  }

  #[test]
  fn single_build_no_deps() {
    let build = make_build("test", None);
    let hash = build.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash.clone(), build);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    assert_eq!(dag.build_count(), 1);
    assert!(!dag.has_dependencies(&hash));

    let topo = dag.topological_builds().unwrap();
    assert_eq!(topo, vec![hash.clone()]);

    let waves = dag.build_waves().unwrap();
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0], vec![hash]);
  }

  #[test]
  fn linear_dependency_chain() {
    // A -> B -> C (C depends on B, B depends on A)
    let build_a = make_build("a", None);
    let hash_a = build_a.compute_hash().unwrap();

    let build_b = make_build("b", Some(InputsRef::Build(hash_a.clone())));
    let hash_b = build_b.compute_hash().unwrap();

    let build_c = make_build("c", Some(InputsRef::Build(hash_b.clone())));
    let hash_c = build_c.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash_a.clone(), build_a);
    manifest.builds.insert(hash_b.clone(), build_b);
    manifest.builds.insert(hash_c.clone(), build_c);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    // Check dependencies
    assert!(!dag.has_dependencies(&hash_a));
    assert!(dag.has_dependencies(&hash_b));
    assert!(dag.has_dependencies(&hash_c));

    assert_eq!(dag.build_dependencies(&hash_b), vec![hash_a.clone()]);
    assert_eq!(dag.build_dependencies(&hash_c), vec![hash_b.clone()]);

    // Check topological order: A must come before B, B before C
    let topo = dag.topological_builds().unwrap();
    let pos_a = topo.iter().position(|h| h == &hash_a).unwrap();
    let pos_b = topo.iter().position(|h| h == &hash_b).unwrap();
    let pos_c = topo.iter().position(|h| h == &hash_c).unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);

    // Check waves: should be 3 waves with 1 build each
    let waves = dag.build_waves().unwrap();
    assert_eq!(waves.len(), 3);
    assert_eq!(waves[0], vec![hash_a]);
    assert_eq!(waves[1], vec![hash_b]);
    assert_eq!(waves[2], vec![hash_c]);
  }

  #[test]
  fn diamond_dependency() {
    //     A
    //    / \
    //   B   C
    //    \ /
    //     D
    let build_a = make_build("a", None);
    let hash_a = build_a.compute_hash().unwrap();

    let build_b = make_build("b", Some(InputsRef::Build(hash_a.clone())));
    let hash_b = build_b.compute_hash().unwrap();

    let build_c = make_build("c", Some(InputsRef::Build(hash_a.clone())));
    let hash_c = build_c.compute_hash().unwrap();

    // D depends on both B and C
    let mut d_inputs = BTreeMap::new();
    d_inputs.insert("b".to_string(), InputsRef::Build(hash_b.clone()));
    d_inputs.insert("c".to_string(), InputsRef::Build(hash_c.clone()));
    let build_d = make_build("d", Some(InputsRef::Table(d_inputs)));
    let hash_d = build_d.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash_a.clone(), build_a);
    manifest.builds.insert(hash_b.clone(), build_b);
    manifest.builds.insert(hash_c.clone(), build_c);
    manifest.builds.insert(hash_d.clone(), build_d);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    // Check topological order
    let topo = dag.topological_builds().unwrap();
    let pos_a = topo.iter().position(|h| h == &hash_a).unwrap();
    let pos_b = topo.iter().position(|h| h == &hash_b).unwrap();
    let pos_c = topo.iter().position(|h| h == &hash_c).unwrap();
    let pos_d = topo.iter().position(|h| h == &hash_d).unwrap();

    assert!(pos_a < pos_b);
    assert!(pos_a < pos_c);
    assert!(pos_b < pos_d);
    assert!(pos_c < pos_d);

    // Check waves
    let waves = dag.build_waves().unwrap();
    assert_eq!(waves.len(), 3);

    // Wave 0: just A
    assert_eq!(waves[0].len(), 1);
    assert!(waves[0].contains(&hash_a));

    // Wave 1: B and C (parallel)
    assert_eq!(waves[1].len(), 2);
    assert!(waves[1].contains(&hash_b));
    assert!(waves[1].contains(&hash_c));

    // Wave 2: just D
    assert_eq!(waves[2].len(), 1);
    assert!(waves[2].contains(&hash_d));
  }

  #[test]
  fn parallel_independent_builds() {
    // Three independent builds should all be in wave 0
    let build_a = make_build("a", None);
    let hash_a = build_a.compute_hash().unwrap();

    let build_b = make_build("b", None);
    let hash_b = build_b.compute_hash().unwrap();

    let build_c = make_build("c", None);
    let hash_c = build_c.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash_a.clone(), build_a);
    manifest.builds.insert(hash_b.clone(), build_b);
    manifest.builds.insert(hash_c.clone(), build_c);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    let waves = dag.build_waves().unwrap();
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0].len(), 3);
  }

  #[test]
  fn nested_inputs_dependencies() {
    let build_a = make_build("a", None);
    let hash_a = build_a.compute_hash().unwrap();

    let build_b = make_build("b", None);
    let hash_b = build_b.compute_hash().unwrap();

    // C has nested dependencies in a table and array
    let mut table = BTreeMap::new();
    table.insert("dep".to_string(), InputsRef::Build(hash_a.clone()));
    table.insert(
      "nested".to_string(),
      InputsRef::Array(vec![InputsRef::Build(hash_b.clone())]),
    );

    let build_c = make_build("c", Some(InputsRef::Table(table)));
    let hash_c = build_c.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash_a.clone(), build_a);
    manifest.builds.insert(hash_b.clone(), build_b);
    manifest.builds.insert(hash_c.clone(), build_c);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    let deps = dag.build_dependencies(&hash_c);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&hash_a));
    assert!(deps.contains(&hash_b));
  }

  // Note: We can't easily test cycle detection because creating a cycle
  // would require hash collisions or manually constructing invalid state.
  // The graph construction naturally prevents cycles through the hash-based
  // references (you can't reference a build that doesn't exist yet).

  #[test]
  fn bind_count_and_all_binds() {
    let bind_a = make_bind(None);
    let hash_a = bind_a.compute_hash().unwrap();

    let bind_b = make_bind(Some(InputsRef::String("different".to_string())));
    let hash_b = bind_b.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.bindings.insert(hash_a.clone(), bind_a);
    manifest.bindings.insert(hash_b.clone(), bind_b);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    assert_eq!(dag.bind_count(), 2);

    let all: Vec<_> = dag.all_binds().cloned().collect();
    assert_eq!(all.len(), 2);
    assert!(all.contains(&hash_a));
    assert!(all.contains(&hash_b));
  }

  #[test]
  fn bind_depends_on_build() {
    let build = make_build("dep", None);
    let build_hash = build.compute_hash().unwrap();

    let bind = make_bind(Some(InputsRef::Build(build_hash.clone())));
    let bind_hash = bind.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(build_hash.clone(), build);
    manifest.bindings.insert(bind_hash.clone(), bind);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    let build_deps = dag.bind_build_dependencies(&bind_hash);
    assert_eq!(build_deps, vec![build_hash]);

    let bind_deps = dag.bind_bind_dependencies(&bind_hash);
    assert!(bind_deps.is_empty());
  }

  #[test]
  fn bind_depends_on_bind() {
    let bind_a = make_bind(None);
    let hash_a = bind_a.compute_hash().unwrap();

    let bind_b = make_bind(Some(InputsRef::Bind(hash_a.clone())));
    let hash_b = bind_b.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.bindings.insert(hash_a.clone(), bind_a);
    manifest.bindings.insert(hash_b.clone(), bind_b);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    let build_deps = dag.bind_build_dependencies(&hash_b);
    assert!(build_deps.is_empty());

    let bind_deps = dag.bind_bind_dependencies(&hash_b);
    assert_eq!(bind_deps, vec![hash_a]);
  }

  #[test]
  fn execution_waves_with_builds_only() {
    // Linear chain: A -> B -> C
    let build_a = make_build("a", None);
    let hash_a = build_a.compute_hash().unwrap();

    let build_b = make_build("b", Some(InputsRef::Build(hash_a.clone())));
    let hash_b = build_b.compute_hash().unwrap();

    let build_c = make_build("c", Some(InputsRef::Build(hash_b.clone())));
    let hash_c = build_c.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(hash_a.clone(), build_a);
    manifest.builds.insert(hash_b.clone(), build_b);
    manifest.builds.insert(hash_c.clone(), build_c);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();
    let waves = dag.execution_waves().unwrap();

    assert_eq!(waves.len(), 3);
    assert_eq!(waves[0], vec![DagNode::Build(hash_a)]);
    assert_eq!(waves[1], vec![DagNode::Build(hash_b)]);
    assert_eq!(waves[2], vec![DagNode::Build(hash_c)]);
  }

  #[test]
  fn execution_waves_with_binds_only() {
    // Linear chain: Bind A -> Bind B
    let bind_a = make_bind(None);
    let hash_a = bind_a.compute_hash().unwrap();

    let bind_b = make_bind(Some(InputsRef::Bind(hash_a.clone())));
    let hash_b = bind_b.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.bindings.insert(hash_a.clone(), bind_a);
    manifest.bindings.insert(hash_b.clone(), bind_b);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();
    let waves = dag.execution_waves().unwrap();

    assert_eq!(waves.len(), 2);
    assert_eq!(waves[0], vec![DagNode::Bind(hash_a)]);
    assert_eq!(waves[1], vec![DagNode::Bind(hash_b)]);
  }

  #[test]
  fn execution_waves_parallel_mixed() {
    // Independent build A and bind B should be in the same wave
    let build_a = make_build("a", None);
    let build_hash_a = build_a.compute_hash().unwrap();

    let bind_b = make_bind(None);
    let bind_hash_b = bind_b.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.builds.insert(build_hash_a.clone(), build_a);
    manifest.bindings.insert(bind_hash_b.clone(), bind_b);

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();
    let waves = dag.execution_waves().unwrap();

    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0].len(), 2);
    assert!(waves[0].contains(&DagNode::Build(build_hash_a)));
    assert!(waves[0].contains(&DagNode::Bind(bind_hash_b)));
  }

  #[test]
  fn get_bind_from_manifest() {
    let bind = make_bind(None);
    let bind_hash = bind.compute_hash().unwrap();

    let mut manifest = Manifest::default();
    manifest.bindings.insert(bind_hash.clone(), bind.clone());

    let dag = ExecutionDag::from_manifest(&manifest).unwrap();

    let retrieved = dag.get_bind(&bind_hash, &manifest);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), &bind);

    // Non-existent bind
    let fake_hash = BindHash("nonexistent".to_string());
    assert!(dag.get_bind(&fake_hash, &manifest).is_none());
  }
}
