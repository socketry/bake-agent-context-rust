// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use bake::{Error, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// A resolved Cargo package that provides a top-level `context/` directory.
#[derive(Clone, Debug)]
pub struct ContextPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub context_path: PathBuf,
    selector: String,
}

impl ContextPackage {
    /// The name accepted by the task's `--package` option.
    pub fn selector(&self) -> &str {
        &self.selector
    }
}

/// A context file relative to its provider's `context/` directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextFile {
    pub path: PathBuf,
}

/// Discovers, lists, reads, and installs context from resolved Cargo dependencies.
#[derive(Clone, Debug)]
pub struct Installer {
    root: PathBuf,
    context_path: PathBuf,
    packages: Vec<ContextPackage>,
}

impl Installer {
    /// Resolve the project's Cargo packages and find dependencies with `context/` directories.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let manifest = root.join("Cargo.toml");
        let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--manifest-path",
            ])
            .arg(&manifest)
            .current_dir(&root);

        let output = command.output().map_err(|error| {
            Error::new(format!(
                "cannot run cargo metadata for {}: {error}",
                manifest.display()
            ))
        })?;
        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(Error::new(format!(
                "cargo metadata failed for {} ({}): {}",
                manifest.display(),
                output.status,
                if details.is_empty() {
                    "run cargo check to resolve and lock the project's dependencies".to_owned()
                } else {
                    details
                }
            )));
        }

        let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
            .map_err(|error| Error::new(format!("cannot parse cargo metadata: {error}")))?;
        let workspace_members: HashSet<_> = metadata.workspace_members.into_iter().collect();
        let resolved_packages: HashSet<_> = metadata
            .resolve
            .map(|resolve| {
                resolve
                    .nodes
                    .into_iter()
                    .map(|node| node.package_id)
                    .collect()
            })
            .unwrap_or_default();

        let mut candidates = Vec::new();
        for package in metadata.packages {
            if workspace_members.contains(&package.package_id)
                || (!resolved_packages.is_empty()
                    && !resolved_packages.contains(&package.package_id))
            {
                continue;
            }

            let Some(package_root) = package.manifest_path.parent() else {
                continue;
            };
            let context_path = package_root.join("context");
            let context_metadata = match fs::symlink_metadata(&context_path) {
                Ok(metadata) if metadata.file_type().is_dir() => metadata,
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Error::new(format!(
                        "cannot inspect {}: {error}",
                        context_path.display()
                    )));
                }
            };
            if !context_metadata.is_dir() {
                continue;
            }

            candidates.push(ContextPackage {
                name: package.name,
                version: package.version,
                description: package.description,
                context_path,
                selector: String::new(),
            });
        }

        let mut name_counts = HashMap::new();
        for package in &candidates {
            *name_counts.entry(package.name.clone()).or_insert(0usize) += 1;
        }
        for package in &mut candidates {
            package.selector = if name_counts[&package.name] == 1 {
                package.name.clone()
            } else {
                format!("{}@{}", package.name, package.version)
            };
        }
        candidates.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.version.cmp(&right.version))
        });

        Ok(Self {
            context_path: root.join(".agents/context"),
            root,
            packages: candidates,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn context_path(&self) -> &Path {
        &self.context_path
    }

    pub fn packages(&self) -> &[ContextPackage] {
        &self.packages
    }

    pub fn find_package(&self, selector: &str) -> Result<Option<ContextPackage>> {
        let matches: Vec<_> = self
            .packages
            .iter()
            .filter(|package| package.selector == selector || package.name == selector)
            .collect();

        match matches.as_slice() {
            [] => Ok(None),
            [package] => Ok(Some((*package).clone())),
            _ => {
                let selectors = matches
                    .iter()
                    .map(|package| package.selector.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(Error::new(format!(
                    "multiple versions of crate {selector:?} provide context; choose one of: {selectors}"
                )))
            }
        }
    }

    pub fn list_context_files(&self, package: &ContextPackage) -> Result<Vec<ContextFile>> {
        let skill_names: HashSet<_> = super::skill::list_package_skills(package)?
            .into_iter()
            .map(|skill| skill.name)
            .collect();
        let mut files = Vec::new();
        collect_files(&package.context_path, &mut files)?;
        files.sort();
        Ok(files
            .into_iter()
            .filter_map(|file| {
                file.strip_prefix(&package.context_path)
                    .ok()
                    .map(|path| ContextFile {
                        path: path.to_path_buf(),
                    })
            })
            .filter(|file| !is_skill_context_path(&file.path, &skill_names))
            .collect())
    }

    pub fn show_context_file(&self, selector: &str, file: &str) -> Result<Option<String>> {
        let Some(package) = self.find_package(selector)? else {
            return Ok(None);
        };
        let Some(path) = find_context_file(&package.context_path, file)? else {
            return Ok(None);
        };

        let skill_names: HashSet<_> = super::skill::list_package_skills(&package)?
            .into_iter()
            .map(|skill| skill.name)
            .collect();
        let context_root = package.context_path.canonicalize().map_err(|error| {
            Error::new(format!(
                "cannot resolve {}: {error}",
                package.context_path.display()
            ))
        })?;
        let relative_path = path
            .strip_prefix(&context_root)
            .map_err(|error| Error::new(format!("cannot make context path relative: {error}")))?;
        if is_skill_context_path(relative_path, &skill_names) {
            return Ok(None);
        }

        fs::read_to_string(&path)
            .map(Some)
            .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))
    }

    /// Install one package's context. Returns `false` when it does not provide context.
    pub fn install_package(&self, selector: &str) -> Result<bool> {
        let Some(package) = self.find_package(selector)? else {
            return Ok(false);
        };
        let skills = super::skill::list_package_skills(&package)?;
        let skill_names: HashSet<_> = skills.into_iter().map(|skill| skill.name).collect();

        fs::create_dir_all(&self.context_path).map_err(|error| {
            Error::new(format!(
                "cannot create {}: {error}",
                self.context_path.display()
            ))
        })?;
        let destination = self.context_path.join(&package.selector);
        remove_existing(&destination)?;
        let copied = copy_context_tree(&package.context_path, &destination, &skill_names, true)?;
        if !copied {
            remove_existing(&destination)?;
        }
        Ok(copied)
    }

    /// Install all resolved dependency packages that provide context.
    pub fn install_all(&self) -> Result<Vec<String>> {
        let mut installed = Vec::new();
        for package in &self.packages {
            if self.install_package(&package.selector)? {
                installed.push(package.selector.clone());
            }
        }
        Ok(installed)
    }
}

