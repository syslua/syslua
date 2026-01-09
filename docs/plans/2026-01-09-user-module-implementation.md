# syslua.user Module Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the `syslua.user` module for declarative cross-platform user management with per-user syslua config execution.

**Architecture:** Two-phase implementation: (1) Rust changes to support `SYSLUA_PARENT_STORE` for store layering with symlink/junction deduplication, (2) Lua module using existing build/bind primitives for user lifecycle management.

**Tech Stack:** Rust (paths, store lookup, cross-platform linking), Lua (module API, platform-specific commands via `ctx:exec`)

---

## Phase 1: Rust - SYSLUA_PARENT_STORE Support

### Task 1: Add parent_store_dir() to paths.rs

**Files:**

- Modify: `crates/lib/src/platform/paths.rs:108-124`
- Test: `crates/lib/src/platform/paths.rs` (inline tests)

**Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `paths.rs`:

```rust
#[test]
#[serial]
fn parent_store_dir_returns_none_when_unset() {
  temp_env::with_vars([("SYSLUA_PARENT_STORE", None::<&str>)], || {
    assert!(parent_store_dir().is_none());
  });
}

#[test]
#[serial]
fn parent_store_dir_returns_path_when_set() {
  temp_env::with_vars([("SYSLUA_PARENT_STORE", Some("/parent/store"))], || {
    assert_eq!(parent_store_dir(), Some(PathBuf::from("/parent/store")));
  });
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p syslua-lib parent_store_dir -- --nocapture`
Expected: FAIL with "cannot find function `parent_store_dir`"

**Step 3: Write minimal implementation**

Add after `store_dir()` function in `paths.rs`:

```rust
/// Returns the parent/fallback store directory for read-only lookups.
/// Used for store layering where user stores fall back to system store.
pub fn parent_store_dir() -> Option<PathBuf> {
  std::env::var("SYSLUA_PARENT_STORE")
    .map(PathBuf::from)
    .ok()
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p syslua-lib parent_store_dir -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/lib/src/platform/paths.rs
git commit -m "feat(platform): add parent_store_dir() for SYSLUA_PARENT_STORE env var"
```

---

### Task 2: Add cross-platform link_dir helper

**Files:**

- Create: `crates/lib/src/platform/link.rs`
- Modify: `crates/lib/src/platform/mod.rs`

**Step 1: Create the module file with tests**

Create `crates/lib/src/platform/link.rs`:

```rust
//! Cross-platform directory linking.
//!
//! Provides symlink creation on Unix and symlink-with-junction-fallback on Windows.

use std::io;
use std::path::Path;

/// Creates a symbolic link (or junction on Windows) from `dst` pointing to `src`.
///
/// On Windows, tries symlink_dir first (requires Developer Mode), falls back to junction.
/// On Unix, creates a standard symlink.
#[cfg(windows)]
pub fn link_dir(src: &Path, dst: &Path) -> io::Result<()> {
  // Ensure parent directory exists
  if let Some(parent) = dst.parent() {
    std::fs::create_dir_all(parent)?;
  }

  // Try symlink first (requires Developer Mode or admin)
  if std::os::windows::fs::symlink_dir(src, dst).is_ok() {
    return Ok(());
  }

  // Fall back to junction (always works for directories)
  junction::create(src, dst)
}

#[cfg(not(windows))]
pub fn link_dir(src: &Path, dst: &Path) -> io::Result<()> {
  // Ensure parent directory exists
  if let Some(parent) = dst.parent() {
    std::fs::create_dir_all(parent)?;
  }

  std::os::unix::fs::symlink(src, dst)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[test]
  fn link_dir_creates_symlink() {
    let temp = tempdir().unwrap();
    let src = temp.path().join("source");
    let dst = temp.path().join("dest");

    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("file.txt"), "content").unwrap();

    link_dir(&src, &dst).unwrap();

    assert!(dst.exists());
    assert!(dst.join("file.txt").exists());
  }

  #[test]
  fn link_dir_creates_parent_directories() {
    let temp = tempdir().unwrap();
    let src = temp.path().join("source");
    let dst = temp.path().join("nested").join("path").join("dest");

    std::fs::create_dir(&src).unwrap();

    link_dir(&src, &dst).unwrap();

    assert!(dst.exists());
  }
}
```

