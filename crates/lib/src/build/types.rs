//! Build types for syslua.
//!
//! This module defines the core types for builds, which are reproducible artifacts
//! that can be cached and reused. Builds follow a two-tier architecture:
//!
//! - [`BuildSpec`]: The Lua-side specification containing closures (not serializable)
//! - [`BuildDef`]: The evaluated, serializable definition stored in manifests
//!
//! Builds are identified by content-addressed hashes ([`BuildHash`]) computed from
//! their [`BuildDef`]. This enables deduplication and caching.

use std::collections::BTreeMap;

use mlua::Function;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::consts::HASH_PREFIX_LEN;
use crate::inputs::{InputsRef, InputsSpec};

/// Marker type name for BuildRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a build. BuildRefs are Lua tables with metatables that allow accessing
/// build outputs (e.g., `build.out` or `build.bin`).
pub const BUILD_REF_TYPE: &str = "BuildRef";

/// The build specification as defined in Lua.
///
/// This is the Lua-side representation of a build, containing the raw closure
/// that will be evaluated to produce a [`BuildDef`]. Because it contains a
/// Lua [`Function`], it cannot be serialized directly.
///
/// # Fields
///
/// - `name`: Human-readable identifier for the build (e.g., "ripgrep", "curl")
/// - `version`: Optional version string for display and organization
/// - `inputs`: Optional inputs that parameterize the build
/// - `apply`: The Lua function called with a [`BuildCtx`] to define build actions
///
/// # Lifecycle
///
/// ```text
/// BuildSpec (Lua) → evaluate apply() → BuildDef (serializable) → BuildHash
/// ```
pub struct BuildSpec {
  /// Human-readable name for the build (e.g., "ripgrep", "neovim").
  pub name: String,
  /// Optional version string (e.g., "15.1.0").
  pub version: Option<String>,
  /// Optional inputs that parameterize the build.
  pub inputs: Option<InputsSpec>,
  /// The Lua function to evaluate with a [`BuildCtx`] to produce actions.
  pub apply: Function,
}

/// A content-addressed hash identifying a unique [`BuildDef`].
///
/// The hash is a 20-character truncated SHA-256 of the JSON-serialized [`BuildDef`].
/// This provides sufficient collision resistance while keeping paths readable.
///
/// # Format
///
/// The hash is a lowercase hexadecimal string, e.g., `"a1b2c3d4e5f6789012ab"`.
///
/// # Usage
///
/// BuildHash is used as:
/// - Keys in the manifest's `builds` map for deduplication
/// - Store paths: `~/.local/share/syslua/build/<hash>/`
/// - References in [`InputsRef::Build`] to track dependencies
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BuildHash(pub String);

impl std::fmt::Display for BuildHash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

/// An action that can be performed during build execution.
///
/// Build actions are the primitive operations that builds can perform.
/// They are recorded during [`BuildCtx`] method calls and stored in [`BuildDef`].
///
/// # Variants
///
/// - [`FetchUrl`](BuildAction::FetchUrl): Download a file with integrity verification
/// - [`Cmd`](BuildAction::Cmd): Execute a shell command
///
/// # Placeholder Resolution
///
/// When actions are executed, their outputs are captured and can be referenced
/// by subsequent actions via placeholders (e.g., `$${action:0}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildAction {
  /// Fetch a URL with SHA-256 integrity verification.
  ///
  /// This is a built-in action to avoid bootstrap problems (e.g., needing curl
  /// to build curl). The runtime handles the download directly.
  ///
  /// # Fields
  ///
  /// - `url`: The URL to download
  /// - `sha256`: Expected SHA-256 hash of the downloaded content (lowercase hex)
  FetchUrl { url: String, sha256: String },
  /// Execute a shell command.
  ///
  /// # Fields
  ///
  /// - `cmd`: The command string to execute (passed to shell)
  /// - `env`: Optional environment variables to set
  /// - `cwd`: Optional working directory (may contain placeholders)
  Cmd {
    cmd: String,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<String>,
  },
}

/// The evaluated, serializable definition of a build.
///
/// This is the manifest-side representation produced by evaluating a [`BuildSpec`].
/// Unlike `BuildSpec`, this type is fully serializable and contains no Lua closures.
///
/// # Content Addressing
///
/// A [`BuildHash`] is computed from the JSON serialization of this struct.
/// Two `BuildDef`s with identical content produce the same hash, enabling
/// deduplication in the manifest.
///
/// # Placeholders
///
/// String fields may contain placeholders that resolve at execution time:
/// - `$${action:N}`: Output of action at index N
/// - `$${build:hash:output}`: Output from another build
/// - `$${bind:hash:output}`: Output from a binding
///
/// Shell variables like `$HOME` pass through unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildDef {
  /// Human-readable name for the build.
  pub name: String,
  /// Optional version string.
  pub version: Option<String>,
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<InputsRef>,
  /// The sequence of actions to execute during `apply`.
  pub apply_actions: Vec<BuildAction>,
  /// Named outputs from the build (e.g., `{"out": "$${action:2}", "bin": "..."}`).
  pub outputs: Option<BTreeMap<String, String>>,
}

