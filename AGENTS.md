# Agent Guidelines for syslua

- Build: `cargo build` (workspace), `cargo build -p syslua-cli` for CLI.
- Test: `cargo test` (all), `cargo test -p <crate> <filter>` for a single test or module; append `-- --nocapture` when debugging.
- Lint/format: run `cargo fmt` and `cargo clippy --all-targets --all-features` before proposing non-trivial changes.
- Use Rust 2024 idioms; `snake_case` for functions/locals, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for consts; avoid one-letter names except for short loops.
- Imports: group `std`, then external crates, then internal modules; prefer explicit imports over glob (`*`) where reasonable.
- Error handling: use `Result` and existing error enums; propagate with `?`; prefer descriptive variants/messages over `.unwrap()`/`.expect()` except in clearly unreachable cases.
- Types: be explicit at public boundaries; prefer references (`&str`, slices) over owned types when clones are not required; keep manifests/DAG types consistent with [the architecture docs](./docs/architecture).
- Prefer extending existing module/option systems to adding ad-hoc flags or environment variables.
- For cross-platform behavior, rely on `syslua_core::platform` abstractions instead of OS-specific APIs where possible.
- Logging: use existing logging facilities and levels; keep messages actionable and avoid excessive default TRACE-level noise.
- Tests: favor fast, deterministic unit tests per crate; for integration flows, mimic `sys apply/plan` behavior with targeted cases rather than broad end-to-end scripts.
- There are currently no Cursor rules (`.cursor/rules/` or `.cursorrules`) or Copilot rules (`.github/copilot-instructions.md`); if they are added later, update this file to reference them.
- Reference [Architecture Docs](./docs/architecture) for high-level design principles and module interactions.