**Step 2: Add junction crate dependency (Windows only)**

Check if `junction` is already a dependency. If not, add to `crates/lib/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
junction = "1"
```

**Step 3: Add module to platform/mod.rs**

Find `mod.rs` and add:

```rust
pub mod link;
```

**Step 4: Run tests**

Run: `cargo test -p syslua-lib link_dir -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/lib/src/platform/link.rs crates/lib/src/platform/mod.rs crates/lib/Cargo.toml
git commit -m "feat(platform): add cross-platform link_dir helper"
```

---

### Task 3: Modify build_dir_path to support parent store fallback

**Files:**

- Modify: `crates/lib/src/build/store.rs`

**Step 1: Write the failing test**

Add to the test module in `store.rs`:

```rust
#[test]
#[serial]
fn build_dir_path_falls_back_to_parent_store() {
  let temp = tempfile::tempdir().unwrap();
  let parent_store = temp.path().join("parent");
  let user_store = temp.path().join("user");

  // Create build in parent store only
  let hash = ObjectHash("abc123def45678901234".to_string());
  let parent_build = parent_store.join("build").join(&hash.0);
  std::fs::create_dir_all(&parent_build).unwrap();
  std::fs::write(parent_build.join("marker.txt"), "exists").unwrap();

  temp_env::with_vars(
    [
      ("SYSLUA_STORE", Some(user_store.to_str().unwrap())),
      ("SYSLUA_PARENT_STORE", Some(parent_store.to_str().unwrap())),
      ("SYSLUA_ROOT", None::<&str>),
    ],
    || {
      let path = build_dir_path(&hash);

      // Should return path in user store (symlinked from parent)
      assert!(path.starts_with(&user_store));

      // The symlink should exist and point to parent content
      assert!(path.join("marker.txt").exists());
    },
  );
}

#[test]
#[serial]
fn build_dir_path_prefers_primary_store() {
  let temp = tempfile::tempdir().unwrap();
  let parent_store = temp.path().join("parent");
  let user_store = temp.path().join("user");

  let hash = ObjectHash("abc123def45678901234".to_string());

  // Create build in BOTH stores
  let parent_build = parent_store.join("build").join(&hash.0);
  std::fs::create_dir_all(&parent_build).unwrap();
  std::fs::write(parent_build.join("marker.txt"), "parent").unwrap();

  let user_build = user_store.join("build").join(&hash.0);
  std::fs::create_dir_all(&user_build).unwrap();
  std::fs::write(user_build.join("marker.txt"), "user").unwrap();

  temp_env::with_vars(
    [
      ("SYSLUA_STORE", Some(user_store.to_str().unwrap())),
      ("SYSLUA_PARENT_STORE", Some(parent_store.to_str().unwrap())),
      ("SYSLUA_ROOT", None::<&str>),
    ],
    || {
      let path = build_dir_path(&hash);

      // Should return user store path (primary)
      assert!(path.starts_with(&user_store));

      // Should contain user content, not parent
      let content = std::fs::read_to_string(path.join("marker.txt")).unwrap();
      assert_eq!(content, "user");
    },
  );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p syslua-lib build_dir_path_falls_back -- --nocapture`
Expected: FAIL (symlink not created, or path wrong)

**Step 3: Modify build_dir_path implementation**

Replace the `build_dir_path` function:

