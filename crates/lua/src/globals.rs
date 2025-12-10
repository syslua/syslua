//! Global functions and types exposed to Lua
//!
//! Core primitives only:
//! - derive{} - Create derivations (build recipes)
//! - activate{} - Create activations (make derivations visible)
//!
//! Note: file{}, env{}, user{} are Lua-level helpers that create
//! derivations/activations - they are NOT separate manifest entries.

use mlua::{Function, Lua, RegistryKey, Result, Table, UserData, UserDataMethods, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::debug;

/// A derivation specification collected from Lua
///
/// This stores the derivation metadata and a reference to the config function.
/// The config function is stored separately in the runtime and called during
/// realization with a DerivationCtx.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Derivation {
    pub name: String,
    pub version: Option<String>,
    pub outputs: Vec<String>,
    /// Computed hash of the derivation (name + version + opts + config)
    pub hash: String,
    /// Resolved opts (after calling opts function if dynamic)
    pub opts: HashMap<String, OptsValue>,
    /// Index into the config function registry (not serialized)
    #[serde(skip)]
    pub config_index: usize,
}

/// Values that can appear in opts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptsValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Array(Vec<OptsValue>),
    Table(HashMap<String, OptsValue>),
}

/// An activation specification collected from Lua
///
/// Activations describe what to do with derivation outputs:
/// - Add to PATH
/// - Create symlinks
/// - Source shell scripts
/// - Run commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activation {
    /// Computed hash of the activation
    pub hash: String,
    /// Resolved opts (after calling opts function if dynamic)
    pub opts: HashMap<String, OptsValue>,
    /// Index into the config function registry (not serialized)
    #[serde(skip)]
    pub config_index: usize,
}

/// Actions collected during activation config execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivationAction {
    /// Add a directory to PATH
    AddToPath { bin_path: String },
    /// Create a symlink
    Symlink {
        source: String,
        target: String,
        mutable: bool,
    },
    /// Source a script in shell initialization
    SourceInShell { script: String, shells: Vec<String> },
    /// Run a command (escape hatch)
    Run { cmd: String },
}

/// Shell types for source_in_shell
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

/// Collector for all declarations made during Lua evaluation
#[derive(Default)]
pub struct Collector {
    pub derivations: Vec<Derivation>,
    pub activations: Vec<Activation>,
    /// Store derivation config functions via registry keys (mlua Functions can't be serialized)
    pub derivation_config_functions: Vec<RegistryKey>,
    /// Store activation config functions via registry keys
    pub activation_config_functions: Vec<RegistryKey>,
}

impl std::fmt::Debug for Collector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collector")
            .field("derivations", &self.derivations)
            .field("activations", &self.activations)
            .field(
                "derivation_config_count",
                &self.derivation_config_functions.len(),
            )
            .field(
                "activation_config_count",
                &self.activation_config_functions.len(),
            )
            .finish()
    }
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Callback type for fetch_url operation
pub type FetchUrlFn =
    Box<dyn Fn(&str, &PathBuf, Option<&str>) -> std::result::Result<(), String> + Send + Sync>;

/// Callback type for unpack_archive operation
pub type UnpackArchiveFn =
    Box<dyn Fn(&PathBuf, &PathBuf) -> std::result::Result<(), String> + Send + Sync>;

/// DerivationCtx - passed to config function during realization
///
/// Provides:
/// - ctx.out - output directory path
/// - ctx.sys - system information table (added separately)
/// - ctx.fetch_url(url, sha256) - download and verify file
/// - ctx.unpack(archive, dest?) - extract archive
/// - ctx.mkdir(path) - create directory
/// - ctx.copy(src, dst) - copy file/directory
/// - ctx.write(path, content) - write string to file
pub struct DerivationCtx {
    /// Output directory for this derivation
    pub out: PathBuf,
    /// Download cache directory
    pub cache_dir: PathBuf,
    /// Platform information
    pub platform: sys_platform::Platform,
    pub hostname: String,
    pub username: String,
    /// Callback for fetching URLs
    fetch_url_fn: Arc<FetchUrlFn>,
    /// Callback for unpacking archives
    unpack_archive_fn: Arc<UnpackArchiveFn>,
}

