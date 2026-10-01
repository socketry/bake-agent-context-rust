// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::Registry;
use bake_agent_context::agent::context::{AgentIndex, Installer, install_skills, list_skills};
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
    write(
        &provider,
        "context/initial-gem-setup.md",
        "---\ntype: skill\ndescription: Set up a new Ruby gem using the project conventions.\n---\n\n# Initial Gem Setup\n\nCreate the gem files and verify the package.\n",
    );
    write(
        &provider,
        "context/initial-gem-setup/references/checklist.md",
        "Run the project checks before publishing.\n",
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
        [
            "example.json",
            "getting-started.md",
            "initial-gem-setup/references/checklist.md",
            "initial-gem-setup.md",
            "reference/usage.md"
        ]
    );
}

#[test]
fn discovers_and_installs_skills_declared_in_yaml_frontmatter() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    let skills = list_skills(&installer, None).unwrap();

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "initial-gem-setup");
    assert_eq!(skills[0].package_selector(), "docs-provider");
    assert_eq!(
        skills[0].description,
        "Set up a new Ruby gem using the project conventions."
    );

    assert_eq!(
        install_skills(&installer, Some("docs-provider"), Some("initial-gem-setup")).unwrap(),
        ["initial-gem-setup (docs-provider)"]
    );

    let skill_directory = root.join(".agents/skills/initial-gem-setup");
    let skill_markdown = fs::read_to_string(skill_directory.join("SKILL.md")).unwrap();
    assert!(skill_markdown.starts_with(
        "---\nname: initial-gem-setup\ndescription: Set up a new Ruby gem using the project conventions.\n---\n\n"
    ));
    assert!(skill_markdown.contains("# Initial Gem Setup"));
    assert_eq!(
        fs::read_to_string(skill_directory.join("references/checklist.md")).unwrap(),
        "Run the project checks before publishing.\n"
    );

    // Reinstallation updates dependency-owned skills without replacing other skills.
    write(
        &root.parent().unwrap().join("provider"),
        "context/initial-gem-setup/references/checklist.md",
        "Updated checklist.\n",
    );
    install_skills(&installer, None, None).unwrap();
    assert_eq!(
        fs::read_to_string(skill_directory.join("references/checklist.md")).unwrap(),
        "Updated checklist.\n"
    );

    fs::remove_file(
        root.parent()
            .unwrap()
            .join("provider/context/initial-gem-setup.md"),
    )
    .unwrap();
    assert!(install_skills(&installer, None, None).unwrap().is_empty());
    assert!(!skill_directory.exists());
}

#[test]
fn skill_installation_does_not_overwrite_project_owned_skills() {
    let (_directory, root) = project();
    write(
        &root,
        ".agents/skills/initial-gem-setup/SKILL.md",
        "Project-owned skill.\n",
    );
    let installer = Installer::new(&root).unwrap();

    let error = install_skills(&installer, None, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not managed by Bake Agent Context")
    );
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/initial-gem-setup/SKILL.md")).unwrap(),
        "Project-owned skill.\n"
    );
}

#[test]
fn installs_context_and_updates_agents_file_without_clobbering_other_sections() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    assert_eq!(installer.install_all().unwrap(), ["docs-provider"]);

    let installed = root.join(".agents/context/docs-provider");
    assert_eq!(
        fs::read_to_string(installed.join("reference/usage.md")).unwrap(),
        "# Usage\n\nNested usage guidance.\n"
    );
    assert!(!installed.join("index.yaml").exists());

    write(
        &root,
        "agents.md",
        "# Agent\n\nProject-specific introduction.\n\n## Context\n\nOld generated section.\n\n## Commands\n\nKeep this section.\n",
    );
    let index = AgentIndex::new(&root).with_packages(installer.packages());
    index.update_agents_md("agents.md").unwrap();
    let first = fs::read_to_string(root.join("agents.md")).unwrap();
    assert!(first.contains("Project-specific introduction."));
    assert!(first.contains("Guidance from the test provider."));
    assert!(first.contains("[Getting Started](.agents/context/docs-provider/getting-started.md)"));
    assert!(first.contains("Set up a new Ruby gem using the project conventions."));
    assert!(first.contains("First paragraph."));
    assert!(!first.contains("Later details."));
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

    let html = "<!--\n# Agent\n\n## Context\nOld context.\n-->\n";
    write(&root, "agents.md", html);
    index.update_agents_md("agents.md").unwrap();
    let parsed_heading = fs::read_to_string(root.join("agents.md")).unwrap();
    assert!(parsed_heading.starts_with("# Agent\n\n## Context\n"));
    assert!(parsed_heading.contains(html));
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
fn extracts_title_and_first_sentence_from_markdown() {
    let (_directory, root) = project();
    write(
        &root.parent().unwrap().join("provider"),
        "context/reference/usage.md",
        "<!--\n# Not a title\n-->\n\n# Nested usage\n\nRead the guide first. Then apply its examples.\n",
    );

    let installer = Installer::new(&root).unwrap();
    installer.install_all().unwrap();

    let section = AgentIndex::new(&root)
        .with_packages(installer.packages())
        .generate_context_section()
        .unwrap();
    assert!(section.contains("[Nested usage](.agents/context/docs-provider/reference/usage.md)"));
    assert!(section.contains("Read the guide first."));
    assert!(!section.contains("Then apply its examples."));
}

#[test]
fn registers_the_ruby_compatible_task_names() {
    let registry = Registry::discover().unwrap();
    let names: Vec<_> = registry.tasks().map(|task| task.name()).collect();

    assert!(names.contains(&"agent:context:list"));
    assert!(names.contains(&"agent:context:show"));
    assert!(names.contains(&"agent:context:install"));
    assert!(names.contains(&"agent:context:agents-md"));
    assert!(names.contains(&"agent:context:skill:list"));
    assert!(names.contains(&"agent:context:skill:install"));
}