impl BuildDef {
  /// Compute the truncated SHA-256 hash for use as manifest key.
  ///
  /// The hash is computed from the JSON serialization of this `BuildDef`,
  /// then truncated to [`HASH_PREFIX_LEN`] characters (20 chars).
  ///
  /// # Determinism
  ///
  /// The hash is deterministic: identical `BuildDef` values always produce
  /// the same hash. This is guaranteed by:
  /// - Using `serde_json` for consistent serialization
  /// - Using `BTreeMap` for ordered keys
  ///
  /// # Errors
  ///
  /// Returns an error if JSON serialization fails (should not happen for
  /// well-formed `BuildDef` values).
  pub fn compute_hash(&self) -> Result<BuildHash, serde_json::Error> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    Ok(BuildHash(full[..HASH_PREFIX_LEN].to_string()))
  }
}

/// Options for executing a shell command in a build.
///
/// This is a builder-pattern struct for configuring [`BuildAction::Cmd`] actions.
/// It can be constructed from a string slice for simple commands.
///
/// # Example
///
/// ```ignore
/// // Simple command
/// ctx.cmd("make install");
///
/// // With environment and working directory
/// ctx.cmd(
///     BuildCmdOptions::new("make")
///         .with_env(env)
///         .with_cwd("/build")
/// );
/// ```
pub struct BuildCmdOptions {
  /// The command string to execute.
  pub cmd: String,
  /// Optional environment variables to set.
  pub env: Option<BTreeMap<String, String>>,
  /// Optional working directory.
  pub cwd: Option<String>,
}

impl BuildCmdOptions {
  /// Create a new command with default options.
  pub fn new(cmd: &str) -> Self {
    Self {
      cmd: cmd.to_string(),
      env: None,
      cwd: None,
    }
  }

  /// Set environment variables for the command.
  pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
    self.env = Some(env);
    self
  }

  /// Set the working directory for the command.
  pub fn with_cwd(mut self, cwd: &str) -> Self {
    self.cwd = Some(cwd.to_string());
    self
  }
}

impl From<&str> for BuildCmdOptions {
  fn from(cmd: &str) -> Self {
    BuildCmdOptions::new(cmd)
  }
}

/// Context passed to build `apply` functions for recording actions.
///
/// When a [`BuildSpec::apply`] function is called, it receives a `BuildCtx`.
/// The Lua code calls methods on this context to record build actions, which
/// are later stored in the [`BuildDef`].
///
/// # Action Recording
///
/// Each method call (e.g., [`fetch_url`](Self::fetch_url), [`cmd`](Self::cmd))
/// appends an action to the internal list and returns a placeholder string.
/// These placeholders can be used in subsequent actions or stored in outputs.
///
/// # Placeholder Format
///
/// Methods return opaque placeholder strings like `$${action:0}`, `$${action:1}`, etc.
/// Users should not construct these manually - they're implementation details.
///
/// # Example (Lua)
///
/// ```lua
/// sys.build {
///     name = "ripgrep",
///     apply = function(ctx)
///         local archive = ctx:fetch_url("https://...", "sha256...")
///         ctx:cmd("tar xf " .. archive)
///         return { out = ctx:cmd("...") }
///     end
/// }
/// ```
#[derive(Default)]
pub struct BuildCtx {
  /// The recorded actions, in order.
  actions: Vec<BuildAction>,
}

impl BuildCtx {
  /// Create a new empty build context.
  pub fn new() -> Self {
    Self { actions: Vec::new() }
  }

