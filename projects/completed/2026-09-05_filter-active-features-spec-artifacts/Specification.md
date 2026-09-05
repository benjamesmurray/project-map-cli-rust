# filter-active-features-spec-artifacts - Specification

Only report valid active features containing spec artifacts in status output

## Core Features & Scope

### In-Scope
- Implement a reusable helper function `get_active_features(root: &Path) -> Vec<String>` in `src/core/utils.rs`.
- Ensure `get_active_features` checks `projects/active` under the provided root directory (or relative path if root is empty/current directory) and filters entries:
  - Entry must be a directory.
  - Directory must contain at least one specification artifact: `Specification.md`, `Tasks.json`, or `.deliver_meta.json`.
  - Non-directories, empty directories, or directories lacking any of the specification artifacts must be excluded.
  - Return names in deterministic sorted order.
- Update `src/main.rs` status command handler to use `get_active_features`.
- Update `src/mcp/server.rs` status tool handler to use `get_active_features`.
- Add unit tests in `src/core/utils.rs` (and/or integration tests) validating:
  - Empty directories are ignored.
  - Directories without spec artifacts are ignored.
  - Directories with `Specification.md`, `Tasks.json`, or `.deliver_meta.json` are properly reported.
- Bump crate version in `Cargo.toml` (0.1.15 -> 0.1.16).
- Verify compilation and test suite passing (`cargo test` and `cargo build --release`).

### Out-of-Scope
- Modifying project archiving or lifecycle workflows in external tools (`deliver-cli`).
- Parsing the contents of the spec artifacts in status check; existence check of the artifact filenames is sufficient.

## Technical Constraints
- **Language/Runtime**: Rust (2021 edition).
- **Dependencies**: Standard library filesystem primitives (`std::fs`, `std::path::Path`).
- **Compatibility**: Must work across POSIX and Windows filesystem paths without panicking if directories do not exist.

## User Stories
- As an engineer or agent using `project-map` CLI or MCP `status`, I want empty category folders or left-over directories in `projects/active` to be omitted from the active features list so that only genuine, active feature specifications are reported.
  - [x] Empty feature folders in `projects/active` do not appear in `status`.
  - [x] Feature folders containing at least one of `Specification.md`, `Tasks.json`, or `.deliver_meta.json` appear in `status`.

## Overview & Architecture
`project-map` exposes workspace status both via CLI (`project-map status`) and MCP (`status` tool). Currently, both duplicate the logic of reading `projects/active` and checking `entry.file_type().is_dir()`.
By centralizing active feature discovery in `crate::core::utils::get_active_features(root: &Path) -> Vec<String>`:
1. Both `src/main.rs` and `src/mcp/server.rs` reuse the exact same verification logic.
2. The logic safely handles missing `projects/active` directories (returning an empty vector).
3. The existence of `Specification.md`, `Tasks.json`, or `.deliver_meta.json` inside the directory is checked before adding to the returned active feature list.
4. Results are sorted alphabetically for deterministic output.

## Technical Stack
- **Backend/Core**: Rust, `std::fs`, `std::path::Path`, `std::path::PathBuf`.
- **Testing**: Rust built-in test framework (`cargo test`), `tempfile` if needed or standard `std::env::temp_dir()`.

## Components and Interfaces
- `crate::core::utils::get_active_features(root: &Path) -> Vec<String>`
- `src/main.rs`: replace manual directory scan with `get_active_features(Path::new("."))`.
- `src/mcp/server.rs`: replace manual directory scan with `get_active_features(Path::new("."))`.

## Data Models
- Input: `root: &Path` representing repository root or relative base.
- Output: `Vec<String>` containing directory names of active features.