use std::collections::{BTreeMap, HashMap};

use mlua::Function;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::{InputsRef, InputsSpec};

/// The bind specification as defined in Lua.
pub struct BindSpec {
  pub inputs: Option<InputsSpec>,
  pub apply: Function,
  pub destroy: Option<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BindHash(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BindAction {
  Cmd {
    cmd: String,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<String>,
  },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindDef {
  pub inputs: Option<InputsRef>,
  pub apply_actions: Vec<BindAction>,
  pub outputs: Option<BTreeMap<String, String>>,
  pub destroy_actions: Option<Vec<BindAction>>,
}

impl BindDef {
  pub fn compute_hash(&self) -> Result<BindHash, serde_json::Error> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    Ok(BindHash(format!("{:x}", hasher.finalize())))
  }
}

pub struct BindCmdOptions {
  pub cmd: String,
  pub env: Option<BTreeMap<String, String>>,
  pub cwd: Option<String>,
}

impl BindCmdOptions {
  pub fn new(cmd: &str) -> Self {
    Self {
      cmd: cmd.to_string(),
      env: None,
      cwd: None,
    }
  }

  pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
    self.env = Some(env);
    self
  }

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

#[derive(Default)]
pub struct BindCtx {
  actions: Vec<BindAction>,
}

impl BindCtx {
  pub fn new() -> Self {
    Self { actions: Vec::new() }
  }

  pub fn cmd(&mut self, opts: impl Into<BindCmdOptions>) -> String {
    let output = format!("${{action:{}}}", self.actions.len());
    let opts = opts.into();
    self.actions.push(BindAction::Cmd {
      cmd: opts.cmd,
      env: opts.env,
      cwd: opts.cwd,
    });
    output
  }

  pub fn into_actions(self) -> Vec<BindAction> {
    self.actions
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindRef {
  pub inputs: Option<InputsRef>,
  pub outputs: Option<HashMap<String, String>>,
  pub hash: BindHash,
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
        outputs: Some(BTreeMap::from([("link".to_string(), "${action:0}".to_string())])),
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

      assert_eq!(p0, "${action:0}");
      assert_eq!(p1, "${action:1}");
      assert_eq!(p2, "${action:2}");
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

  mod bind_ref {
    use super::*;

    #[test]
    fn serialization_roundtrip_with_nested_inputs() {
      // BindRef with nested InputsRef containing a BuildRef - tests the
      // cross-reference capability that's central to the type system
      let build_ref = crate::build::BuildRef {
        name: "source".to_string(),
        version: Some("1.0.0".to_string()),
        inputs: None,
        outputs: HashMap::from([("out".to_string(), "/store/obj/source-abc123".to_string())]),
        hash: crate::build::BuildHash("abc123".to_string()),
      };

      let bind_ref = BindRef {
        inputs: Some(InputsRef::Build(Box::new(build_ref))),
        outputs: Some(HashMap::from([(
          "link".to_string(),
          "/home/user/.config/app".to_string(),
        )])),
        hash: BindHash("def456".to_string()),
      };

      let json = serde_json::to_string(&bind_ref).unwrap();
      let deserialized: BindRef = serde_json::from_str(&json).unwrap();

      assert_eq!(bind_ref, deserialized);
    }
  }
}
