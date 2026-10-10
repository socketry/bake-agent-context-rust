// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

#[cfg(test)]
use super::test_filesystem as fs;
use bake::{Error, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
#[cfg(not(test))]
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

    #[cfg(test)]
    pub(super) fn for_test(name: &str, version: &str, context_path: PathBuf) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            description: Some(format!("{name} documentation")),
            context_path,
            selector: format!("{name}@{version}"),
        }
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
    pub fn discover(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let cargo = cargo_executable(env::var_os("CARGO"));
        Self::discover_with_cargo(root, cargo)
    }

    fn discover_with_cargo(root: PathBuf, cargo: OsString) -> Result<Self> {
        let manifest = root.join("Cargo.toml");
        let output = run_cargo_metadata(&root, &cargo)?;
        if !output.status.success() {
            return Err(cargo_metadata_failure(
                &manifest,
                output.status,
                &output.stderr,
            ));
        }

        let metadata = parse_metadata(&output.stdout)?;
        Self::discover_from_metadata(root, metadata)
    }

    #[cfg(test)]
    pub(super) fn for_test(root: &Path, packages: Vec<ContextPackage>) -> Self {
        Self {
            root: root.to_path_buf(),
            context_path: root.join(".agents/context"),
            packages,
        }
    }

    fn discover_from_metadata(root: PathBuf, metadata: CargoMetadata) -> Result<Self> {
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
            match fs::symlink_metadata(&context_path) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Error::new(format!(
                        "cannot inspect {}: {error}",
                        context_path.display()
                    )));
                }
            };
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
        let mut files = Vec::new();
        collect_files(&package.context_path, &mut files)?;
        files.sort();

        let skill_names: HashSet<_> = super::skill::list_package_skills(package)?
            .into_iter()
            .map(|skill| skill.source_name)
            .collect();
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
        let Some((context_root, path)) = find_context_file_with_root(&package.context_path, file)?
        else {
            return Ok(None);
        };

        let skill_names: HashSet<_> = super::skill::list_package_skills(&package)?
            .into_iter()
            .map(|skill| skill.source_name)
            .collect();
        let relative_path = path
            .strip_prefix(&context_root)
            .expect("find_context_file only returns paths within context_root");
        if is_skill_context_path(relative_path, &skill_names) {
            return Ok(None);
        }

        read_context_file(&path)
    }

    /// Install one package's context. Returns `false` when it does not provide context.
    pub fn install_package(&self, selector: &str) -> Result<bool> {
        let Some(package) = self.find_package(selector)? else {
            return Ok(false);
        };
        let skills = super::skill::list_package_skills(&package)?;
        let skill_names: HashSet<_> = skills.into_iter().map(|skill| skill.source_name).collect();

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

fn run_cargo_metadata(root: &Path, cargo: &OsStr) -> Result<std::process::Output> {
    let manifest = root.join("Cargo.toml");
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
        .current_dir(root);

    command.output().map_err(|error| {
        Error::new(format!(
            "cannot run cargo metadata for {}: {error}",
            manifest.display()
        ))
    })
}

fn cargo_executable(cargo: Option<OsString>) -> OsString {
    cargo.unwrap_or_else(|| "cargo".into())
}

fn cargo_metadata_failure(manifest: &Path, status: impl std::fmt::Display, stderr: &[u8]) -> Error {
    let details = String::from_utf8_lossy(stderr).trim().to_owned();
    Error::new(format!(
        "cargo metadata failed for {} ({}): {}",
        manifest.display(),
        status,
        if details.is_empty() {
            "run cargo check to resolve and lock the project's dependencies".to_owned()
        } else {
            details
        }
    ))
}

fn parse_metadata(stdout: &[u8]) -> Result<CargoMetadata> {
    serde_json::from_slice(stdout)
        .map_err(|error| Error::new(format!("cannot parse cargo metadata: {error}")))
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

#[cfg(test)]
fn find_context_file(context_path: &Path, file: &str) -> Result<Option<PathBuf>> {
    Ok(find_context_file_with_root(context_path, file)?.map(|(_, path)| path))
}

fn find_context_file_with_root(
    context_path: &Path,
    file: &str,
) -> Result<Option<(PathBuf, PathBuf)>> {
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

    let canonical_root = canonical_context_root(context_path)?;
    let mut candidates = vec![context_path.join(requested)];
    if requested.extension().is_none() {
        candidates.push(context_path.join(requested).with_extension("md"));
    }

    for candidate in candidates {
        let Ok(canonical_candidate) = candidate.canonicalize() else {
            continue;
        };
        if !canonical_candidate.starts_with(&canonical_root) || !canonical_candidate.is_file() {
            continue;
        }
        return Ok(Some((canonical_root, canonical_candidate)));
    }

    Ok(None)
}

fn canonical_context_root(context_path: &Path) -> Result<PathBuf> {
    context_path.canonicalize().map_err(|error| {
        Error::new(format!(
            "cannot resolve {}: {error}",
            context_path.display()
        ))
    })
}

fn read_context_file(path: &Path) -> Result<Option<String>> {
    fs::read_to_string(path)
        .map(Some)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))
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
                remove_empty_context_directory(&destination_path)?;
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

fn remove_empty_context_directory(path: &Path) -> Result<()> {
    fs::remove_dir(path).map_err(|error| {
        Error::new(format!(
            "cannot remove empty context directory {}: {error}",
            path.display()
        ))
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn package(name: &str, version: &str, context_path: PathBuf) -> ContextPackage {
        ContextPackage {
            name: name.to_owned(),
            version: version.to_owned(),
            description: None,
            context_path,
            selector: format!("{name}@{version}"),
        }
    }

    fn installer(root: &Path, packages: Vec<ContextPackage>) -> Installer {
        Installer {
            root: root.to_path_buf(),
            context_path: root.join(".agents/context"),
            packages,
        }
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn cargo_package(id: &str, name: &str, version: &str, manifest_path: PathBuf) -> CargoPackage {
        CargoPackage {
            package_id: id.to_owned(),
            name: name.to_owned(),
            version: version.to_owned(),
            description: Some(format!("{name} description")),
            manifest_path,
        }
    }

    #[test]
    fn reports_cargo_execution_status_and_json_errors() {
        let directory = tempdir().unwrap();
        let error = Installer::discover(directory.path()).err().unwrap();
        assert!(error.to_string().contains("cargo metadata failed"));

        let error = Installer::discover_with_cargo(
            directory.path().to_path_buf(),
            directory.path().join("missing-cargo").into_os_string(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot run cargo metadata"));

        assert_eq!(cargo_executable(None), OsString::from("cargo"));
        assert_eq!(
            cargo_executable(Some(OsString::from("custom-cargo"))),
            OsString::from("custom-cargo")
        );

        let error = cargo_metadata_failure(
            &directory.path().join("Cargo.toml"),
            "exit status: 1",
            b"  cargo stderr \n",
        );
        assert!(error.to_string().ends_with("cargo stderr"));
        let error = cargo_metadata_failure(&PathBuf::from("Cargo.toml"), "exit status: 1", b"\n");
        assert!(error.to_string().contains("run cargo check to resolve"));

        let error = parse_metadata(b"not JSON").err().unwrap();
        assert!(error.to_string().contains("cannot parse cargo metadata"));
    }

    #[cfg(unix)]
    #[test]
    fn reports_metadata_parse_errors_from_cargo() {
        let directory = tempdir().unwrap();
        let error =
            Installer::discover_with_cargo(directory.path().to_path_buf(), "/usr/bin/true".into())
                .unwrap_err();
        assert!(error.to_string().contains("cannot parse cargo metadata"));
    }

    #[test]
    fn discovers_only_resolved_non_workspace_context_packages_and_disambiguates_versions() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        for path in [
            "workspace",
            "duplicate-v1",
            "duplicate-v2",
            "missing",
            "file",
        ] {
            fs::create_dir_all(root.join(path)).unwrap();
        }
        fs::create_dir(root.join("workspace/context")).unwrap();
        fs::create_dir(root.join("duplicate-v1/context")).unwrap();
        fs::create_dir(root.join("duplicate-v2/context")).unwrap();
        fs::write(root.join("file/context"), "not a directory").unwrap();

        let packages = vec![
            cargo_package(
                "root 0.1.0",
                "workspace",
                "0.1.0",
                root.join("workspace/Cargo.toml"),
            ),
            cargo_package(
                "dup 1.0.0",
                "duplicate",
                "1.0.0",
                root.join("duplicate-v1/Cargo.toml"),
            ),
            cargo_package(
                "dup 2.0.0",
                "duplicate",
                "2.0.0",
                root.join("duplicate-v2/Cargo.toml"),
            ),
            cargo_package(
                "unresolved 1.0.0",
                "unresolved",
                "1.0.0",
                root.join("missing/Cargo.toml"),
            ),
            cargo_package("file 1.0.0", "file", "1.0.0", root.join("file/Cargo.toml")),
        ];
        let metadata = CargoMetadata {
            workspace_members: vec!["root 0.1.0".to_owned()],
            packages,
            resolve: Some(Resolve {
                nodes: vec![
                    ResolveNode {
                        package_id: "root 0.1.0".to_owned(),
                    },
                    ResolveNode {
                        package_id: "dup 1.0.0".to_owned(),
                    },
                    ResolveNode {
                        package_id: "dup 2.0.0".to_owned(),
                    },
                    ResolveNode {
                        package_id: "file 1.0.0".to_owned(),
                    },
                ],
            }),
        };
        let installer = Installer::discover_from_metadata(root.to_path_buf(), metadata).unwrap();

        assert_eq!(installer.root(), root);
        assert_eq!(installer.context_path(), root.join(".agents/context"));
        assert_eq!(
            installer
                .packages()
                .iter()
                .map(ContextPackage::selector)
                .collect::<Vec<_>>(),
            ["duplicate@1.0.0", "duplicate@2.0.0"]
        );
        assert!(installer.find_package("missing").unwrap().is_none());
        assert!(
            installer
                .find_package("duplicate")
                .unwrap_err()
                .to_string()
                .contains("duplicate@1.0.0, duplicate@2.0.0")
        );
        assert_eq!(
            installer
                .find_package("duplicate@2.0.0")
                .unwrap()
                .unwrap()
                .version,
            "2.0.0"
        );
    }

    #[test]
    fn supports_metadata_without_a_resolve_graph_and_skips_missing_or_malformed_paths() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("valid/context")).unwrap();
        fs::create_dir(root.join("path-is-file")).unwrap();
        fs::write(root.join("path-is-file/context"), "not a directory").unwrap();

        let metadata = CargoMetadata {
            workspace_members: Vec::new(),
            packages: vec![
                cargo_package("valid", "valid", "1.0.0", root.join("valid/Cargo.toml")),
                cargo_package(
                    "missing",
                    "missing",
                    "1.0.0",
                    root.join("absent/Cargo.toml"),
                ),
                cargo_package(
                    "file",
                    "file",
                    "1.0.0",
                    root.join("path-is-file/Cargo.toml"),
                ),
                cargo_package("empty", "empty", "1.0.0", PathBuf::new()),
            ],
            resolve: None,
        };
        let installer = Installer::discover_from_metadata(root.to_path_buf(), metadata).unwrap();
        assert_eq!(installer.packages().len(), 1);
        assert_eq!(installer.packages()[0].selector(), "valid");
    }

    #[cfg(unix)]
    #[test]
    fn reports_context_provider_path_inspection_errors() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let blocker = root.join("blocker");
        fs::write(&blocker, "file").unwrap();
        let metadata = CargoMetadata {
            workspace_members: Vec::new(),
            packages: vec![cargo_package(
                "broken",
                "broken",
                "1.0.0",
                blocker.join("Cargo.toml"),
            )],
            resolve: None,
        };
        assert!(Installer::discover_from_metadata(root.to_path_buf(), metadata).is_err());
    }

    #[test]
    fn locates_context_files_safely_and_finds_markdown_extensions() {
        let directory = tempdir().unwrap();
        let context = directory.path().join("context");
        fs::create_dir_all(context.join("nested")).unwrap();
        fs::write(context.join("guide.md"), "guide").unwrap();
        fs::write(context.join("nested/readme.MD"), "nested").unwrap();
        fs::create_dir(context.join("directory")).unwrap();
        fs::write(context.join("plain.txt"), "plain").unwrap();

        assert_eq!(
            find_context_file(&context, "guide").unwrap(),
            Some(context.join("guide.md").canonicalize().unwrap())
        );
        assert!(
            find_context_file(&context, "nested/readme.MD")
                .unwrap()
                .is_some()
        );
        assert_eq!(find_context_file(&context, "not-found").unwrap(), None);
        assert_eq!(find_context_file(&context, "directory").unwrap(), None);
        assert!(find_context_file(&context, "/etc/passwd").is_err());
        assert!(find_context_file(&context, "./guide.md").is_err());
        assert!(find_context_file(&context, "nested/../guide.md").is_err());
        assert!(find_context_file(&context.join("missing"), "guide.md").is_err());
        assert!(canonical_context_root(&context.join("does-not-exist")).is_err());
        assert!(read_context_file(&context).is_err());

        let markdown = markdown_files(&context).unwrap();
        assert_eq!(markdown.len(), 2);
        assert!(markdown[0] < markdown[1]);

        let files_path = context.join("not-a-directory");
        fs::write(&files_path, "file").unwrap();
        assert!(markdown_files(&files_path).is_err());
        assert!(collect_files(&files_path, &mut Vec::new()).is_err());
    }

    #[test]
    fn hides_skill_files_assets_and_preserves_regular_context_files() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let source = root.join("provider/context");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(source.join("empty")).unwrap();
        write(&source, "guide.md", "# Guide\n");
        write(
            &source,
            "my-skill.md",
            "---\ntype: skill\ndescription: A skill.\n---\n\n# Skill\n",
        );
        write(&source, "my-skill/reference.md", "asset\n");
        write(&source, "nested/guide.md", "nested\n");
        write(&source, "nested/my-skill.md", "ordinary nested context\n");
        write(&source, "README.MD", "upper-case extension\n");
        fs::write(source.join("plain.txt"), "text").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(source.join("guide.md"), source.join("linked.md")).unwrap();
        }

        let package = package("provider", "1.0.0", source.clone());
        let installer = installer(root, vec![package.clone()]);
        let listed = installer.list_context_files(&package).unwrap();
        let listed: Vec<_> = listed
            .iter()
            .map(|file| file.path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            listed,
            [
                "README.MD",
                "guide.md",
                "nested/guide.md",
                "nested/my-skill.md",
                "plain.txt"
            ]
        );

        assert!(!installer.install_package("missing").unwrap());
        assert!(installer.install_package("provider@1.0.0").unwrap());
        let installed = root.join(".agents/context/provider@1.0.0");
        assert!(installed.join("guide.md").is_file());
        assert!(installed.join("nested/my-skill.md").is_file());
        assert!(!installed.join("my-skill.md").exists());
        assert!(!installed.join("my-skill").exists());
        assert!(!installed.join("empty").exists());
    }

    #[test]
    fn removes_empty_installations_and_surfaces_destination_errors() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let source = root.join("provider/context");
        fs::create_dir_all(&source).unwrap();
        write(
            &source,
            "skill.md",
            "---\ntype: skill\ndescription: Skill.\n---\n\n# Skill\n",
        );
        fs::create_dir_all(source.join("skill/assets")).unwrap();
        fs::write(source.join("skill/assets/image.png"), "asset").unwrap();
        let installer = installer(root, vec![package("provider", "1.0.0", source)]);

        assert!(!installer.install_package("provider@1.0.0").unwrap());
        assert!(!root.join(".agents/context/provider@1.0.0").exists());

        let blocker = root.join(".agents");
        fs::remove_dir_all(&blocker).unwrap();
        fs::write(&blocker, "not a directory").unwrap();
        assert!(installer.install_package("provider@1.0.0").is_err());
    }

    #[test]
    fn copies_context_tree_while_skipping_symlinks_and_reports_copy_errors() {
        #[cfg(unix)]
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let root = directory.path();
        let source = root.join("source");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("guide.md"), "guide").unwrap();
        fs::write(source.join("skill.md"), "skill").unwrap();
        fs::write(source.join("nested/guide.md"), "nested").unwrap();
        #[cfg(unix)]
        symlink(root.join("source/guide.md"), source.join("linked.md")).unwrap();
        let destination = root.join("destination");
        assert!(
            copy_context_tree(
                &source,
                &destination,
                &HashSet::from(["skill".to_owned()]),
                true
            )
            .unwrap()
        );
        assert!(destination.join("guide.md").is_file());
        assert!(destination.join("nested/guide.md").is_file());
        assert!(!destination.join("skill.md").exists());
        assert!(!destination.join("empty").exists());
        assert!(!destination.join("linked.md").exists());

        let non_directory = root.join("not-a-directory");
        fs::write(&non_directory, "file").unwrap();
        assert!(
            copy_context_tree(&non_directory, &root.join("out"), &HashSet::new(), true).is_err()
        );
        let missing = root.join("missing");
        assert!(copy_context_tree(&missing, &root.join("out"), &HashSet::new(), true).is_err());

        let blocker = root.join("blocker");
        fs::write(&blocker, "file").unwrap();
        assert!(copy_context_tree(&source, &blocker.join("out"), &HashSet::new(), true).is_err());

        let bad_destination = root.join("bad-copy");
        fs::create_dir_all(bad_destination.join("guide.md")).unwrap();
        assert!(copy_context_tree(&source, &bad_destination, &HashSet::new(), true).is_err());

        let nonempty = root.join("nonempty");
        fs::create_dir_all(&nonempty).unwrap();
        fs::write(nonempty.join("child"), "file").unwrap();
        assert!(remove_empty_context_directory(&nonempty).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reports_context_tree_directory_read_errors() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();

        let result = copy_context_tree(
            &source,
            &directory.path().join("destination"),
            &HashSet::new(),
            true,
        );
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.unwrap_err().to_string().contains("cannot read"));
    }

    #[test]
    fn reports_directory_entry_type_and_recursive_read_errors() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("context");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("guide.md"), "guide").unwrap();

        let failure_path = root.clone();
        let _failure = fs::fail_once(fs::Operation::ReadDirectoryEntry, move |path| {
            path == failure_path
        });
        assert!(
            markdown_files(&root)
                .unwrap_err()
                .to_string()
                .contains("injected")
        );
        drop(_failure);

        let file = root.join("guide.md");
        let failure_path = file.clone();
        let _failure = fs::fail_once(fs::Operation::FileType, move |path| path == failure_path);
        assert!(
            markdown_files(&root)
                .unwrap_err()
                .to_string()
                .contains("injected")
        );
        drop(_failure);

        let failure_path = root.join("nested");
        let _failure = fs::fail_once(fs::Operation::ReadDirectory, move |path| {
            path == failure_path
        });
        assert!(
            markdown_files(&root)
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );
        drop(_failure);

        let failure_path = root.clone();
        let _failure = fs::fail_once(fs::Operation::ReadDirectoryEntry, move |path| {
            path == failure_path
        });
        assert!(collect_files(&root, &mut Vec::new()).is_err());
        drop(_failure);

        let failure_path = file;
        let _failure = fs::fail_once(fs::Operation::FileType, move |path| path == failure_path);
        assert!(collect_files(&root, &mut Vec::new()).is_err());
        drop(_failure);

        let failure_path = root.join("nested");
        let _failure = fs::fail_once(fs::Operation::ReadDirectory, move |path| {
            path == failure_path
        });
        assert!(collect_files(&root, &mut Vec::new()).is_err());
    }

    #[test]
    fn reports_copy_entry_type_recursive_and_empty_directory_errors() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir(source.join("empty")).unwrap();
        fs::write(source.join("guide.md"), "guide").unwrap();

        let failure_path = source.clone();
        let _failure = fs::fail_once(fs::Operation::ReadDirectoryEntry, move |path| {
            path == failure_path
        });
        assert!(
            copy_context_tree(
                &source,
                &directory.path().join("entry"),
                &HashSet::new(),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("injected")
        );
        drop(_failure);

        let failure_path = source.join("guide.md");
        let _failure = fs::fail_once(fs::Operation::FileType, move |path| path == failure_path);
        assert!(
            copy_context_tree(
                &source,
                &directory.path().join("type"),
                &HashSet::new(),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("injected")
        );
        drop(_failure);

        let failure_path = source.join("nested");
        let _failure = fs::fail_once(fs::Operation::ReadDirectory, move |path| {
            path == failure_path
        });
        assert!(
            copy_context_tree(
                &source,
                &directory.path().join("recursive"),
                &HashSet::new(),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("cannot read")
        );
        drop(_failure);

        let empty_destination = directory.path().join("empty-destination/empty");
        let failure_path = empty_destination.clone();
        let _failure = fs::fail_once(fs::Operation::RemoveDirectory, move |path| {
            path == failure_path
        });
        assert!(
            copy_context_tree(
                &source,
                &directory.path().join("empty-destination"),
                &HashSet::new(),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("cannot remove empty context directory")
        );
    }

    #[test]
    fn propagates_package_operation_errors() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let source = root.join("provider/context");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("guide.md"), "# Guide\n").unwrap();
        fs::write(source.join("binary.txt"), [0xff]).unwrap();
        let provider_package = package("provider", "1.0.0", source.clone());
        let provider_installer = installer(root, vec![provider_package.clone()]);

        assert_eq!(
            provider_installer
                .show_context_file("provider", "guide.md")
                .unwrap(),
            Some("# Guide\n".to_owned())
        );
        assert!(
            provider_installer
                .show_context_file("provider", "binary.txt")
                .unwrap_err()
                .to_string()
                .contains("cannot read")
        );

        let mut ambiguous = provider_package.clone();
        ambiguous.version = "2.0.0".to_owned();
        ambiguous.selector = "provider@2.0.0".to_owned();
        let ambiguous_installer = installer(root, vec![provider_package.clone(), ambiguous]);
        assert!(
            ambiguous_installer
                .show_context_file("provider", "guide.md")
                .unwrap_err()
                .to_string()
                .contains("multiple versions")
        );

        let destination = root.join(".agents/context/provider@1.0.0");
        let failure_path = destination.clone();
        let _failure = fs::fail_once(fs::Operation::Inspect, move |path| path == failure_path);
        assert!(provider_installer.install_package("provider").is_err());
        drop(_failure);

        let failure_path = source.join("guide.md");
        let _failure = fs::fail_once(fs::Operation::Copy, move |path| path == failure_path);
        assert!(provider_installer.install_package("provider").is_err());
        drop(_failure);

        let skill_only = root.join("skills/context");
        fs::create_dir_all(skill_only.join("skill/assets")).unwrap();
        fs::write(
            skill_only.join("skill.md"),
            "---\ntype: skill\ndescription: Skill.\n---\n\n# Skill\n",
        )
        .unwrap();
        fs::write(skill_only.join("skill/assets/image.png"), "asset").unwrap();
        let skill_installer = installer(root, vec![package("skills", "1.0.0", skill_only)]);
        let empty_destination = root.join(".agents/context/skills@1.0.0");
        let failure_path = empty_destination.clone();
        let _failure = fs::fail_once(fs::Operation::RemoveDirectoryTree, move |path| {
            path == failure_path
        });
        assert!(skill_installer.install_package("skills").is_err());
        drop(_failure);
        assert!(skill_installer.install_all().unwrap().is_empty());

        let bad = root.join("broken/context");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("broken.md"), "---\ntype: guide\n---\n# Bad\n").unwrap();
        let broken_installer = installer(root, vec![package("broken", "1.0.0", bad)]);
        assert!(
            broken_installer
                .list_context_files(&broken_installer.packages[0])
                .is_err()
        );
        assert!(
            broken_installer
                .show_context_file("broken", "broken.md")
                .is_err()
        );
        assert!(broken_installer.install_package("broken").is_err());
        assert!(broken_installer.install_all().is_err());

        let files_installer = installer(root, vec![provider_package]);
        let failure_path = source.clone();
        let _failure = fs::fail_once(fs::Operation::ReadDirectory, move |path| {
            path == failure_path
        });
        assert!(
            files_installer
                .list_context_files(&files_installer.packages[0])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn reports_context_removal_errors() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let parent = directory.path().join("context");
        fs::create_dir(&parent).unwrap();
        let destination = parent.join("installed");
        fs::write(&destination, "previous context").unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let result = remove_existing(&destination);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.unwrap_err().to_string().contains("cannot remove"));
    }

    #[test]
    fn shows_only_regular_context_and_handles_missing_packages_and_files() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let source = root.join("provider/context");
        fs::create_dir_all(&source).unwrap();
        write(&source, "guide.md", "# Guide\n");
        write(
            &source,
            "skill.md",
            "---\ntype: skill\ndescription: A skill.\n---\n\n# Skill\n",
        );
        let package = package("provider", "1.0.0", source.clone());
        let installer = installer(root, vec![package]);

        assert!(
            installer
                .show_context_file("missing", "guide.md")
                .unwrap()
                .is_none()
        );
        assert!(
            installer
                .show_context_file("provider@1.0.0", "missing.md")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            installer
                .show_context_file("provider@1.0.0", "guide")
                .unwrap(),
            Some("# Guide\n".to_owned())
        );
        assert!(
            installer
                .show_context_file("provider@1.0.0", "skill")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn installs_all_context_packages_in_selector_order() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        let zeta = root.join("zeta/context");
        let alpha = root.join("alpha/context");
        fs::create_dir_all(&zeta).unwrap();
        fs::create_dir_all(&alpha).unwrap();
        fs::write(zeta.join("guide.md"), "Zeta guide.\n").unwrap();
        fs::write(alpha.join("guide.md"), "Alpha guide.\n").unwrap();

        let populated = installer(
            root,
            vec![
                package("alpha", "1.0.0", alpha),
                package("zeta", "1.0.0", zeta),
            ],
        );
        assert_eq!(
            populated.install_all().unwrap(),
            ["alpha@1.0.0", "zeta@1.0.0"]
        );
        assert!(root.join(".agents/context/alpha@1.0.0/guide.md").is_file());
        assert!(root.join(".agents/context/zeta@1.0.0/guide.md").is_file());

        assert!(
            installer(root, Vec::new())
                .install_all()
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_context_file_symlinks_that_escape_the_provider() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let context = directory.path().join("context");
        fs::create_dir_all(&context).unwrap();
        let outside = directory.path().join("private.md");
        fs::write(&outside, "secret").unwrap();
        symlink(&outside, context.join("external.md")).unwrap();

        assert_eq!(find_context_file(&context, "external.md").unwrap(), None);
    }

    #[test]
    fn path_classification_and_removal_cover_files_directories_and_errors() {
        let names = HashSet::from(["skill".to_owned()]);
        assert!(is_skill_context_path(
            Path::new("skill/references/a.md"),
            &names
        ));
        assert!(is_skill_context_path(Path::new("skill.MD"), &names));
        assert!(!is_skill_context_path(Path::new("nested/skill.md"), &names));
        assert!(!is_skill_context_path(Path::new(""), &names));
        assert!(is_skill_markdown(Path::new("skill.MD"), &names));
        assert!(!is_skill_markdown(Path::new("skill.txt"), &names));

        let directory = tempdir().unwrap();
        let root = directory.path();
        let absent = root.join("absent");
        remove_existing(&absent).unwrap();
        let file = root.join("file");
        fs::write(&file, "file").unwrap();
        remove_existing(&file).unwrap();
        let folder = root.join("folder");
        fs::create_dir(&folder).unwrap();
        remove_existing(&folder).unwrap();
        assert!(!folder.exists());
    }

    #[cfg(unix)]
    #[test]
    fn reports_removal_errors_for_paths_beneath_files() {
        let directory = tempdir().unwrap();
        let blocker = directory.path().join("blocker");
        fs::write(&blocker, "file").unwrap();
        assert!(remove_existing(&blocker.join("child")).is_err());
    }
}
