# Repository Conventions

- Keep shared Rust guidance in `context/rust.md`; this file is for conventions specific to this repository.
- Keep the public API and Cargo dependency discovery in `src/agent/context/`.
- Keep task names under `agent:context` and link this crate from the private `bake/` executable.
- Keep package usage, provider instructions, and the language-agnostic specification in `context/`.
- Keep release notes and license updates composed through the local `cargo:after_version_bump` task.
- The `bake/` package is private; publish only `bake-agent-context`.
