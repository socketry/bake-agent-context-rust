# Releases

## v0.1.5

- Explain how to organize reusable package context and repository-only agent
  instructions.

## v0.1.4

- Install dependency-provided Agent Skills from context documents with YAML front matter.
- Install context and skills together with `agent:context:install`, keeping skill documents and resources out of the context index.
- Prefix installed skill names with their provider's Cargo package name.

## v0.1.3

- Parse context documents and update the generated index with the Markdown AST.

## v0.1.2

- Create or update GitHub Releases after successful crates.io publication.
- Resolve the local task crate during version updates.

## v0.1.1

- Switch the runtime dependency from `socketry-bake` to `bake` 0.17.0.

- Add shared Rust context for Socketry crate development.
- Keep project-owned `.agents/` guidance while ignoring generated `.agents/context/` files.

## v0.1.0

- Add Bake tasks to list, inspect, and install dependency context, then maintain a generated `agents.md` section with links and summaries.
- Include Rust usage guides and the language-agnostic Agent Context specification in the crate.
