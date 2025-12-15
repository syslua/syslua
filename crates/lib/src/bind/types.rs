//! Bind types for syslua.
//!
//! This module defines the core types for bindings, which represent system state
//! changes that can be applied and destroyed. Unlike builds, bindings are not
//! cached - they represent mutable state like symlinks, config files, or services.
//!
//! Bindings follow a two-tier architecture:
//!
//! - [`BindSpec`]: The Lua-side specification containing closures (not serializable)
//! - [`BindDef`]: The evaluated, serializable definition stored in manifests
//!
//! Bindings are identified by content-addressed hashes ([`BindHash`]) computed from
//! their [`BindDef`]. This enables tracking which bindings need to be reapplied.

use std::collections::BTreeMap;

use mlua::Function;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::consts::HASH_PREFIX_LEN;
use crate::inputs::{InputsRef, InputsSpec};

/// Marker type name for BindRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a binding. BindRefs are Lua tables with metatables that allow accessing
/// bind outputs (e.g., `bind.status` or `bind.path`).
pub const BIND_REF_TYPE: &str = "BindRef";

/// The bind specification as defined in Lua.
///
/// This is the Lua-side representation of a binding, containing the raw closures
/// that will be evaluated to produce a [`BindDef`]. Because it contains Lua
/// [`Function`]s, it cannot be serialized directly.
///
/// # Fields
///
/// - `inputs`: Optional inputs that parameterize the binding
/// - `apply`: The Lua function called with a [`BindCtx`] to define apply actions
/// - `destroy`: Optional Lua function for cleanup when the binding is removed
///
/// # Lifecycle
///
/// ```text
/// BindSpec (Lua) → evaluate apply()/destroy() → BindDef (serializable) → BindHash
/// ```
pub struct BindSpec {
  /// Optional inputs that parameterize the binding.
  pub inputs: Option<InputsSpec>,
  /// The Lua function to evaluate with a [`BindCtx`] to produce apply actions.
  pub apply: Function,
  /// Optional Lua function for cleanup when the binding is removed.
  pub destroy: Option<Function>,
}

/// A content-addressed hash identifying a unique [`BindDef`].
///
/// The hash is a 20-character truncated SHA-256 of the JSON-serialized [`BindDef`].
/// This provides sufficient collision resistance while keeping paths readable.
///
/// # Format
///
/// The hash is a lowercase hexadecimal string, e.g., `"a1b2c3d4e5f6789012ab"`.
///
/// # Usage
///
/// BindHash is used as:
/// - Keys in the manifest's `bindings` map for deduplication
/// - Store paths: `~/.local/share/syslua/bind/<hash>/`
/// - References in [`InputsRef::Bind`] to track dependencies
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BindHash(pub String);

impl std::fmt::Display for BindHash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

/// An action that can be performed during bind execution.
///
/// Bind actions are the primitive operations that bindings can perform.
/// They are recorded during [`BindCtx`] method calls and stored in [`BindDef`].
///
/// # Variants
///
/// Currently only [`Cmd`](BindAction::Cmd) is supported. Unlike builds, bindings
/// cannot use `FetchUrl` - they're meant for system state changes, not downloads.
///
/// # Placeholder Resolution
///
/// When actions are executed, their outputs are captured and can be referenced
/// by subsequent actions via placeholders (e.g., `$${action:0}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BindAction {
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

/// The evaluated, serializable definition of a binding.
///
/// This is the manifest-side representation produced by evaluating a [`BindSpec`].
/// Unlike `BindSpec`, this type is fully serializable and contains no Lua closures.
///
/// # Content Addressing
///
/// A [`BindHash`] is computed from the JSON serialization of this struct.
/// Two `BindDef`s with identical content produce the same hash, enabling
/// deduplication in the manifest.
///
/// # Apply vs Destroy
///
/// - `apply_actions`: Run when the binding is created or updated
/// - `destroy_actions`: Run when the binding is removed (optional cleanup)
///
/// # Placeholders
///
/// String fields may contain placeholders that resolve at execution time:
/// - `$${action:N}`: Output of action at index N
/// - `$${build:hash:output}`: Output from a build
/// - `$${bind:hash:output}`: Output from another binding
///
/// Shell variables like `$HOME` pass through unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindDef {
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<InputsRef>,
  /// The sequence of actions to execute during `apply`.
  pub apply_actions: Vec<BindAction>,
  /// Named outputs from the binding (e.g., `{"path": "$${action:0}"}`).
  pub outputs: Option<BTreeMap<String, String>>,
  /// Optional actions to execute during `destroy` (cleanup).
  pub destroy_actions: Option<Vec<BindAction>>,
}

