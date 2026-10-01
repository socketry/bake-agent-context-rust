# Agent Context

Keep reusable package guidance separate from repository-only instructions,
and make both easy for agents to discover.

## Package guidance

Put guidance that benefits users of a crate in its tracked `context/`
directory. Explain package-specific architecture, important invariants, common
usage patterns, and decisions that are not obvious from the public API. Keep
each guide focused on one topic, use descriptive lowercase filenames, and link
related guides together.

Include `context/**` in the published Cargo package when downstream users need
the guidance. Bake Agent Context builds the generated `agents.md` index from
each document's first heading and first prose sentence. See [Using and
Providing Context](usage.md) for the file format, skill metadata, and
installation commands.

Use a skill for a task-oriented procedure that an agent should follow, such as
setting up a repository or preparing a pull request. Skills are source Markdown
files in `context/` with YAML front matter containing `type: skill` and a
description. Bake installs them under `.agents/skills/` with names prefixed by
the provider crate. Skill documents are not copied to `.agents/context/` or
listed in the generated `agents.md` index.

## Repository-only instructions

Put guidance that applies only to the current checkout in `.agents/`. This can
include local development instructions, project-specific skills, and
instructions that should not ship to crate users. Do not put credentials or
machine-specific private details in package context.

Commit the lower-case `agents.md` file so agents have an entrypoint. It can
contain project-owned instructions and the generated Context section. Add
`.agents/context/` and `.agents/skills/` to `.gitignore`: both are installed
from package sources and can be regenerated. Edit tracked package sources
under `context/`, not generated copies under `.agents/`.

## Install and update

Add `bake-agent-context` to the project's private `bake/` package, directly or
through a shared task package such as `socketry-project`. Run
`cargo bake agent:context:install` to install ordinary context and skills from
resolved dependencies and update `agents.md`. Run it again after changing a
context-providing dependency. Use `--package CRATE` to install from one
provider, or the skill-specific tasks to list and install selected skills.

Before changing a project, read `agents.md`, the relevant installed context,
and any skills that apply to the work. For details about the available Bake
tasks and their options, see [Using and Providing Context](usage.md).
