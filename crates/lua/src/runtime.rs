//! Lua runtime for sys.lua configuration evaluation
//!
//! Implements two-phase evaluation:
//! 1. Load config, extract M.inputs
//! 2. Resolve inputs, call M.setup(inputs)

use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

use crate::error::{Error, Result};
use crate::globals::{self, opts_to_lua_table, Collector, DerivationCtx};
use crate::manifest::Manifest;

/// The Lua runtime environment
pub struct Runtime {
    lua: Lua,
    collector: Arc<Mutex<Collector>>,
}

impl Runtime {
    /// Create a new Lua runtime with all globals registered
    pub fn new() -> Result<Self> {
        Self::with_store_root(None)
    }

    /// Create a new Lua runtime with a specific store root
    ///
    /// The store root is used to compute derivation output paths (.out).
    /// If None, the default detected path is used.
    pub fn with_store_root(store_root: Option<std::path::PathBuf>) -> Result<Self> {
        let lua = Lua::new();
        let collector = Arc::new(Mutex::new(Collector::new()));

        // Register global functions (derive, activate)
        globals::register_globals(&lua, Arc::clone(&collector), store_root)?;

        // Register syslua table with system information
        Self::register_syslua_table(&lua)?;

        // Set up package.path for local modules
        Self::setup_package_path(&lua)?;

        Ok(Self { lua, collector })
    }

    /// Register the global `syslua` table with system information
    fn register_syslua_table(lua: &Lua) -> LuaResult<()> {
        let platform_info = sys_platform::PlatformInfo::current();
        let globals = lua.globals();

        let syslua = lua.create_table()?;

        // Platform information
        syslua.set("platform", platform_info.platform.to_string())?;
        syslua.set("os", platform_info.platform.os.to_string())?;
        syslua.set("arch", platform_info.platform.arch.to_string())?;
        syslua.set("hostname", platform_info.hostname)?;
        syslua.set("username", platform_info.username)?;

        // Convenience booleans
        syslua.set(
            "is_linux",
            platform_info.platform.os == sys_platform::Os::Linux,
        )?;
        syslua.set(
            "is_darwin",
            platform_info.platform.os == sys_platform::Os::Darwin,
        )?;
        syslua.set(
            "is_windows",
            platform_info.platform.os == sys_platform::Os::Windows,
        )?;

        // Version
        syslua.set("version", env!("CARGO_PKG_VERSION"))?;

        // Add lib subtable with utility functions
        let lib = lua.create_table()?;

        // toJSON function
        let to_json = lua.create_function(|lua, value: Value| {
            let json = value_to_json(lua, &value)?;
            Ok(json)
        })?;
        lib.set("toJSON", to_json)?;

        syslua.set("lib", lib)?;
        globals.set("syslua", syslua)?;

        debug!("Registered syslua table: {}", platform_info.platform);
        Ok(())
    }

    /// Set up package.path to include common locations
    fn setup_package_path(lua: &Lua) -> LuaResult<()> {
        let package: Table = lua.globals().get("package")?;
        let current_path: String = package.get("path")?;

        // Add common paths for local modules
        let new_path = format!(
            "{};./?.lua;./?/init.lua;lib/?.lua;lib/?/init.lua",
            current_path
        );
        package.set("path", new_path)?;

        Ok(())
    }

    /// Evaluate a Lua configuration file using two-phase evaluation
    ///
    /// Phase 1: Load the file, extract M.inputs
    /// Phase 2: Resolve inputs (TODO), call M.setup(inputs)
    pub fn evaluate_file(&self, path: &Path) -> Result<Manifest> {
        info!("Evaluating {}", path.display());

        // Add the config file's directory to package.path
        if let Some(parent) = path.parent() {
            self.add_to_package_path(parent)?;
        }

        // Load and execute the file
        let source = std::fs::read_to_string(path)?;
        let chunk = self.lua.load(&source).set_name(path.to_string_lossy());

        // Execute the chunk - it should return a table with M.inputs and M.setup
        let module: Table = chunk
            .eval()
            .map_err(|e| Error::Eval(format!("Failed to load {}: {}", path.display(), e)))?;

        // Phase 1: Extract M.inputs (optional)
        let inputs = self.extract_inputs(&module)?;
        debug!("Extracted {} inputs", inputs.len());

        // TODO: Resolve inputs (clone git repos, validate paths)
        // For now, we skip this and pass an empty table

        // Phase 2: Call M.setup(inputs)
        self.call_setup(&module, &inputs)?;

        // Build manifest from collected declarations
        let collector = self.collector.lock().unwrap();
        let manifest = Manifest {
            derivations: collector.derivations.clone(),
            activations: collector.activations.clone(),
        };

        info!("Evaluation complete: {}", manifest.summary());
        Ok(manifest)
    }

