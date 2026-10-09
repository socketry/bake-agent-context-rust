# `bake-agent-context`

Bake tasks for discovering and installing context files shipped by Cargo dependencies. It is the Rust counterpart to the Ruby [`agent-context`](https://github.com/socketry/agent-context) gem.

## Setup

Add the task library to the unpublished `bake/` crate and regenerate its task links:

```sh
cargo bake --regenerate
cargo add --manifest-path bake/Cargo.toml bake-agent-context
cargo bake --regenerate
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

Prepare a release with `cargo bake cargo:version:patch` (or `minor`, `major`, or `bump --version X.Y.Z`), then run `cargo bake cargo:release` and open a pull request. After review and merge, GitHub Actions publishes the release when the configured `crates-io` environment approves it. Follow the shared [Releasing skill](https://github.com/socketry/socketry-project-rust/blob/main/context/releasing.md) for the standard process.

## Releases

<!-- bake-readme:releases:start -->

See [releases.md](releases.md) for the full release history.

### v0.3.3

- Use Cargo-selected dependency versions and generated task links in setup instructions.

### v0.3.2

- Adopt `socketry-project` 0.3.7 for shared project tasks and Markdown normalization.
- Require the aggregate test and coverage result for pull request merges.
- Refresh dependency examples and repository-owned agent guidance.

### v0.3.1

- Require `socketry-markdown` 0.2.0 or newer for safe inline Markdown serialization.

<!-- bake-readme:releases:end -->

## See Also

- [`bake`](https://github.com/socketry/bake-rust).
- [`socketry-project`](https://github.com/socketry/socketry-project-rust).

## Contributing

Please open an issue or pull request on [GitHub](https://github.com/socketry/bake-agent-context-rust).

### Agent Context

Run `cargo bake agent:context:install` to install shared context and skills. Read `.agents/context/index.md` to find relevant guides, follow `agents.md` if present, and apply skills under `.agents/skills/`. The installer preserves repository-owned `agents.md`; it does not create or regenerate that file.

The shared skill ownership index uses version-two JSON with ecosystem, package, and version fields. Cargo refreshes preserve gem-owned entries and reject conflicting ownership. Version-one Cargo ownership migrates when rewritten. See the portable specification for the shared format.