```rust
use crate::platform::link::link_dir;
use crate::platform::paths::{parent_store_dir, store_dir};
use tracing::warn;

pub fn build_dir_path(hash: &ObjectHash) -> PathBuf {
  let dir_name = build_dir_name(hash);
  let primary = store_dir().join("build").join(&dir_name);

  // If exists in primary store, use it
  if primary.exists() {
    return primary;
  }

  // Check parent store for fallback
  if let Some(parent) = parent_store_dir() {
    let fallback = parent.join("build").join(&dir_name);
    if fallback.exists() {
      // Create symlink in primary store pointing to parent
      if let Err(e) = link_dir(&fallback, &primary) {
        warn!(hash = %hash.0, error = %e, "Failed to link from parent store, using direct path");
        return fallback;
      }
      return primary;
    }
  }

  // Return primary path even if doesn't exist (for new builds)
  primary
}
```

**Step 4: Add imports at top of file**

```rust
use crate::platform::link::link_dir;
use crate::platform::paths::{parent_store_dir, store_dir};
use tracing::warn;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p syslua-lib build_dir_path -- --nocapture`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/lib/src/build/store.rs
git commit -m "feat(build): add parent store fallback with symlink deduplication"
```

---

### Task 4: Run full test suite and fix any regressions

**Step 1: Run all library tests**

Run: `cargo test -p syslua-lib`
Expected: All tests PASS

**Step 2: Run clippy**

Run: `cargo clippy -p syslua-lib --all-targets --all-features`
Expected: No errors (warnings OK)

**Step 3: Run formatter**

Run: `cargo fmt`

**Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: address test regressions from parent store changes"
```

---

## Phase 2: Lua - syslua.user Module

### Task 5: Create module skeleton with type definitions

**Files:**

- Create: `lua/syslua/user.lua`

**Step 1: Create the module with types and empty setup**

```lua
---@class syslua.user
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.user.Options
---@field description? string User description/comment
---@field homeDir string Home directory path (required)
---@field config string Path to user's syslua config (required)
---@field shell? BuildRef Login shell package
---@field initialPassword? string Initial password (plaintext, set on creation only)
---@field groups? string[] Groups to add user to (must exist)
---@field preserveHomeOnRemove? boolean Keep home directory when user is removed (default: false)

---@alias syslua.user.UserMap table<string, syslua.user.Options>

-- ============================================================================
-- Constants
-- ============================================================================

local BIND_ID_PREFIX = '__syslua_user_'

-- ============================================================================
-- Validation
-- ============================================================================

---@param name string
---@param opts syslua.user.Options
local function validate_user_options(name, opts)
  if not opts.homeDir then
    error(string.format("user '%s': homeDir is required", name), 0)
  end
  if not opts.config then
    error(string.format("user '%s': config is required", name), 0)
  end
  if not sys.is_elevated then
    error("syslua.user requires elevated privileges (root/Administrator)", 0)
  end
end

-- ============================================================================
-- Public API
-- ============================================================================

---Set up users according to the provided definitions
---@param users syslua.user.UserMap
function M.setup(users)
  if not users or next(users) == nil then
    error("syslua.user.setup: at least one user definition is required", 2)
  end

  for name, opts in pairs(users) do
    validate_user_options(name, opts)
    -- TODO: Create bind for user
  end
end

return M
```

**Step 2: Verify module loads**

Create a test fixture `crates/lib/tests/fixtures/user/user_basic.lua`:

```lua
local user = require('syslua.user')
-- Just verify it loads without error
```

**Step 3: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add module skeleton with type definitions"
```

---

### Task 6: Add platform-specific user creation commands

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Add helper functions for platform detection and command building**

Add after the constants section:

```lua
-- ============================================================================
-- Platform-Specific Commands
-- ============================================================================

---Get the default shell for the current platform
---@return string
local function get_default_shell()
  if sys.os == 'windows' then
    return 'cmd.exe'
  elseif sys.os == 'darwin' then
    return '/bin/zsh'
  else
    return '/bin/bash'
  end
end