    /// Add a directory to the Lua package.path
    fn add_to_package_path(&self, dir: &Path) -> Result<()> {
        let package: Table = self.lua.globals().get("package")?;
        let current_path: String = package.get("path")?;

        let dir_str = dir.to_string_lossy();
        let new_path = format!("{}/?.lua;{}/?/init.lua;{}", dir_str, dir_str, current_path);
        package.set("path", new_path)?;

        debug!("Added to package.path: {}", dir_str);
        Ok(())
    }

    /// Extract M.inputs from the module table
    fn extract_inputs(&self, module: &Table) -> Result<HashMap<String, String>> {
        let mut inputs = HashMap::new();

        if let Ok(inputs_table) = module.get::<Table>("inputs") {
            for pair in inputs_table.pairs::<String, String>() {
                match pair {
                    Ok((k, v)) => {
                        inputs.insert(k, v);
                    }
                    Err(e) => {
                        debug!("Skipping non-string input: {}", e);
                    }
                }
            }
        }

        Ok(inputs)
    }

    /// Call M.setup(inputs) on the module
    fn call_setup(&self, module: &Table, inputs: &HashMap<String, String>) -> Result<()> {
        let setup: Function = module.get("setup").map_err(|_| {
            Error::InvalidEntryPoint(
                "Entry point must return a table with a 'setup' function".to_string(),
            )
        })?;

        // Create inputs table to pass to setup
        let inputs_table = self.lua.create_table()?;
        for (k, v) in inputs {
            inputs_table.set(k.as_str(), v.as_str())?;
        }

        // Call setup(inputs)
        setup
            .call::<()>(inputs_table)
            .map_err(|e| Error::Eval(format!("Error in setup(): {}", e)))?;

        Ok(())
    }

    /// Realize a derivation by calling its config function with a DerivationCtx
    ///
    /// This executes the build logic: downloading, unpacking, etc.
    pub fn realize_derivation(
        &self,
        derivation: &crate::Derivation,
        ctx: DerivationCtx,
    ) -> Result<()> {
        let collector = self.collector.lock().unwrap();

        // Get the config function registry key by index
        let registry_key = collector
            .derivation_config_functions
            .get(derivation.config_index)
            .ok_or_else(|| {
                Error::Eval(format!(
                    "Config function not found for derivation '{}'",
                    derivation.name
                ))
            })?;

        // Get the function from the registry
        let config_fn: Function = self.lua.registry_value(registry_key)?;

        // Convert opts to Lua table
        let opts_table = opts_to_lua_table(&self.lua, &derivation.opts)?;

        // Create the DerivationCtx userdata (sys table is provided via fields)
        let ctx_userdata = self.lua.create_userdata(ctx)?;

        // Call config(opts, ctx)
        drop(collector); // Release lock before calling Lua
        config_fn
            .call::<()>((opts_table, ctx_userdata))
            .map_err(|e| {
                Error::Eval(format!(
                    "Error in config() for '{}': {}",
                    derivation.name, e
                ))
            })?;

        Ok(())
    }

    /// Realize an activation by calling its config function with an ActivationCtx
    ///
    /// This executes the activation logic and returns the collected actions.
    pub fn realize_activation(
        &self,
        activation: &crate::Activation,
        ctx: crate::ActivationCtx,
    ) -> Result<Vec<crate::ActivationAction>> {
        let collector = self.collector.lock().unwrap();

        // Get the config function registry key by index
        let registry_key = collector
            .activation_config_functions
            .get(activation.config_index)
            .ok_or_else(|| {
                Error::Eval(format!(
                    "Config function not found for activation '{}'",
                    activation.hash
                ))
            })?;

        // Get the function from the registry
        let config_fn: Function = self.lua.registry_value(registry_key)?;

        // Convert opts to Lua table
        let opts_table = opts_to_lua_table(&self.lua, &activation.opts)?;

        // Create the ActivationCtx userdata
        let ctx_clone = ctx.clone();
        let ctx_userdata = self.lua.create_userdata(ctx)?;

        // Call config(opts, ctx)
        drop(collector); // Release lock before calling Lua
        config_fn
            .call::<()>((opts_table, ctx_userdata))
            .map_err(|e| {
                Error::Eval(format!(
                    "Error in activation config() for '{}': {}",
                    activation.hash, e
                ))
            })?;

        // Return collected actions
        Ok(ctx_clone.take_actions())
    }

    /// Get access to the raw Lua state (for advanced use cases)
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Get access to the collector (for testing or introspection)
    pub fn collector(&self) -> Arc<Mutex<Collector>> {
        Arc::clone(&self.collector)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new().expect("Failed to create Lua runtime")
    }
}