impl BindDef {
  /// Compute the truncated SHA-256 hash for use as manifest key.
  ///
  /// The hash is computed from the JSON serialization of this `BindDef`,
  /// then truncated to [`HASH_PREFIX_LEN`] characters (20 chars).
  ///
  /// # Determinism
  ///
  /// The hash is deterministic: identical `BindDef` values always produce
  /// the same hash. This includes `destroy_actions` - adding cleanup logic
  /// changes the hash.
  ///
  /// # Errors
  ///
  /// Returns an error if JSON serialization fails (should not happen for
  /// well-formed `BindDef` values).
  pub fn compute_hash(&self) -> Result<BindHash, serde_json::Error> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    Ok(BindHash(full[..HASH_PREFIX_LEN].to_string()))
  }
}

/// Options for executing a shell command in a binding.
///
/// This is a builder-pattern struct for configuring [`BindAction::Cmd`] actions.
/// It can be constructed from a string slice for simple commands.
///
/// # Example
///
/// ```ignore
/// // Simple command
/// ctx.cmd("ln -s /src /dest");
///
/// // With environment and working directory
/// ctx.cmd(
///     BindCmdOptions::new("systemctl enable service")
///         .with_env(env)
///         .with_cwd("/etc/systemd")
/// );
/// ```
pub struct BindCmdOptions {
  /// The command string to execute.
  pub cmd: String,
  /// Optional environment variables to set.
  pub env: Option<BTreeMap<String, String>>,
  /// Optional working directory.
  pub cwd: Option<String>,
}

impl BindCmdOptions {
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

impl From<&str> for BindCmdOptions {
  fn from(cmd: &str) -> Self {
    BindCmdOptions::new(cmd)
  }
}

/// Context passed to bind `apply` and `destroy` functions for recording actions.
///
/// When a [`BindSpec::apply`] or [`BindSpec::destroy`] function is called, it
/// receives a `BindCtx`. The Lua code calls methods on this context to record
/// actions, which are later stored in the [`BindDef`].
///
/// # Action Recording
///
/// Each [`cmd`](Self::cmd) call appends an action to the internal list and returns
/// a placeholder string. These placeholders can be used in subsequent actions or
/// stored in outputs.
///
/// # Placeholder Format
///
/// The `cmd` method returns opaque placeholder strings like `$${action:0}`, `$${action:1}`, etc.
/// Users should not construct these manually - they're implementation details.
///
/// # Example (Lua)
///
/// ```lua
/// sys.bind {
///     inputs = { target = "/path/to/target" },
///     apply = function(ctx, inputs)
///         ctx:cmd("ln -sf " .. inputs.target .. " ~/.config/app")
///         return { path = "~/.config/app" }
///     end,
///     destroy = function(ctx, inputs)
///         ctx:cmd("rm ~/.config/app")
///     end
/// }
/// ```
#[derive(Default)]
pub struct BindCtx {
  /// The recorded actions, in order.
  actions: Vec<BindAction>,
}

impl BindCtx {
  /// Create a new empty bind context.
  pub fn new() -> Self {
    Self { actions: Vec::new() }
  }

  /// Returns a placeholder string that resolves to the bind's output directory.
  ///
  /// This should be used in commands and outputs to reference where the bind
  /// can store its state. At execution time, this placeholder resolves
  /// to the actual store path (e.g., `/syslua/store/bind/abc123/`).
  ///
  /// # Returns
  ///
  /// The string `"$${out}"` which is substituted at execution time.
  ///
  /// # Example (Lua)
  ///
  /// ```lua
  /// sys.bind {
  ///     apply = function(inputs, ctx)
  ///         ctx:cmd("ln -sf /src " .. ctx.out .. "/link")
  ///         return { link = ctx.out .. "/link" }
  ///     end
  /// }
  /// ```
  pub fn out(&self) -> &'static str {
    "$${out}"
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
  /// An opaque placeholder string (e.g., `$${action:0}`) that resolves to
  /// the command's output at execution time.
  pub fn cmd(&mut self, opts: impl Into<BindCmdOptions>) -> String {
    let output = format!("$${{action:{}}}", self.actions.len());
    let opts = opts.into();
    self.actions.push(BindAction::Cmd {
      cmd: opts.cmd,
      env: opts.env,
      cwd: opts.cwd,
    });
    output
  }

