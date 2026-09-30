// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::installer::{ContextPackage, markdown_files};
use bake::{Error, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct ContextDocument {
    path: PathBuf,
    title: String,
    description: Option<String>,
}

/// Manages the generated Context section in a project's agents.md file.
#[derive(Clone, Debug)]
pub struct AgentIndex {
    root: PathBuf,
    context_path: PathBuf,
    context_link_path: PathBuf,
    package_descriptions: HashMap<String, String>,
}

impl AgentIndex {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            context_path: root.join(".agents/context"),
            root,
            context_link_path: PathBuf::from(".agents/context"),
            package_descriptions: HashMap::new(),
        }
    }

    /// Add Cargo package descriptions to the generated context section.
    pub fn with_packages(mut self, packages: &[ContextPackage]) -> Self {
        self.package_descriptions = packages
            .iter()
            .filter_map(|package| {
                package
                    .description
                    .as_ref()
                    .map(|description| (package.selector().to_owned(), description.clone()))
            })
            .collect();
        self
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

        for (package_name, files) in packages {
            sections.push(format!("### {package_name}"));
            sections.push(String::new());
            sections.push(
                self.package_descriptions
                    .get(&package_name)
                    .cloned()
                    .unwrap_or_else(|| format!("Context files for {package_name}")),
            );
            sections.push(String::new());

            for document in files {
                append_document(
                    &mut sections,
                    &self.context_link_path.join(&package_name),
                    &document.path,
                    &document.title,
                    document.description.as_deref(),
                );
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

    fn collect_context_packages(&self) -> Result<Vec<(String, Vec<ContextDocument>)>> {
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
            let mut files = Vec::new();
            for path in markdown_files(&package_path)? {
                let (title, description) = extract_content(&path)?;
                let relative_path = path.strip_prefix(&package_path).map_err(|error| {
                    Error::new(format!("cannot make context path relative: {error}"))
                })?;
                files.push(ContextDocument {
                    path: relative_path.to_path_buf(),
                    title,
                    description,
                });
            }
            files.sort_by_key(|document| canonical_order(&document.path));
            if !files.is_empty() {
                packages.push((entry.file_name().to_string_lossy().into_owned(), files));
            }
        }
        packages.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(packages)
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
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Documentation")
                .replace('-', " ")
        });

    let first_paragraph = lines
        .iter()
        .copied()
        .filter(|line| heading_level(line).is_none())
        .skip_while(|line| line.is_empty())
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let description = first_sentence(&first_paragraph);

    Ok((title, description))
}

fn first_sentence(paragraph: &str) -> Option<String> {
    let paragraph = paragraph.trim();
    if paragraph.is_empty() {
        return None;
    }

    for (index, character) in paragraph.char_indices() {
        if matches!(character, '.' | '!' | '?')
            && paragraph[index + character.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            return Some(paragraph[..index + character.len_utf8()].to_owned());
        }
    }

    Some(paragraph.to_owned())
}

fn path_to_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
