# Getting Started

This guide shows how to add Bake Agent Context tasks to a Rust project and install context from its Cargo dependencies.

## Add the Bake tasks

Bake tasks belong in your project's private `bake/` crate. Add the Bake task library and Bake Agent Context to `bake/Cargo.toml`:

```toml
[dependencies]
bake = "0.17"
bake-agent-context = "0.1"
```

Link the task library from `bake/src/main.rs` and run the discovered task registry:

```rust,ignore
use bake_agent_context as _;

fn main() -> bake::Result<()> {
    bake::Registry::discover()?.run()
}
```

## Install dependency context

Run this from the project root:

```sh
cargo bake agent:context:install
```

The task scans resolved Cargo dependencies for a top-level `context/` directory, copies ordinary context files into `.agents/context/`, installs skill-marked documents and their resources into `.agents/skills/`, and writes `.agents/context/index.md`. Skill-marked documents are not copied into the context directory or listed in its index. The task does not create or modify `agents.md`, which remains under the repository owner's control.

Use `cargo bake agent:context:list` to see which dependencies provide ordinary context. Install context and skills from one provider with `cargo bake agent:context:install --package CRATE`, or inspect one of its context files with `cargo bake agent:context:show --package CRATE --file getting-started`.

List skills provided by dependencies with `cargo bake agent:context:skill:list`. The main install task installs all of them; use `cargo bake agent:context:skill:install --package CRATE --skill CRATE-SKILL` to install a specific skill independently. The skill name is prefixed with the provider crate name.

## Generated files

The install task maintains a marked block in the repository's local Git exclude file (`.git/info/exclude`) for generated `.agents/context/` files, the skill ownership registry, and dependency-installed skill directories. These local exclusions are not committed. Project-owned skill directories remain trackable. If the project wants a top-level agent entrypoint, its owner can add a stable link to `.agents/context/index.md` in `agents.md`.
