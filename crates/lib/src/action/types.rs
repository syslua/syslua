use serde::{Deserialize, Serialize};

use crate::action::actions::exec::ExecOpts;

/// Key for storing registered build ctx methods in Lua's registry.
pub const BUILD_CTX_METHODS_REGISTRY_KEY: &str = "__syslua_build_ctx_methods";

/// Key for storing registered bind ctx methods in Lua's registry.
pub const BIND_CTX_METHODS_REGISTRY_KEY: &str = "__syslua_bind_ctx_methods";

/// An action that can be performed during build execution.
///
/// Build actions are the primitive operations that builds can perform.
/// They are recorded during [`ActionCtx`] method calls and stored in [`BuildDef`].
///
/// # Variants
///
/// - [`FetchUrl`](Action::FetchUrl): Download a file with integrity verification
/// - [`Exec`](Action::Exec): Execute a shell command
///
/// # Placeholder Resolution
///
/// When actions are executed, their outputs are captured and can be referenced
/// by subsequent actions via placeholders (e.g., `$${{action:0}}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
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
  /// Execute a binary.
  ///
  /// # Fields
  ///
  /// - `opts`: Execution options
  Exec(ExecOpts),
}

/// Context passed to build `apply` functions for recording actions.
///
/// When a [`BuildSpec::apply`] function is called, it receives a `ActionCtx`.
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
/// Methods return opaque placeholder strings like `$${{action:0}}`, `$${{action:1}}`, etc.
/// Users should not construct these manually - they're implementation details.
///
/// # Example (Lua)
///
/// ```lua
/// sys.build {
///     name = "ripgrep",
///     apply = function(ctx)
///         local archive = ctx:fetch_url("https://...", "sha256...")
///         ctx:exec("tar xf " .. archive)
///         return { out = ctx:exec("...") }
///     end
/// }
/// ```
#[derive(Default)]
pub struct ActionCtx {
  /// The recorded actions, in order.
  actions: Vec<Action>,
}

impl ActionCtx {
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
  /// The string `"$${{out}}"` which is substituted at execution time.
  ///
  /// # Example (Lua)
  ///
  /// ```lua
  /// sys.build {
  ///     name = "my-tool",
  ///     apply = function(inputs, ctx)
  ///         ctx:exec("mkdir -p " .. ctx.out .. "/bin")
  ///         ctx:exec("cp binary " .. ctx.out .. "/bin/")
  ///         return { out = ctx.out }
  ///     end
  /// }
  /// ```
  pub fn out(&self) -> &'static str {
    "$${{out}}"
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
  /// An opaque placeholder string (e.g., `$${{action:0}}`) that resolves to
  /// the downloaded file path at execution time.
  pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String {
    self.record_action(Action::FetchUrl {
      url: url.to_string(),
      sha256: sha256.to_string(),
    })
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
  /// An opaque placeholder string (e.g., `$${{action:1}}`) that resolves to
  /// the command's output at execution time.
  pub fn exec(&mut self, opts: impl Into<ExecOpts>) -> String {
    let opts = opts.into();
    self.record_action(Action::Exec(opts))
  }

  /// Internal helper to record an action and return its placeholder.
  fn record_action(&mut self, action: Action) -> String {
    let index = self.actions.len();
    self.actions.push(action);
    format!("$${{{{action:{}}}}}", index)
  }

  /// Returns the number of actions recorded so far.
  pub fn action_count(&self) -> usize {
    self.actions.len()
  }

  /// Consume the context and return the recorded actions.
  ///
  /// This is called after the `apply` function completes to extract
  /// the actions for storage in [`BuildDef::apply_actions`].
  pub fn into_actions(self) -> Vec<Action> {
    self.actions
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::*;

  #[test]
  fn cmd_preserves_env_and_cwd() {
    let mut ctx = ActionCtx::new();
    let mut env = BTreeMap::new();
    env.insert("CC".to_string(), "clang".to_string());

    ctx.exec(ExecOpts::new("make").with_env(env.clone()).with_cwd("/build"));

    let actions = ctx.into_actions();
    assert_eq!(actions.len(), 1);

    match &actions[0] {
      Action::Exec(opts) => {
        assert_eq!(opts.bin, "make");
        assert_eq!(opts.env, Some(env));
        assert_eq!(opts.cwd, Some("/build".to_string()));
      }
      _ => panic!("Expected Cmd action"),
    }
  }
}
