# Agent Context

Keep reusable package guidance separate from repository-only instructions, and make both easy for agents to discover.

## Package guidance

Put guidance that benefits users of a crate in its tracked `context/` directory. Explain package-specific architecture, important invariants, common usage patterns, and decisions that are not obvious from the public API. Keep each guide focused on one topic, use descriptive lowercase filenames, and link related guides together.

Include `context/**` in the published Cargo package when downstream users need the guidance. Bake Agent Context builds `.agents/context/index.md` from each installed document's first heading and first prose sentence. It does not create or modify the repository owner's `agents.md`. Use the `bake-agent-context-usage` skill for file format, skill metadata, and installation details.

Use a skill for a task-oriented procedure that an agent should follow, such as setting up a repository or preparing a pull request. Skills are source Markdown files in `context/` with YAML front matter containing `type: skill` and a description. Bake installs them under `.agents/skills/` with names prefixed by the provider crate. Skill documents are not copied to `.agents/context/` or listed in the generated context index.

## Repository-only instructions

Put guidance that applies only to the current checkout in `.agents/`. This can include local development instructions, project-specific skills, and instructions that should not ship to crate users. Do not put credentials or machine-specific private details in package context.

The repository owner controls `agents.md`; context installation leaves it untouched. If agents should use dependency guidance, add a stable link to `.agents/context/index.md` there. `cargo bake agent:context:install` maintains a marked block in the local Git exclude file (`.git/info/exclude`) for generated `.agents/context/` files and each dependency-installed skill directory. These rules stay in the local checkout instead of becoming project files. Project-owned skill directories remain trackable. Edit package skill sources under `context/` and repository-only skills under `.agents/skills/`, not generated dependency copies.

## Install and update

Add `bake-agent-context` to the project's private `bake/` package, directly or through a shared task package such as `socketry-project`. Run `cargo bake agent:context:install` to install ordinary context and skills from resolved dependencies and update `.agents/context/index.md`. Run it again after changing a context-providing dependency. Use `--package CRATE` to install from one provider, or the skill-specific tasks to list and install selected skills.

Before changing a project, follow its `agents.md` instructions, read relevant installed context from `.agents/context/index.md`, and apply any skills that fit the work. For details about the available Bake tasks and their options, use the `bake-agent-context-usage` skill.

Each installed skill directory contains `skill.json` with its provider ecosystem, package, and version. Cargo refreshes reconcile Cargo-owned skills and reject conflicting ownership. See the portable specification for the shared format.
