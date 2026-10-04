// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::Registry;
use bake_agent_context::agent::context::{ContextIndex, Installer, install_skills, list_skills};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const INSTALLED_SKILL_NAME: &str = "docs-provider-initial-gem-setup";

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
    write(
        &provider,
        "context/initial-gem-setup/examples/minimal-project.md",
        "Start with the smallest working project.\n",
    );

    let output = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
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
fn lists_context_files_without_listing_skill_documents_or_assets() {
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
fn discovers_and_installs_skills_declared_in_yaml_frontmatter() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    let skills = list_skills(&installer, None).unwrap();

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, INSTALLED_SKILL_NAME);
    assert_eq!(skills[0].package_selector(), "docs-provider");
    assert_eq!(
        skills[0].description,
        "Set up a new Ruby gem using the project conventions."
    );

    assert_eq!(
        install_skills(
            &installer,
            Some("docs-provider"),
            Some(INSTALLED_SKILL_NAME)
        )
        .unwrap(),
        [format!("{INSTALLED_SKILL_NAME} (docs-provider)")]
    );

    let exclude = fs::read_to_string(root.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains("# BEGIN bake-agent-context\n"));
    assert!(exclude.contains("/.agents/context/\n"));
    assert!(exclude.contains("/.agents/skills/.agent-context-skills.json\n"));
    assert!(exclude.contains(&format!("/.agents/skills/{INSTALLED_SKILL_NAME}/\n")));
    assert!(exclude.contains("# END bake-agent-context\n"));
    assert!(!exclude.lines().any(|line| line == "/.agents/skills/"));

    let skill_directory = root.join(".agents/skills").join(INSTALLED_SKILL_NAME);
    let skill_markdown = fs::read_to_string(skill_directory.join("SKILL.md")).unwrap();
    assert!(skill_markdown.starts_with(&format!(
        "---\nname: {INSTALLED_SKILL_NAME}\ndescription: Set up a new Ruby gem using the project conventions.\n---\n\n"
    )));
    assert!(skill_markdown.contains("# Initial Gem Setup"));
    assert_eq!(
        fs::read_to_string(skill_directory.join("references/checklist.md")).unwrap(),
        "Run the project checks before publishing.\n"
    );
    assert_eq!(
        fs::read_to_string(skill_directory.join("examples/minimal-project.md")).unwrap(),
        "Start with the smallest working project.\n"
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
    let exclude = fs::read_to_string(root.join(".git/info/exclude")).unwrap();
    assert!(!exclude.contains(&format!("/.agents/skills/{INSTALLED_SKILL_NAME}/")));
}

#[test]
fn names_the_usage_skill_from_its_file_and_package() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nbake-agent-context = { path = \"../provider\" }\n",
    );
    write(
        &provider,
        "Cargo.toml",
        "[package]\nname = \"bake-agent-context\"\nversion = \"1.2.3\"\nedition = \"2024\"\n",
    );
    write(
        &provider,
        "context/usage.md",
        "---\ntype: skill\ndescription: Use this skill to install and navigate dependency context.\n---\n\n# Usage\n\nInstall context and follow the generated index.\n",
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
    let skills = list_skills(&installer, None).unwrap();
    let usage = skills
        .iter()
        .find(|skill| skill.name == "bake-agent-context-usage")
        .unwrap();

    assert_eq!(
        usage.description,
        "Use this skill to install and navigate dependency context."
    );
}

#[test]
fn skill_installation_does_not_overwrite_project_owned_skills() {
    let (_directory, root) = project();
    write(
        &root,
        &format!(".agents/skills/{INSTALLED_SKILL_NAME}/SKILL.md"),
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
        fs::read_to_string(
            root.join(".agents/skills")
                .join(INSTALLED_SKILL_NAME)
                .join("SKILL.md")
        )
        .unwrap(),
        "Project-owned skill.\n"
    );
}

