// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::installer::{ContextPackage, Installer, markdown_files};
#[cfg(test)]
use super::test_filesystem as filesystem;
use bake::{Error, Result};
use serde::{Deserialize, Serialize};
use socketry_markdown::{ParseOptions, mdast::Node, to_mdast};
use std::cmp::Ordering as Comparison;
use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(not(test))]
use std::fs as filesystem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

const OWNERSHIP_FILE: &str = "skill.json";
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A skill declared by a Markdown file in a dependency's `context/` directory.
#[derive(Clone, Debug)]
pub struct Skill {
    /// Globally unique installed name, prefixed with the provider crate name.
    pub name: String,
    pub description: String,
    pub package: ContextPackage,
    pub(crate) source_name: String,
    assets: Option<PathBuf>,
    body: String,
    metadata: BTreeMap<String, serde_yaml_ng::Value>,
}

impl Skill {
    /// The package selector accepted by `--package`.
    pub fn package_selector(&self) -> &str {
        self.package.selector()
    }
}

#[derive(Deserialize)]
struct ContextFrontmatter {
    #[serde(rename = "type")]
    document_type: Option<String>,
    description: Option<String>,
    #[serde(flatten)]
    metadata: BTreeMap<String, serde_yaml_ng::Value>,
}

#[derive(Serialize)]
struct SkillFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(flatten)]
    metadata: &'a BTreeMap<String, serde_yaml_ng::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SkillOwner {
    ecosystem: String,
    package: String,
    version: String,
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
        skills.extend(list_package_skills(&package)?);
    }

    skills.sort_by(compare_skills);
    Ok(skills)
}

fn compare_skills(left: &Skill, right: &Skill) -> Comparison {
    let package_order = left.package.name.cmp(&right.package.name);
    if package_order != Comparison::Equal {
        return package_order;
    }

    let version_order = left.package.version.cmp(&right.package.version);
    if version_order != Comparison::Equal {
        return version_order;
    }

    left.name.cmp(&right.name)
}

pub(crate) fn list_package_skills(package: &ContextPackage) -> Result<Vec<Skill>> {
    let mut skills = Vec::new();
    let mut files = markdown_files(&package.context_path)?;
    files.sort_by_key(|path| {
        (
            path.parent() != Some(package.context_path.as_path()),
            path.clone(),
        )
    });

    for source in files {
        if skills.iter().any(|skill: &Skill| {
            skill
                .assets
                .as_ref()
                .is_some_and(|assets| source.starts_with(assets))
        }) {
            continue;
        }
        let contents = filesystem::read_to_string(&source)
            .map_err(|error| Error::new(format!("cannot read {}: {error}", source.display())))?;
        if let Some(skill) = parse_skill_document(package, &source, &contents)? {
            skills.push(skill);
        }
    }

    Ok(skills)
}

