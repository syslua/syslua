# syslua.user Module Design

> **Date:** 2026-01-09

## Overview

The `syslua.user` module provides declarative user management for syslua. It enables system administrators to define users in their syslua config, with each user having their own user-scoped syslua configuration that is applied as a subprocess running as that user.

## Use Case

System-level user management where root/admin runs `sys apply` and it:

1. Creates users (if they don't exist)
2. Sets up their home directories
3. Runs their user-scoped syslua configs as those users
4. Removes users (and optionally their data) when removed from config

## API

```lua
local user = require('syslua.user')
local pkgs = require('syslua.pkgs')

user.setup({
  alice = {
    description = "Alice Developer",
    homeDir = "/home/alice",
    config = "/etc/syslua/users/alice",
    shell = pkgs.shells.zsh,
    initialPassword = "changeme123",
    groups = { "sudo", "docker" },
    preserveHomeOnRemove = true,
  },
  bob = {
    description = "Bob Admin",
    homeDir = "/home/bob",
    config = "/etc/syslua/users/bob.lua",
    shell = pkgs.shells.bash,
    initialPassword = "changeme456",
    groups = { "wheel" },
    preserveHomeOnRemove = false,
  },
})
```

## Options

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| (key) | string | yes | - | Username |
| `description` | string | no | `""` | User description/comment |
| `homeDir` | string | yes | - | Home directory path |
| `config` | string | yes | - | Path to user's syslua config (file or directory) |
| `shell` | BuildRef | no | system default | Login shell package from `syslua.pkgs` |
| `initialPassword` | string | no | none | Initial password (plaintext, set on creation only) |
| `groups` | string[] | no | `{}` | Groups to add user to (must exist) |
| `preserveHomeOnRemove` | boolean | no | `false` | Keep home directory when user is removed |

### Config Path

The `config` option accepts either:

- A file path (e.g., `/etc/syslua/users/alice.lua`)
- A directory path containing `init.lua` (e.g., `/etc/syslua/users/alice/`)

The module detects which form is provided and handles accordingly.

### Shell

The `shell` option is a `BuildRef` from a package (e.g., `pkgs.shells.zsh`). The module uses `shell.outputs.bin` as the login shell path. If not specified, the system default shell is used.

## Implementation

### Module Structure

Single file: `lua/syslua/user.lua`

The module creates one bind per user with ID `__syslua_user_<username>`.

### Bind Lifecycle

**Create:**

1. Validate options (config path exists, groups exist)
2. Create user account (platform-specific)
3. Set initial password if provided
4. Add user to specified groups
5. Run `sys apply` as that user with their config

**Destroy:**

1. Run `sys destroy` as that user (rollback their binds)
2. Remove user from groups (automatic on user deletion)
3. Remove user account
4. Optionally remove home directory based on `preserveHomeOnRemove`

### Platform-Specific User Creation

**Linux:**

```bash
useradd -m -d "/home/alice" -c "Alice Developer" -s "/path/to/shell" -G sudo,docker alice
echo "alice:<password>" | chpasswd
```

**macOS:**

```bash
sysadminctl -addUser alice -fullName "Alice Developer" \
  -home /Users/alice -shell /path/to/shell -password <password>
dseditgroup -o edit -a alice -t user sudo
dseditgroup -o edit -a alice -t user docker
```

**Windows:**

```powershell
$securePass = ConvertTo-SecureString "<password>" -AsPlainText -Force
New-LocalUser -Name "alice" -Description "Alice Developer" -Password $securePass
New-Item -ItemType Directory -Path "C:\Users\alice" -Force
Add-LocalGroupMember -Group "Administrators" -Member "alice"
# Shell is set via registry or profile
```

### Platform-Specific User Destruction

**Linux:**

```bash
# With home directory removal
userdel -r alice

# Preserving home directory
userdel alice
```

**macOS:**

```bash
# With home directory removal
sysadminctl -deleteUser alice -secure

# Preserving home directory
sysadminctl -deleteUser alice -keepHome
```

**Windows:**

```powershell
Remove-LocalUser -Name "alice"
# Optionally remove home directory
Remove-Item -Recurse -Force "C:\Users\alice"
```

### User Config Execution

The user's syslua config is executed as a subprocess:

**Unix:**

```bash
su - alice -c 'SYSLUA_STORE=~/.syslua/store SYSLUA_PARENT_STORE=/syslua/store sys apply /path/to/config'
```

**Windows:**
Uses scheduled task or `runas` approach to execute as the user with appropriate environment variables set.

**Destruction:**

```bash
su - alice -c 'SYSLUA_STORE=~/.syslua/store SYSLUA_PARENT_STORE=/syslua/store sys destroy'
```

## Rust Changes

### SYSLUA_PARENT_STORE Environment Variable

Add support for a fallback/parent store for read-only lookups.

**File:** `crates/lib/src/platform/paths.rs`

```rust
pub fn parent_store_dir() -> Option<PathBuf> {
  std::env::var("SYSLUA_PARENT_STORE")
    .map(PathBuf::from)
    .ok()
}
```

### Store Lookup with Fallback

When looking up a build by hash:

1. Check primary store (`SYSLUA_STORE`)
2. If not found, check parent store (`SYSLUA_PARENT_STORE`)
3. If found in parent, create symlink/junction in primary store
4. Return path in primary store

```rust
fn find_or_link_build(hash: &ObjectHash) -> Option<PathBuf> {
  let primary = store_dir().join("build").join(&hash.0);
  if primary.exists() {
    return Some(primary);
  }

  if let Some(parent) = parent_store_dir() {
    let fallback = parent.join("build").join(&hash.0);
    if fallback.exists() {
      // Create symlink/junction in primary store
      if let Err(e) = link_dir(&fallback, &primary) {
        warn!("Failed to link from parent store: {}", e);
        return Some(fallback); // Fall back to direct path
      }
      return Some(primary);
    }
  }

  None
}
```

### Cross-Platform Directory Linking

```rust
#[cfg(windows)]
fn link_dir(src: &Path, dst: &Path) -> io::Result<()> {
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
fn link_dir(src: &Path, dst: &Path) -> io::Result<()> {
  // Ensure parent directory exists
  if let Some(parent) = dst.parent() {
    std::fs::create_dir_all(parent)?;
  }

  std::os::unix::fs::symlink(src, dst)
}
```

### Benefits of SYSLUA_PARENT_STORE

- **Deduplication**: System packages are stored once, users reference them via symlinks
- **Isolation**: User writes go to their own store
- **Backwards compatible**: If not set, behaves exactly as before
- **Simple**: Single fallback location, not a search path

## Error Handling

### Fail-Fast Behavior

- User creation failure → bind fails → `sys apply` fails
- User's config apply failure → bind fails → `sys apply` fails
- Group doesn't exist → fail immediately with clear error
- Config path doesn't exist → fail immediately with clear error

### Edge Cases

| Scenario | Behavior |
|----------|----------|
| User already exists | Update if changed (description, groups, shell), skip if identical |
| Group doesn't exist | Fail with error message listing missing group |
| Config path invalid | Fail with error message |
| Home exists, user doesn't | Create user, reuse existing home directory |
| Not elevated | Fail immediately - user management requires admin |
| Shell package not built | Built first as dependency (standard build ordering) |
| Windows home directory | Explicitly created after user creation |

## Security Considerations

### Initial Password

- `initialPassword` is passed as plaintext in the Lua config
- Only set on user creation, not managed afterward
- Users should change their password after first login
- Future: SOPS integration for encrypted secrets

### Elevation Requirement

The module checks `sys.is_elevated` and fails immediately if false. User management requires:

- Linux/macOS: root
- Windows: Administrator

### Store Permissions

- System store (`/syslua/store`): Read access for all users
- User store (`~/.syslua/store`): Read/write for that user only
- Parent store symlinks allow users to reference system builds without write access

## Future Work

- **SOPS integration**: Encrypted secrets for passwords and sensitive config
- **Group module**: `syslua.groups` for creating/managing groups
- **Service accounts**: Users with login disabled for running services
- **SSH key management**: Authorized keys as part of user config

## Dependencies

- Existing `sys.build` and `sys.bind` primitives
- `sys.is_elevated` global
- `sys.os` global for platform detection
- Shell packages from `syslua.pkgs.shells` (to be implemented)