impl DerivationCtx {
    pub fn new(
        out: PathBuf,
        cache_dir: PathBuf,
        fetch_url_fn: FetchUrlFn,
        unpack_archive_fn: UnpackArchiveFn,
    ) -> Self {
        let platform_info = sys_platform::PlatformInfo::current();
        Self {
            out,
            cache_dir,
            platform: platform_info.platform,
            hostname: platform_info.hostname,
            username: platform_info.username,
            fetch_url_fn: Arc::new(fetch_url_fn),
            unpack_archive_fn: Arc::new(unpack_archive_fn),
        }
    }
}

impl Clone for DerivationCtx {
    fn clone(&self) -> Self {
        Self {
            out: self.out.clone(),
            cache_dir: self.cache_dir.clone(),
            platform: self.platform,
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            fetch_url_fn: Arc::clone(&self.fetch_url_fn),
            unpack_archive_fn: Arc::clone(&self.unpack_archive_fn),
        }
    }
}

impl UserData for DerivationCtx {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        // ctx.out - the output directory
        fields.add_field_method_get("out", |_, this| Ok(this.out.to_string_lossy().to_string()));

        // ctx.sys - system information table
        fields.add_field_method_get("sys", |lua, this| {
            let sys = lua.create_table()?;
            sys.set("platform", this.platform.to_string())?;
            sys.set("os", this.platform.os.to_string())?;
            sys.set("arch", this.platform.arch.to_string())?;
            sys.set("hostname", this.hostname.clone())?;
            sys.set("username", this.username.clone())?;
            sys.set("is_darwin", this.platform.os == sys_platform::Os::Darwin)?;
            sys.set("is_linux", this.platform.os == sys_platform::Os::Linux)?;
            sys.set("is_windows", this.platform.os == sys_platform::Os::Windows)?;
            Ok(sys)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // ctx.fetch_url(url, sha256) -> path
        methods.add_method("fetch_url", |_, this, (url, sha256): (String, String)| {
            let filename = url.rsplit('/').next().unwrap_or("download");
            let dest = this.cache_dir.join(filename);

            // Skip if already cached
            if !dest.exists() {
                debug!("Fetching {} -> {}", url, dest.display());
                (this.fetch_url_fn)(&url, &dest, Some(&sha256))
                    .map_err(|e| mlua::Error::RuntimeError(format!("fetch_url failed: {}", e)))?;
            }

            Ok(dest.to_string_lossy().to_string())
        });

        // ctx.unpack(archive, dest?) - dest defaults to ctx.out
        methods.add_method(
            "unpack",
            |_, this, (archive, dest): (String, Option<String>)| {
                let archive_path = PathBuf::from(&archive);
                let dest_path = dest.map(PathBuf::from).unwrap_or_else(|| this.out.clone());

                debug!("Unpacking {} -> {}", archive, dest_path.display());
                (this.unpack_archive_fn)(&archive_path, &dest_path)
                    .map_err(|e| mlua::Error::RuntimeError(format!("unpack failed: {}", e)))?;

                Ok(())
            },
        );

        // ctx.mkdir(path)
        methods.add_method("mkdir", |_, _, path: String| {
            std::fs::create_dir_all(&path)
                .map_err(|e| mlua::Error::RuntimeError(format!("mkdir failed: {}", e)))?;
            Ok(())
        });

        // ctx.copy(src, dst)
        methods.add_method("copy", |_, _, (src, dst): (String, String)| {
            let src_path = PathBuf::from(&src);
            let dst_path = PathBuf::from(&dst);

            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dst_path)
                    .map_err(|e| mlua::Error::RuntimeError(format!("copy failed: {}", e)))?;
            } else {
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| mlua::Error::RuntimeError(format!("copy failed: {}", e)))?;
                }
                std::fs::copy(&src_path, &dst_path)
                    .map_err(|e| mlua::Error::RuntimeError(format!("copy failed: {}", e)))?;
            }
            Ok(())
        });

        // ctx.write(path, content)
        methods.add_method("write", |_, _, (path, content): (String, String)| {
            let path = PathBuf::from(&path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| mlua::Error::RuntimeError(format!("write failed: {}", e)))?;
            }
            std::fs::write(&path, content)
                .map_err(|e| mlua::Error::RuntimeError(format!("write failed: {}", e)))?;
            Ok(())
        });

        // ctx.symlink(target, link)
        #[cfg(unix)]
        methods.add_method("symlink", |_, _, (target, link): (String, String)| {
            let link_path = PathBuf::from(&link);
            if let Some(parent) = link_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| mlua::Error::RuntimeError(format!("symlink failed: {}", e)))?;
            }
            // Remove existing symlink if present
            if link_path.exists() || link_path.is_symlink() {
                std::fs::remove_file(&link_path).ok();
            }
            std::os::unix::fs::symlink(&target, &link_path)
                .map_err(|e| mlua::Error::RuntimeError(format!("symlink failed: {}", e)))?;
            Ok(())
        });

        // ctx.chmod(path, mode) - Unix only
        #[cfg(unix)]
        methods.add_method("chmod", |_, _, (path, mode): (String, u32)| {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(mode);
            std::fs::set_permissions(&path, perms)
                .map_err(|e| mlua::Error::RuntimeError(format!("chmod failed: {}", e)))?;
            Ok(())
        });
    }
}

