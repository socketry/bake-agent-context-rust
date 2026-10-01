// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::installer::{ContextPackage, markdown_files};
use bake::{Error, Result};
use socketry_markdown::{
    ParseOptions,
    mdast::{Heading, Link, Node, Paragraph, Text},
    to_mdast,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct ContextDocument {
    path: PathBuf,
    title: String,
    description: Option<String>,
}

struct SourceHeading {
    index: usize,
    level: u8,
    title: String,
    start: usize,
    body_start: usize,
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
    let Ok(root) = to_mdast(contents, &ParseOptions::default()) else {
        return contents.to_owned();
    };
    let Some(children) = root.children() else {
        return contents.to_owned();
    };
    let headings: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let Node::Heading(heading) = node else {
                return None;
            };
            let position = node.position()?;
            Some(SourceHeading {
                index,
                level: heading.depth,
                title: heading_text(node),
                start: position.start.offset,
                body_start: after_heading_line(contents, position.end.offset),
            })
        })
        .collect();
    let Some(agent_heading) = headings
        .iter()
        .find(|heading| heading.level == 1 && heading.title.eq_ignore_ascii_case("agent"))
    else {
        return format!("# Agent\n\n## Context\n\n{context}\n\n{contents}");
    };

    let agent_end_index = headings
        .iter()
        .find(|heading| heading.index > agent_heading.index && heading.level <= 1)
        .map_or(children.len(), |heading| heading.index);
    let context_heading = headings.iter().find(|heading| {
        heading.index > agent_heading.index
            && heading.index < agent_end_index
            && heading.level == 2
            && heading.title.eq_ignore_ascii_case("context")
    });

    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let context = context.replace("\r\n", "\n").replace('\n', newline);
    let mut updated = String::with_capacity(contents.len() + context.len() + 32);

    if let Some(context_heading) = context_heading {
        let end = headings
            .iter()
            .find(|heading| heading.index > context_heading.index && heading.level <= 2)
            .map_or(contents.len(), |heading| heading.start);
        updated.push_str(&contents[..context_heading.start]);
        updated.push_str("## Context");
        updated.push_str(newline);
        updated.push_str(newline);
        updated.push_str(&context);
        if end < contents.len() {
            updated.push_str(newline);
            updated.push_str(newline);
        } else if contents.ends_with('\n') || !context.is_empty() {
            updated.push_str(newline);
        }
        updated.push_str(&contents[end..]);
    } else {
        updated.push_str(&contents[..agent_heading.body_start]);
        updated.push_str(newline);
        updated.push_str("## Context");
        updated.push_str(newline);
        updated.push_str(newline);
        updated.push_str(&context);
        if agent_heading.body_start < contents.len() {
            updated.push_str(newline);
            updated.push_str(newline);
        } else if contents.ends_with('\n') || !context.is_empty() {
            updated.push_str(newline);
        }
        updated.push_str(&contents[agent_heading.body_start..]);
    }

    updated
}

fn after_heading_line(contents: &str, offset: usize) -> usize {
    let Some(rest) = contents.get(offset..) else {
        return offset;
    };

    if rest.starts_with("\r\n") {
        offset + 2
    } else if rest.starts_with('\r') || rest.starts_with('\n') {
        offset + 1
    } else {
        offset
    }
}

fn heading_text(node: &Node) -> String {
    let text = node.text_content();
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text.as_str())
        .to_owned()
}

fn append_document(
    sections: &mut Vec<String>,
    link_root: &Path,
    relative_path: &Path,
    title: &str,
    description: Option<&str>,
) {
    let heading = Node::Heading(Heading {
        children: vec![Node::Link(Link {
            children: vec![Node::Text(Text {
                value: title.to_owned(),
                position: None,
            })],
            position: None,
            url: markdown_link(&link_root.join(relative_path)),
            title: None,
        })],
        position: None,
        depth: 4,
    });
    sections.push(heading.to_markdown().trim_end().to_owned());
    sections.push(String::new());
    if let Some(description) = description.filter(|description| !description.is_empty()) {
        let paragraph = Node::Paragraph(Paragraph {
            children: vec![Node::Text(Text {
                value: description.to_owned(),
                position: None,
            })],
            position: None,
        });
        sections.push(paragraph.to_markdown().trim_end().to_owned());
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
    let mut options = ParseOptions::default();
    options.constructs.frontmatter = true;
    let root = to_mdast(&content, &options)
        .map_err(|error| Error::new(format!("could not parse {}: {error}", path.display())))?;
    let children = root
        .children()
        .ok_or_else(|| Error::new(format!("{} is not a Markdown document", path.display())))?;
    let title = children
        .iter()
        .find_map(|node| match node {
            Node::Heading(_) => {
                let title = heading_text(node);
                (!title.trim().is_empty()).then_some(title)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Documentation")
                .replace('-', " ")
        });

    let first_paragraph = children.iter().find_map(|node| match node {
        Node::Paragraph(_) => Some(node.text_content()),
        _ => None,
    });
    let description = first_paragraph.as_deref().and_then(first_sentence);

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
