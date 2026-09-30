# Bake Agent Context

Bake tasks for discovering and installing context files shipped by Cargo dependencies. It is the Rust counterpart to the Ruby [`agent-context`](https://github.com/socketry/agent-context) gem.

## Setup

Add the task library to the unpublished `bake/` crate in your project, then link it once from that crate's `main.rs`:

```toml
[dependencies]
bake = { package = "socketry-bake", version = "0.2" }
bake-agent-context = "0.1"
```

```rust,ignore
use bake_agent_context as _;

fn main() -> bake::Result<()> {
    bake::Registry::discover()?.run()
}
```

Install context from all resolved dependencies and update `agents.md`:

```sh
cargo bake agent:context:install
```

Other tasks:

```sh
cargo bake agent:context:list
cargo bake agent:context:list --package socketry-executor
cargo bake agent:context:show --package socketry-executor --file getting-started
cargo bake agent:context:install --package socketry-executor
cargo bake agent:context:agents-md
```

Generated context files are written to `.agents/context/`. Add `.agents/` to `.gitignore` if you want to keep this generated cache out of version control. The generated `agents.md` is intended to be committed.

## Provide context from a crate

Add a top-level `context/` directory to the crate and ensure Cargo packages it. When using an explicit `include` list in `Cargo.toml`, include `context/**`:

```text
my-crate/
├── Cargo.toml
├── context/
│   └── getting-started.md
└── src/
```

Bake Agent Context uses the crate description from `Cargo.toml` and extracts each Markdown file's title and first sentence for the generated `agents.md` section. No separate index file or Markdown front matter is needed.

## Context

This crate includes guides for its users and for other crate authors:

- [Getting Started](context/getting-started.md) explains how to add and run the Bake tasks.
- [Using and Providing Context](context/usage.md) describes dependency context and how to publish context from a crate.
- [Agent Context Specification](context/specification.md) defines the language-agnostic context directory and installation conventions.

## Release process

This crate has its own version and release history. See [releasing.md](releasing.md).