/// Helper to recursively copy a directory
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// ActivationCtx - passed to activation config function during apply
///
/// Provides:
/// - ctx.sys - system information table
/// - ctx:add_to_path(bin_path) - add directory to PATH
/// - ctx:symlink(source, target, opts?) - create symlink
/// - ctx:source_in_shell(script, opts?) - source script in shell init
/// - ctx:run(cmd) - escape hatch for arbitrary commands
pub struct ActivationCtx {
    /// Platform information
    pub platform: sys_platform::Platform,
    pub hostname: String,
    pub username: String,
    /// Collected actions (filled during config execution)
    pub actions: Arc<Mutex<Vec<ActivationAction>>>,
    /// Store root for resolving paths
    pub store_root: PathBuf,
}

impl ActivationCtx {
    pub fn new(store_root: PathBuf) -> Self {
        let platform_info = sys_platform::PlatformInfo::current();
        Self {
            platform: platform_info.platform,
            hostname: platform_info.hostname,
            username: platform_info.username,
            actions: Arc::new(Mutex::new(Vec::new())),
            store_root,
        }
    }

    /// Get the collected actions after config function execution
    pub fn take_actions(&self) -> Vec<ActivationAction> {
        let mut actions = self.actions.lock().unwrap();
        std::mem::take(&mut *actions)
    }
}

impl Clone for ActivationCtx {
    fn clone(&self) -> Self {
        Self {
            platform: self.platform,
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            actions: Arc::clone(&self.actions),
            store_root: self.store_root.clone(),
        }
    }
}

