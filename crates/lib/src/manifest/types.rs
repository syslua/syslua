use serde::{Deserialize, Serialize};

use crate::{bind::BindDef, build::BuildDef};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
  pub builds: Vec<BuildDef>,
  pub bindings: Vec<BindDef>,
}
