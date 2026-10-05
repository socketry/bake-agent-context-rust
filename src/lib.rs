// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

//! Bake tasks and APIs for consuming context directories shipped by Cargo packages.
mod exclude;
mod index;
mod installer;
mod skill;
#[cfg(test)]
mod test_filesystem;

pub use index::ContextIndex;
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
#[bake::task(name = "agent:context:list")]
pub fn list(context: &mut Context, package: Option<String>) -> Result<String> {
    let installer = installer(context)?;

    if let Some(package) = package {
        let Some(package) = installer.find_package(&package)? else {
            return Ok(format!("No context found for crate '{package}'"));
        };

        let files = installer.list_context_files(&package)?;
        if files.is_empty() {
            return Ok(format!(
                "No context files found for crate '{}'.",
                package.selector()
            ));
        }
        let mut output = format!("Context files for crate '{}':", package.selector());
        for file in files {
            output.push_str(&format!("\n  {}", file.path.display()));
        }
        return Ok(output);
    }

    let mut packages = Vec::new();
    for package in installer.packages() {
        if !installer.list_context_files(package)?.is_empty() {
            packages.push(package);
        }
    }
    if packages.is_empty() {
        return Ok("No dependency context files found".to_owned());
    }

    let mut output = String::from("Crates with context available:");
    for package in packages {
        output.push_str(&format!("\n  {} ({})", package.selector(), package.version));
    }
    Ok(output)
}

/// Show a context file from one resolved dependency crate.
#[bake::task(name = "agent:context:show")]
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

/// Install context and skills from one crate or all dependencies, then update the context index.
#[bake::task(name = "agent:context:install")]
pub fn install(context: &mut Context, package: Option<String>) -> Result<String> {
    let installer = installer(context)?;
    let installed_context = if let Some(package) = package.as_deref() {
        if installer.install_package(package)? {
            vec![package.to_owned()]
        } else {
            Vec::new()
        }
    } else {
        installer.install_all()?
    };
    let installed_skills = install_skills(&installer, package.as_deref(), None)?;

    ContextIndex::new(context.root())
        .with_packages(installer.packages())
        .update_index()?;

    let mut output = Vec::new();
    if !installed_context.is_empty() {
        output.push(format!(
            "Installed context from: {}",
            installed_context.join(", ")
        ));
    }
    if !installed_skills.is_empty() {
        output.push(format!("Installed skills: {}", installed_skills.join(", ")));
    }

    if output.is_empty() {
        Ok("No dependency context or skills were installed".to_owned())
    } else {
        Ok(output.join("\n"))
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

/// Install skills from all dependencies or select a crate and/or package-prefixed skill name.
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

/// Create or update `.agents/context/index.md` from installed context files.
#[bake::task(name = "agent:context:index")]
pub fn index(context: &mut Context) -> Result<String> {
    let installer = installer(context)?;
    ContextIndex::new(context.root())
        .with_packages(installer.packages())
        .update_index()?;
    Ok("Updated .agents/context/index.md".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn make_context(root: &Path, packages: Vec<ContextPackage>) -> Context {
        let mut context = bake::Registry::new().context(root);
        context.insert(Installer::for_test(root, packages));
        context
    }

    fn package(root: &Path, name: &str, version: &str) -> ContextPackage {
        ContextPackage::for_test(name, version, root.join(name).join("context"))
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn task_wrappers_return_outputs_and_propagate_errors() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let guide = package(root, "provider", "1.0.0");
        write(&guide.context_path.join("guide.md"), "# Guide\n");
        write(
            &guide.context_path.join("broken.md"),
            "---\ntype: guide\n---\n# Unsupported\n",
        );
        let mut context = make_context(root, vec![guide.clone()]);

        assert!(
            list(&mut context, Some("unknown".to_owned()))
                .unwrap()
                .contains("No context found")
        );
        assert!(list(&mut context, None).is_err());
        assert!(list(&mut context, Some("provider".to_owned())).is_err());
        assert!(install(&mut context, None).is_err());
        assert!(show(&mut context, "provider".to_owned(), "missing.md".to_owned()).is_err());
        assert!(install(&mut context, Some("provider".to_owned())).is_err());
        assert!(skill_list(&mut context, Some("provider".to_owned())).is_err());
        assert!(skill_install(&mut context, Some("provider".to_owned()), None).is_err());

        let mut duplicates = make_context(
            root,
            vec![
                package(root, "duplicate", "1.0.0"),
                package(root, "duplicate", "2.0.0"),
            ],
        );
        assert!(list(&mut duplicates, Some("duplicate".to_owned())).is_err());
        assert!(
            show(
                &mut duplicates,
                "duplicate".to_owned(),
                "guide.md".to_owned()
            )
            .is_err()
        );
        assert!(
            duplicates
                .get::<Installer>()
                .unwrap()
                .install_package("duplicate")
                .is_err()
        );

        let listing_package = package(root, "listing", "1.0.0");
        write(
            &listing_package.context_path.join("guide.md"),
            "# Listing\n",
        );
        let mut listing_context = make_context(root, vec![listing_package]);
        assert!(
            list(&mut listing_context, None)
                .unwrap()
                .contains("listing@1.0.0 (1.0.0)")
        );

        let empty_package = package(root, "empty", "1.0.0");
        fs::create_dir_all(&empty_package.context_path).unwrap();
        let mut empty_context = make_context(root, vec![empty_package]);
        assert!(
            list(&mut empty_context, None)
                .unwrap()
                .contains("No dependency context files")
        );
    }

    #[test]
    fn task_wrappers_propagate_install_and_index_failures() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write(&provider.context_path.join("guide.md"), "# Guide\n");

        let registry = root.join(".agents/skills/.agent-context-skills.json");
        write(&registry, r#"{"version":1,"skills":{}}"#);
        let failure_path = registry.clone();
        let _failure = test_filesystem::fail_once(test_filesystem::Operation::Read, move |path| {
            path == failure_path
        });
        let mut context = make_context(root, vec![provider.clone()]);
        assert!(install(&mut context, Some("provider".to_owned())).is_err());
        drop(_failure);

        let index_file = root.join(".agents/context/index.md");
        fs::create_dir_all(&index_file).unwrap();
        let mut context = make_context(root, Vec::new());
        assert!(install(&mut context, None).is_err());
        assert!(index(&mut context).is_err());

        let mut empty = make_context(root, Vec::new());
        assert!(
            list(&mut empty, None)
                .unwrap()
                .contains("No dependency context files")
        );
        assert_eq!(
            skill_list(&mut empty, None).unwrap(),
            "No dependency skills found"
        );
        assert_eq!(
            skill_install(&mut empty, None, None).unwrap(),
            "No dependency skills were installed"
        );
    }

    #[test]
    fn installer_creation_errors_are_returned_by_tasks() {
        let directory = tempdir().unwrap();
        let mut context = bake::Registry::new().context(directory.path());
        assert!(list(&mut context, None).is_err());
        assert!(show(&mut context, "provider".to_owned(), "guide.md".to_owned()).is_err());
        assert!(install(&mut context, None).is_err());
        assert!(skill_list(&mut context, None).is_err());
        assert!(skill_install(&mut context, None, None).is_err());
        assert!(index(&mut context).is_err());
    }
}