  /// Returns a placeholder string that resolves to the build's output directory.
  ///
  /// This should be used in commands and outputs to reference where the build
  /// should write its artifacts. At execution time, this placeholder resolves
  /// to the actual store path (e.g., `/syslua/store/obj/ripgrep-15.1.0-abc123/`).
  ///
  /// # Returns
  ///
  /// The string `"$${out}"` which is substituted at execution time.
  ///
  /// # Example (Lua)
  ///
  /// ```lua
  /// sys.build {
  ///     name = "my-tool",
  ///     apply = function(inputs, ctx)
  ///         ctx:cmd("mkdir -p " .. ctx.out .. "/bin")
  ///         ctx:cmd("cp binary " .. ctx.out .. "/bin/")
  ///         return { out = ctx.out }
  ///     end
  /// }
  /// ```
  pub fn out(&self) -> &'static str {
    "$${out}"
  }

  /// Record a URL fetch action and return a placeholder for its output.
  ///
  /// The returned placeholder resolves to the path of the downloaded file
  /// at execution time.
  ///
  /// # Arguments
  ///
  /// - `url`: The URL to download
  /// - `sha256`: Expected SHA-256 hash (lowercase hex) for integrity verification
  ///
  /// # Returns
  ///
  /// An opaque placeholder string (e.g., `$${action:0}`) that resolves to
  /// the downloaded file path at execution time.
  pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String {
    let output = format!("$${{action:{}}}", self.actions.len());
    self.actions.push(BuildAction::FetchUrl {
      url: url.to_string(),
      sha256: sha256.to_string(),
    });
    output
  }

  /// Record a command execution action and return a placeholder for its output.
  ///
  /// The returned placeholder resolves to the command's stdout at execution time.
  ///
  /// # Arguments
  ///
  /// - `opts`: Command options (can be a string slice for simple commands)
  ///
  /// # Returns
  ///
  /// An opaque placeholder string (e.g., `$${action:1}`) that resolves to
  /// the command's output at execution time.
  pub fn cmd(&mut self, opts: impl Into<BuildCmdOptions>) -> String {
    let output = format!("$${{action:{}}}", self.actions.len());
    let opts = opts.into();
    self.actions.push(BuildAction::Cmd {
      cmd: opts.cmd,
      env: opts.env,
      cwd: opts.cwd,
    });
    output
  }

  /// Consume the context and return the recorded actions.
  ///
  /// This is called after the `apply` function completes to extract
  /// the actions for storage in [`BuildDef::apply_actions`].
  pub fn into_actions(self) -> Vec<BuildAction> {
    self.actions
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  mod build_def {
    use super::*;

    fn simple_def() -> BuildDef {
      BuildDef {
        name: "ripgrep".to_string(),
        version: Some("15.1.0".to_string()),
        inputs: None,
        apply_actions: vec![BuildAction::FetchUrl {
          url: "https://example.com/rg.tar.gz".to_string(),
          sha256: "abc123".to_string(),
        }],
        outputs: None,
      }
    }

    #[test]
    fn hash_is_deterministic() {
      let def = simple_def();

      let hash1 = def.compute_hash().unwrap();
      let hash2 = def.compute_hash().unwrap();

      assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_is_truncated() {
      let def = simple_def();
      let hash = def.compute_hash().unwrap();
      assert_eq!(hash.0.len(), HASH_PREFIX_LEN);
    }

    #[test]
    fn hash_changes_when_name_differs() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.name = "fd".to_string();

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_actions_differ() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.apply_actions.push(BuildAction::Cmd {
        cmd: "make".to_string(),
        env: None,
        cwd: None,
      });

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_action_order_differs() {
      // Action order matters for reproducibility - same actions in different
      // order should produce different hashes
      let def1 = BuildDef {
        name: "test".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![
          BuildAction::Cmd {
            cmd: "step1".to_string(),
            env: None,
            cwd: None,
          },
          BuildAction::Cmd {
            cmd: "step2".to_string(),
            env: None,
            cwd: None,
          },
        ],
        outputs: None,
      };

      let def2 = BuildDef {
        name: "test".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![
          BuildAction::Cmd {
            cmd: "step2".to_string(),
            env: None,
            cwd: None,
          },
          BuildAction::Cmd {
            cmd: "step1".to_string(),
            env: None,
            cwd: None,
          },
        ],
        outputs: None,
      };

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
      let mut env = BTreeMap::new();
      env.insert("CC".to_string(), "gcc".to_string());

      let def = BuildDef {
        name: "complex".to_string(),
        version: Some("1.0.0".to_string()),
        inputs: Some(InputsRef::String("test".to_string())),
        apply_actions: vec![
          BuildAction::FetchUrl {
            url: "https://example.com/src.tar.gz".to_string(),
            sha256: "abc123".to_string(),
          },
          BuildAction::Cmd {
            cmd: "make".to_string(),
            env: Some(env),
            cwd: Some("/build".to_string()),
          },
        ],
        outputs: Some(BTreeMap::from([("out".to_string(), "$${action:1}".to_string())])),
      };

      let json = serde_json::to_string(&def).unwrap();
      let deserialized: BuildDef = serde_json::from_str(&json).unwrap();

      assert_eq!(def, deserialized);
    }
  }

  mod build_ctx {
    use super::*;

    #[test]
    fn actions_return_sequential_placeholders() {
      let mut ctx = BuildCtx::new();

      let p0 = ctx.fetch_url("https://example.com/a.tar.gz", "hash1");
      let p1 = ctx.cmd("tar xf a.tar.gz");
      let p2 = ctx.fetch_url("https://example.com/b.tar.gz", "hash2");

      // All actions use the same placeholder format with sequential indices
      assert_eq!(p0, "$${action:0}");
      assert_eq!(p1, "$${action:1}");
      assert_eq!(p2, "$${action:2}");
    }

    #[test]
    fn cmd_preserves_env_and_cwd() {
      let mut ctx = BuildCtx::new();
      let mut env = BTreeMap::new();
      env.insert("CC".to_string(), "clang".to_string());

      ctx.cmd(BuildCmdOptions::new("make").with_env(env.clone()).with_cwd("/build"));

      let actions = ctx.into_actions();
      assert_eq!(actions.len(), 1);

      match &actions[0] {
        BuildAction::Cmd {
          cmd,
          env: action_env,
          cwd,
        } => {
          assert_eq!(cmd, "make");
          assert_eq!(action_env, &Some(env));
          assert_eq!(cwd, &Some("/build".to_string()));
        }
        _ => panic!("Expected Cmd action"),
      }
    }
  }
}