  /// Consume the context and return the recorded actions.
  ///
  /// This is called after the `apply` or `destroy` function completes to extract
  /// the actions for storage in [`BindDef::apply_actions`] or [`BindDef::destroy_actions`].
  pub fn into_actions(self) -> Vec<BindAction> {
    self.actions
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  mod bind_def {
    use super::*;

    fn simple_def() -> BindDef {
      BindDef {
        inputs: None,
        apply_actions: vec![BindAction::Cmd {
          cmd: "ln -s /src /dest".to_string(),
          env: None,
          cwd: None,
        }],
        outputs: None,
        destroy_actions: None,
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
    fn hash_changes_when_apply_actions_differ() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.apply_actions.push(BindAction::Cmd {
        cmd: "echo done".to_string(),
        env: None,
        cwd: None,
      });

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_destroy_actions_added() {
      // This is critical: adding destroy_actions should change the hash
      // because it changes what the bind does (cleanup behavior)
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.destroy_actions = Some(vec![BindAction::Cmd {
        cmd: "rm /dest".to_string(),
        env: None,
        cwd: None,
      }]);

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_action_order_differs() {
      let def1 = BindDef {
        inputs: None,
        apply_actions: vec![
          BindAction::Cmd {
            cmd: "step1".to_string(),
            env: None,
            cwd: None,
          },
          BindAction::Cmd {
            cmd: "step2".to_string(),
            env: None,
            cwd: None,
          },
        ],
        outputs: None,
        destroy_actions: None,
      };

      let def2 = BindDef {
        inputs: None,
        apply_actions: vec![
          BindAction::Cmd {
            cmd: "step2".to_string(),
            env: None,
            cwd: None,
          },
          BindAction::Cmd {
            cmd: "step1".to_string(),
            env: None,
            cwd: None,
          },
        ],
        outputs: None,
        destroy_actions: None,
      };

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
      let mut env = BTreeMap::new();
      env.insert("HOME".to_string(), "/home/user".to_string());

      let def = BindDef {
        inputs: Some(InputsRef::String("test".to_string())),
        apply_actions: vec![BindAction::Cmd {
          cmd: "ln -s /src /dest".to_string(),
          env: Some(env),
          cwd: Some("/home".to_string()),
        }],
        outputs: Some(BTreeMap::from([("link".to_string(), "$${action:0}".to_string())])),
        destroy_actions: Some(vec![BindAction::Cmd {
          cmd: "rm /dest".to_string(),
          env: None,
          cwd: None,
        }]),
      };

      let json = serde_json::to_string(&def).unwrap();
      let deserialized: BindDef = serde_json::from_str(&json).unwrap();

      assert_eq!(def, deserialized);
    }
  }

  mod bind_ctx {
    use super::*;

    #[test]
    fn cmd_returns_sequential_placeholders() {
      let mut ctx = BindCtx::new();

      let p0 = ctx.cmd("step1");
      let p1 = ctx.cmd("step2");
      let p2 = ctx.cmd("step3");

      assert_eq!(p0, "$${action:0}");
      assert_eq!(p1, "$${action:1}");
      assert_eq!(p2, "$${action:2}");
    }

    #[test]
    fn cmd_preserves_env_and_cwd() {
      let mut ctx = BindCtx::new();
      let mut env = BTreeMap::new();
      env.insert("HOME".to_string(), "/home/user".to_string());

      ctx.cmd(
        BindCmdOptions::new("ln -s /src /dest")
          .with_env(env.clone())
          .with_cwd("/home"),
      );

      let actions = ctx.into_actions();
      assert_eq!(actions.len(), 1);

      match &actions[0] {
        BindAction::Cmd {
          cmd,
          env: action_env,
          cwd,
        } => {
          assert_eq!(cmd, "ln -s /src /dest");
          assert_eq!(action_env, &Some(env));
          assert_eq!(cwd, &Some("/home".to_string()));
        }
      }
    }
  }
}
