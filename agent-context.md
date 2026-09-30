# Agent context

Read `conventions.md` before changing this repository.

## Purpose

`bake-agent-context` provides Bake tasks and a Rust API for discovering context directories in resolved Cargo dependencies, copying them into `.agents/context/`, and maintaining an `agents.md` index.

## Important behavior

- Context providers place files in a top-level `context/` directory and include it in the published crate archive.
- Installed context is grouped by crate name beneath `.agents/context/`.
- Workspace members are skipped by default; resolved dependency packages are scanned.
- The default generated index is `agents.md`, preserving the behavior of the Ruby `agent-context` gem.
- Task names keep the Ruby-compatible `agent:context:*` namespace.
- `cargo metadata` is run from the project root and cached within a Bake task chain.

## Verification

Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`.
