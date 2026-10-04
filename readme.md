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

Install context and skills from all resolved dependencies and update the generated context index:

```sh
cargo bake agent:context:install
cargo bake agent:context:index
```

Other tasks:

```sh
cargo bake agent:context:list
cargo bake agent:context:list --package socketry-executor
cargo bake agent:context:show --package socketry-executor --file getting-started
cargo bake agent:context:install --package socketry-executor
cargo bake agent:context:skill:list
cargo bake agent:context:skill:install
cargo bake agent:context:skill:install --package socketry-executor --skill socketry-executor-initial-gem-setup
```

`cargo bake agent:context:install` writes `.agents/context/index.md` and maintains a marked section in the local Git exclude file (`.git/info/exclude`) for generated context, the skill registry, and dependency-installed skill directories. These exclusions stay in the checkout and are not committed. Project-owned skill directories remain trackable. The task does not create or modify the repository owner's `agents.md`; a project can add a stable link to the generated index if desired.

Context documents marked with `type: skill` are installed only under `.agents/skills/`, along with their companion resources. Their installed names are prefixed with the provider crate name, and they are omitted from `.agents/context/` and its generated index. Use `agent:context:install --package CRATE` to install both context and skills from one provider. The separate skill tasks are useful for listing skills or installing a selected skill. Project-owned skills can live in `.agents/skills/` and remain version controlled.

## Provide context from a crate

Add a top-level `context/` directory to the crate and ensure Cargo packages it. When using an explicit `include` list in `Cargo.toml`, include `context/**`:

```text
my-crate/
├── Cargo.toml
├── context/
│   └── getting-started.md
└── src/
```

Bake Agent Context uses the crate description from `Cargo.toml` and extracts each Markdown file's title and first sentence for `.agents/context/index.md`. Ordinary context files need no Markdown front matter. A context document can opt into skill installation with YAML front matter; see [Using and Providing Context](context/usage.md), which is installed as the `bake-agent-context-usage` skill.

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
when the configured `crates-io` environment approves it. Follow the shared
[Releasing skill](https://github.com/socketry/socketry-project-rust/blob/main/context/releasing.md)
for the standard process.

## Releases

<!-- bake-readme:releases:start -->
See [releases.md](releases.md) for the full release history.

### v0.2.2

- Declare compatibility with the Bake 0.x API so task libraries can share one task registry
  when upgrading to crate-derived task namespaces.

### v0.2.1

- Use the shared `socketry-project` Releasing skill for the standard release
  process and remove references to the duplicate Bake Cargo publishing context.

### v0.2.0

- Write the context index to `.agents/context/index.md` without creating or
  modifying the repository owner's `agents.md`.
- Install the usage guide as the `bake-agent-context-usage` skill.
- Keep generated context and dependency-provided skills out of Git using local
  excludes, while allowing projects to track their own skills.
<!-- bake-readme:releases:end -->

## Contributing

Please open an issue or pull request on [GitHub](https://github.com/socketry/bake-agent-context-rust).

### Agent Context

Before contributing, follow `agents.md` if present, then read relevant guides linked from `.agents/context/index.md` and apply any matching skills. If the index or context files are missing or out of date, run `cargo bake agent:context:install` to refresh them.
