// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::installer::{ContextPackage, markdown_files};
use bake::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct ContextDocumentIndex {
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) metadata: Option<Value>,
    #[serde(default)]
    pub(crate) files: Vec<ContextDocument>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ContextDocument {
    pub(crate) path: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
}

/// Manages the generated Context section in a project's agents.md file.
#[derive(Clone, Debug)]
pub struct AgentIndex {
    root: PathBuf,
    context_path: PathBuf,
    context_link_path: PathBuf,
}

impl AgentIndex {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            context_path: root.join(".agents/context"),
            root,
            context_link_path: PathBuf::from(".agents/context"),
        }
    }

    pub fn context_path(&self) -> &Path {
        &self.context_path
    }

    pub fn generate_context_section(&self) -> Result<String> {
        let mut sections = vec![
            "This section links to documentation from installed packages. It is automatically generated and can be refreshed with `cargo bake agent:context:install`.".to_owned(),
            String::new(),
            "**Before working on a package, read the relevant context files below. They contain package-specific guidance and workflows.**".to_owned(),
            String::new(),
            "If these files are missing or dependencies have changed, run `cargo bake agent:context:install` to install them.".to_owned(),
            String::new(),
        ];

        let packages = self.collect_context_packages()?;
        if packages.is_empty() {
            sections.push(
                "No context files found. Run `cargo bake agent:context:install` to install context from dependencies.".to_owned(),
            );
            return Ok(sections.join("\n"));
        }

        for (package_name, package_path, files) in packages {
            sections.push(format!("### {package_name}"));
            sections.push(String::new());

            let index = load_index(&package_path, &package_name)?;
            sections.push(
                index
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Context files for {package_name}")),
            );
            sections.push(String::new());

            if index.files.is_empty() {
                for file in files {
                    let (title, description) = extract_content(&file)?;
                    let relative_path = file.strip_prefix(&package_path).map_err(|error| {
                        Error::new(format!("cannot make context path relative: {error}"))
                    })?;
                    append_document(
                        &mut sections,
                        &self.context_link_path.join(&package_name),
                        relative_path,
                        &title,
                        description.as_deref(),
                    );
                }
            } else {
                for document in index.files {
                    let relative_path = match safe_relative_path(&document.path) {
                        Some(path) => path,
                        None => continue,
                    };
                    if !package_path.join(&relative_path).is_file() {
                        continue;
                    }
                    append_document(
                        &mut sections,
                        &self.context_link_path.join(&package_name),
                        &relative_path,
                        &document.title,
                        document.description.as_deref(),
                    );
                }
            }
        }

        while sections.last().is_some_and(String::is_empty) {
            sections.pop();
        }
        Ok(sections.join("\n"))
    }

    pub fn update_agents_md(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let context = self.generate_context_section()?;

        let existing = match fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Error::new(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        };

        let updated = match existing {
            Some(contents) => update_existing(&contents, &context),
            None => format!("# Agent\n\n## Context\n\n{context}\n"),
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                Error::new(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        fs::write(&path, updated)
            .map_err(|error| Error::new(format!("cannot write {}: {error}", path.display())))
    }

    fn collect_context_packages(&self) -> Result<Vec<(String, PathBuf, Vec<PathBuf>)>> {
        let entries = match fs::read_dir(&self.context_path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(Error::new(format!(
                    "cannot read {}: {error}",
                    self.context_path.display()
                )));
            }
        };

        let mut packages = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() {
                continue;
            }
            let package_path = entry.path();
            let files = markdown_files(&package_path)?;
            if !files.is_empty() {
                packages.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    package_path,
                    files,
                ));
            }
        }
        packages.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(packages)
    }
}