/// Convert a Lua value to JSON string
fn value_to_json(_lua: &Lua, value: &Value) -> LuaResult<String> {
    match value {
        Value::Nil => Ok("null".to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Integer(n) => Ok(n.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => {
            let str_val = s.to_str()?.to_string();
            Ok(serde_json::to_string(&str_val).unwrap_or_else(|_| format!("\"{}\"", str_val)))
        }
        Value::Table(t) => {
            // Check if it's an array (sequential integer keys starting at 1)
            let is_array = t.clone().pairs::<i64, Value>().all(|pair| pair.is_ok());

            if is_array && !t.is_empty() {
                let mut items = Vec::new();
                for pair in t.clone().pairs::<i64, Value>() {
                    let (_, v) = pair?;
                    items.push(value_to_json(_lua, &v)?);
                }
                Ok(format!("[{}]", items.join(",")))
            } else {
                let mut items = Vec::new();
                for pair in t.clone().pairs::<String, Value>() {
                    let (k, v) = pair?;
                    let key_json =
                        serde_json::to_string(&k).unwrap_or_else(|_| format!("\"{}\"", k));
                    items.push(format!("{}:{}", key_json, value_to_json(_lua, &v)?));
                }
                Ok(format!("{{{}}}", items.join(",")))
            }
        }
        _ => Err(mlua::Error::RuntimeError(
            "Cannot convert function/userdata/thread to JSON".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_runtime_creation() {
        let runtime = Runtime::new().unwrap();
        // Check that syslua table exists
        let syslua: Table = runtime.lua.globals().get("syslua").unwrap();
        let os: String = syslua.get("os").unwrap();
        assert!(!os.is_empty());
    }

    #[test]
    fn test_syslua_platform_info() {
        let runtime = Runtime::new().unwrap();
        let syslua: Table = runtime.lua.globals().get("syslua").unwrap();

        // Check platform fields exist
        let _platform: String = syslua.get("platform").unwrap();
        let _os: String = syslua.get("os").unwrap();
        let _arch: String = syslua.get("arch").unwrap();
        let _version: String = syslua.get("version").unwrap();
    }

    #[test]
    fn test_minimal_config() {
        let runtime = Runtime::new().unwrap();

        let config = r#"
            local M = {}
            function M.setup()
                -- Empty setup
            end
            return M
        "#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(config.as_bytes()).unwrap();
        file.flush().unwrap();

        let manifest = runtime.evaluate_file(file.path()).unwrap();
        assert!(manifest.is_empty());
    }

    #[test]
    fn test_derive_global() {
        let runtime = Runtime::new().unwrap();

        // New API requires opts + config pattern
        let config = r#"
            local M = {}
            function M.setup()
                derive {
                    name = "test-pkg",
                    version = "1.0.0",
                    opts = {
                        url = "https://example.com/test.tar.gz",
                        sha256 = "abc123",
                    },
                    config = function(opts, ctx)
                        -- Build logic goes here
                    end,
                }
            end
            return M
        "#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(config.as_bytes()).unwrap();
        file.flush().unwrap();

        let manifest = runtime.evaluate_file(file.path()).unwrap();
        assert_eq!(manifest.derivations.len(), 1);
        assert_eq!(manifest.derivations[0].name, "test-pkg");
        assert_eq!(manifest.derivations[0].version, Some("1.0.0".to_string()));
        // Check that opts were captured
        assert!(manifest.derivations[0].opts.contains_key("url"));
    }

    #[test]
    fn test_activate_global() {
        let runtime = Runtime::new().unwrap();

        // New API requires opts + config pattern
        let config = r#"
            local M = {}
            function M.setup()
                activate {
                    opts = {
                        package = "test-pkg",
                        bin_dir = "/opt/test/bin",
                    },
                    config = function(opts, ctx)
                        ctx:add_to_path(opts.bin_dir)
                    end,
                }
            end
            return M
        "#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(config.as_bytes()).unwrap();
        file.flush().unwrap();

        let manifest = runtime.evaluate_file(file.path()).unwrap();
        assert_eq!(manifest.activations.len(), 1);
        // New API: activation has a hash, not a derivation name
        assert!(!manifest.activations[0].hash.is_empty());
        assert!(manifest.activations[0].opts.contains_key("package"));
    }

    #[test]
    fn test_to_json() {
        let runtime = Runtime::new().unwrap();

        let result: String = runtime
            .lua
            .load(r#"return syslua.lib.toJSON({ name = "test", count = 42 })"#)
            .eval()
            .unwrap();

        assert!(result.contains("\"name\""));
        assert!(result.contains("\"test\""));
        assert!(result.contains("42"));
    }
}
