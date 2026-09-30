// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::Registry;
use bake_agent_context::agent::context::{AgentIndex, Installer};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn write(root: &Path, relative_path: &str, contents: &str) {
    let path = root.join(relative_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn project() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::Builder::new()
        .prefix("agent context project ")
        .tempdir()
        .unwrap();
    let root = directory.path().join("consumer");
    let provider = directory.path().join("provider");

    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndocs-provider = { path = \"../provider\" }\n",
    );
    write(&root, "src/main.rs", "fn main() {}\n");
    write(
        &provider,
        "Cargo.toml",
        "[package]\nname = \"docs-provider\"\nversion = \"1.2.3\"\nedition = \"2024\"\ndescription = \"Guidance from the test provider.\"\n",
    );
    write(&provider, "src/lib.rs", "pub fn provider() {}\n");
    write(
        &provider,
        "context/getting-started.md",
        "# Getting Started\n\nFirst paragraph.\n\nLater details.\n",
    );
    write(
        &provider,
        "context/reference/usage.md",
        "# Usage\n\nNested usage guidance.\n",
    );
    write(&provider, "context/example.json", "{\"enabled\": true}\n");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    (directory, root)
}

#[test]
fn discovers_context_from_resolved_dependencies_and_lists_all_files() {
    let (_directory, root) = project();
    let manifest_path = root.join("Cargo.toml");
    let mut manifest = fs::read_to_string(&manifest_path).unwrap();
    manifest.push_str("\n[workspace]\nmembers = [\"workspace-member\"]\nresolver = \"3\"\n");
    fs::write(manifest_path, manifest).unwrap();
    write(
        &root,
        "workspace-member/Cargo.toml",
        "[package]\nname = \"workspace-member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(&root, "workspace-member/src/lib.rs", "pub fn local() {}\n");
    write(
        &root,
        "workspace-member/context/local.md",
        "# Local\n\nThis workspace context should be skipped.\n",
    );

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let installer = Installer::new(&root).unwrap();

    let packages = installer.packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].selector(), "docs-provider");
    assert_eq!(packages[0].version, "1.2.3");

    let package = installer.find_package("docs-provider").unwrap().unwrap();
    let files = installer.list_context_files(&package).unwrap();
    let paths: Vec<_> = files
        .iter()
        .map(|file| file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        paths,
        ["example.json", "getting-started.md", "reference/usage.md"]
    );
}

#[test]
fn installs_context_generates_index_and_updates_agents_file_without_clobbering_other_sections() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    assert_eq!(installer.install_all().unwrap(), ["docs-provider"]);

    let installed = root.join(".agents/context/docs-provider");
    assert_eq!(
        fs::read_to_string(installed.join("reference/usage.md")).unwrap(),
        "# Usage\n\nNested usage guidance.\n"
    );
    let generated_index = fs::read_to_string(installed.join("index.yaml")).unwrap();
    assert!(generated_index.contains("Guidance from the test provider."));
    assert!(generated_index.contains("getting-started.md"));

    write(
        &root,
        "agents.md",
        "# Agent\n\nProject-specific introduction.\n\n## Context\n\nOld generated section.\n\n## Commands\n\nKeep this section.\n",
    );
    let index = AgentIndex::new(&root);
    index.update_agents_md("agents.md").unwrap();
    let first = fs::read_to_string(root.join("agents.md")).unwrap();
    assert!(first.contains("Project-specific introduction."));
    assert!(first.contains("[Getting Started](.agents/context/docs-provider/getting-started.md)"));
    assert!(first.contains("## Commands\n\nKeep this section."));
    assert!(!first.contains("Old generated section."));

    index.update_agents_md("agents.md").unwrap();
    assert_eq!(fs::read_to_string(root.join("agents.md")).unwrap(), first);
}

#[test]
fn inserts_context_under_agent_heading_or_creates_agent_heading() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    installer.install_all().unwrap();
    let index = AgentIndex::new(&root);

    write(&root, "agents.md", "# Agent\n\nProject guidance.\n");
    index.update_agents_md("agents.md").unwrap();
    let existing_heading = fs::read_to_string(root.join("agents.md")).unwrap();
    assert!(existing_heading.contains("# Agent\n\n## Context\n"));
    assert!(existing_heading.contains("Project guidance."));

    write(&root, "agents.md", "Project notes without a heading.\n");
    index.update_agents_md("agents.md").unwrap();
    let new_heading = fs::read_to_string(root.join("agents.md")).unwrap();
    assert!(new_heading.starts_with("# Agent\n\n## Context\n"));
    assert!(new_heading.ends_with("Project notes without a heading.\n"));
}

#[test]
fn show_supports_implicit_markdown_extension_and_rejects_parent_paths() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();

    assert_eq!(
        installer
            .show_context_file("docs-provider", "getting-started")
            .unwrap()
            .unwrap(),
        "# Getting Started\n\nFirst paragraph.\n\nLater details.\n"
    );
    assert!(
        installer
            .show_context_file("docs-provider", "../Cargo.toml")
            .is_err()
    );
}

#[test]
fn preserves_provider_index_and_ignores_unsafe_index_paths() {
    let (_directory, root) = project();
    let provider_index = "description: Curated package guidance.\nfiles:\n  - path: getting-started.md\n    title: Curated title\n    description: Curated summary.\n  - path: ../Cargo.toml\n    title: Outside file\n    description: Must not be linked.\n";
    write(
        &root.parent().unwrap().join("provider"),
        "context/index.yaml",
        provider_index,
    );

    let installer = Installer::new(&root).unwrap();
    installer.install_all().unwrap();
    let installed_index = root.join(".agents/context/docs-provider/index.yaml");
    assert_eq!(fs::read_to_string(installed_index).unwrap(), provider_index);

    let section = AgentIndex::new(&root).generate_context_section().unwrap();
    assert!(section.contains("Curated package guidance."));
    assert!(section.contains("[Curated title](.agents/context/docs-provider/getting-started.md)"));
    assert!(section.contains("Curated summary."));
    assert!(!section.contains("Outside file"));
    assert!(!section.contains("Must not be linked."));
}

#[test]
fn registers_the_ruby_compatible_task_names() {
    let registry = Registry::discover().unwrap();
    let names: Vec<_> = registry.tasks().map(|task| task.name()).collect();

    assert!(names.contains(&"agent:context:list"));
    assert!(names.contains(&"agent:context:show"));
    assert!(names.contains(&"agent:context:install"));
    assert!(names.contains(&"agent:context:agents-md"));
}