#[test]
fn maintains_local_excludes_without_hiding_project_owned_skills() {
    let (_directory, root) = project();
    let gitignore = "/target/\n# Project-specific ignore rules.\n";
    write(&root, ".gitignore", gitignore);
    write(
        &root,
        ".agents/skills/local-workflow/SKILL.md",
        "Project-owned skill.\n",
    );
    write(&root, ".git/info/exclude", "# Local user rule\n*.local\n");
    let installer = Installer::new(&root).unwrap();

    install_skills(&installer, None, None).unwrap();

    let actual_gitignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    let exclude = fs::read_to_string(root.join(".git/info/exclude")).unwrap();
    assert_eq!(actual_gitignore, gitignore);
    assert!(exclude.contains("/.agents/context/\n"));
    assert!(exclude.contains("/.agents/skills/.agent-context-skills.json\n"));
    assert!(exclude.contains(&format!("/.agents/skills/{INSTALLED_SKILL_NAME}/\n")));
    assert!(exclude.contains("# Local user rule\n*.local\n"));
    assert!(!exclude.lines().any(|line| line == "/.agents/skills/"));
    assert!(!exclude.contains("/.agents/skills/local-workflow/"));
    assert_eq!(
        exclude
            .lines()
            .filter(|line| *line == "# BEGIN bake-agent-context")
            .count(),
        1
    );
    assert_eq!(
        exclude
            .lines()
            .filter(|line| *line == "# END bake-agent-context")
            .count(),
        1
    );

    let generated_skill = Command::new("git")
        .args([
            "check-ignore",
            "--quiet",
            ".agents/skills/docs-provider-initial-gem-setup/SKILL.md",
        ])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(generated_skill.success());
    let project_skill = Command::new("git")
        .args([
            "check-ignore",
            "--quiet",
            ".agents/skills/local-workflow/SKILL.md",
        ])
        .current_dir(&root)
        .status()
        .unwrap();
    assert_eq!(project_skill.code(), Some(1));

    install_skills(&installer, None, None).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".git/info/exclude")).unwrap(),
        exclude
    );
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        gitignore
    );
}

#[test]
fn skill_installation_works_without_a_git_checkout() {
    let (_directory, root) = project();
    fs::remove_dir_all(root.join(".git")).unwrap();
    let installer = Installer::new(&root).unwrap();

    install_skills(&installer, None, None).unwrap();

    assert!(
        root.join(".agents/skills")
            .join(INSTALLED_SKILL_NAME)
            .join("SKILL.md")
            .is_file()
    );
    assert!(!root.join(".gitignore").exists());
}

#[test]
fn installs_context_and_updates_index_without_changing_agents_file() {
    let (_directory, root) = project();
    write(
        &root,
        "agents.md",
        "# Agent\n\nProject-specific introduction.\n\n## Context\n\nOld generated section.\n\n## Commands\n\nKeep this section.\n",
    );

    let output = Registry::discover()
        .unwrap()
        .run_arguments(&root, &["agent:context:install".to_owned()])
        .unwrap();
    assert!(output.contains("Installed context from: docs-provider"));
    assert!(output.contains(&format!(
        "Installed skills: {INSTALLED_SKILL_NAME} (docs-provider)"
    )));

    let installed = root.join(".agents/context/docs-provider");
    assert_eq!(
        fs::read_to_string(installed.join("reference/usage.md")).unwrap(),
        "# Usage\n\nNested usage guidance.\n"
    );
    assert!(!installed.join("index.yaml").exists());
    assert!(!installed.join("initial-gem-setup.md").exists());
    assert!(!installed.join("initial-gem-setup").exists());
    assert!(
        root.join(".agents/skills")
            .join(INSTALLED_SKILL_NAME)
            .join("SKILL.md")
            .is_file()
    );

    let agents_file = fs::read_to_string(root.join("agents.md")).unwrap();
    assert_eq!(
        agents_file,
        "# Agent\n\nProject-specific introduction.\n\n## Context\n\nOld generated section.\n\n## Commands\n\nKeep this section.\n"
    );

    let index_path = root.join(".agents/context/index.md");
    let first = fs::read_to_string(&index_path).unwrap();
    assert!(first.contains("# Context Index"));
    assert!(first.contains("## docs-provider"));
    assert!(first.contains("Guidance from the test provider."));
    assert!(first.contains("[Getting Started](docs-provider/getting-started.md)"));
    assert!(!first.contains("Set up a new Ruby gem using the project conventions."));
    assert!(first.contains("First paragraph."));
    assert!(!first.contains("Later details."));

    let installer = Installer::new(&root).unwrap();
    let index = ContextIndex::new(&root).with_packages(installer.packages());
    index.update_index().unwrap();
    assert_eq!(fs::read_to_string(index_path).unwrap(), first);
}

