// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::Registry;
use std::process::ExitCode;

fn main() -> ExitCode {
    report(Registry::discover().and_then(|registry| registry.run().map(|_| ())))
}

fn report(result: bake::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bake: {error}");
            ExitCode::FAILURE
        }
    }
}

#[path = "bake_generated_tasks/mod.rs"]
mod bake_generated_tasks;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn cargo_executable(configured: Option<OsString>) -> OsString {
        configured.unwrap_or_else(|| "cargo".into())
    }

    fn temporary_project() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname = \"coverage-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"coverage-fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        directory
    }

    fn temporary_project_with_context_provider() -> tempfile::TempDir {
        let directory = temporary_project();
        let root = directory.path();

        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"coverage-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndocs-provider = { path = \"provider\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"coverage-fixture\"\nversion = \"0.1.0\"\ndependencies = [\n \"docs-provider\",\n]\n\n[[package]]\nname = \"docs-provider\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("provider/src")).unwrap();
        fs::create_dir_all(root.join("provider/context/usage")).unwrap();
        fs::write(
            root.join("provider/Cargo.toml"),
            "[package]\nname = \"docs-provider\"\nversion = \"1.0.0\"\nedition = \"2024\"\ndescription = \"Fixture documentation provider\"\n",
        )
        .unwrap();
        fs::write(root.join("provider/src/lib.rs"), "pub fn provider() {}\n").unwrap();
        fs::write(
            root.join("provider/context/guide.md"),
            "---\ndescription: A guide for the fixture.\n---\n\n# Guide\n\nRead this guide first.\n",
        )
        .unwrap();
        fs::write(
            root.join("provider/context/usage.md"),
            "---\ntype: skill\ndescription: Use the fixture provider.\n---\n\n# Usage\n\nFollow the provider instructions.\n",
        )
        .unwrap();
        fs::write(
            root.join("provider/context/workflow.md"),
            "---\ntype: skill\ndescription: Follow the fixture workflow.\n---\n\n# Workflow\n\nUse this workflow.\n",
        )
        .unwrap();
        fs::write(
            root.join("provider/context/usage/example.txt"),
            "skill asset\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("provider/context/usage/references")).unwrap();
        fs::write(
            root.join("provider/context/usage/references/guide.md"),
            "A reference guide.\n",
        )
        .unwrap();

        directory
    }

    #[test]
    fn reports_success_and_failure_exit_codes() {
        assert_eq!(report(Ok(())), ExitCode::SUCCESS);
        assert_eq!(
            report(Err(bake::Error::new("task failed"))),
            ExitCode::FAILURE
        );
    }

    #[test]
    fn selects_the_cargo_executable_or_uses_the_default() {
        assert_eq!(
            cargo_executable(Some("custom-cargo".into())),
            "custom-cargo"
        );
        assert_eq!(cargo_executable(None), "cargo");
    }

    #[test]
    fn main_runs_the_process_registry() {
        // The test harness starts with no Bake task arguments, so the real entry
        // point prints the task list and returns successfully.
        assert_eq!(main(), ExitCode::SUCCESS);
    }

    #[test]
    fn executes_context_tasks_from_the_bake_executable() {
        let project = temporary_project();
        let root = project.path();

        let registry = Registry::discover().unwrap();
        let output = registry
            .run_arguments(root, &["agent:context:list".to_owned()])
            .unwrap();
        assert!(output.contains("No dependency context files found"));

        let registry = Registry::discover().unwrap();
        let output = registry
            .run_arguments(
                root,
                &[
                    "agent:context:skill:list".to_owned(),
                    "::".to_owned(),
                    "agent:context:skill:install".to_owned(),
                    "::".to_owned(),
                    "agent:context:index".to_owned(),
                    "::".to_owned(),
                    "agent:context:install".to_owned(),
                ],
            )
            .unwrap();
        assert!(output.contains("No dependency context or skills were installed"));
        assert!(root.join(".agents/context/index.md").is_file());
        assert!(
            root.join(".agents/skills/.agent-context-skills.json")
                .is_file()
        );

        let registry = Registry::discover().unwrap();
        let error = registry
            .run_arguments(
                root,
                &[
                    "agent:context:show".to_owned(),
                    "--package".to_owned(),
                    "missing".to_owned(),
                    "--file".to_owned(),
                    "guide.md".to_owned(),
                ],
            )
            .unwrap_err();
        assert!(error.to_string().contains("context file"));
    }

    #[test]
    fn executes_context_and_skill_tasks_with_a_dependency_provider() {
        let project = temporary_project_with_context_provider();
        let root = project.path();

        let output = Registry::discover()
            .unwrap()
            .run_arguments(root, &["agent:context:list".to_owned()])
            .unwrap();
        assert!(output.contains("docs-provider (1.0.0)"));

        let output = Registry::discover()
            .unwrap()
            .run_arguments(root, &["agent:context:skill:list".to_owned()])
            .unwrap();
        assert!(output.contains("docs-provider-usage"));
        assert!(output.contains("docs-provider-workflow"));
        assert!(output.contains("Use the fixture provider."));

        let output = Registry::discover()
            .unwrap()
            .run_arguments(
                root,
                &[
                    "agent:context:show".to_owned(),
                    "--package".to_owned(),
                    "docs-provider".to_owned(),
                    "--file".to_owned(),
                    "guide".to_owned(),
                ],
            )
            .unwrap();
        assert!(output.contains("Read this guide first."));

        let output = Registry::discover()
            .unwrap()
            .run_arguments(
                root,
                &[
                    "agent:context:skill:install".to_owned(),
                    "--skill".to_owned(),
                    "docs-provider-usage".to_owned(),
                ],
            )
            .unwrap();
        assert!(output.contains("docs-provider-usage"));
        assert_eq!(
            fs::read_to_string(root.join(".agents/skills/docs-provider-usage/example.txt"))
                .unwrap(),
            "skill asset\n"
        );

        let output = Registry::discover()
            .unwrap()
            .run_arguments(
                root,
                &[
                    "agent:context:install".to_owned(),
                    "--package".to_owned(),
                    "docs-provider".to_owned(),
                ],
            )
            .unwrap();
        assert!(output.contains("Installed context from: docs-provider"));
        assert!(
            root.join(".agents/context/docs-provider/guide.md")
                .is_file()
        );

        let output = Registry::discover()
            .unwrap()
            .run_arguments(root, &["agent:context:index".to_owned()])
            .unwrap();
        assert!(output.contains("Updated .agents/context/index.md"));
        let index = fs::read_to_string(root.join(".agents/context/index.md")).unwrap();
        assert!(index.contains("Fixture documentation provider"));
        assert!(index.contains("A guide for the fixture."));
        assert!(index.contains("[Guide](docs-provider/guide.md)"));
        assert!(!index.contains("usage.md"));
    }

    #[cfg(unix)]
    #[test]
    fn reports_skill_registry_read_and_directory_creation_errors() {
        let project = temporary_project_with_context_provider();
        let root = project.path();
        let registry_path = root.join(".agents/skills/.agent-context-skills.json");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        fs::write(&registry_path, "{\"version\":1,\"skills\":{}}\n").unwrap();
        fs::set_permissions(&registry_path, fs::Permissions::from_mode(0o000)).unwrap();

        let error = Registry::discover()
            .unwrap()
            .run_arguments(root, &["agent:context:skill:install".to_owned()])
            .unwrap_err()
            .to_string();
        fs::set_permissions(&registry_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(error.contains("cannot read"), "{error}");

        let project = temporary_project_with_context_provider();
        let root = project.path();
        let agents_directory = root.join(".agents");
        fs::create_dir(&agents_directory).unwrap();
        fs::set_permissions(&agents_directory, fs::Permissions::from_mode(0o555)).unwrap();

        let error = Registry::discover()
            .unwrap()
            .run_arguments(root, &["agent:context:skill:install".to_owned()])
            .unwrap_err()
            .to_string();
        fs::set_permissions(&agents_directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(error.contains("cannot create"), "{error}");
    }
}
