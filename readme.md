# Bake Agent Context

Bake tasks for discovering and installing context files shipped by Cargo dependencies. It is the Rust counterpart to the Ruby [`agent-context`](https://github.com/socketry/agent-context) gem.

## Setup

Add the task library to the unpublished `bake/` crate in your project, then link it once from that crate's `main.rs`:

```toml
[dependencies]
bake = "0.17"
bake-agent-context = "0.1"
```

```rust,ignore
use bake_agent_context as _;

fn main() -> bake::Result<()> {
    bake::Registry::discover()?.run()
}
```

Install context and skills from all resolved dependencies and update `agents.md`:

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
cargo bake agent:context:skill:list
cargo bake agent:context:skill:install
cargo bake agent:context:skill:install --package socketry-executor --skill socketry-executor-initial-gem-setup
```

`cargo bake agent:context:install` maintains a marked section in the local Git exclude file (`.git/info/exclude`) for generated context, the skill registry, and dependency-installed skill directories. These exclusions stay in the checkout and are not committed. Project-owned skill directories remain trackable. The generated `agents.md` is intended to be committed.

Context documents marked with `type: skill` are installed only under `.agents/skills/`, along with their companion resources. Their installed names are prefixed with the provider crate name, and they are omitted from `.agents/context/` and the generated `agents.md` index. Use `agent:context:install --package CRATE` to install both context and skills from one provider. The separate skill tasks are useful for listing skills or installing a selected skill. Project-owned skills can live in `.agents/skills/` and remain version controlled.

## Provide context from a crate

Add a top-level `context/` directory to the crate and ensure Cargo packages it. When using an explicit `include` list in `Cargo.toml`, include `context/**`:

```text
my-crate/
├── Cargo.toml
├── context/
│   └── getting-started.md
└── src/
```

Bake Agent Context uses the crate description from `Cargo.toml` and extracts each Markdown file's title and first sentence for the generated `agents.md` section. Ordinary context files need no Markdown front matter. A context document can opt into skill installation with YAML front matter; see [Using and Providing Context](context/usage.md).

## Context

This crate includes guides for its users and for other crate authors:

- [Getting Started](context/getting-started.md) explains how to add and run the Bake tasks.
- [Using and Providing Context](context/usage.md) describes dependency context and how to publish context from a crate.
- [Agent Context](context/agent-context.md) explains how to organize package guidance and repository-only instructions.
- [Agent Context Specification](context/specification.md) defines the language-agnostic context directory and installation conventions.
- [Rust Context](context/rust.md) provides shared development guidance for Socketry's Rust crates.

## Releasing

Prepare a release with `cargo bake cargo:version:patch` (or `minor`, `major`,
or `bump --version X.Y.Z`), then run `cargo bake cargo:release` and open a
pull request. After review and merge, GitHub Actions publishes the release
when the configured `crates-io` environment approves it. See the
[Cargo publishing guide](https://github.com/socketry/bake-cargo-rust/blob/main/context/publishing.md).

## Contributing

Please open an issue or pull request on [GitHub](https://github.com/socketry/bake-agent-context-rust).

### Agent Context

Before contributing, read `agents.md` and the relevant context files it links. If `agents.md` is missing or out of date, run `cargo bake agent:context:install` to install context from dependencies and update the index.