#[test]
fn context_index_does_not_create_or_modify_agents_file() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    installer.install_all().unwrap();
    let index = ContextIndex::new(&root);

    assert!(!root.join("agents.md").exists());
    index.update_index().unwrap();
    assert!(!root.join("agents.md").exists());
    assert!(root.join(".agents/context/index.md").is_file());

    let agents_file = "# Agent\n\nOwner-maintained project guidance.\n";
    write(&root, "agents.md", agents_file);
    index.update_index().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("agents.md")).unwrap(),
        agents_file
    );
}

#[test]
fn public_context_paths_render_fallback_titles_and_remove_empty_directories() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &provider,
        "context/no-heading.md",
        "Words without punctuation\n",
    );
    fs::create_dir_all(provider.join("context/empty-directory")).unwrap();

    let installer = Installer::new(&root).unwrap();
    assert_eq!(installer.context_path(), root.join(".agents/context"));
    assert_eq!(
        ContextIndex::new(&root).context_path(),
        root.join(".agents/context")
    );
    assert_eq!(installer.install_all().unwrap(), ["docs-provider"]);
    assert!(
        !root
            .join(".agents/context/docs-provider/empty-directory")
            .exists()
    );

    let index = ContextIndex::new(&root);
    let rendered = index.generate_index().unwrap();
    assert!(rendered.contains("[no heading](docs-provider/no-heading.md)"));
    assert!(rendered.contains("Words without punctuation"));

    write(
        &root,
        ".agents/context/zeta/guide.md",
        "# Zeta\n\nZeta guide.\n",
    );
    write(
        &root,
        ".agents/context/alpha/guide.md",
        "# Alpha\n\nAlpha guide.\n",
    );
    let rendered = index.generate_index().unwrap();
    assert!(rendered.find("## alpha").unwrap() < rendered.find("## zeta").unwrap());
}

#[test]
fn discovers_multiple_context_providers_and_orders_their_skills() {
    let (_directory, root) = project();
    let second_provider = root.parent().unwrap().join("second-provider");
    write(
        &second_provider,
        "Cargo.toml",
        "[package]\nname = \"second-provider\"\nversion = \"2.0.0\"\nedition = \"2024\"\ndescription = \"A second context provider.\"\n",
    );
    write(&second_provider, "src/lib.rs", "pub fn provider() {}\n");
    write(
        &second_provider,
        "context/guide.md",
        "# Second Provider\n\nUse the second provider guide.\n",
    );
    write(
        &second_provider,
        "context/release.md",
        "---\ntype: skill\ndescription: Prepare a release.\n---\n\n# Release\n\nReview the release notes first.\n",
    );
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndocs-provider = { path = \"../provider\" }\nsecond-provider = { path = \"../second-provider\" }\n",
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
    assert_eq!(
        installer
            .packages()
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>(),
        ["docs-provider", "second-provider"]
    );
    assert_eq!(
        installer.install_all().unwrap(),
        ["docs-provider", "second-provider"]
    );

    let skills = list_skills(&installer, None).unwrap();
    assert_eq!(
        skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        ["docs-provider-initial-gem-setup", "second-provider-release"]
    );
    assert_eq!(
        install_skills(&installer, None, None).unwrap(),
        [
            "docs-provider-initial-gem-setup (docs-provider)",
            "second-provider-release (second-provider)"
        ]
    );

    let index = ContextIndex::new(&root).with_packages(installer.packages());
    let rendered = index.generate_index().unwrap();
    assert!(
        rendered.find("## docs-provider").unwrap() < rendered.find("## second-provider").unwrap()
    );
    assert!(rendered.contains("A second context provider."));
}

#[test]
fn sorts_skills_within_a_provider_by_name() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &provider,
        "context/another-skill.md",
        "---\ntype: skill\ndescription: A second test skill.\n---\n\n# Another Skill\n",
    );

    let installer = Installer::new(&root).unwrap();
    let skills = list_skills(&installer, None).unwrap();
    let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "docs-provider-another-skill",
            "docs-provider-initial-gem-setup"
        ]
    );
}

