# Using dependency context

Install the context files shipped by resolved Cargo dependencies with `cargo bake agent:context:install`. The task copies package guidance into `.agents/context/` and updates the generated Context section in `agents.md`.

Use `cargo bake agent:context:list` to find providers, `cargo bake agent:context:show --package CRATE --file FILE` to inspect a document, or `cargo bake agent:context:install --package CRATE` to refresh one provider.

Package authors can place Markdown guides, examples, and configuration files in a top-level `context/` directory. When a crate uses an explicit Cargo `include` list, it must include `context/**` so those files reach downstream users.
