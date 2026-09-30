# Releasing

This repository publishes `bake-agent-context` as an independently versioned crate.

## Prepare a release

1. Run `cargo bake cargo:version:patch`, `cargo bake cargo:version:minor`, `cargo bake cargo:version:major`, or `cargo bake cargo:version:bump --version X.Y.Z`.
2. The local `cargo:after_version_bump` task updates `license.md` and changes the `Unreleased` heading in `releases.md` to the selected version.
3. Review the version, release notes, and generated changes. Run formatting, Clippy, and tests, then commit the release changes in a pull request.
4. After the commit reaches `main`, GitHub Actions checks the version and release heading. The `crates-io` environment requires approval from `socketry/managers`; after approval the workflow publishes the crate using trusted publishing and creates the annotated version tag.

The workflow does not create a GitHub Release page. After publishing, run `cargo bake releases:github:release vX.Y.Z`, adding `--draft true` if you want to review it before publication.

## Initial publication

The first version must be published from a machine authenticated with crates.io. After publishing, configure the `crates-io` trusted publisher for this repository's `publish.yml` workflow and set up the `crates-io` environment reviewers. Later versions use the GitHub workflow.