#[test]
fn context_index_includes_frontmatter_descriptions() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &provider,
        "context/getting-started.md",
        "---\ndescription: A concise getting started summary.\n---\n\n# Getting Started\n\nFirst paragraph.\n",
    );
    write(
        &provider,
        "context/undocumented.md",
        "---\ndescription: '  '\n---\n\n# Undocumented\n\nNo summary is provided.\n",
    );
    write(
        &provider,
        "context/no-description.md",
        "---\n---\n\n# No Description\n\nUse the first paragraph as its summary.\n",
    );

    let installer = Installer::new(&root).unwrap();
    installer.install_all().unwrap();
    let rendered = ContextIndex::new(&root)
        .with_packages(installer.packages())
        .generate_index()
        .unwrap();

    assert!(rendered.contains("A concise getting started summary."));
    assert!(rendered.contains("[Undocumented](docs-provider/undocumented.md)"));
    assert!(rendered.contains("No summary is provided."));
    assert!(rendered.contains("Use the first paragraph as its summary."));
}

#[test]
fn selecting_one_skill_keeps_other_installed_skills() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &provider,
        "context/secondary.md",
        "---\ntype: skill\ndescription: A secondary skill.\n---\n\n# Secondary\n",
    );
    let installer = Installer::new(&root).unwrap();

    install_skills(&installer, None, None).unwrap();
    assert_eq!(
        install_skills(
            &installer,
            Some("docs-provider"),
            Some(INSTALLED_SKILL_NAME)
        )
        .unwrap(),
        [format!("{INSTALLED_SKILL_NAME} (docs-provider)")]
    );
    assert!(install_skills(&installer, None, Some("missing-skill")).is_err());
    fs::remove_file(provider.join("context/secondary.md")).unwrap();
    assert_eq!(
        install_skills(&installer, None, Some(INSTALLED_SKILL_NAME)).unwrap(),
        [format!("{INSTALLED_SKILL_NAME} (docs-provider)")]
    );

    let skills_root = root.join(".agents/skills");
    assert!(skills_root.join("docs-provider-secondary").is_dir());
    let registry = fs::read_to_string(skills_root.join(".agent-context-skills.json")).unwrap();
    assert!(registry.contains("docs-provider-secondary"));
}