fn parse_skill_document(
    package: &ContextPackage,
    source: &Path,
    contents: &str,
) -> Result<Option<Skill>> {
    let mut options = ParseOptions::default();
    options.constructs.frontmatter = true;
    // MDX parsing is disabled here, so Markdown syntax itself cannot fail.
    let mut document =
        to_mdast(contents, &options).expect("Markdown parsing without MDX support is infallible");

    let Some(frontmatter) = context_frontmatter(&document, source)? else {
        return Ok(None);
    };
    let Some(document_type) = frontmatter.document_type else {
        return Ok(None);
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

    let source_name = source_name(source)?;
    validate_skill_name(&source_name)?;

    let package_prefix = package.name.to_ascii_lowercase().replace('_', "-");
    let name = format!("{package_prefix}-{source_name}");
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

    let assets = skill_assets_path(&package.context_path, &source_name)?;
    let body = skill_body(&mut document);

    Ok(Some(Skill {
        name,
        description,
        package: package.clone(),
        source_name,
        assets,
        body,
        metadata: {
            let mut metadata = frontmatter.metadata;
            metadata.remove("name");
            metadata
        },
    }))
}

fn skill_assets_path(context_path: &Path, source_name: &str) -> Result<Option<PathBuf>> {
    let assets = context_path.join(source_name);
    match filesystem::symlink_metadata(&assets) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(assets)),
        Ok(_) => Err(Error::new(format!(
            "skill assets path {} is not a directory",
            assets.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::new(format!(
            "cannot inspect skill assets {}: {error}",
            assets.display()
        ))),
    }
}

fn skill_body(document: &mut Node) -> String {
    // parse_skill_document only calls this after context_frontmatter has found
    // and parsed the leading YAML block.
    let children = document
        .children_mut()
        .expect("a Markdown document root always has children");
    children.remove(0);
    document.to_markdown()
}

/// Install skills from all providers, one provider, or one named skill.
///
/// If both filters are omitted, all discovered skills are installed.
/// Dependency skill ownership is recorded in each installed directory’s `skill.json`.
pub fn install_skills(
    installer: &Installer,
    package_selector: Option<&str>,
    skill_name: Option<&str>,
) -> Result<Vec<String>> {
    // Resolve a package selector only once. Resolving it again after listing
    // skills introduced a second, unreachable error path and a small TOCTOU
    // window if package state ever becomes mutable.
    let (selected_package, mut skills) = if let Some(selector) = package_selector {
        let Some(package) = installer.find_package(selector)? else {
            return Err(Error::new(format!(
                "no context found for crate {selector:?}"
            )));
        };
        let skills = list_package_skills(&package)?;
        (Some(package), skills)
    } else {
        (None, list_skills(installer, None)?)
    };

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
    let owners = installed_skill_owners(&skills_root)?;

    let selected_by_name: HashMap<_, _> = skills
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect();

    let mut destination_exists = HashMap::new();
    for skill in &skills {
        if let Some(owner) = owners.get(&skill.name)
            && (owner.ecosystem != "cargo" || owner.package != skill.package.name)
        {
            return Err(Error::new(format!(
                "skill {:?} is already installed from {} package {:?}; it cannot be replaced by Cargo package {:?}",
                skill.name, owner.ecosystem, owner.package, skill.package.name
            )));
        }

        let destination = skills_root.join(&skill.name);
        let exists = path_exists(&destination)?;
        destination_exists.insert(skill.name.clone(), exists);
        if exists
            && owners.get(&skill.name).is_none_or(|owner| {
                owner.ecosystem != "cargo" || owner.package != skill.package.name
            })
        {
            return Err(Error::new(format!(
                "skill destination {} already exists and is not managed by Bake Agent Context",
                destination.display()
            )));
        }
    }

    let stale_skills: Vec<_> = owners
        .iter()
        .filter(|(name, owner)| {
            owner.ecosystem == "cargo"
                && !selected_by_name.contains_key(name.as_str())
                && (reconcile_all
                    || match reconcile_package {
                        Some(package) => owner.package == package,
                        None => false,
                    })
        })
        .map(|(name, _)| name.clone())
        .collect();
    let remaining_names = owners.keys().filter(|name| !stale_skills.contains(name));
    let skill_names = remaining_names
        .cloned()
        .chain(skills.iter().map(|skill| skill.name.clone()));
    let exclude_update = super::exclude::prepare(installer.root(), skill_names)?;

    let stage = staging_path(&skills_root);
    filesystem::create_dir(&stage)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", stage.display())))?;
    let new_skills = stage.join("new");
    let backups = stage.join("backups");
    if let Err(error) =
        filesystem::create_dir(&new_skills).and_then(|_| filesystem::create_dir(&backups))
    {
        let _ = filesystem::remove_dir_all(&stage);
        return Err(Error::new(format!(
            "cannot prepare {}: {error}",
            stage.display()
        )));
    }

    for skill in &skills {
        let result = write_staged_skill(skill, &new_skills.join(&skill.name));
        if let Err(error) = result {
            let _ = filesystem::remove_dir_all(&stage);
            return Err(error);
        }
    }

    if let Err(error) = apply_exclude_update(exclude_update.as_ref()) {
        let _ = filesystem::remove_dir_all(&stage);
        return Err(error);
    }

    let mut changes = Vec::new();
    for skill in &skills {
        let destination = skills_root.join(&skill.name);
        let backup = backups.join(&skill.name);
        let had_previous = destination_exists[&skill.name];
        if had_previous && let Err(error) = filesystem::rename(&destination, &backup) {
            rollback(&skills_root, &backups, &changes);
            if let Some(update) = &exclude_update {
                let _ = update.restore();
            }
            let _ = filesystem::remove_dir_all(&stage);
            return Err(Error::new(format!(
                "cannot move existing skill {}: {error}",
                destination.display()
            )));
        }

        if let Err(error) = filesystem::rename(new_skills.join(&skill.name), &destination) {
            if had_previous {
                let _ = filesystem::rename(&backup, &destination);
            }
            rollback(&skills_root, &backups, &changes);
            if let Some(update) = &exclude_update {
                let _ = update.restore();
            }
            let _ = filesystem::remove_dir_all(&stage);
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
        if let Err(error) = filesystem::rename(&destination, backups.join(name)) {
            rollback(&skills_root, &backups, &changes);
            if let Some(update) = &exclude_update {
                let _ = update.restore();
            }
            let _ = filesystem::remove_dir_all(&stage);
            return Err(Error::new(format!(
                "cannot remove stale installed skill {}: {error}",
                destination.display()
            )));
        }
        changes.push(AppliedChange::Removed { name: name.clone() });
    }

    filesystem::remove_dir_all(&stage)
        .map_err(|error| Error::new(format!("cannot remove {}: {error}", stage.display())))?;

    Ok(skills
        .iter()
        .map(|skill| format!("{} ({})", skill.name, skill.package_selector()))
        .collect())
}

fn apply_exclude_update(update: Option<&super::exclude::Update>) -> Result<()> {
    if let Some(update) = update {
        update.apply()?;
    }
    Ok(())
}

fn staging_path(skills_root: &Path) -> PathBuf {
    let sequence = STAGING_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    skills_root.join(format!(
        ".agent-context-staging-{}-{sequence}",
        std::process::id()
    ))
}

pub(crate) fn frontmatter_description(document: &Node, source: &Path) -> Result<Option<String>> {
    let Some(frontmatter) = context_frontmatter(document, source)? else {
        return Ok(None);
    };
    let Some(description) = frontmatter.description else {
        return Ok(None);
    };
    let description = description.trim();
    if description.is_empty() {
        return Ok(None);
    }
    Ok(Some(description.to_owned()))
}

fn context_frontmatter(document: &Node, source: &Path) -> Result<Option<ContextFrontmatter>> {
    let children = document
        .children()
        .expect("a Markdown document root always has children");
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

fn source_name(source: &Path) -> Result<String> {
    let Some(stem) = source.file_stem().and_then(|stem| stem.to_str()) else {
        return Err(Error::new(format!(
            "invalid skill filename: {}",
            source.display()
        )));
    };
    Ok(stem.to_owned())
}

fn write_staged_skill(skill: &Skill, destination: &Path) -> Result<()> {
    filesystem::create_dir_all(destination)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", destination.display())))?;

    if let Some(assets) = &skill.assets {
        copy_skill_assets(assets, destination, true)?;
    }

    let metadata = SkillFrontmatter {
        name: &skill.name,
        description: &skill.description,
        metadata: &skill.metadata,
    };
    let yaml = serde_yaml_ng::to_string(&metadata)
        .expect("skill front matter contains only serializable YAML values");
    let mut output = format!("---\n{yaml}---\n\n");
    output.push_str(&skill.body);
    if !output.ends_with('\n') {
        output.push('\n');
    }

    let skill_file = destination.join("SKILL.md");
    filesystem::write(&skill_file, output)
        .map_err(|error| Error::new(format!("cannot write {}: {error}", skill_file.display())))?;
    write_skill_owner(
        destination,
        &SkillOwner {
            ecosystem: "cargo".to_owned(),
            package: skill.package.name.clone(),
            version: skill.package.version.clone(),
        },
    )
}

fn copy_skill_assets(source: &Path, destination: &Path, top_level: bool) -> Result<()> {
    let entries = filesystem::read_dir(source)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", source.display())))?;
    let mut entries = entries.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        copy_skill_asset(&source_path, &destination_path, top_level)?;
    }

    Ok(())
}

fn copy_skill_asset(source: &Path, destination: &Path, top_level: bool) -> Result<()> {
    let metadata = inspect_skill_asset(source)?;
    let file_type = metadata.file_type();

    let name = source.file_name().unwrap_or_default().to_string_lossy();
    if top_level
        && (name.eq_ignore_ascii_case("SKILL.md") || name.eq_ignore_ascii_case(OWNERSHIP_FILE))
    {
        return Err(Error::new(format!(
            "{} is reserved for generated skill files",
            source.display()
        )));
    }

    if file_type.is_symlink() {
        return Err(Error::new(format!(
            "skill assets cannot contain symbolic links: {}",
            source.display()
        )));
    } else if file_type.is_dir() {
        filesystem::create_dir(destination).map_err(|error| {
            Error::new(format!("cannot create {}: {error}", destination.display()))
        })?;
        copy_skill_assets(source, destination, false)?;
    } else if file_type.is_file() {
        filesystem::copy(source, destination)
            .map(|_| ())
            .map_err(|error| {
                Error::new(format!(
                    "cannot copy {} to {}: {error}",
                    source.display(),
                    destination.display()
                ))
            })?;
    } else {
        return Err(Error::new(format!(
            "unsupported skill asset: {}",
            source.display()
        )));
    }
    Ok(())
}

fn inspect_skill_asset(path: &Path) -> Result<filesystem::Metadata> {
    filesystem::symlink_metadata(path)
        .map_err(|error| Error::new(format!("cannot inspect {}: {error}", path.display())))
}

fn ensure_directory(path: &Path) -> Result<()> {
    match filesystem::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(Error::new(format!(
            "skill installation path {} is not a regular directory",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match filesystem::create_dir_all(path) {
                Ok(()) => Ok(()),
                Err(error) => Err(Error::new(format!(
                    "cannot create {}: {error}",
                    path.display()
                ))),
            }
        }
        Err(error) => Err(Error::new(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    }
}

pub(crate) fn installed_skill_names(root: &Path) -> Result<Vec<String>> {
    Ok(installed_skill_owners(&root.join(".agents/skills"))?
        .into_keys()
        .collect())
}

fn write_skill_owner(directory: &Path, owner: &SkillOwner) -> Result<()> {
    let path = directory.join(OWNERSHIP_FILE);
    let mut bytes = serde_json::to_vec_pretty(owner).expect("skill ownership is serializable");
    bytes.push(b'\n');
    filesystem::write(&path, bytes)
        .map_err(|error| Error::new(format!("cannot write {}: {error}", path.display())))
}

fn load_skill_owner(directory: &Path) -> Result<Option<SkillOwner>> {
    let path = directory.join(OWNERSHIP_FILE);
    match filesystem::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => {
            return Err(Error::new(format!(
                "skill ownership {} is not a regular file",
                path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::new(format!(
                "cannot inspect {}: {error}",
                path.display()
            )));
        }
    }
    let bytes = filesystem::read(&path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let owner: SkillOwner = serde_json::from_slice(&bytes).map_err(|error| {
        Error::new(format!(
            "invalid skill ownership {}: {error}",
            path.display()
        ))
    })?;
    if owner.ecosystem.trim().is_empty()
        || owner.package.trim().is_empty()
        || owner.version.trim().is_empty()
    {
        return Err(Error::new(format!("invalid owner in {}", path.display())));
    }
    Ok(Some(owner))
}

fn installed_skill_owners(root: &Path) -> Result<BTreeMap<String, SkillOwner>> {
    match filesystem::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(Error::new(format!(
                "skill installation path {} is not a regular directory",
                root.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(Error::new(format!(
                "cannot inspect {}: {error}",
                root.display()
            )));
        }
    }
    let entries = filesystem::read_dir(root)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", root.display())))?;
    let mut owners = BTreeMap::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(owner) = load_skill_owner(&entry.path())? {
            let name = entry.file_name().to_string_lossy().into_owned();
            validate_skill_name(&name)?;
            owners.insert(name, owner);
        }
    }
    Ok(owners)
}

fn path_exists(path: &Path) -> Result<bool> {
    match filesystem::symlink_metadata(path) {
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
                    let _ = filesystem::rename(backups.join(name), destination);
                }
            }
            AppliedChange::Removed { name } => {
                let _ = filesystem::rename(backups.join(name), root.join(name));
            }
        }
    }
}

fn remove_existing(path: &Path) -> Result<()> {
    let metadata = match filesystem::symlink_metadata(path) {
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
        filesystem::remove_dir_all(path)
    } else {
        filesystem::remove_file(path)
    };
    result.map_err(|error| Error::new(format!("cannot remove {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn package(root: &Path, name: &str, version: &str) -> ContextPackage {
        let context_path = root.join(format!("{name}-{version}/context"));
        fs::create_dir_all(&context_path).unwrap();
        super::super::installer::ContextPackage::for_test(name, version, context_path)
    }

    fn make_installer(root: &Path, packages: Vec<ContextPackage>) -> Installer {
        super::super::installer::Installer::for_test(root, packages)
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn skill_document(description: &str, body: &str) -> String {
        format!("---\ntype: skill\ndescription: {description}\n---\n\n{body}")
    }

    fn write_skill(package: &ContextPackage, file: &str, contents: &str) {
        write(&package.context_path, file, contents);
    }

    fn error_for_document(name: &str, file: &str, contents: &str) -> String {
        let directory = tempdir().unwrap();
        let package = package(directory.path(), name, "1.0.0");
        write_skill(&package, file, contents);
        list_package_skills(&package).err().unwrap().to_string()
    }

    #[test]
    fn discovers_skills_in_sorted_order_with_normalized_package_prefixes() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let later = package(root, "zeta", "1.0.0");
        let earlier = package(root, "alpha_provider", "2.0.0");
        write_skill(
            &later,
            "second.md",
            &skill_document(" Second skill. ", "# Second"),
        );
        write_skill(
            &earlier,
            "first.md",
            &skill_document("First skill.", "# First"),
        );
        write_skill(&earlier, "ordinary.md", "# Ordinary context\n");
        write_skill(
            &earlier,
            "other-type.md",
            "---\ndescription: not opted in\n---\n\n# Ordinary\n",
        );

        let installer = make_installer(root, vec![later, earlier]);
        let skills = list_skills(&installer, None).unwrap();
        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha-provider-first", "zeta-second",]
        );
        assert_eq!(skills[0].package_selector(), "alpha_provider@2.0.0");
        assert_eq!(skills[0].description, "First skill.");
        assert!(list_skills(&installer, Some("missing")).unwrap().is_empty());
        assert_eq!(
            list_skills(&installer, Some("zeta@1.0.0")).unwrap().len(),
            1
        );
    }

    #[test]
    fn sorts_skills_by_provider_name_version_and_skill_name() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let newer = package(root, "provider", "2.0.0");
        let older = package(root, "provider", "1.0.0");
        let alpha = package(root, "alpha", "1.0.0");
        write_skill(&newer, "first.md", &skill_document("First.", "# First"));
        write_skill(&older, "zeta.md", &skill_document("Zeta.", "# Zeta"));
        write_skill(&older, "alpha.md", &skill_document("Alpha.", "# Alpha"));
        write_skill(&alpha, "one.md", &skill_document("One.", "# One"));

        let skills = list_skills(&make_installer(root, vec![newer, older, alpha]), None).unwrap();
        let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "alpha-one",
                "provider-alpha",
                "provider-zeta",
                "provider-first"
            ]
        );
    }

    #[test]
    fn rejects_unsupported_nested_invalid_and_incomplete_skill_documents() {
        assert!(
            error_for_document(
                "provider",
                "unsupported.md",
                "---\ntype: guide\ndescription: Guide.\n---\n\n# Guide\n"
            )
            .contains("unsupported context type")
        );
        assert!(
            error_for_document(
                "provider",
                "nested/skill.md",
                &skill_document("Nested skill.", "# Skill\n")
            )
            .contains("directly inside context")
        );
        assert!(
            error_for_document(
                "provider",
                "Bad_Name.md",
                &skill_document("Invalid name.", "# Skill\n")
            )
            .contains("invalid skill name")
        );
        assert!(
            error_for_document(
                &"p".repeat(61),
                "name.md",
                &skill_document("Long prefix.", "# Skill\n")
            )
            .contains("invalid skill name")
        );
        assert!(
            error_for_document(
                "provider",
                "missing-description.md",
                "---\ntype: skill\n---\n\n# Skill\n"
            )
            .contains("requires a non-empty")
        );
        assert!(
            error_for_document(
                "provider",
                "blank-description.md",
                "---\ntype: skill\ndescription: '  '\n---\n\n# Skill\n"
            )
            .contains("requires a non-empty")
        );
        assert!(
            error_for_document(
                "provider",
                "long-description.md",
                &skill_document(&"x".repeat(1025), "# Skill\n")
            )
            .contains("1024 character limit")
        );
    }

    #[test]
    fn propagates_skill_discovery_errors_through_public_operations() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(
            &provider,
            "unsupported.md",
            "---\ntype: guide\ndescription: Guide.\n---\n\n# Guide\n",
        );
        let installer = make_installer(root, vec![provider]);

        assert!(
            list_skills(&installer, None)
                .unwrap_err()
                .to_string()
                .contains("unsupported context type")
        );
        assert!(
            install_skills(&installer, None, None)
                .unwrap_err()
                .to_string()
                .contains("unsupported context type")
        );
        assert!(
            install_skills(&installer, Some("provider@1.0.0"), None)
                .unwrap_err()
                .to_string()
                .contains("unsupported context type")
        );
    }

    #[test]
    fn reports_context_directory_read_errors_during_skill_discovery() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        fs::remove_dir(&provider.context_path).unwrap();
        fs::write(&provider.context_path, "not a directory").unwrap();

        assert!(
            list_package_skills(&provider)
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );
    }

    #[cfg(unix)]
    #[test]
    fn parses_invalid_unicode_filenames_without_creating_them_on_disk() {
        use std::os::unix::ffi::OsStrExt;

        let directory = tempdir().unwrap();
        let provider = package(directory.path(), "provider", "1.0.0");
        let source = provider
            .context_path
            .join(std::ffi::OsStr::from_bytes(b"invalid\xff.md"));
        let error = parse_skill_document(
            &provider,
            &source,
            &skill_document("Invalid filename.", "# Skill\n"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("invalid skill filename"));
    }

    #[test]
    fn validates_skill_assets_and_frontmatter_structure() {
        let directory = tempdir().unwrap();
        let package = package(directory.path(), "provider", "1.0.0");
        write_skill(&package, "asset.md", &skill_document("Asset.", "# Asset\n"));
        fs::write(package.context_path.join("asset"), "not a directory").unwrap();
        assert!(
            list_package_skills(&package)
                .err()
                .unwrap()
                .to_string()
                .contains("is not a directory")
        );

        let blocker = directory.path().join("asset-parent-is-file");
        fs::write(&blocker, "file").unwrap();
        let failure_path = blocker.join("skill");
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });
        assert!(
            skill_assets_path(&blocker, "skill")
                .err()
                .unwrap()
                .to_string()
                .contains("cannot inspect skill assets")
        );

        let no_frontmatter = to_mdast("# Heading\n", &ParseOptions::default()).unwrap();
        assert!(
            context_frontmatter(&no_frontmatter, Path::new("plain.md"))
                .unwrap()
                .is_none()
        );
        assert!(
            frontmatter_description(&no_frontmatter, Path::new("plain.md"))
                .unwrap()
                .is_none()
        );
        assert!(
            parse_skill_document(&package, Path::new("plain.md"), "# Plain\n")
                .unwrap()
                .is_none()
        );

        assert!(validate_skill_name("valid-name-12").is_ok());
        for invalid in [
            "",
            "-first",
            "last-",
            "double--dash",
            "Upper",
            "white space",
            &"a".repeat(65),
        ] {
            assert!(validate_skill_name(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn normalizes_frontmatter_descriptions_and_omits_blank_descriptions() {
        let mut options = ParseOptions::default();
        options.constructs.frontmatter = true;
        let document = to_mdast(
            "---\ndescription: \"  A useful guide.  \"\n---\n\n# Guide\n",
            &options,
        )
        .unwrap();
        assert_eq!(
            frontmatter_description(&document, Path::new("guide.md")).unwrap(),
            Some("A useful guide.".to_owned())
        );

        let document = to_mdast("---\ndescription: \"   \"\n---\n\n# Guide\n", &options).unwrap();
        assert_eq!(
            frontmatter_description(&document, Path::new("guide.md")).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn reports_invalid_filenames_and_skill_asset_inspection_errors() {
        use std::os::unix::ffi::OsStrExt;

        assert_eq!(
            source_name(Path::new("valid-name.md")).unwrap(),
            "valid-name"
        );
        let invalid_filename = Path::new(std::ffi::OsStr::from_bytes(b"invalid\xff.md"));
        assert!(
            source_name(invalid_filename)
                .unwrap_err()
                .to_string()
                .contains("invalid skill filename")
        );

        let directory = tempdir().unwrap();
        let error = inspect_skill_asset(&directory.path().join("missing/assets"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot inspect"));
    }

    #[cfg(unix)]
    #[test]
    fn reports_skill_removal_errors() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let parent = directory.path().join("skills");
        fs::create_dir(&parent).unwrap();
        let skill = parent.join("installed");
        fs::write(&skill, "previous skill").unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let result = remove_existing(&skill);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.unwrap_err().to_string().contains("cannot remove"));
    }

    #[test]
    fn staged_skill_includes_assets_frontmatter_and_final_newline() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let assets = root.join("assets");
        fs::create_dir_all(assets.join("references")).unwrap();
        fs::write(assets.join("references/guide.md"), "Guide.\n").unwrap();
        fs::write(assets.join("alpha.txt"), "Alpha asset.\n").unwrap();
        fs::write(assets.join("zeta.txt"), "Zeta asset.\n").unwrap();
        let package = package(root, "provider", "1.0.0");
        let skill = Skill {
            metadata: BTreeMap::new(),
            name: "provider-example".to_owned(),
            description: "Example skill.".to_owned(),
            package,
            source_name: "example".to_owned(),
            assets: Some(assets),
            body: "# Example".to_owned(),
        };
        let destination = root.join("installed/provider-example");
        write_staged_skill(&skill, &destination).unwrap();
        let markdown = fs::read_to_string(destination.join("SKILL.md")).unwrap();
        assert!(
            markdown
                .starts_with("---\nname: provider-example\ndescription: Example skill.\n---\n\n")
        );
        assert!(markdown.ends_with("# Example\n"));
        assert_eq!(
            fs::read_to_string(destination.join("references/guide.md")).unwrap(),
            "Guide.\n"
        );
        assert_eq!(
            fs::read_to_string(destination.join("alpha.txt")).unwrap(),
            "Alpha asset.\n"
        );
        assert_eq!(
            fs::read_to_string(destination.join("zeta.txt")).unwrap(),
            "Zeta asset.\n"
        );

        let invalid_assets = root.join("invalid-assets");
        fs::write(&invalid_assets, "not a directory").unwrap();
        let invalid_skill = Skill {
            metadata: BTreeMap::new(),
            assets: Some(invalid_assets),
            ..skill.clone()
        };
        assert!(
            write_staged_skill(&invalid_skill, &root.join("invalid-install"))
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );

        let bad_destination = root.join("blocked");
        fs::write(&bad_destination, "file").unwrap();
        assert!(write_staged_skill(&skill, &bad_destination).is_err());

        let blocked_skill = root.join("blocked-skill");
        fs::create_dir_all(blocked_skill.join("SKILL.md")).unwrap();
        assert!(
            write_staged_skill(&skill, &blocked_skill)
                .unwrap_err()
                .to_string()
                .contains("cannot write")
        );
    }

    #[test]
    fn reports_skill_asset_directory_entry_errors() {
        let directory = tempdir().unwrap();
        let assets = directory.path().join("assets");
        let destination = directory.path().join("destination");
        fs::create_dir(&assets).unwrap();
        fs::create_dir(&destination).unwrap();

        let failure_path = assets.clone();
        let _failure =
            filesystem::fail_once(filesystem::Operation::ReadDirectoryEntry, move |path| {
                path == failure_path
            });

        let error = copy_skill_assets(&assets, &destination, true).unwrap_err();

        assert!(error.to_string().contains("injected filesystem failure"));
    }

    #[cfg(unix)]
    #[test]
    fn reports_context_document_read_errors() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let package = package(directory.path(), "provider", "1.0.0");
        let document = package.context_path.join("unreadable.md");
        fs::write(&document, "---\ntype: skill\ndescription: Skill.\n---\n").unwrap();
        let mut permissions = fs::metadata(&document).unwrap().permissions();
        permissions.set_mode(0o0);
        fs::set_permissions(&document, permissions).unwrap();

        let error = list_package_skills(&package).unwrap_err();
        assert!(error.to_string().contains("cannot read"));

        let mut permissions = fs::metadata(&document).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&document, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_reserved_files_and_unsupported_skill_assets() {
        use std::os::unix::fs::symlink;
        use std::process::Command;

        let directory = tempdir().unwrap();
        let root = directory.path();
        let assets = root.join("assets");
        let destination = root.join("destination");
        fs::create_dir_all(&assets).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let target = root.join("target");
        fs::write(&target, "target").unwrap();
        symlink(&target, assets.join("link")).unwrap();
        assert!(
            copy_skill_assets(&assets, &destination, true)
                .err()
                .unwrap()
                .to_string()
                .contains("symbolic links")
        );

        fs::remove_file(assets.join("link")).unwrap();
        fs::write(assets.join("skill.MD"), "reserved").unwrap();
        assert!(
            copy_skill_assets(&assets, &destination, true)
                .err()
                .unwrap()
                .to_string()
                .contains("reserved")
        );

        fs::remove_file(assets.join("skill.MD")).unwrap();
        let socket_path = assets.join("socket");
        assert!(
            Command::new("mkfifo")
                .arg(&socket_path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            copy_skill_assets(&assets, &destination, true)
                .err()
                .unwrap()
                .to_string()
                .contains("unsupported skill asset")
        );
        fs::remove_file(socket_path).unwrap();

        let nested = assets.join("nested");
        fs::create_dir(&nested).unwrap();
        let nested_socket_path = nested.join("socket");
        assert!(
            Command::new("mkfifo")
                .arg(&nested_socket_path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            copy_skill_assets(&assets, &destination, true)
                .unwrap_err()
                .to_string()
                .contains("unsupported skill asset")
        );
        fs::remove_file(nested_socket_path).unwrap();
        fs::remove_dir(nested).unwrap();

        let copy_source = root.join("copy-source");
        let copy_destination = root.join("copy-destination");
        fs::create_dir_all(&copy_source).unwrap();
        fs::create_dir_all(copy_destination.join("note.txt")).unwrap();
        fs::write(copy_source.join("note.txt"), "note").unwrap();
        assert!(
            copy_skill_assets(&copy_source, &copy_destination, true)
                .err()
                .unwrap()
                .to_string()
                .contains("cannot copy")
        );

        let missing = root.join("missing");
        assert!(copy_skill_assets(&missing, &destination, true).is_err());
        assert!(
            copy_skill_asset(&missing, &destination.join("missing"), true)
                .unwrap_err()
                .to_string()
                .contains("cannot inspect")
        );
        fs::create_dir(assets.join("folder")).unwrap();
        fs::create_dir(destination.join("folder")).unwrap();
        assert!(copy_skill_assets(&assets, &destination, true).is_err());
    }

    #[test]
    fn validates_skill_ownership_and_installation_directory_states() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        assert!(load_skill_owner(root).unwrap().is_none());
        assert!(installed_skill_names(root).unwrap().is_empty());
        let marker = root.join(OWNERSHIP_FILE);
        fs::create_dir(&marker).unwrap();
        assert!(
            load_skill_owner(root)
                .unwrap_err()
                .to_string()
                .contains("not a regular file")
        );
        fs::remove_dir(&marker).unwrap();
        for invalid in [
            "not json",
            "[]",
            r#"{"ecosystem":2,"package":"provider","version":"1"}"#,
            r#"{"ecosystem":"cargo","package":"provider"}"#,
        ] {
            fs::write(&marker, invalid).unwrap();
            assert!(load_skill_owner(root).is_err());
        }
        let failure_path = marker.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });
        assert!(
            load_skill_owner(root)
                .unwrap_err()
                .to_string()
                .contains("cannot inspect")
        );
        assert!(path_exists(&marker).unwrap());
        assert!(!path_exists(&root.join("absent")).unwrap());
        let child = marker.join("child");
        let failure_path = child.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });
        assert!(path_exists(&child).is_err());
        let failure_path = child.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });
        assert!(remove_existing(&child).is_err());
        assert!(ensure_directory(&marker).is_err());
        assert!(ensure_directory(&child).is_err());
        let new_directory = root.join("new-directory");
        ensure_directory(&new_directory).unwrap();
        ensure_directory(&new_directory).unwrap();
        remove_existing(&marker).unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn reconciles_owned_skills_and_rejects_missing_or_ambiguous_selections() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let first = package(root, "provider", "1.0.0");
        let second = package(root, "provider", "2.0.0");
        write_skill(&first, "one.md", &skill_document("One.", "# One\n"));
        write_skill(&second, "one.md", &skill_document("One v2.", "# One\n"));
        let installer = make_installer(root, vec![first.clone(), second.clone()]);

        assert!(list_skills(&installer, Some("provider")).is_err());
        assert!(install_skills(&installer, Some("provider"), None).is_err());

        let error = install_skills(&installer, None, Some("provider-one"))
            .err()
            .unwrap();
        assert!(error.to_string().contains("provided by multiple crates"));
        let error = install_skills(&installer, None, None).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("multiple selected crates provide skill")
        );
        assert!(install_skills(&installer, None, Some("unknown")).is_err());
        assert!(install_skills(&installer, Some("missing"), None).is_err());

        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(&provider, "one.md", &skill_document("One.", "# One\n"));
        let installer = make_installer(root, vec![provider.clone()]);
        let skills_root = root.join(".agents/skills");
        fs::create_dir_all(&skills_root).unwrap();
        seed_owner(
            &skills_root.join("provider-one"),
            "cargo",
            "different-provider",
            "1.0.0",
        );
        assert!(
            install_skills(&installer, None, None)
                .err()
                .unwrap()
                .to_string()
                .contains("already installed from cargo package")
        );

        let directory = tempdir().unwrap();
        let root = directory.path();
        let empty = make_installer(root, Vec::new());
        assert!(install_skills(&empty, None, None).unwrap().is_empty());
        assert!(install_skills(&empty, None, None).unwrap().is_empty());
        assert!(installed_skill_names(root).unwrap().is_empty());
    }

    #[test]
    fn reconciliation_removes_stale_skills_and_preserves_other_packages() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        let other = package(root, "other", "1.0.0");
        let installer = make_installer(root, vec![provider.clone(), other]);
        let skills_root = root.join(".agents/skills");
        fs::create_dir_all(skills_root.join("provider-stale")).unwrap();
        fs::create_dir_all(skills_root.join("other-stale")).unwrap();
        seed_owner(
            &skills_root.join("provider-stale"),
            "cargo",
            "provider",
            "0.9.0",
        );
        seed_owner(&skills_root.join("other-stale"), "cargo", "other", "0.9.0");

        assert!(
            install_skills(&installer, Some("provider@1.0.0"), None)
                .unwrap()
                .is_empty()
        );
        assert!(!skills_root.join("provider-stale").exists());
        assert!(skills_root.join("other-stale").is_dir());
        let updated = installed_skill_owners(&skills_root).unwrap();
        assert!(!updated.contains_key("provider-stale"));

        assert!(updated.contains_key("other-stale"));
    }

    #[test]
    fn installing_one_skill_preserves_stale_skills_from_other_installations() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(&provider, "one.md", &skill_document("One.", "# One\n"));
        let installer = make_installer(root, vec![provider]);
        let skills_root = root.join(".agents/skills");
        let stale_skill = skills_root.join("provider-stale");
        fs::create_dir_all(&stale_skill).unwrap();
        fs::write(stale_skill.join("SKILL.md"), "stale skill\n").unwrap();
        seed_owner(&stale_skill, "cargo", "provider", "0.9.0");

        install_skills(&installer, None, Some("provider-one")).unwrap();

        assert!(stale_skill.join("SKILL.md").is_file());
        let owners = installed_skill_owners(&skills_root).unwrap();
        assert!(owners.contains_key("provider-stale"));
        assert!(owners.contains_key("provider-one"));
    }

    #[test]
    fn does_not_replace_a_project_owned_skill_directory() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(&provider, "one.md", &skill_document("One.", "# One\n"));
        let installer = make_installer(root, vec![provider]);
        let destination = root.join(".agents/skills/provider-one");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), "project-owned skill\n").unwrap();

        let error = install_skills(&installer, None, None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("not managed by Bake Agent Context")
        );
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "project-owned skill\n"
        );
        assert!(!root.join(".agents/skills").join(OWNERSHIP_FILE).exists());
    }

    #[test]
    fn rolls_back_installed_and_removed_skill_changes() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("skills");
        let backups = directory.path().join("backups");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&backups).unwrap();
        fs::create_dir(root.join("new")).unwrap();
        fs::write(root.join("new/SKILL.md"), "new").unwrap();
        fs::create_dir_all(backups.join("replaced")).unwrap();
        fs::write(backups.join("replaced/SKILL.md"), "old").unwrap();
        fs::create_dir_all(backups.join("removed")).unwrap();
        fs::write(backups.join("removed/SKILL.md"), "removed").unwrap();

        rollback(
            &root,
            &backups,
            &[
                AppliedChange::Installed {
                    name: "new".to_owned(),
                    had_previous: false,
                },
                AppliedChange::Installed {
                    name: "replaced".to_owned(),
                    had_previous: true,
                },
                AppliedChange::Removed {
                    name: "removed".to_owned(),
                },
            ],
        );
        assert!(!root.join("new").exists());
        assert_eq!(
            fs::read_to_string(root.join("replaced/SKILL.md")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(root.join("removed/SKILL.md")).unwrap(),
            "removed"
        );
    }

    fn transaction_fixture() -> (
        tempfile::TempDir,
        Installer,
        PathBuf,
        PathBuf,
        PathBuf,
        Vec<u8>,
    ) {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(
            &provider,
            "example.md",
            &skill_document("Updated example.", "# Updated example"),
        );
        let installer = make_installer(root, vec![provider]);

        let skills_root = root.join(".agents/skills");
        let previous_skill = skills_root.join("provider-example");
        let stale_skill = skills_root.join("provider-stale");
        fs::create_dir_all(&previous_skill).unwrap();
        fs::write(previous_skill.join("SKILL.md"), "previous version\n").unwrap();
        fs::create_dir_all(&stale_skill).unwrap();
        fs::write(stale_skill.join("SKILL.md"), "stale skill\n").unwrap();

        let ownership_path = previous_skill.join(OWNERSHIP_FILE);
        seed_owner(&previous_skill, "cargo", "provider", "0.9.0");
        seed_owner(&stale_skill, "cargo", "provider", "0.9.0");
        let previous_ownership = fs::read(&ownership_path).unwrap();

        (
            directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        )
    }

    fn fresh_install_fixture() -> (tempfile::TempDir, Installer, PathBuf, PathBuf) {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(
            &provider,
            "example.md",
            &skill_document("Example.", "# Example"),
        );
        let installer = make_installer(root, vec![provider]);
        let skills_root = root.join(".agents/skills");
        let skill_path = skills_root.join("provider-example");
        let ownership_path = skill_path.join(OWNERSHIP_FILE);

        (directory, installer, skill_path, ownership_path)
    }

    fn assert_transaction_restored(
        previous_skill: &Path,
        stale_skill: &Path,
        ownership_path: &Path,
        previous_ownership: &[u8],
    ) {
        assert_eq!(
            fs::read_to_string(previous_skill.join("SKILL.md")).unwrap(),
            "previous version\n"
        );
        assert_eq!(
            fs::read_to_string(stale_skill.join("SKILL.md")).unwrap(),
            "stale skill\n"
        );
        assert_eq!(fs::read(ownership_path).unwrap(), previous_ownership);
    }

    #[test]
    fn restores_the_previous_skill_when_replacement_fails() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let failure_path = previous_skill.clone();
        let _failure =
            filesystem::fail_once(filesystem::Operation::RenameDestination, move |path| {
                path == failure_path
            });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot install skill"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn rolls_back_a_first_install_when_skill_rename_fails() {
        let (_directory, installer, skill_path, ownership_path) = fresh_install_fixture();
        let failure_path = skill_path.clone();
        let _failure =
            filesystem::fail_once(filesystem::Operation::RenameDestination, move |path| {
                path == failure_path
            });

        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot install skill"));
        assert!(!skill_path.exists());
        assert!(!ownership_path.exists());
    }

    #[test]
    fn preserves_the_previous_install_when_existing_skill_cannot_be_moved() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let failure_path = previous_skill.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::RenameSource, move |path| {
            path == failure_path
        });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot move existing skill"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn rolls_back_installed_skills_when_stale_skill_cannot_be_moved() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let failure_path = stale_skill.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::RenameSource, move |path| {
            path == failure_path
        });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cannot remove stale installed skill")
        );
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn preserves_installed_skills_when_staged_ownership_write_fails() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let _failure = filesystem::fail_once(filesystem::Operation::Write, |path| {
            path.file_name() == Some(std::ffi::OsStr::new(OWNERSHIP_FILE))
        });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot write"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn cleans_staging_directory_when_preparing_staging_files_fails() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let _failure = filesystem::fail_once(filesystem::Operation::CreateDirectory, |path| {
            path.file_name() == Some(std::ffi::OsStr::new("backups"))
        });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot prepare"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
        assert!(
            fs::read_dir(ownership_path.parent().unwrap().parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".agent-context-staging-"))
        );
    }

    #[test]
    fn reports_staging_directory_creation_failure() {
        let (_directory, installer, _, _, _, _) = transaction_fixture();
        let _failure = filesystem::fail_once(filesystem::Operation::CreateDirectory, |path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with(".agent-context-staging-"))
        });
        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot create"));
    }

    #[test]
    fn cleans_staging_files_when_writing_a_skill_fails() {
        let (_directory, installer, skill_path, ownership_path) = fresh_install_fixture();
        let skills_root = ownership_path.parent().unwrap().parent().unwrap();
        fs::create_dir_all(skills_root).unwrap();
        let unrelated_file = skills_root.join("unrelated-file");
        fs::write(&unrelated_file, "keep this file\n").unwrap();
        let _failure = filesystem::fail_once(filesystem::Operation::Write, |path| {
            path.file_name() == Some(std::ffi::OsStr::new("SKILL.md"))
        });

        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot write"));
        assert!(!skill_path.exists());
        assert!(!ownership_path.exists());
        assert!(unrelated_file.is_file());
        assert!(fs::read_dir(skills_root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".agent-context-staging-")
        }));
    }

    #[test]
    fn reports_directory_tree_creation_errors() {
        let directory = tempdir().unwrap();
        let destination = directory.path().join("created/nested");
        let failure_path = destination.clone();
        let _failure =
            filesystem::fail_once(filesystem::Operation::CreateDirectoryTree, move |path| {
                path == failure_path
            });

        let error = ensure_directory(&destination).unwrap_err();

        assert!(error.to_string().contains("cannot create"));
        assert!(!destination.exists());
    }

    #[test]
    fn reports_skill_ownership_read_errors() {
        let directory = tempdir().unwrap();
        seed_owner(directory.path(), "cargo", "provider", "1.0.0");
        let marker = directory.path().join(OWNERSHIP_FILE);
        let _failure =
            filesystem::fail_once(filesystem::Operation::Read, move |path| path == marker);
        assert!(
            load_skill_owner(directory.path())
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );
    }

    #[test]
    fn reports_destination_inspection_errors_before_changing_installed_skills() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let failure_path = previous_skill.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });

        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot inspect"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn reports_stale_skill_inspection_errors_before_changing_installed_skills() {
        let (
            _directory,
            installer,
            previous_skill,
            stale_skill,
            ownership_path,
            previous_ownership,
        ) = transaction_fixture();
        let failure_path = stale_skill.join(OWNERSHIP_FILE);
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });

        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot inspect"));
        assert_transaction_restored(
            &previous_skill,
            &stale_skill,
            &ownership_path,
            &previous_ownership,
        );
    }

    #[test]
    fn reports_staging_cleanup_errors_after_committing_the_install() {
        let (_directory, installer, previous_skill, stale_skill, _ownership_path, _) =
            transaction_fixture();
        let _failure = filesystem::fail_once(filesystem::Operation::RemoveDirectoryTree, |path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with(".agent-context-staging-"))
        });

        let error = install_skills(&installer, Some("provider@1.0.0"), None).unwrap_err();

        assert!(error.to_string().contains("cannot remove"));
        assert!(
            fs::read_to_string(previous_skill.join("SKILL.md"))
                .unwrap()
                .contains("# Updated example")
        );
        assert!(!stale_skill.exists());
        let owners = installed_skill_owners(previous_skill.parent().unwrap()).unwrap();
        assert_eq!(owners["provider-example"].version, "1.0.0");
        assert!(!owners.contains_key("provider-stale"));
    }

    #[cfg(unix)]
    #[test]
    fn handles_exclude_errors_before_installing_skills() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let root = directory.path();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        let exclude = root.join(".git/info/exclude");
        fs::remove_file(&exclude).unwrap();
        fs::create_dir(&exclude).unwrap();
        let provider = package(root, "provider", "1.0.0");
        write_skill(&provider, "one.md", &skill_document("One.", "# One\n"));
        let installer = make_installer(root, vec![provider]);
        let error = install_skills(&installer, None, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot read"), "{error}");
        assert!(!root.join(".agents/skills/provider-one").exists());

        let directory = tempdir().unwrap();
        let root = directory.path();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        let exclude = root.join(".git/info/exclude");
        fs::set_permissions(&exclude, fs::Permissions::from_mode(0o444)).unwrap();
        let provider = package(root, "provider", "1.0.0");
        write_skill(&provider, "one.md", &skill_document("One.", "# One\n"));
        let installer = make_installer(root, vec![provider]);
        let result = install_skills(&installer, None, None);
        fs::set_permissions(&exclude, fs::Permissions::from_mode(0o644)).unwrap();
        let error = result.unwrap_err().to_string();
        assert!(error.contains("cannot update"), "{error}");
        assert!(!root.join(".agents/skills/provider-one").exists());
        assert!(!root.join(".agents/skills").join(OWNERSHIP_FILE).exists());
    }

    #[test]
    fn preserves_extra_metadata_and_treats_resources_as_opaque_files() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let provider = package(root, "provider", "1.0.0");
        write_skill(
            &provider,
            "workflow.md",
            "---\ntype: skill\ndescription: Run workflow.\nlicense: MIT\nmetadata:\n  author: Provider\n---\n\n# Workflow\n",
        );
        write_skill(
            &provider,
            "workflow/references/guide.md",
            "---\ntype: resource\nlayout: example\n---\n\n# Resource\n",
        );
        let installer = make_installer(root, vec![provider]);
        install_skills(&installer, None, None).unwrap();
        let installed =
            fs::read_to_string(root.join(".agents/skills/provider-workflow/SKILL.md")).unwrap();
        assert!(installed.contains("license: MIT"));
        assert!(installed.contains("author: Provider"));
        assert!(
            root.join(".agents/skills/provider-workflow/references/guide.md")
                .is_file()
        );
    }

    #[test]
    fn refresh_preserves_foreign_ownership_and_refuses_foreign_collisions() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let foreign = root.join(".agents/skills/gem-workflow");
        seed_owner(&foreign, "gem", "provider", "1.0.0");
        fs::write(foreign.join("SKILL.md"), "Ruby skill").unwrap();
        let before = fs::read(foreign.join(OWNERSHIP_FILE)).unwrap();
        let provider = package(root, "provider", "2.0.0");
        write_skill(
            &provider,
            "workflow.md",
            &skill_document("Workflow.", "# Workflow\n"),
        );
        let installer = make_installer(root, vec![provider]);
        install_skills(&installer, None, None).unwrap();
        install_skills(&make_installer(root, Vec::new()), None, None).unwrap();
        assert_eq!(installed_skill_names(root).unwrap(), ["gem-workflow"]);
        assert_eq!(fs::read(foreign.join(OWNERSHIP_FILE)).unwrap(), before);
        assert_eq!(
            fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
            "Ruby skill"
        );
        seed_owner(
            &root.join(".agents/skills/provider-workflow"),
            "gem",
            "provider",
            "1.0.0",
        );
        assert!(install_skills(&installer, None, None).is_err());
    }

    #[test]
    fn rejects_incomplete_owners_before_installation() {
        for invalid in [
            r#"{"package":"provider","version":"1.0.0"}"#,
            r#"{"ecosystem":" ","package":"provider","version":"1.0.0"}"#,
            r#"{"ecosystem":"cargo","package":"","version":"1.0.0"}"#,
            r#"{"ecosystem":"cargo","package":"provider","version":""}"#,
        ] {
            let directory = tempdir().unwrap();
            let root = directory.path();
            write(root, ".agents/skills/provider-workflow/skill.json", invalid);
            write(
                root,
                ".agents/skills/provider-workflow/SKILL.md",
                "Existing instructions",
            );
            let installer = make_installer(root, Vec::new());
            assert!(install_skills(&installer, None, None).is_err());
            assert_eq!(
                fs::read_to_string(root.join(".agents/skills/provider-workflow/skill.json"))
                    .unwrap(),
                invalid
            );
            assert_eq!(
                fs::read_to_string(root.join(".agents/skills/provider-workflow/SKILL.md")).unwrap(),
                "Existing instructions"
            );
        }
    }

    #[test]
    fn reads_the_portable_skill_ownership_fixture() {
        let directory = tempdir().unwrap();
        let fixture = include_str!("../tests/fixtures/skill.json");
        fs::write(directory.path().join(OWNERSHIP_FILE), fixture).unwrap();
        let owner = load_skill_owner(directory.path()).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(owner).unwrap(),
            serde_json::from_str::<serde_json::Value>(fixture).unwrap()
        );
    }

    #[test]
    fn scans_ownership_and_validates_managed_directory_names() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let skills = root.join(".agents/skills");
        assert!(installed_skill_owners(&skills).unwrap().is_empty());
        fs::create_dir_all(skills.join("project-owned")).unwrap();
        fs::write(skills.join("unrelated"), "file").unwrap();
        seed_owner(
            &skills.join("provider-workflow"),
            "cargo",
            "provider",
            "1.0.0",
        );
        assert_eq!(installed_skill_names(root).unwrap(), ["provider-workflow"]);
        for name in ["Bad_Name".to_owned(), "a".repeat(65)] {
            seed_owner(&skills.join(&name), "cargo", "provider", "1.0.0");
            assert!(installed_skill_names(root).is_err());
            fs::remove_dir_all(skills.join(name)).unwrap();
        }
        let file_root = root.join("file-root");
        fs::write(&file_root, "file").unwrap();
        assert!(installed_skill_owners(&file_root).is_err());
        let failure_path = skills.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::Inspect, move |path| {
            path == failure_path
        });
        assert!(
            installed_skill_names(root)
                .unwrap_err()
                .to_string()
                .contains("cannot inspect")
        );
        let failure_path = skills.clone();
        let _failure = filesystem::fail_once(filesystem::Operation::ReadDirectory, move |path| {
            path == failure_path
        });
        assert!(
            installed_skill_names(root)
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );
        let failure_path = skills.clone();
        let _failure =
            filesystem::fail_once(filesystem::Operation::ReadDirectoryEntry, move |path| {
                path == failure_path
            });
        assert!(installed_skill_names(root).is_err());
        let failure_path = skills.join("project-owned");
        let _failure = filesystem::fail_once(filesystem::Operation::FileType, move |path| {
            path == failure_path
        });
        assert!(installed_skill_names(root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_ownership_symlinks_and_skips_linked_directories() {
        use std::os::unix::fs::symlink;
        let directory = tempdir().unwrap();
        let skills = directory.path().join(".agents/skills");
        let provider = skills.join("provider-workflow");
        seed_owner(&provider, "cargo", "provider", "1.0.0");
        symlink(&provider, skills.join("linked-skill")).unwrap();
        assert_eq!(
            installed_skill_names(directory.path()).unwrap(),
            ["provider-workflow"]
        );
        let linked_root = directory.path().join("linked-root");
        symlink(&skills, &linked_root).unwrap();
        assert!(installed_skill_owners(&linked_root).is_err());
        fs::remove_file(provider.join(OWNERSHIP_FILE)).unwrap();
        symlink("missing.json", provider.join(OWNERSHIP_FILE)).unwrap();
        assert!(installed_skill_names(directory.path()).is_err());
    }

    #[test]
    fn reserves_generated_resource_names_and_allows_nested_metadata() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("resources");
        let destination = directory.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        for name in ["skill.json", "SKILL.JSON", "SKILL.md"] {
            let path = source.join(name);
            fs::write(&path, "Conflict").unwrap();
            assert!(
                copy_skill_assets(&source, &destination, true)
                    .unwrap_err()
                    .to_string()
                    .contains("reserved")
            );
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(
                copy_skill_assets(&source, &destination, true)
                    .unwrap_err()
                    .to_string()
                    .contains("reserved")
            );
            fs::remove_dir(path).unwrap();
        }
        fs::create_dir(source.join("references")).unwrap();
        fs::write(source.join("references/skill.json"), "Opaque resource").unwrap();
        copy_skill_assets(&source, &destination, true).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("references/skill.json")).unwrap(),
            "Opaque resource"
        );
    }

    #[test]
    fn restores_git_exclusions_and_all_skills_when_commit_fails() {
        for failure in 0..3 {
            let (directory, installer, previous_skill, stale_skill, marker, original_marker) =
                transaction_fixture();
            let root = directory.path();
            assert!(
                std::process::Command::new("git")
                    .args(["init", "--quiet"])
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
            let exclude = root.join(".git/info/exclude");
            let original = fs::read(&exclude).unwrap();
            let provider = &installer.packages()[0];
            write_skill(
                provider,
                "aardvark.md",
                &skill_document("New workflow.", "# New"),
            );
            let (operation, path) = match failure {
                0 => (filesystem::Operation::RenameSource, previous_skill.clone()),
                1 => (
                    filesystem::Operation::RenameDestination,
                    previous_skill.clone(),
                ),
                _ => (filesystem::Operation::RenameSource, stale_skill.clone()),
            };
            let _failure = filesystem::fail_once(operation, move |candidate| candidate == path);
            assert!(install_skills(&installer, None, None).is_err());
            assert_transaction_restored(&previous_skill, &stale_skill, &marker, &original_marker);
            assert_eq!(fs::read(&exclude).unwrap(), original);
            assert!(!root.join(".agents/skills/provider-aardvark").exists());
        }
    }

    fn seed_owner(directory: &Path, ecosystem: &str, package: &str, version: &str) {
        fs::create_dir_all(directory).unwrap();
        write_skill_owner(
            directory,
            &SkillOwner {
                ecosystem: ecosystem.to_owned(),
                package: package.to_owned(),
                version: version.to_owned(),
            },
        )
        .unwrap();
    }
}