---Get shell path from BuildRef or use default
---@param shell? BuildRef
---@return string
local function get_shell_path(shell)
  if shell and shell.outputs and shell.outputs.bin then
    return shell.outputs.bin
  end
  return get_default_shell()
end

---Build Linux user creation command
---@param name string
---@param opts syslua.user.Options
---@return string bin, string[] args
local function linux_create_user_cmd(name, opts)
  local args = { '-m', '-d', opts.homeDir }

  if opts.description and opts.description ~= '' then
    table.insert(args, '-c')
    table.insert(args, opts.description)
  end

  local shell = get_shell_path(opts.shell)
  table.insert(args, '-s')
  table.insert(args, shell)

  if opts.groups and #opts.groups > 0 then
    table.insert(args, '-G')
    table.insert(args, table.concat(opts.groups, ','))
  end

  table.insert(args, name)

  return '/usr/sbin/useradd', args
end

---Build macOS user creation command
---@param name string
---@param opts syslua.user.Options
---@return string bin, string[] args
local function darwin_create_user_cmd(name, opts)
  local args = { '-addUser', name }

  if opts.description and opts.description ~= '' then
    table.insert(args, '-fullName')
    table.insert(args, opts.description)
  end

  table.insert(args, '-home')
  table.insert(args, opts.homeDir)

  local shell = get_shell_path(opts.shell)
  table.insert(args, '-shell')
  table.insert(args, shell)

  if opts.initialPassword then
    table.insert(args, '-password')
    table.insert(args, opts.initialPassword)
  end

  return '/usr/sbin/sysadminctl', args
end

---Build Windows user creation PowerShell script
---@param name string
---@param opts syslua.user.Options
---@return string
local function windows_create_user_script(name, opts)
  local lines = {}

  -- Create user
  if opts.initialPassword then
    table.insert(lines, string.format(
      '$securePass = ConvertTo-SecureString "%s" -AsPlainText -Force',
      opts.initialPassword
    ))
    table.insert(lines, string.format(
      'New-LocalUser -Name "%s" -Description "%s" -Password $securePass',
      name,
      opts.description or ''
    ))
  else
    table.insert(lines, string.format(
      'New-LocalUser -Name "%s" -Description "%s" -NoPassword',
      name,
      opts.description or ''
    ))
  end

  -- Create home directory
  table.insert(lines, string.format(
    'New-Item -ItemType Directory -Path "%s" -Force | Out-Null',
    opts.homeDir
  ))

  -- Add to groups
  if opts.groups then
    for _, group in ipairs(opts.groups) do
      table.insert(lines, string.format(
        'Add-LocalGroupMember -Group "%s" -Member "%s" -ErrorAction Stop',
        group,
        name
      ))
    end
  end

  return table.concat(lines, '; ')
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add platform-specific user creation commands"
```

---

### Task 7: Add platform-specific user destruction commands

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Add destruction command helpers**

Add after the creation commands:

```lua
---Build Linux user deletion command
---@param name string
---@param preserve_home boolean
---@return string bin, string[] args
local function linux_delete_user_cmd(name, preserve_home)
  local args = {}
  if not preserve_home then
    table.insert(args, '-r')
  end
  table.insert(args, name)
  return '/usr/sbin/userdel', args
end

---Build macOS user deletion command
---@param name string
---@param preserve_home boolean
---@return string bin, string[] args
local function darwin_delete_user_cmd(name, preserve_home)
  local args = { '-deleteUser', name }
  if preserve_home then
    table.insert(args, '-keepHome')
  else
    table.insert(args, '-secure')
  end
  return '/usr/sbin/sysadminctl', args
end