impl UserData for ActivationCtx {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        // ctx.sys - system information table
        fields.add_field_method_get("sys", |lua, this| {
            let sys = lua.create_table()?;
            sys.set("platform", this.platform.to_string())?;
            sys.set("os", this.platform.os.to_string())?;
            sys.set("arch", this.platform.arch.to_string())?;
            sys.set("hostname", this.hostname.clone())?;
            sys.set("username", this.username.clone())?;
            sys.set("is_darwin", this.platform.os == sys_platform::Os::Darwin)?;
            sys.set("is_linux", this.platform.os == sys_platform::Os::Linux)?;
            sys.set("is_windows", this.platform.os == sys_platform::Os::Windows)?;
            Ok(sys)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // ctx:add_to_path(bin_path)
        methods.add_method("add_to_path", |_, this, bin_path: String| {
            debug!("add_to_path: {}", bin_path);
            let mut actions = this.actions.lock().unwrap();
            actions.push(ActivationAction::AddToPath { bin_path });
            Ok(())
        });

        // ctx:symlink(source, target, opts?)
        methods.add_method(
            "symlink",
            |_, this, (source, target, opts): (String, String, Option<Table>)| {
                let mutable = opts
                    .and_then(|t| t.get::<bool>("mutable").ok())
                    .unwrap_or(false);
                debug!("symlink: {} -> {} (mutable={})", source, target, mutable);
                let mut actions = this.actions.lock().unwrap();
                actions.push(ActivationAction::Symlink {
                    source,
                    target,
                    mutable,
                });
                Ok(())
            },
        );

        // ctx:source_in_shell(script, opts?)
        methods.add_method(
            "source_in_shell",
            |_, this, (script, opts): (String, Option<Table>)| {
                let shells = if let Some(opts_table) = opts {
                    if let Ok(shells_table) = opts_table.get::<Table>("shells") {
                        shells_table
                            .pairs::<i64, String>()
                            .filter_map(|pair| pair.ok().map(|(_, shell)| shell))
                            .collect()
                    } else {
                        vec!["bash".to_string(), "zsh".to_string(), "fish".to_string()]
                    }
                } else {
                    vec!["bash".to_string(), "zsh".to_string(), "fish".to_string()]
                };
                debug!("source_in_shell: {} for {:?}", script, shells);
                let mut actions = this.actions.lock().unwrap();
                actions.push(ActivationAction::SourceInShell { script, shells });
                Ok(())
            },
        );

        // ctx:run(cmd) - escape hatch
        methods.add_method("run", |_, this, cmd: String| {
            debug!("run: {}", cmd);
            let mut actions = this.actions.lock().unwrap();
            actions.push(ActivationAction::Run { cmd });
            Ok(())
        });
    }
}

/// Compute a short hash (first 12 characters) for use in paths
fn short_hash(full_hash: &str) -> &str {
    &full_hash[..12.min(full_hash.len())]
}

/// Register all global functions in the Lua state
///
/// The store_root is used to compute derivation output paths (.out).
/// If not provided, a default user-level path is used.
pub fn register_globals(
    lua: &Lua,
    collector: Arc<Mutex<Collector>>,
    store_root: Option<PathBuf>,
) -> Result<()> {
    let globals = lua.globals();

    // Determine store root - use provided or detect
    let store_root = store_root.unwrap_or_else(|| sys_platform::SysluaPaths::detect().store.root);

    // derive {} - Create a derivation
    let derive_collector = Arc::clone(&collector);
    let derive_store_root = store_root.clone();
    let derive_fn = lua.create_function(move |lua, spec: Table| {
        let deriv = parse_derivation(lua, &spec, &derive_collector)?;
        debug!(
            "derive: {} v{:?} hash={}",
            deriv.name, deriv.version, deriv.hash
        );

        // Compute the store path for .out
        let version = deriv.version.as_deref().unwrap_or("latest");
        let short = short_hash(&deriv.hash);
        let out_path = derive_store_root
            .join("obj")
            .join(format!("{}-{}-{}", deriv.name, version, short));
        let out_str = out_path.to_string_lossy().to_string();

        // Return a table representing the derivation (can be passed to activate)
        let result = lua.create_table()?;
        result.set("name", deriv.name.clone())?;
        if let Some(v) = &deriv.version {
            result.set("version", v.clone())?;
        }
        result.set("hash", deriv.hash.clone())?;
        result.set("out", out_str.clone())?;

        // outputs table: { out = "<path>" }
        let outputs_table = lua.create_table()?;
        outputs_table.set("out", out_str)?;
        result.set("outputs", outputs_table)?;

        // Store the derivation
        let mut collector = derive_collector.lock().unwrap();
        collector.derivations.push(deriv);

        Ok(result)
    })?;
    globals.set("derive", derive_fn)?;

    // activate {} - Create an activation
    let activate_collector = Arc::clone(&collector);
    let activate_fn = lua.create_function(move |lua, spec: Table| {
        let activation = parse_activation(lua, &spec, &activate_collector)?;
        debug!("activate: hash={}", activation.hash);

        let mut collector = activate_collector.lock().unwrap();
        collector.activations.push(activation);
        Ok(())
    })?;
    globals.set("activate", activate_fn)?;

    Ok(())
}

fn parse_derivation(
    lua: &Lua,
    spec: &Table,
    collector: &Arc<Mutex<Collector>>,
) -> Result<Derivation> {
    let name: String = spec
        .get("name")
        .map_err(|_| mlua::Error::RuntimeError("derive: 'name' is required".to_string()))?;

    let version: Option<String> = spec.get("version").ok();

    // Parse outputs (defaults to ["out"])
    let outputs: Vec<String> = match spec.get::<Value>("outputs") {
        Ok(Value::Table(t)) => {
            let mut outs = Vec::new();
            for pair in t.pairs::<i64, String>() {
                let (_, v) = pair?;
                outs.push(v);
            }
            if outs.is_empty() {
                vec!["out".to_string()]
            } else {
                outs
            }
        }
        _ => vec!["out".to_string()],
    };

    // Parse opts - can be a table or a function(sys)
    let opts = parse_opts(lua, spec)?;

    // Get config function (REQUIRED per architecture doc)
    let config_fn: Function = spec.get("config").map_err(|_| {
        mlua::Error::RuntimeError("derive: 'config' function is required".to_string())
    })?;

    // Store the config function in the registry and get its index
    let config_index = {
        let registry_key = lua.create_registry_value(config_fn)?;
        let mut coll = collector.lock().unwrap();
        let idx = coll.derivation_config_functions.len();
        coll.derivation_config_functions.push(registry_key);
        idx
    };

    // Compute hash from name + version + opts
    // In a real implementation, we'd also hash the config function source
    let hash = compute_derivation_hash(&name, &version, &opts);

    Ok(Derivation {
        name,
        version,
        outputs,
        hash,
        opts,
        config_index,
    })
}

/// Parse opts from spec - can be static table or function(sys)
fn parse_opts(lua: &Lua, spec: &Table) -> Result<HashMap<String, OptsValue>> {
    match spec.get::<Value>("opts") {
        Ok(Value::Table(t)) => table_to_opts(&t),
        Ok(Value::Function(f)) => {
            // Call opts(sys) to get the resolved options
            let sys = create_sys_table(lua)?;
            let result: Table = f.call(sys)?;
            table_to_opts(&result)
        }
        Ok(Value::Nil) => Ok(HashMap::new()),
        _ => Ok(HashMap::new()),
    }
}

/// Create the sys table passed to opts(sys)
fn create_sys_table(lua: &Lua) -> Result<Table> {
    let platform_info = sys_platform::PlatformInfo::current();
    let sys = lua.create_table()?;
    sys.set("platform", platform_info.platform.to_string())?;
    sys.set("os", platform_info.platform.os.to_string())?;
    sys.set("arch", platform_info.platform.arch.to_string())?;
    sys.set("hostname", platform_info.hostname)?;
    sys.set("username", platform_info.username)?;
    sys.set(
        "is_darwin",
        platform_info.platform.os == sys_platform::Os::Darwin,
    )?;
    sys.set(
        "is_linux",
        platform_info.platform.os == sys_platform::Os::Linux,
    )?;
    sys.set(
        "is_windows",
        platform_info.platform.os == sys_platform::Os::Windows,
    )?;
    Ok(sys)
}

/// Convert a Lua table to HashMap<String, OptsValue>
fn table_to_opts(table: &Table) -> Result<HashMap<String, OptsValue>> {
    let mut opts = HashMap::new();
    for pair in table.clone().pairs::<String, Value>() {
        let (k, v) = pair?;
        if let Some(val) = value_to_opts_value(&v)? {
            opts.insert(k, val);
        }
    }
    Ok(opts)
}

/// Convert a Lua Value to OptsValue
fn value_to_opts_value(value: &Value) -> Result<Option<OptsValue>> {
    match value {
        Value::Nil => Ok(None),
        Value::Boolean(b) => Ok(Some(OptsValue::Boolean(*b))),
        Value::Integer(n) => Ok(Some(OptsValue::Number(*n as f64))),
        Value::Number(n) => Ok(Some(OptsValue::Number(*n))),
        Value::String(s) => Ok(Some(OptsValue::String(s.to_str()?.to_string()))),
        Value::Table(t) => {
            // Check if it's an array (sequential integer keys starting at 1)
            let first_key: mlua::Result<i64> = t
                .clone()
                .pairs::<i64, Value>()
                .next()
                .map_or(Err(mlua::Error::RuntimeError("empty".to_string())), |r| {
                    r.map(|(k, _)| k)
                });

            if first_key.is_ok() {
                // It's an array
                let mut arr = Vec::new();
                for pair in t.clone().pairs::<i64, Value>() {
                    let (_, v) = pair?;
                    if let Some(val) = value_to_opts_value(&v)? {
                        arr.push(val);
                    }
                }
                Ok(Some(OptsValue::Array(arr)))
            } else {
                // It's a table/map
                let mut map = HashMap::new();
                for pair in t.clone().pairs::<String, Value>() {
                    let (k, v) = pair?;
                    if let Some(val) = value_to_opts_value(&v)? {
                        map.insert(k, val);
                    }
                }
                Ok(Some(OptsValue::Table(map)))
            }
        }
        _ => Ok(None), // Skip functions, userdata, etc.
    }
}

/// Compute a hash for the derivation
fn compute_derivation_hash(
    name: &str,
    version: &Option<String>,
    opts: &HashMap<String, OptsValue>,
) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    if let Some(v) = version {
        hasher.update(v.as_bytes());
    }

