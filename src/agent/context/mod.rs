// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

mod index;
mod installer;
mod skill;

pub use index::AgentIndex;
pub use installer::{ContextFile, ContextPackage, Installer};
pub use skill::{Skill, install_skills, list_skills};

use bake::{Context, Error, Result};

fn installer(context: &mut Context) -> Result<Installer> {
    if let Some(installer) = context.get::<Installer>() {
        return Ok(installer.clone());
    }

    let installer = Installer::new(context.root())?;
    context.insert(installer.clone());
    Ok(installer)
}

/// List dependency crates that ship context, or list one crate's context files.
#[bake::task]
pub fn list(context: &mut Context, package: Option<String>) -> Result<String> {
    let installer = installer(context)?;

    if let Some(package) = package {
        let Some(package) = installer.find_package(&package)? else {
            return Ok(format!("No context found for crate '{package}'"));
        };

        let files = installer.list_context_files(&package)?;
        let mut output = format!("Context files for crate '{}':", package.selector());
        for file in files {
            output.push_str(&format!("\n  {}", file.path.display()));
        }
        return Ok(output);
    }

    let packages = installer.packages();
    if packages.is_empty() {
        return Ok("No Cargo dependencies with context found".to_owned());
    }

    let mut output = String::from("Crates with context available:");
    for package in packages {
        output.push_str(&format!("\n  {} ({})", package.selector(), package.version));
    }
    Ok(output)
}

/// Show a context file from one resolved dependency crate.
#[bake::task]
pub fn show(
    context: &mut Context,
    #[bake(named)] package: String,
    #[bake(named)] file: String,
) -> Result<String> {
    let installer = installer(context)?;
    let Some(content) = installer.show_context_file(&package, &file)? else {
        return Err(Error::new(format!(
            "context file {file:?} was not found in crate {package:?}"
        )));
    };
    Ok(content)
}

/// Install one crate's context or all dependency context, then update agents.md.
#[bake::task]
pub fn install(context: &mut Context, package: Option<String>) -> Result<String> {
    let installer = installer(context)?;
    let installed = if let Some(package) = package {
        if installer.install_package(&package)? {
            vec![package]
        } else {
            Vec::new()
        }
    } else {
        installer.install_all()?
    };

    AgentIndex::new(context.root())
        .with_packages(installer.packages())
        .update_agents_md("agents.md")?;

    if installed.is_empty() {
        Ok("No dependency context was installed".to_owned())
    } else {
        Ok(format!("Installed context from: {}", installed.join(", ")))
    }
}

/// List skills declared by dependency context documents.
#[bake::task(name = "agent:context:skill:list")]
pub fn skill_list(context: &mut Context, package: Option<String>) -> Result<String> {
    let installer = installer(context)?;
    let skills = list_skills(&installer, package.as_deref())?;
    if skills.is_empty() {
        return Ok("No dependency skills found".to_owned());
    }

    let mut output = String::from("Available dependency skills:");
    for skill in skills {
        output.push_str(&format!(
            "\n  {} ({}) — {}",
            skill.name,
            skill.package_selector(),
            skill.description
        ));
    }
    Ok(output)
}

/// Install skills from all dependencies or select a crate and/or skill.
#[bake::task(name = "agent:context:skill:install")]
pub fn skill_install(
    context: &mut Context,
    package: Option<String>,
    skill: Option<String>,
) -> Result<String> {
    let installer = installer(context)?;
    let installed = install_skills(&installer, package.as_deref(), skill.as_deref())?;
    if installed.is_empty() {
        Ok("No dependency skills were installed".to_owned())
    } else {
        Ok(format!("Installed skills: {}", installed.join(", ")))
    }
}

/// Create or update the generated Context section in an agents.md file.
#[bake::task(name = "agents-md")]
pub fn agents_md(
    context: &mut Context,
    #[bake(default = "agents.md")] path: String,
) -> Result<String> {
    let installer = installer(context)?;
    AgentIndex::new(context.root())
        .with_packages(installer.packages())
        .update_agents_md(&path)?;
    Ok(format!("Updated {path}"))
}