---Build Windows user deletion PowerShell script
---@param name string
---@param home_dir string
---@param preserve_home boolean
---@return string
local function windows_delete_user_script(name, home_dir, preserve_home)
  local lines = {
    string.format('Remove-LocalUser -Name "%s"', name),
  }
  if not preserve_home then
    table.insert(lines, string.format(
      'Remove-Item -Recurse -Force "%s" -ErrorAction SilentlyContinue',
      home_dir
    ))
  end
  return table.concat(lines, '; ')
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add platform-specific user destruction commands"
```

---

### Task 8: Add macOS group membership commands

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Add macOS group helpers**

macOS doesn't support `-G` in sysadminctl, so groups must be added separately:

```lua
---Add user to group on macOS
---@param username string
---@param group string
---@return string bin, string[] args
local function darwin_add_to_group_cmd(username, group)
  return '/usr/sbin/dseditgroup', { '-o', 'edit', '-a', username, '-t', 'user', group }
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add macOS group membership commands"
```

---

### Task 9: Add subprocess execution for user config

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Add helpers for running sys apply/destroy as user**

```lua
-- ============================================================================
-- User Config Execution
-- ============================================================================

local lib = require('syslua.lib')

---Get the store path for a user
---@param home_dir string
---@return string
local function get_user_store(home_dir)
  return home_dir .. '/.syslua/store'
end

---Get the parent store path (system store)
---@return string
local function get_parent_store()
  -- Use the current store as parent for user subprocesses
  local store = sys.getenv('SYSLUA_STORE')
  if store and store ~= '' then
    return store
  end
  -- Fallback to default system store
  if sys.os == 'windows' then
    local drive = sys.getenv('SYSTEMDRIVE') or 'C:'
    return drive .. '\\syslua\\store'
  else
    return '/syslua/store'
  end
end

---Resolve config path (file or directory with init.lua)
---@param config_path string
---@return string
local function resolve_config_path(config_path)
  -- If it's a directory, append init.lua
  -- The actual check happens at runtime in the bind
  if config_path:match('%.lua$') then
    return config_path
  else
    return config_path .. '/init.lua'
  end
end

---Build Unix command to run sys apply as user
---@param username string
---@param home_dir string
---@param config_path string
---@return string bin, string[] args
local function unix_run_as_user_cmd(username, home_dir, config_path)
  local user_store = get_user_store(home_dir)
  local parent_store = get_parent_store()
  local resolved_config = resolve_config_path(config_path)

  local env_prefix = string.format(
    'SYSLUA_STORE=%s SYSLUA_PARENT_STORE=%s',
    user_store,
    parent_store
  )

  local cmd = string.format('%s sys apply %s', env_prefix, resolved_config)

  return '/bin/su', { '-', username, '-c', cmd }
end

---Build Unix command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string bin, string[] args
local function unix_destroy_as_user_cmd(username, home_dir)
  local user_store = get_user_store(home_dir)
  local parent_store = get_parent_store()

  local env_prefix = string.format(
    'SYSLUA_STORE=%s SYSLUA_PARENT_STORE=%s',
    user_store,
    parent_store
  )

  local cmd = string.format('%s sys destroy', env_prefix)

  return '/bin/su', { '-', username, '-c', cmd }
end

---Build Windows command to run sys apply as user (via scheduled task)
---@param username string
---@param home_dir string
---@param config_path string
---@return string
local function windows_run_as_user_script(username, home_dir, config_path)
  local user_store = get_user_store(home_dir):gsub('/', '\\')
  local parent_store = get_parent_store():gsub('/', '\\')
  local resolved_config = resolve_config_path(config_path):gsub('/', '\\')

  return string.format([[
$env:SYSLUA_STORE = "%s"
$env:SYSLUA_PARENT_STORE = "%s"
$taskName = "SysluaApply_%s"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "apply %s"
$principal = New-ScheduledTaskPrincipal -UserId "%s" -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
]], user_store, parent_store, username, resolved_config, username)
end