    // Include opts in hash (sorted for determinism)
    let mut keys: Vec<_> = opts.keys().collect();
    keys.sort();
    for key in keys {
        hasher.update(key.as_bytes());
        if let Some(val) = opts.get(key) {
            hash_opts_value(&mut hasher, val);
        }
    }

    let result = hasher.finalize();
    hex::encode(&result[..16]) // Use first 16 bytes (32 hex chars)
}

fn hash_opts_value(hasher: &mut sha2::Sha256, value: &OptsValue) {
    use sha2::Digest;
    match value {
        OptsValue::String(s) => hasher.update(s.as_bytes()),
        OptsValue::Number(n) => hasher.update(n.to_le_bytes()),
        OptsValue::Boolean(b) => hasher.update([*b as u8]),
        OptsValue::Array(arr) => {
            for v in arr {
                hash_opts_value(hasher, v);
            }
        }
        OptsValue::Table(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for key in keys {
                hasher.update(key.as_bytes());
                if let Some(v) = map.get(key) {
                    hash_opts_value(hasher, v);
                }
            }
        }
    }
}

fn parse_activation(
    lua: &Lua,
    spec: &Table,
    collector: &Arc<Mutex<Collector>>,
) -> Result<Activation> {
    // Parse opts - can be a table or a function(sys)
    let opts = parse_opts(lua, spec)?;

    // Get config function (REQUIRED per architecture doc)
    let config_fn: Function = spec.get("config").map_err(|_| {
        mlua::Error::RuntimeError("activate: 'config' function is required".to_string())
    })?;

    // Store the config function in the registry and get its index
    let config_index = {
        let registry_key = lua.create_registry_value(config_fn)?;
        let mut coll = collector.lock().unwrap();
        let idx = coll.activation_config_functions.len();
        coll.activation_config_functions.push(registry_key);
        idx
    };

    // Compute hash from opts
    let hash = compute_activation_hash(&opts);

    Ok(Activation {
        hash,
        opts,
        config_index,
    })
}