#[derive(Deserialize)]
struct CargoMetadata {
    workspace_members: Vec<String>,
    packages: Vec<CargoPackage>,
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Deserialize)]
struct ResolveNode {
    #[serde(rename = "id")]
    package_id: String,
}

#[derive(Deserialize)]
struct CargoPackage {
    #[serde(rename = "id")]
    package_id: String,
    name: String,
    version: String,
    description: Option<String>,
    manifest_path: PathBuf,
}

fn find_context_file(context_path: &Path, file: &str) -> Result<Option<PathBuf>> {
    let requested = Path::new(file);
    if requested.is_absolute()
        || requested
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::new(
            "context file must be a relative path inside context/",
        ));
    }

    let mut candidates = vec![context_path.join(requested)];
    if requested.extension().is_none() {
        candidates.push(context_path.join(requested).with_extension("md"));
    }

    for candidate in candidates {
        let Ok(canonical_candidate) = candidate.canonicalize() else {
            continue;
        };
        let canonical_root = context_path.canonicalize().map_err(|error| {
            Error::new(format!(
                "cannot resolve {}: {error}",
                context_path.display()
            ))
        })?;
        if !canonical_candidate.starts_with(&canonical_root) || !canonical_candidate.is_file() {
            continue;
        }
        return Ok(Some(canonical_candidate));
    }

    Ok(None)
}

pub(crate) fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_markdown_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", directory.display())))?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", directory.display())))?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn copy_context_tree(
    source: &Path,
    destination: &Path,
    skill_names: &HashSet<String>,
    root: bool,
) -> Result<bool> {
    let source_type = fs::symlink_metadata(source)
        .map_err(|error| Error::new(format!("cannot inspect {}: {error}", source.display())))?
        .file_type();
    if !source_type.is_dir() {
        return Err(Error::new(format!(
            "context provider {} is not a regular directory",
            source.display()
        )));
    }

    fs::create_dir_all(destination)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", destination.display())))?;
    let entries = fs::read_dir(source)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", source.display())))?;
    let mut copied = false;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if file_type.is_dir() {
            if root
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| skill_names.contains(name))
            {
                continue;
            }

            if copy_context_tree(&source_path, &destination_path, skill_names, false)? {
                copied = true;
            } else {
                fs::remove_dir(&destination_path).map_err(|error| {
                    Error::new(format!(
                        "cannot remove empty context directory {}: {error}",
                        destination_path.display()
                    ))
                })?;
            }
        } else if file_type.is_file() {
            if root && is_skill_markdown(&source_path, skill_names) {
                continue;
            }
            fs::copy(&source_path, &destination_path).map_err(|error| {
                Error::new(format!(
                    "cannot copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                ))
            })?;
            copied = true;
        }
    }
    Ok(copied)
}

fn is_skill_context_path(path: &Path, skill_names: &HashSet<String>) -> bool {
    let mut components = path.components();
    let Some(first) = components
        .next()
        .and_then(|component| component.as_os_str().to_str())
    else {
        return false;
    };
    if skill_names.contains(first) {
        return true;
    }

    components.next().is_none() && is_skill_markdown(path, skill_names)
}

fn is_skill_markdown(path: &Path, skill_names: &HashSet<String>) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| skill_names.contains(stem))
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