#[cfg(unix)]
#[test]
fn reports_registry_read_and_skill_directory_creation_errors() {
    let (_directory, root) = project();
    let registry_path = root.join(".agents/skills/.agent-context-skills.json");
    write(
        &root,
        ".agents/skills/.agent-context-skills.json",
        "{\"version\":1,\"skills\":{}}\n",
    );
    fs::set_permissions(&registry_path, fs::Permissions::from_mode(0o000)).unwrap();
    let installer = Installer::new(&root).unwrap();
    let error = install_skills(&installer, None, None)
        .unwrap_err()
        .to_string();
    fs::set_permissions(&registry_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(error.contains("cannot read"), "{error}");

    let (_directory, root) = project();
    let agents_directory = root.join(".agents");
    fs::create_dir_all(&agents_directory).unwrap();
    fs::set_permissions(&agents_directory, fs::Permissions::from_mode(0o555)).unwrap();
    let installer = Installer::new(&root).unwrap();
    let error = install_skills(&installer, None, None)
        .unwrap_err()
        .to_string();
    fs::set_permissions(&agents_directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(error.contains("cannot create"), "{error}");
}

#[cfg(unix)]
#[test]
fn reports_context_removal_errors_without_overwriting_existing_files() {
    let (_directory, root) = project();
    let context_root = root.join(".agents/context");
    write(
        &root,
        ".agents/context/docs-provider/old.md",
        "Old content.\n",
    );
    fs::set_permissions(&context_root, fs::Permissions::from_mode(0o555)).unwrap();

    let installer = Installer::new(&root).unwrap();
    let result = installer.install_package("docs-provider");
    fs::set_permissions(&context_root, fs::Permissions::from_mode(0o755)).unwrap();

    let error = result.unwrap_err().to_string();
    assert!(error.contains("cannot remove"), "{error}");
}

#[test]
fn validates_skill_documents_through_the_dependency_build() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider/context");
    let installer = Installer::new(&root).unwrap();

    assert_eq!(
        list_skills(&installer, Some("docs-provider"))
            .unwrap()
            .len(),
        1
    );
    assert!(
        list_skills(&installer, Some("missing-provider"))
            .unwrap()
            .is_empty()
    );
    assert!(
        install_skills(&installer, Some("missing-provider"), None)
            .unwrap_err()
            .to_string()
            .contains("no context found")
    );

    write(
        &provider,
        "another-skill.md",
        "---\ntype: skill\ndescription: Another skill.\n---\n\n# Another Skill\n",
    );
    assert_eq!(
        list_skills(&installer, Some("docs-provider"))
            .unwrap()
            .len(),
        2
    );

    for (name, contents, message) in [
        (
            "unsupported.md",
            "---\ntype: guide\ndescription: Guide.\n---\n\n# Guide\n",
            "unsupported context type",
        ),
        (
            "invalid-yaml.md",
            "---\ntype: [skill\n---\n\n# Skill\n",
            "invalid YAML front matter",
        ),
        (
            "Bad_Name.md",
            "---\ntype: skill\ndescription: Invalid name.\n---\n\n# Skill\n",
            "invalid skill name",
        ),
        (
            "missing-description.md",
            "---\ntype: skill\n---\n\n# Skill\n",
            "requires a non-empty",
        ),
        (
            "long-description.md",
            &format!(
                "---\ntype: skill\ndescription: {}\n---\n\n# Skill\n",
                "x".repeat(1025)
            ),
            "1024 character limit",
        ),
    ] {
        write(&provider, name, contents);
        assert!(
            list_skills(&installer, None)
                .unwrap_err()
                .to_string()
                .contains(message),
            "{name}"
        );
        assert!(
            install_skills(&installer, Some("docs-provider"), None)
                .unwrap_err()
                .to_string()
                .contains(message),
            "{name}"
        );
        fs::remove_file(provider.join(name)).unwrap();
    }

    write(
        &provider,
        "nested/skill.md",
        "---\ntype: skill\ndescription: Nested.\n---\n\n# Skill\n",
    );
    assert!(
        list_skills(&installer, None)
            .unwrap_err()
            .to_string()
            .contains("directly inside context")
    );
    assert!(
        install_skills(&installer, Some("docs-provider"), None)
            .unwrap_err()
            .to_string()
            .contains("directly inside context")
    );
    fs::remove_dir_all(provider.join("nested")).unwrap();

    write(
        &provider,
        "blocked.md",
        "---\ntype: skill\ndescription: Blocked.\n---\n\n# Blocked\n",
    );
    write(&provider, "blocked", "not a directory");
    assert!(
        list_skills(&installer, None)
            .unwrap_err()
            .to_string()
            .contains("is not a directory")
    );
    assert!(
        install_skills(&installer, Some("docs-provider"), None)
            .unwrap_err()
            .to_string()
            .contains("is not a directory")
    );
}

#[cfg(unix)]
#[test]
fn rejects_non_regular_skill_assets_in_the_dependency_build() {
    use std::os::unix::fs::symlink;
    use std::process::Command;

    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    let assets = provider.join("context/initial-gem-setup");
    let target = root.join("asset-target.txt");
    fs::write(&target, "target").unwrap();
    symlink(&target, assets.join("link")).unwrap();
    let error = install_skills(&Installer::new(&root).unwrap(), None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot contain symbolic links"), "{error}");

    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    fs::write(
        provider.join("context/initial-gem-setup/SKILL.md"),
        "Reserved skill asset.\n",
    )
    .unwrap();
    let error = install_skills(&Installer::new(&root).unwrap(), None, None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("reserved for the generated skill instructions"),
        "{error}"
    );

    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    let socket_path = provider.join("context/initial-gem-setup/socket");
    let status = Command::new("mkfifo").arg(&socket_path).status().unwrap();
    assert!(status.success(), "mkfifo failed: {status}");
    let error = install_skills(&Installer::new(&root).unwrap(), None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported skill asset"), "{error}");
}

#[test]
fn adds_a_final_newline_to_parsed_skill_documents() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider");
    write(
        &provider,
        "context/no-final-newline.md",
        "---\ntype: skill\ndescription: A skill without a final newline.\n---\n\n# No Final Newline",
    );

    install_skills(&Installer::new(&root).unwrap(), None, None).unwrap();
    let installed =
        fs::read_to_string(root.join(".agents/skills/docs-provider-no-final-newline/SKILL.md"))
            .unwrap();
    assert!(installed.ends_with('\n'));
}

#[test]
fn reports_public_index_installer_and_cargo_metadata_errors() {
    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    fs::create_dir_all(root.join(".agents")).unwrap();
    fs::write(root.join(".agents/context"), "not a directory").unwrap();
    assert!(
        installer
            .install_package("docs-provider")
            .unwrap_err()
            .to_string()
            .contains("cannot create")
    );
    assert!(ContextIndex::new(&root).update_index().is_err());

    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        "Cargo.toml",
        "this is not valid TOML = [\n",
    );
    let error = Installer::new(directory.path()).unwrap_err();
    assert!(error.to_string().contains("cargo metadata failed"));

    let directory = tempfile::tempdir().unwrap();
    let index = ContextIndex::new(directory.path());
    fs::create_dir_all(index.context_path().join("index.md")).unwrap();
    assert!(
        index
            .update_index()
            .unwrap_err()
            .to_string()
            .contains("cannot write")
    );
}

#[test]
fn rejects_invalid_skill_registries_and_installation_paths() {
    let (_directory, root) = project();
    let skills_root = root.join(".agents/skills");
    fs::create_dir_all(root.join(".agents")).unwrap();
    fs::write(&skills_root, "not a directory").unwrap();
    let installer = Installer::new(&root).unwrap();
    assert!(
        install_skills(&installer, None, None)
            .unwrap_err()
            .to_string()
            .contains("not a regular directory")
    );

    let (_directory, root) = project();
    let installer = Installer::new(&root).unwrap();
    let registry_path = root.join(".agents/skills/.agent-context-skills.json");
    write(
        &root,
        ".agents/skills/.agent-context-skills.json",
        "{invalid json}",
    );
    assert!(
        install_skills(&installer, None, None)
            .unwrap_err()
            .to_string()
            .contains("invalid skill registry")
    );

    fs::write(&registry_path, r#"{"version":2,"skills":{}}"#).unwrap();
    assert!(
        install_skills(&installer, None, None)
            .unwrap_err()
            .to_string()
            .contains("unsupported skill registry version")
    );
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

    let section = ContextIndex::new(&root)
        .with_packages(installer.packages())
        .generate_index()
        .unwrap();
    assert!(section.contains("[Nested usage](docs-provider/reference/usage.md)"));
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
    assert!(names.contains(&"agent:context:index"));
    assert!(!names.contains(&"agent:context:agents-md"));
    assert!(names.contains(&"agent:context:skill:list"));
    assert!(names.contains(&"agent:context:skill:install"));
}

#[test]
fn bake_tasks_cover_context_and_skill_lookup_installation_and_indexing() {
    let (_directory, root) = project();
    let registry = Registry::discover().unwrap();

    let output = registry
        .run_arguments(&root, &["agent:context:list".to_owned()])
        .unwrap();
    assert!(output.contains("Crates with context available:"));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:list".to_owned(),
                "::".to_owned(),
                "agent:context:skill:list".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains(INSTALLED_SKILL_NAME));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:list".to_owned(),
                "--package".to_owned(),
                "missing".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("No context found for crate 'missing'"));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:list".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("getting-started.md"));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:show".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
                "--file".to_owned(),
                "getting-started".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("First paragraph."));
    assert!(
        Registry::discover()
            .unwrap()
            .run_arguments(
                &root,
                &[
                    "agent:context:show".to_owned(),
                    "--package".to_owned(),
                    "docs-provider".to_owned(),
                    "--file".to_owned(),
                    "absent".to_owned(),
                ],
            )
            .is_err()
    );

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:install".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("Installed context from: docs-provider"));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:skill:list".to_owned(),
                "--package".to_owned(),
                "missing".to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(output.trim_end(), "No dependency skills found");

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:skill:install".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
                "--skill".to_owned(),
                INSTALLED_SKILL_NAME.to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains(INSTALLED_SKILL_NAME));
    assert!(
        Registry::discover()
            .unwrap()
            .run_arguments(
                &root,
                &[
                    "agent:context:skill:install".to_owned(),
                    "--skill".to_owned(),
                    "missing".to_owned(),
                ],
            )
            .is_err()
    );

    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:index".to_owned()])
            .unwrap()
            .trim_end(),
        "Updated .agents/context/index.md"
    );
}

