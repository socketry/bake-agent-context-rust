---
type: skill
description: Use this skill to install and navigate agent context and skills provided by Cargo dependencies.
---

# Using and Providing Context

Use Bake Agent Context to discover and install practical guidance shipped in dependency crates.

## Use dependency context

Context providers store agent-focused documentation in a `context/` directory at the root of their crate. Bake Agent Context scans resolved Cargo dependencies for this directory.

Run `cargo bake agent:context:list` to see available providers. Each provider is listed by crate name and version. If multiple versions of one crate provide context, select the desired version using the `crate@version` selector shown by the task.

Run `cargo bake agent:context:list --package CRATE` to list one provider's files. Read a file without installing it using `cargo bake agent:context:show --package CRATE --file FILE`; the `.md` extension is optional for Markdown files. Install or refresh one provider with `cargo bake agent:context:install --package CRATE`.

Run `cargo bake agent:context:install` to install or refresh context from all resolved dependencies. Files are copied under `.agents/context/SELECTOR/`, where the selector is the crate name or the displayed `crate@version` selector. The task writes `.agents/context/index.md`, linking to Markdown files using each file's first heading as its title and first prose sentence as its summary.

The `.agents/context/` directory and its index are generated. In a Git checkout, the install task maintains a marked block in `.git/info/exclude` for that directory, the skill ownership registry, and dependency-installed skill directories. These local rules are not committed, and project-owned skill directories remain trackable. Each checkout gets the rules when the install task runs. Bake Agent Context does not create or modify the project's `agents.md`; repository owners can add a stable link to `.agents/context/index.md` if they want. Do not edit files under `.agents/context/` or dependency-installed skill directories; reinstalling replaces them with provider files.

Use `cargo bake agent:context:index` to refresh the index after changing installed files without reinstalling them.

## Provide context from a crate

Create a top-level `context/` directory in the crate and add Markdown guides that help users and coding agents complete practical tasks with the crate. For example:

```text
my-crate/
├── Cargo.toml
├── context/
│   ├── getting-started.md
│   ├── configuration.md
│   └── troubleshooting.md
└── src/
```

When `Cargo.toml` has an explicit `include` list, include `context/**` so the guides are present in the published crate. Bake Agent Context uses the crate's Cargo description as the package summary.

Write focused, actionable guides with a clear first heading and a useful first sentence. The generated index uses those as the link title and summary. A YAML `description` in front matter takes precedence over the first sentence. Keep detailed examples and guidance in the context files themselves.

## Provide a skill from context

A context document can also be distributed as an Agent Skill. Put the Markdown document directly in `context/` and give it YAML front matter with `type: skill` and a description that says what the skill does and when to use it:

```text
my-crate/
├── context/
│   ├── initial-gem-setup.md
│   └── initial-gem-setup/
│       └── references/
│           └── project-checklist.md
└── src/
```

```markdown
---
type: skill
description: Set up a new Ruby gem using the project conventions.
---

# Initial Gem Setup

Follow the setup steps and use the bundled references when needed.
```

The Markdown filename supplies the local skill name. Bake prefixes it with the Cargo package name, so `initial-gem-setup.md` in `my-crate` becomes `my-crate-initial-gem-setup/SKILL.md`. Files under the matching `context/initial-gem-setup/` directory are copied alongside it as skill resources. Bake generates the required YAML `name` and `description` fields in `SKILL.md`. Skill documents and their resource directories are installed only under `.agents/skills/`; they are omitted from `.agents/context/` and its index. In this crate, `context/usage.md` is installed as `bake-agent-context-usage/SKILL.md`.

The supported front matter keys are `type` and `description`. The type must be `skill`; other types and unknown keys are rejected. There is no portable file-pattern field for skills, so describe the relevant tasks and files in `description`.

Run `cargo bake agent:context:install` to install ordinary context and all discovered skills. Use `--package CRATE` to install both from one provider. For independent skill management, `cargo bake agent:context:skill:install` installs all skills by default; `--package CRATE` selects one provider, `--skill NAME` selects one uniquely named skill using its package-prefixed name, and both select a skill from a specific crate. Run `cargo bake agent:context:skill:list` to discover available skills.

Installed skills live under `.agents/skills/`. Bake records ownership there so it can update dependency-provided skills without overwriting project-owned skills. The installer adds exact entries to `.git/info/exclude` for dependency-owned skill directories and the ownership registry; it does not ignore the whole `.agents/skills/` directory. Skill names are global within that directory and are prefixed by the Cargo package name. If multiple versions of one crate provide the same skill, select one provider explicitly. Keep the companion assets directory free of a top-level `SKILL.md`, which Bake generates from the context document.

See `.agents/context/index.md` for installed guidance from this and other providers, including the language-agnostic specification and recommendations for keeping package context separate from repository-owned instructions.

The provider's `context/` directory is source content and should be versioned with the crate. It is different from `.agents/context/`, which is a generated copy installed into a consumer project.