/// Compute a hash for the activation
fn compute_activation_hash(opts: &HashMap<String, OptsValue>) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(b"activation");

    // Include opts in hash (sorted for determinism)
    let mut keys: Vec<_> = opts.keys().collect();
    keys.sort();
    for key in keys {
        hasher.update(key.as_bytes());
        if let Some(val) = opts.get(key) {
            hash_opts_value(&mut hasher, val);
        }
    }

    let result = hasher.finalize();
    hex::encode(&result[..16]) // Use first 16 bytes (32 hex chars)
}

/// Convert opts HashMap back to a Lua table for passing to config function
pub fn opts_to_lua_table(lua: &Lua, opts: &HashMap<String, OptsValue>) -> Result<Table> {
    let table = lua.create_table()?;
    for (k, v) in opts {
        table.set(k.as_str(), opts_value_to_lua(lua, v)?)?;
    }
    Ok(table)
}

fn opts_value_to_lua(lua: &Lua, value: &OptsValue) -> Result<Value> {
    match value {
        OptsValue::String(s) => Ok(Value::String(lua.create_string(s)?)),
        OptsValue::Number(n) => Ok(Value::Number(*n)),
        OptsValue::Boolean(b) => Ok(Value::Boolean(*b)),
        OptsValue::Array(arr) => {
            let t = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                t.set(i + 1, opts_value_to_lua(lua, v)?)?;
            }
            Ok(Value::Table(t))
        }
        OptsValue::Table(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.set(k.as_str(), opts_value_to_lua(lua, v)?)?;
            }
            Ok(Value::Table(t))
        }
    }
}
