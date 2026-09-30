# Getting Started

This guide shows how to add Bake Agent Context tasks to a Rust project and install context from its Cargo dependencies.

## Add the Bake tasks

Bake tasks belong in your project's private `bake/` crate. Add the Bake task library and Bake Agent Context to `bake/Cargo.toml`:

```toml
[dependencies]
bake = { package = "socketry-bake", version = "0.2" }
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

The task scans resolved Cargo dependencies for a top-level `context/` directory, copies each provider's files into `.agents/context/`, and updates the Context section of `agents.md`.

Use `cargo bake agent:context:list` to see which dependencies provide context. Install one provider with `cargo bake agent:context:install --package CRATE`, or inspect one of its files with `cargo bake agent:context:show --package CRATE --file getting-started`.

## Generated files

`.agents/context/` is a generated cache and can be excluded from version control. The lower-case `agents.md` file is the project-facing guide index; review it and commit it with the project.