#[test]
fn bake_tasks_report_empty_context_and_skill_sets() {
    let (_directory, root) = project();
    fs::remove_dir_all(root.parent().unwrap().join("provider/context")).unwrap();

    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:list".to_owned()])
            .unwrap()
            .trim_end(),
        "No dependency context files found"
    );
    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:skill:list".to_owned()])
            .unwrap()
            .trim_end(),
        "No dependency skills found"
    );
    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:install".to_owned()])
            .unwrap()
            .trim_end(),
        "No dependency context or skills were installed"
    );
    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:skill:install".to_owned()])
            .unwrap()
            .trim_end(),
        "No dependency skills were installed"
    );
    assert_eq!(
        Registry::discover()
            .unwrap()
            .run_arguments(&root, &["agent:context:index".to_owned()])
            .unwrap()
            .trim_end(),
        "Updated .agents/context/index.md"
    );
}

#[test]
fn tasks_distinguish_skill_only_dependencies_from_missing_context() {
    let (_directory, root) = project();
    let provider = root.parent().unwrap().join("provider/context");
    fs::remove_file(provider.join("getting-started.md")).unwrap();
    fs::remove_file(provider.join("example.json")).unwrap();
    fs::remove_dir_all(provider.join("reference")).unwrap();

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:list".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("No context files found for crate 'docs-provider'."));

    let output = Registry::discover()
        .unwrap()
        .run_arguments(
            &root,
            &[
                "agent:context:install".to_owned(),
                "--package".to_owned(),
                "docs-provider".to_owned(),
            ],
        )
        .unwrap();
    assert!(output.contains("Installed skills: docs-provider-initial-gem-setup"));
    assert!(!output.contains("Installed context from:"));
}
