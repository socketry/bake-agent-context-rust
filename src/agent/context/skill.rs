// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::installer::{ContextPackage, Installer, markdown_files};
use bake::{Error, Result};
use serde::{Deserialize, Serialize};
use socketry_markdown::{ParseOptions, mdast::Node, to_mdast};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_VERSION: u32 = 1;
const REGISTRY_FILE: &str = ".agent-context-skills.json";

/// A skill declared by a Markdown file in a dependency's `context/` directory.
#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub package: ContextPackage,
    assets: Option<PathBuf>,
    body: String,
}

impl Skill {
    /// The package selector accepted by `--package`.
    pub fn package_selector(&self) -> &str {
        self.package.selector()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextFrontmatter {
    #[serde(rename = "type")]
    document_type: Option<String>,
    description: Option<String>,
}

#[derive(Serialize)]
struct SkillFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Registry {
    version: u32,
    skills: BTreeMap<String, SkillOwner>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SkillOwner {
    package: String,
    version: String,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            skills: BTreeMap::new(),
        }
    }
}

/// Find context documents marked with `type: skill` in YAML front matter.
pub fn list_skills(installer: &Installer, package: Option<&str>) -> Result<Vec<Skill>> {
    let packages = if let Some(selector) = package {
        let Some(package) = installer.find_package(selector)? else {
            return Ok(Vec::new());
        };
        vec![package]
    } else {
        installer.packages().to_vec()
    };

    let mut skills = Vec::new();
    for package in packages {
        let mut files = markdown_files(&package.context_path)?;
        files.sort();

        for source in files {
            let contents = fs::read_to_string(&source).map_err(|error| {
                Error::new(format!("cannot read {}: {error}", source.display()))
            })?;
            let mut options = ParseOptions::default();
            options.constructs.frontmatter = true;
            let mut document = to_mdast(&contents, &options).map_err(|error| {
                Error::new(format!("could not parse {}: {error}", source.display()))
            })?;

            let Some(frontmatter) = context_frontmatter(&document, &source)? else {
                continue;
            };
            let Some(document_type) = frontmatter.document_type else {
                continue;
            };
            if document_type != "skill" {
                return Err(Error::new(format!(
                    "unsupported context type {document_type:?} in {}; supported type: skill",
                    source.display()
                )));
            }

            if source.parent() != Some(package.context_path.as_path()) {
                return Err(Error::new(format!(
                    "skill document {} must be directly inside context/",
                    source.display()
                )));
            }

            let name = source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| Error::new(format!("invalid skill filename: {}", source.display())))?
                .to_owned();
            validate_skill_name(&name)?;

            let description = frontmatter
                .description
                .map(|description| description.trim().to_owned())
                .filter(|description| !description.is_empty())
                .ok_or_else(|| {
                    Error::new(format!(
                        "skill {} in crate {} requires a non-empty `description`",
                        name, package.name
                    ))
                })?;
            if description.chars().count() > 1024 {
                return Err(Error::new(format!(
                    "skill description for {name:?} exceeds the 1024 character limit"
                )));
            }

            let assets = package.context_path.join(&name);
            let assets = match fs::symlink_metadata(&assets) {
                Ok(metadata) if metadata.file_type().is_dir() => Some(assets),
                Ok(_) => {
                    return Err(Error::new(format!(
                        "skill assets path {} is not a directory",
                        assets.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(Error::new(format!(
                        "cannot inspect skill assets {}: {error}",
                        assets.display()
                    )));
                }
            };

            let Some(children) = document.children_mut() else {
                return Err(Error::new(format!(
                    "{} is not a Markdown document",
                    source.display()
                )));
            };
            if !matches!(children.first(), Some(Node::Yaml(_))) {
                return Err(Error::new(format!(
                    "skill {} must use YAML front matter delimited by `---`",
                    source.display()
                )));
            }
            children.remove(0);
            let body = document.to_markdown();

            skills.push(Skill {
                name,
                description,
                package: package.clone(),
                assets,
                body,
            });
        }
    }

    skills.sort_by(|left, right| {
        left.package
            .name
            .cmp(&right.package.name)
            .then_with(|| left.package.version.cmp(&right.package.version))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(skills)
}

/// Install skills from all providers, one provider, or one named skill.
///
/// If both filters are omitted, all discovered skills are installed. Existing
/// project-owned skill directories are never replaced; installed dependency
/// skills are tracked in a registry under `.agents/skills/`.
pub fn install_skills(
    installer: &Installer,
    package_selector: Option<&str>,
    skill_name: Option<&str>,
) -> Result<Vec<String>> {
    let mut skills = list_skills(installer, package_selector)?;
    if let Some(skill_name) = skill_name {
        skills.retain(|skill| skill.name == skill_name);
        if skills.is_empty() {
            return Err(Error::new(format!(
                "no dependency skill named {skill_name:?} was found"
            )));
        }
    }

    if let Some(skill_name) = skill_name
        && skills.len() > 1
    {
        let packages = skills
            .iter()
            .map(|skill| skill.package_selector())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::new(format!(
            "skill {skill_name:?} is provided by multiple crates ({packages}); select one with --package"
        )));
    }

    let mut selected_names = HashSet::new();
    for skill in &skills {
        if !selected_names.insert(skill.name.as_str()) {
            return Err(Error::new(format!(
                "multiple selected crates provide skill {:?}; select one with --package",
                skill.name
            )));
        }
    }

    let selected_package = if let Some(selector) = package_selector {
        Some(
            installer
                .find_package(selector)?
                .ok_or_else(|| Error::new(format!("no context found for crate {selector:?}")))?,
        )
    } else {
        None
    };
    let reconcile_all = package_selector.is_none() && skill_name.is_none();
    let reconcile_package = if skill_name.is_none() {
        selected_package
            .as_ref()
            .map(|package| package.name.as_str())
    } else {
        None
    };

    let skills_root = installer.root().join(".agents/skills");
    ensure_directory(&skills_root)?;
    let registry_path = skills_root.join(REGISTRY_FILE);
    let mut registry = load_registry(&registry_path)?;
    let had_registry = path_exists(&registry_path)?;

    let selected_by_name: HashMap<_, _> = skills
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect();

    let mut destination_exists = HashMap::new();
    for skill in &skills {
        if let Some(owner) = registry.skills.get(&skill.name)
            && owner.package != skill.package.name
        {
            return Err(Error::new(format!(
                "skill {:?} is already installed from crate {:?}; it cannot be replaced by {:?}",
                skill.name, owner.package, skill.package.name
            )));
        }

        let destination = skills_root.join(&skill.name);
        let exists = path_exists(&destination)?;
        destination_exists.insert(skill.name.clone(), exists);
        if exists
            && registry
                .skills
                .get(&skill.name)
                .is_none_or(|owner| owner.package != skill.package.name)
        {
            return Err(Error::new(format!(
                "skill destination {} already exists and is not managed by Bake Agent Context",
                destination.display()
            )));
        }
    }

    let stale_skills: Vec<_> = registry
        .skills
        .iter()
        .filter(|(name, owner)| {
            !selected_by_name.contains_key(name.as_str())
                && (reconcile_all
                    || reconcile_package.is_some_and(|package| owner.package == package))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut stale_exists = HashMap::new();
    for name in &stale_skills {
        stale_exists.insert(name.clone(), path_exists(&skills_root.join(name))?);
    }

    for name in &stale_skills {
        registry.skills.remove(name);
    }
    for skill in &skills {
        registry.skills.insert(
            skill.name.clone(),
            SkillOwner {
                package: skill.package.name.clone(),
                version: skill.package.version.clone(),
            },
        );
    }
    let encoded_registry = serde_json::to_vec_pretty(&registry)
        .map_err(|error| Error::new(format!("cannot encode skill registry: {error}")))?;

    let stage = skills_root.join(format!(".agent-context-staging-{}", std::process::id()));
    fs::create_dir(&stage)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", stage.display())))?;
    let new_skills = stage.join("new");
    let backups = stage.join("backups");
    if let Err(error) = fs::create_dir(&new_skills).and_then(|_| fs::create_dir(&backups)) {
        let _ = fs::remove_dir_all(&stage);
        return Err(Error::new(format!(
            "cannot prepare {}: {error}",
            stage.display()
        )));
    }

    for skill in &skills {
        if let Err(error) = write_staged_skill(skill, &new_skills.join(&skill.name)) {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
    }

    let mut changes = Vec::new();
    for skill in &skills {
        let destination = skills_root.join(&skill.name);
        let backup = backups.join(&skill.name);
        let had_previous = destination_exists[&skill.name];
        if had_previous && let Err(error) = fs::rename(&destination, &backup) {
            rollback(&skills_root, &backups, &changes);
            let _ = fs::remove_dir_all(&stage);
            return Err(Error::new(format!(
                "cannot move existing skill {}: {error}",
                destination.display()
            )));
        }

        if let Err(error) = fs::rename(new_skills.join(&skill.name), &destination) {
            if had_previous {
                let _ = fs::rename(&backup, &destination);
            }
            rollback(&skills_root, &backups, &changes);
            let _ = fs::remove_dir_all(&stage);
            return Err(Error::new(format!(
                "cannot install skill {}: {error}",
                destination.display()
            )));
        }
        changes.push(AppliedChange::Installed {
            name: skill.name.clone(),
            had_previous,
        });
    }

    for name in &stale_skills {
        let destination = skills_root.join(name);
        if stale_exists[name] {
            if let Err(error) = fs::rename(&destination, backups.join(name)) {
                rollback(&skills_root, &backups, &changes);
                let _ = fs::remove_dir_all(&stage);
                return Err(Error::new(format!(
                    "cannot remove stale installed skill {}: {error}",
                    destination.display()
                )));
            }
            changes.push(AppliedChange::Removed { name: name.clone() });
        }
    }

    let staged_registry = stage.join("registry.json");
    if let Err(error) = fs::write(&staged_registry, encoded_registry) {
        rollback(&skills_root, &backups, &changes);
        let _ = fs::remove_dir_all(&stage);
        return Err(Error::new(format!(
            "cannot write staged skill registry {}: {error}",
            staged_registry.display()
        )));
    }

    if had_registry && let Err(error) = fs::rename(&registry_path, backups.join("registry.json")) {
        rollback(&skills_root, &backups, &changes);
        let _ = fs::remove_dir_all(&stage);
        return Err(Error::new(format!(
            "cannot move existing skill registry {}: {error}",
            registry_path.display()
        )));
    }
    if let Err(error) = fs::rename(&staged_registry, &registry_path) {
        if had_registry {
            let _ = fs::rename(backups.join("registry.json"), &registry_path);
        }
        rollback(&skills_root, &backups, &changes);
        let _ = fs::remove_dir_all(&stage);
        return Err(Error::new(format!(
            "cannot update skill registry {}: {error}",
            registry_path.display()
        )));
    }

    fs::remove_dir_all(&stage)
        .map_err(|error| Error::new(format!("cannot remove {}: {error}", stage.display())))?;

    Ok(skills
        .iter()
        .map(|skill| format!("{} ({})", skill.name, skill.package_selector()))
        .collect())
}

pub(crate) fn frontmatter_description(document: &Node, source: &Path) -> Result<Option<String>> {
    Ok(context_frontmatter(document, source)?
        .and_then(|frontmatter| frontmatter.description)
        .map(|description| description.trim().to_owned())
        .filter(|description| !description.is_empty()))
}

fn context_frontmatter(document: &Node, source: &Path) -> Result<Option<ContextFrontmatter>> {
    let Some(children) = document.children() else {
        return Err(Error::new(format!(
            "{} is not a Markdown document",
            source.display()
        )));
    };
    let Some(Node::Yaml(frontmatter)) = children.first() else {
        return Ok(None);
    };

    serde_yaml_ng::from_str(&frontmatter.value)
        .map(Some)
        .map_err(|error| {
            Error::new(format!(
                "invalid YAML front matter in {}: {error}",
                source.display()
            ))
        })
}

fn validate_skill_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(Error::new(format!(
            "invalid skill name {name:?}; use 1–64 lowercase ASCII letters, digits, or single hyphens"
        )))
    }
}

fn write_staged_skill(skill: &Skill, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", destination.display())))?;

    if let Some(assets) = &skill.assets {
        copy_skill_assets(assets, destination, true)?;
    }

    let metadata = SkillFrontmatter {
        name: &skill.name,
        description: &skill.description,
    };
    let yaml = serde_yaml_ng::to_string(&metadata)
        .map_err(|error| Error::new(format!("cannot encode skill front matter: {error}")))?;
    let mut output = format!("---\n{yaml}---\n\n");
    output.push_str(&skill.body);
    if !output.ends_with('\n') {
        output.push('\n');
    }

    let skill_file = destination.join("SKILL.md");
    fs::write(&skill_file, output)
        .map_err(|error| Error::new(format!("cannot write {}: {error}", skill_file.display())))
}

fn copy_skill_assets(source: &Path, destination: &Path, top_level: bool) -> Result<()> {
    let mut entries = fs::read_dir(source)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", source.display())))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
            Error::new(format!("cannot inspect {}: {error}", source_path.display()))
        })?;
        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            return Err(Error::new(format!(
                "skill assets cannot contain symbolic links: {}",
                source_path.display()
            )));
        } else if file_type.is_dir() {
            fs::create_dir(&destination_path).map_err(|error| {
                Error::new(format!(
                    "cannot create {}: {error}",
                    destination_path.display()
                ))
            })?;
            copy_skill_assets(&source_path, &destination_path, false)?;
        } else if file_type.is_file() {
            if top_level
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SKILL.md")
            {
                return Err(Error::new(format!(
                    "{} is reserved for the generated skill instructions",
                    source_path.display()
                )));
            }
            fs::copy(&source_path, &destination_path).map_err(|error| {
                Error::new(format!(
                    "cannot copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                ))
            })?;
        } else {
            return Err(Error::new(format!(
                "unsupported skill asset: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(Error::new(format!(
            "skill installation path {} is not a regular directory",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|error| Error::new(format!("cannot create {}: {error}", path.display()))),
        Err(error) => Err(Error::new(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    }
}

fn load_registry(path: &Path) -> Result<Registry> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry::default());
        }
        Err(error) => {
            return Err(Error::new(format!(
                "cannot inspect {}: {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(Error::new(format!(
            "skill registry {} is not a regular file",
            path.display()
        )));
    }

    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let registry: Registry = serde_json::from_slice(&bytes).map_err(|error| {
        Error::new(format!(
            "invalid skill registry {}: {error}",
            path.display()
        ))
    })?;
    if registry.version != REGISTRY_VERSION {
        return Err(Error::new(format!(
            "unsupported skill registry version {} in {}",
            registry.version,
            path.display()
        )));
    }
    for name in registry.skills.keys() {
        validate_skill_name(name)?;
    }
    Ok(registry)
}

fn path_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::new(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    }
}

enum AppliedChange {
    Installed { name: String, had_previous: bool },
    Removed { name: String },
}

fn rollback(root: &Path, backups: &Path, changes: &[AppliedChange]) {
    for change in changes.iter().rev() {
        match change {
            AppliedChange::Installed { name, had_previous } => {
                let destination = root.join(name);
                let _ = remove_existing(&destination);
                if *had_previous {
                    let _ = fs::rename(backups.join(name), destination);
                }
            }
            AppliedChange::Removed { name } => {
                let _ = fs::rename(backups.join(name), root.join(name));
            }
        }
    }
}

fn remove_existing(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::new(format!(
                "cannot inspect {}: {error}",
                path.display()
            )));
        }
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| Error::new(format!("cannot remove {}: {error}", path.display())))
}