---Build Windows command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string
local function windows_destroy_as_user_script(username, home_dir)
  local user_store = get_user_store(home_dir):gsub('/', '\\')
  local parent_store = get_parent_store():gsub('/', '\\')

  return string.format([[
$env:SYSLUA_STORE = "%s"
$env:SYSLUA_PARENT_STORE = "%s"
$taskName = "SysluaDestroy_%s"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "destroy"
$principal = New-ScheduledTaskPrincipal -UserId "%s" -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
]], user_store, parent_store, username, username)
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add subprocess execution for user config apply/destroy"
```

---

### Task 10: Implement the user bind creation

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Update setup() to create binds**

Replace the setup function and add the bind creation:

```lua
---Create a bind for a single user
---@param name string
---@param opts syslua.user.Options
local function create_user_bind(name, opts)
  local bind_id = BIND_ID_PREFIX .. name

  sys.bind({
    id = bind_id,
    replace = true,
    inputs = {
      username = name,
      description = opts.description or '',
      home_dir = opts.homeDir,
      config_path = opts.config,
      shell = opts.shell,
      initial_password = opts.initialPassword,
      groups = opts.groups or {},
      preserve_home = opts.preserveHomeOnRemove or false,
      os = sys.os,
    },
    create = function(inputs, ctx)
      -- Step 1: Create the user account
      if inputs.os == 'linux' then
        local bin, args = linux_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          groups = inputs.groups,
        })
        ctx:exec({ bin = bin, args = args })

        -- Set password separately on Linux
        if inputs.initial_password and inputs.initial_password ~= '' then
          ctx:exec({
            bin = '/bin/sh',
            args = { '-c', string.format(
              'echo "%s:%s" | chpasswd',
              inputs.username,
              inputs.initial_password
            )},
          })
        end

      elseif inputs.os == 'darwin' then
        local bin, args = darwin_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          initialPassword = inputs.initial_password,
        })
        ctx:exec({ bin = bin, args = args })

        -- Add to groups separately on macOS
        for _, group in ipairs(inputs.groups) do
          local grp_bin, grp_args = darwin_add_to_group_cmd(inputs.username, group)
          ctx:exec({ bin = grp_bin, args = grp_args })
        end

      elseif inputs.os == 'windows' then
        local script = windows_create_user_script(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          initialPassword = inputs.initial_password,
          groups = inputs.groups,
        })
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      end

      -- Step 2: Apply user's syslua config
      if inputs.os == 'windows' then
        local script = windows_run_as_user_script(
          inputs.username,
          inputs.home_dir,
          inputs.config_path
        )
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      else
        local bin, args = unix_run_as_user_cmd(
          inputs.username,
          inputs.home_dir,
          inputs.config_path
        )
        ctx:exec({ bin = bin, args = args })
      end

      return {
        username = inputs.username,
        home_dir = inputs.home_dir,
        preserve_home = inputs.preserve_home,
      }
    end,
    destroy = function(outputs, ctx)
      -- Step 1: Destroy user's syslua config
      if sys.os == 'windows' then
        local script = windows_destroy_as_user_script(
          outputs.username,
          outputs.home_dir
        )
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      else
        local bin, args = unix_destroy_as_user_cmd(
          outputs.username,
          outputs.home_dir
        )
        ctx:exec({ bin = bin, args = args })
      end

      -- Step 2: Remove user account
      if sys.os == 'linux' then
        local bin, args = linux_delete_user_cmd(outputs.username, outputs.preserve_home)
        ctx:exec({ bin = bin, args = args })

      elseif sys.os == 'darwin' then
        local bin, args = darwin_delete_user_cmd(outputs.username, outputs.preserve_home)
        ctx:exec({ bin = bin, args = args })

      elseif sys.os == 'windows' then
        local script = windows_delete_user_script(
          outputs.username,
          outputs.home_dir,
          outputs.preserve_home
        )
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      end
    end,
  })
end

