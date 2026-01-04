# Module: syslua_lib::build

**Generated:** 2026-01-04 | **Scope:** Build definitions and realization

## OVERVIEW
Immutable, content-addressed artifacts produced by executing a sequence of actions.

## FILES
- `types.rs`: Core types (Spec, Def, Ref, Inputs)
- `execute.rs`: Realization logic, caching, and completion markers
- `lua.rs`: Lua bindings for `sys.build{}` and `BuildCtx` userdata
- `store.rs`: Path resolution for `<store>/build/<hash>/`

## KEY TYPES
- `BuildSpec`: Lua-side input with `create` closure (non-serializable).
- `BuildDef`: Fully evaluated, serializable definition used for hashing.
- `BuildRef`: Handle returned to Lua containing hash and output placeholders.
- `BuildCtx`: Userdata wrapping `ActionCtx` for recording actions in Lua.

## SPEC vs DEF
- **Spec**: Ephemeral, Lua-resident; contains the "how-to" logic.
- **Def**: Persistent, manifest-resident; contains the recorded "result" data.
- Evaluation of `Spec.create` records actions and outputs into a `Def`.

## HASHING
- `ObjectHash`: 20-char truncated SHA256 of the `BuildDef` JSON.
- Content-addressed: Identical definitions yield same store path.
- Identity: Hash covers inputs, recorded actions, and named output keys.

## STORE STRUCTURE
- Path: `<store>/build/<hash>/`
- Output: Root directory is always `$${out}`.
- Marker: `.syslua-complete` stores JSON with full output directory hash.
