# Plan: `sys shell` Command

## Goal

Implement the `sys shell` command to spawn a subshell with the syslua-managed environment.

## Problem

After `sys apply`, users must source environment scripts manually or restart their shell. A `sys shell` command would provide immediate access to the managed environment.

## Architecture Reference

- [05-snapshots.md](../architecture/05-snapshots.md):297-303 - Shell uses shared (read) lock
- [09-platform.md](../architecture/09-platform.md):35-70 - Environment scripts

## Approach

1. Add `shell` subcommand to CLI
2. Detect user's preferred shell ($SHELL or default)
3. Source the appropriate environment script
4. Spawn interactive subshell with modified environment

## CLI Interface

```bash
sys shell                   # Start subshell with syslua environment
sys shell --shell zsh       # Use specific shell
sys shell -- command        # Run command in environment, then exit
```

## Implementation

```rust
// Pseudocode
fn cmd_shell(shell: Option<String>, command: Option<Vec<String>>) {
    let env_script = get_env_script_path()?;  // ~/.local/share/syslua/env.sh
    let shell = shell.unwrap_or_else(|| std::env::var("SHELL").unwrap_or("/bin/sh".into()));
    
    let mut cmd = Command::new(&shell);
    
    if let Some(command) = command {
        // Run command and exit
        cmd.arg("-c").arg(format!("source {} && {}", env_script, command.join(" ")));
    } else {
        // Interactive shell
        cmd.arg("-c").arg(format!("source {} && exec {}", env_script, shell));
    }
    
    cmd.exec();  // Replace current process
}
```

## Files to Create

| Path | Purpose |
|------|---------|
| `crates/cli/src/cmd/shell.rs` | CLI command implementation |

## Files to Modify

| Path | Changes |
|------|---------|
| `crates/cli/src/cmd/mod.rs` | Add shell module |
| `crates/cli/src/main.rs` | Add shell subcommand |

## Success Criteria

1. `sys shell` spawns subshell with syslua environment
2. Works with bash, zsh, fish, and PowerShell
3. `sys shell -- command` runs single command
4. Environment includes all managed packages in PATH
5. Works on Linux, macOS, and Windows

## Open Questions

- [ ] How to handle fish (different syntax for sourcing)?
- [ ] How to handle PowerShell on Windows?
- [ ] Should `sys shell` work without running `sys apply` first?
- [ ] How to indicate user is in a syslua shell (prompt modification)?
