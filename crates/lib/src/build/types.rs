use std::collections::{BTreeMap, HashMap};

use mlua::Function;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::{InputsRef, InputsSpec};

pub struct BuildSpec {
  pub name: String,
  pub version: Option<String>,
  pub inputs: Option<InputsSpec>,
  pub apply: Function,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BuildHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildAction {
  FetchUrl {
    url: String,
    sha256: String,
  }, // Avoid chicken-and-egg "fetch_url build needs curl and curl build needs
  // fetch_url"
  Cmd {
    cmd: String,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<String>,
  },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildDef {
  pub name: String,
  pub version: Option<String>,
  pub inputs: Option<InputsRef>,
  pub apply_actions: Vec<BuildAction>,
  pub outputs: Option<BTreeMap<String, String>>,
}

impl BuildDef {
  pub fn compute_hash(&self) -> Result<BuildHash, serde_json::Error> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    Ok(BuildHash(format!("{:x}", hasher.finalize())))
  }
}

pub struct BuildCmdOptions {
  pub cmd: String,
  pub env: Option<BTreeMap<String, String>>,
  pub cwd: Option<String>,
}

impl BuildCmdOptions {
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

impl From<&str> for BuildCmdOptions {
  fn from(cmd: &str) -> Self {
    BuildCmdOptions::new(cmd)
  }
}

#[derive(Default)]
pub struct BuildCtx {
  actions: Vec<BuildAction>,
}

impl BuildCtx {
  pub fn new() -> Self {
    Self { actions: Vec::new() }
  }

  pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String {
    let output = format!("${{action:{}}}", self.actions.len());
    self.actions.push(BuildAction::FetchUrl {
      url: url.to_string(),
      sha256: sha256.to_string(),
    });
    output
  }

  pub fn cmd(&mut self, opts: impl Into<BuildCmdOptions>) -> String {
    let output = format!("${{action:{}}}", self.actions.len());
    let opts = opts.into();
    self.actions.push(BuildAction::Cmd {
      cmd: opts.cmd,
      env: opts.env,
      cwd: opts.cwd,
    });
    output
  }

  pub fn into_actions(self) -> Vec<BuildAction> {
    self.actions
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildRef {
  pub name: String,
  pub version: Option<String>,
  pub inputs: Option<InputsRef>,
  pub outputs: HashMap<String, String>,
  pub hash: BuildHash,
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
        outputs: Some(BTreeMap::from([("out".to_string(), "${action:1}".to_string())])),
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
      assert_eq!(p0, "${action:0}");
      assert_eq!(p1, "${action:1}");
      assert_eq!(p2, "${action:2}");
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
