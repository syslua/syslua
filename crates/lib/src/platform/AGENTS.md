# Platform Abstraction

**Generated:** 2026-01-04

## OVERVIEW
Cross-platform abstractions for OS-specific operations, treating Windows as a first-class citizen.

## FILES
- `mod.rs`: Entry point; platform detection and elevation checks.
- `os.rs`: `Os` enum (Linux, MacOs, Windows) with triple string mapping.
- `arch.rs`: `Arch` enum (X86_64, Aarch64) for CPU architecture.
- `paths.rs`: OS-specific path conventions (config, data, cache, store).
- `immutable.rs`: Store object write-protection via permissions/flags.

## KEY TYPES
- `Os`: Runtime OS detection and string identifiers.
- `Arch`: Runtime CPU architecture detection.
- `Platform`: Composite of Os/Arch forming a platform triple.
- `ImmutableError`: Errors during file protection operations.

## USAGE
- **Mandatory Abstraction**: Use this module instead of direct OS APIs or `std::env::consts`.
- **Elevation**: Use `is_elevated()` for root/admin permission checks.
- **Paths**: Access system-standard directories via `paths.rs` functions.
- **Immutability**: Call `make_immutable()` after builds to protect store content.

## UNSAFE BLOCKS
1. **macOS chflags** (`immutable.rs`): Clears BSD flags via `libc::chflags` for GC.
2. **Windows token check** (`mod.rs`): Queries process token for admin status.
3. **Windows OVERLAPPED** (`store_lock.rs`): Used for file locking; zero-initialized struct safety.
