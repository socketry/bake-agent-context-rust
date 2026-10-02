# Agent

## Context

This section links to documentation from installed packages. It is automatically generated and can be refreshed with `cargo bake agent:context:install`.

**Before working on a package, read the relevant context files below. They contain package-specific guidance and workflows.**

If these files are missing or dependencies have changed, run `cargo bake agent:context:install` to install them.

### bake

Composable, typed development tasks for Rust projects

#### [Design](.agents/context/bake/design.md)

The cargo-bake executable asks Cargo for workspace metadata, identifies an
unpublished task binary, and runs it.

#### [Development Context](.agents/context/bake/development.md)

This guide summarizes the implementation and task composition in the Bake
repository.

#### [Structuring Bake Tasks in Crates](.agents/context/bake/task-libraries.md)

Bake has two common places for task functions:

### bake-cargo

Cargo project and release automation tasks for Bake

#### [Cargo Publishing](.agents/context/bake-cargo/publishing.md)

Use bake-cargo to prepare and publish Cargo projects through reviewed GitHub
Actions releases.

### bake-readme

Reusable readme.md maintenance tasks for Bake

#### [Readme Structure](.agents/context/bake-readme/readme-structure.md)

Use this guidance when creating or updating a project's readme.md.

### bake-test-rust

Reusable Rust test tasks for Bake

#### [Rust Testing Tasks](.agents/context/bake-test-rust/testing.md)

Add bake-test-rust as a dependency of the private bake/ package and link it
from bake/src/main.rs:

### socketry-markdown

CommonMark compliant markdown parser in Rust with ASTs and extensions

#### [Markdown Parser Architecture](.agents/context/socketry-markdown/markdown-architecture.md)

This crate implements a Markdown state machine that tokenizes source, resolves
constructs, and can either compile events directly to HTML or build an mdast
syntax tree.

### socketry-project

Shared project conventions and development tasks for Socketry Rust crates

#### [Socketry Rust Conventions](.agents/context/socketry-project/conventions.md)

Use these conventions for Rust repositories in the Socketry organization.

#### [Rust Repository Layout](.agents/context/socketry-project/layout.md)

Use Cargo's standard structure so contributors can find package code, tests,
examples, and project documentation quickly.

#### [Rust Testing](.agents/context/socketry-project/testing.md)

Use Cargo's built-in test command for Rust tests.