pub(crate) fn write_generated_index(package: &ContextPackage, destination: &Path) -> Result<()> {
    let index_path = destination.join("index.yaml");
    if index_path.exists() {
        return Ok(());
    }

    let mut markdown = markdown_files(destination)?;
    markdown.retain(|path| path != &index_path);
    markdown.sort_by_key(|path| canonical_order(path));

    let files = markdown
        .into_iter()
        .map(|path| {
            let (title, description) = extract_content(&path)?;
            let relative = path.strip_prefix(destination).map_err(|error| {
                Error::new(format!("cannot make context path relative: {error}"))
            })?;
            Ok(ContextDocument {
                path: path_to_string(relative),
                title,
                description,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let index = ContextDocumentIndex {
        description: Some(
            package
                .description
                .clone()
                .unwrap_or_else(|| format!("Context files for {}", package.name)),
        ),
        metadata: package.metadata.clone(),
        files,
    };
    let contents = yaml_serde::to_string(&index)
        .map_err(|error| Error::new(format!("cannot serialize context index: {error}")))?;
    fs::write(&index_path, contents)
        .map_err(|error| Error::new(format!("cannot write {}: {error}", index_path.display())))
}

fn load_index(context_path: &Path, package_name: &str) -> Result<ContextDocumentIndex> {
    let index_path = context_path.join("index.yaml");
    match fs::read_to_string(&index_path) {
        Ok(contents) => match yaml_serde::from_str(&contents) {
            Ok(index) => Ok(index),
            Err(_) => Ok(fallback_index(package_name)),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(fallback_index(package_name))
        }
        Err(error) => Err(Error::new(format!(
            "cannot read {}: {error}",
            index_path.display()
        ))),
    }
}

fn fallback_index(package_name: &str) -> ContextDocumentIndex {
    ContextDocumentIndex {
        description: Some(format!("Context files for {package_name}")),
        metadata: None,
        files: Vec::new(),
    }
}

fn update_existing(contents: &str, context: &str) -> String {
    let mut lines: Vec<String> = contents.lines().map(str::to_owned).collect();
    let had_trailing_newline = contents.ends_with('\n');
    let agent_heading = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case("# agent"));

    let Some(agent_heading) = agent_heading else {
        return format!("# Agent\n\n## Context\n\n{context}\n\n{contents}");
    };

    let mut context_heading = None;
    for (index, line) in lines.iter().enumerate().skip(agent_heading + 1) {
        let trimmed = line.trim();
        if heading_level(trimmed) == Some(1) {
            break;
        }
        if trimmed.eq_ignore_ascii_case("## context") {
            context_heading = Some(index);
            break;
        }
    }

    let replacement: Vec<String> = std::iter::once("## Context".to_owned())
        .chain(std::iter::once(String::new()))
        .chain(context.lines().map(str::to_owned))
        .collect();

    if let Some(context_heading) = context_heading {
        let end = (context_heading + 1..lines.len())
            .find(|index| heading_level(lines[*index].trim()).is_some_and(|level| level <= 2))
            .unwrap_or(lines.len());
        lines.splice(context_heading..end, replacement);
    } else {
        lines.splice(
            agent_heading + 1..agent_heading + 1,
            std::iter::once(String::new()).chain(replacement),
        );
    }

    let mut updated = lines.join("\n");
    if had_trailing_newline || !updated.is_empty() {
        updated.push('\n');
    }
    updated
}

fn heading_level(line: &str) -> Option<usize> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if (1..=6).contains(&hashes) && line.chars().nth(hashes).is_some_and(char::is_whitespace) {
        Some(hashes)
    } else {
        None
    }
}

fn append_document(
    sections: &mut Vec<String>,
    link_root: &Path,
    relative_path: &Path,
    title: &str,
    description: Option<&str>,
) {
    sections.push(format!(
        "#### [{}]({})",
        title.replace(']', "\\]"),
        markdown_link(&link_root.join(relative_path))
    ));
    sections.push(String::new());
    if let Some(description) = description.filter(|description| !description.is_empty()) {
        sections.push(description.to_owned());
        sections.push(String::new());
    }
}

fn markdown_link(path: &Path) -> String {
    path_to_string(path)
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('?', "%3F")
        .replace('(', "%28")
        .replace(')', "%29")
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn canonical_order(path: &Path) -> (usize, String, String) {
    const CANONICAL: &[&str] = &[
        "getting-started",
        "overview",
        "usage",
        "configuration",
        "migration",
        "troubleshooting",
        "debugging",
    ];
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let order = CANONICAL
        .iter()
        .position(|canonical| *canonical == name)
        .unwrap_or(CANONICAL.len());
    (order, name, path_to_string(path))
}

fn extract_content(path: &Path) -> Result<(String, Option<String>)> {
    let content = fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let lines: Vec<_> = content.lines().map(str::trim).collect();
    let title = lines
        .iter()
        .find_map(|line| {
            heading_level(line).map(|_| line.trim_start_matches('#').trim().to_owned())
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Documentation".to_owned());

    let mut description_lines = Vec::new();
    let mut content_started = false;
    for line in &lines {
        if heading_level(line).is_some() {
            continue;
        }
        if line.is_empty() {
            if content_started {
                break;
            }
            continue;
        }
        content_started = true;
        description_lines.push(*line);
    }

    let description = description_lines.join(" ");
    let description = if description.chars().count() > 200 {
        Some(format!(
            "{}...",
            description.chars().take(197).collect::<String>()
        ))
    } else if description.is_empty() {
        None
    } else {
        Some(description)
    };

    Ok((title, description))
}

fn path_to_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
