# Development

Read `.agents/conventions.md` before changing this repository.

## Purpose

`bake-agent-context` provides Bake tasks and a Rust API for discovering context directories in resolved Cargo dependencies, copying them into `.agents/context/`, and generating an index at `.agents/context/index.md`.

## Important behavior

- Context providers place files in a top-level `context/` directory and include it in the published crate archive.
- Ordinary context is grouped by crate name beneath `.agents/context/`; skill-marked documents and their assets are installed only under `.agents/skills/`.
- Workspace members are skipped by default; resolved dependency packages are scanned.
- The generated `.agents/context/index.md` contains links to context Markdown files, using each file's first heading as its title and its first sentence as its summary. Installation leaves the repository owner's `agents.md` untouched.
- A YAML front matter `description`, when present, takes precedence over the first sentence; `type: skill` opts a root-level document into skill installation and excludes it from context copying and indexing.
- Dependency skills install into `.agents/skills/` with names prefixed by their Cargo package name; an ownership registry prevents overwriting project-owned skills.
- Task names keep the Ruby-compatible `agent:context:*` namespace.
- `cargo metadata` is run from the project root and cached within a Bake task chain.

## Verification

Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`.
