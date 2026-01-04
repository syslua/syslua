# Agent Guide: syslua-lib :: bind

## OVERVIEW
Handles system side effects (symlinks, services, files) through reversible bind definitions.

## FILES
- `types.rs`: Defines core types like `BindSpec`, `BindDef`, and `BindInputsDef`.
- `execute.rs`: Orchestrates the execution of apply, destroy, update, and check logic.
- `lua.rs`: Implements `BindCtx` LuaUserData and conversion of Lua specs to Rust definitions.
- `state.rs`: Manages persistent `BindState` (`state.json`) to track applied system outputs.
- `store.rs`: Provides path resolution for bind-specific metadata within the store.
- `mod.rs`: Serves as the module entry point and provides high-level lifecycle documentation.

## KEY TYPES
- `BindSpec`: The Lua-side representation containing `LuaFunction` closures for lifecycle hooks.
- `BindDef`: The evaluated, serializable IR stored in manifests for reproducible execution.
- `BindState`: A persisted record of resolved output paths used for state tracking and cleanup.
- `BindInputsDef`: Resolved inputs including primitives, arrays, and build/bind object hashes.

## SPEC vs DEF
- **BindSpec**: Runtime-only representation. Closures allow dynamic logic during Lua evaluation.
- **BindDef**: Serializable representation. Closures are converted into static `Action` lists.
- **Duality**: This pattern enables complex configuration logic while ensuring the final side effects are recorded, serializable, and verifiable in the manifest.

## EXECUTION FLOW
1. **Apply**: Executes recorded `create` actions. Final output paths are saved to `BindState`.
2. **Destroy**: Reverses side effects by running `destroy` actions using saved `BindState` data.
3. **Update**: Optional hook for in-place updates when a stable `id` is provided. Has access to old outputs.
4. **Check**: Probes current system state for drift by executing `check` actions without modification.
5. **State Tracking**: Uses `ObjectHash` for content-addressed identity and journaling to enable rollbacks.
6. **Execution Context**: Leverages `BindCtx` (Lua) and `ActionCtx` (Rust) for platform-safe operations like `exec`.
