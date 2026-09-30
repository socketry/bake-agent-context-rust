// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::{Context, Registry, Result};
use bake_agent_context as _;
use bake_cargo as _;
use bake_license as _;
use bake_releases as _;
use std::process::ExitCode;

/// Update the license and release notes after changing the Cargo version.
#[bake::task(name = "cargo:after_version_bump")]
fn after_version_bump(context: &mut Context, version: String) -> Result<()> {
    context.call("license:update", &[])?;
    context.call("releases:update", &[&format!("v{version}")])?;
    Ok(())
}

fn main() -> ExitCode {
    match Registry::discover().and_then(|registry| registry.run().map(|_| ())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bake: {error}");
            ExitCode::FAILURE
        }
    }
}
