use std::collections::BTreeMap;

use mlua::Function;
use serde::{Deserialize, Serialize};

use crate::{bind::BindRef, build::BuildRef};

/// The inputs specification, either static or dynamic (function).
pub enum InputsSpec {
  Static(InputsRef),
  Dynamic(Function),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputsRef {
  String(String),
  Number(f64),
  Boolean(bool),
  Table(BTreeMap<String, InputsRef>),
  Array(Vec<InputsRef>),
  Build(Box<BuildRef>),
  Bind(Box<BindRef>),
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::build::BuildHash;
  use std::collections::HashMap;

  #[test]
  fn complex_nested_structure_roundtrip() {
    // Simulates a realistic input structure like:
    // {
    //   src = { url = "...", sha256 = "..." },
    //   features = ["a", "b"],
    //   debug = false,
    //   rust = <derivation ref>,
    // }
    let mut src = BTreeMap::new();
    src.insert(
      "url".to_string(),
      InputsRef::String("https://example.com/pkg.tar.gz".to_string()),
    );
    src.insert("sha256".to_string(), InputsRef::String("abc123".to_string()));

    let features = InputsRef::Array(vec![
      InputsRef::String("feature_a".to_string()),
      InputsRef::String("feature_b".to_string()),
    ]);

    let mut outputs = HashMap::new();
    outputs.insert("out".to_string(), "/syslua/store/obj/rust-1.75.0-abc123".to_string());

    let rust_ref = BuildRef {
      name: "rust".to_string(),
      version: Some("1.75.0".to_string()),
      inputs: None,
      outputs,
      hash: BuildHash("abc123".to_string()),
    };

    let mut inputs = BTreeMap::new();
    inputs.insert("src".to_string(), InputsRef::Table(src));
    inputs.insert("features".to_string(), features);
    inputs.insert("debug".to_string(), InputsRef::Boolean(false));
    inputs.insert("rust".to_string(), InputsRef::Build(Box::new(rust_ref)));

    let value = InputsRef::Table(inputs);

    // Verify serialization roundtrip preserves all nested structure
    let json = serde_json::to_string(&value).unwrap();
    let deserialized: InputsRef = serde_json::from_str(&json).unwrap();
    assert_eq!(value, deserialized);
  }
}