---Set up users according to the provided definitions
---@param users syslua.user.UserMap
function M.setup(users)
  if not users or next(users) == nil then
    error("syslua.user.setup: at least one user definition is required", 2)
  end

  for name, opts in pairs(users) do
    validate_user_options(name, opts)
    create_user_bind(name, opts)
  end
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): implement user bind creation with full lifecycle"
```

---

### Task 11: Add user existence check for updates

**Files:**

- Modify: `lua/syslua/user.lua`

**Step 1: Add user existence detection**

Add after the platform commands section:

```lua
-- ============================================================================
-- User Existence Checks
-- ============================================================================

---Check if user exists on Linux
---@param username string
---@return string
local function linux_user_exists_check(username)
  return string.format('id "%s" >/dev/null 2>&1', username)
end

---Check if user exists on macOS
---@param username string
---@return string
local function darwin_user_exists_check(username)
  return string.format('dscl . -read /Users/%s >/dev/null 2>&1', username)
end

---Check if user exists on Windows (PowerShell)
---@param username string
---@return string
local function windows_user_exists_check(username)
  return string.format('if (-not (Get-LocalUser -Name "%s" -ErrorAction SilentlyContinue)) { exit 1 }', username)
end
```

**Step 2: Commit**

```bash
git add lua/syslua/user.lua
git commit -m "feat(user): add user existence checks"
```

---

### Task 12: Add module to syslua namespace

**Files:**

- Modify: `lua/syslua/init.lua`

**Step 1: Check current init.lua structure**

Read `lua/syslua/init.lua` and add `user` to the exports.

**Step 2: Add user module export**

Add `user` to the class definition and lazy loading if using that pattern, or direct require.

**Step 3: Commit**

```bash
git add lua/syslua/init.lua
git commit -m "feat(user): export module from syslua namespace"
```

---

### Task 13: Create integration test fixture

**Files:**

- Create: `crates/lib/tests/fixtures/user/user_basic.lua`

**Step 1: Create test fixture**

```lua
-- Test fixture for syslua.user module
-- Note: This requires elevated privileges and creates real users
-- Only run in isolated test environments

local user = require('syslua.user')

user.setup({
  testuser = {
    description = "Test User",
    homeDir = sys.os == 'windows' and 'C:\\Users\\testuser' or '/home/testuser',
    config = './test_user_config.lua',
    groups = {},
    preserveHomeOnRemove = true,
  },
})
```

**Step 2: Create minimal user config**

Create `crates/lib/tests/fixtures/user/test_user_config.lua`:

```lua
-- Minimal user config for testing
local env = require('syslua.environment')

env.variables.setup({
  TEST_USER_VAR = 'hello from user config',
})
```

**Step 3: Commit**

```bash
git add crates/lib/tests/fixtures/user/
git commit -m "test(user): add integration test fixtures"
```

---

### Task 14: Run full test suite and lint

**Step 1: Run Rust tests**

Run: `cargo test`
Expected: All PASS

**Step 2: Run clippy**

Run: `cargo clippy --all-targets --all-features`
Expected: No errors

**Step 3: Run formatter**

Run: `cargo fmt`

**Step 4: Commit any fixes**

```bash
git add -A
git commit -m "chore: format and fix lint issues"
```

---

### Task 15: Update AGENTS.md with new module info

**Files:**

- Modify: `AGENTS.md`

**Step 1: Add user module to code map**

Add entry to the appropriate section documenting the new module.

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs: add syslua.user module to AGENTS.md"
```

---

## Summary

**Phase 1 (Rust):** 4 tasks

- Add `parent_store_dir()` to paths.rs
- Add cross-platform `link_dir` helper
- Modify `build_dir_path` for parent store fallback
- Run tests and fix regressions

**Phase 2 (Lua):** 11 tasks

- Module skeleton with types
- Platform-specific creation commands
- Platform-specific destruction commands
- macOS group commands
- Subprocess execution helpers
- User bind implementation
- User existence checks
- Namespace export
- Test fixtures
- Lint and format
- Documentation update

**Total:** 15 tasks

---

Plan complete and saved to `docs/plans/2026-01-09-user-module-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
