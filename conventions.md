# Conventions

- Use lowercase Markdown filenames, including `readme.md`, `license.md`, and `releases.md`.
- Start `license.md` with `# MIT License` and preserve upstream attribution.
- Put public Rust APIs in `src/`, integration tests in `tests/`, and reusable context in `context/`.
- Use clear names; avoid abbreviations in source code.
- Keep published crate versioning and release notes in this repository. The private `bake/` package is not published.
- Add path and version to dependencies on sibling Socketry crates for local development and crates.io publication.
