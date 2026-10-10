// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

#[cfg(test)]
use super::test_filesystem as fs;
use bake::{Error, Result};
use std::collections::BTreeSet;
use std::ffi::OsStr;
#[cfg(not(test))]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BEGIN_MARKER: &str = "# BEGIN bake-agent-context";
const END_MARKER: &str = "# END bake-agent-context";

pub(crate) struct Update {
    path: PathBuf,
    original: String,
    updated: String,
}

impl Update {
    pub(crate) fn apply(&self) -> Result<()> {
        if self.original == self.updated {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                Error::new(format!("cannot create {}: {error}", parent.display()))
            })?;
        }

        fs::write(&self.path, &self.updated)
            .map_err(|error| Error::new(format!("cannot update {}: {error}", self.path.display())))
    }
    pub(crate) fn restore(&self) -> Result<()> {
        fs::write(&self.path, &self.original)
            .map_err(|error| Error::new(format!("cannot restore {}: {error}", self.path.display())))
    }
}

/// Prepare local Git excludes for generated context and installed skills.
///
/// These rules live in `.git/info/exclude`, so projects do not need to commit
/// ignore entries for generated local files.
pub(crate) fn prepare(
    root: &Path,
    skill_names: impl IntoIterator<Item = String>,
) -> Result<Option<Update>> {
    prepare_with_locator(root, skill_names, local_exclude_path)
}

#[cfg(all(test, unix))]
fn prepare_with_executable(
    root: &Path,
    skill_names: impl IntoIterator<Item = String>,
    executable: &OsStr,
) -> Result<Option<Update>> {
    prepare_with_locator(root, skill_names, |root| {
        local_exclude_path_with(root, executable)
    })
}

fn prepare_with_locator(
    root: &Path,
    skill_names: impl IntoIterator<Item = String>,
    locate: impl FnOnce(&Path) -> Result<Option<(PathBuf, String)>>,
) -> Result<Option<Update>> {
    prepare_from_path(locate(root)?, skill_names)
}

fn prepare_from_path(
    location: Option<(PathBuf, String)>,
    skill_names: impl IntoIterator<Item = String>,
) -> Result<Option<Update>> {
    let Some((path, prefix)) = location else {
        return Ok(None);
    };

    let skill_names: BTreeSet<_> = skill_names.into_iter().collect();
    Ok(Some(prepare_local_exclude(path, &prefix, &skill_names)?))
}

fn local_exclude_path(root: &Path) -> Result<Option<(PathBuf, String)>> {
    local_exclude_path_with(root, OsStr::new("git"))
}

fn local_exclude_path_with(root: &Path, executable: &OsStr) -> Result<Option<(PathBuf, String)>> {
    let output = match Command::new(executable)
        .args(["rev-parse", "--show-prefix", "--git-path", "info/exclude"])
        .current_dir(root)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::new(format!(
                "cannot locate Git exclude file: {error}"
            )));
        }
    };

    if !output.status.success() {
        // Context can still be installed from a source archive or other
        // directory which is not a Git checkout.
        return Ok(None);
    }

    exclude_path_from_output(root, output.status.success(), &output.stdout)
}

fn exclude_path_from_output(
    root: &Path,
    success: bool,
    stdout: &[u8],
) -> Result<Option<(PathBuf, String)>> {
    if !success {
        // Context can still be installed from a source archive or other
        // directory which is not a Git checkout.
        return Ok(None);
    }

    let path = String::from_utf8(stdout.to_vec())
        .map_err(|error| Error::new(format!("Git returned an invalid exclude path: {error}")))?;
    let (prefix, path) = path
        .split_once('\n')
        .ok_or_else(|| Error::new("Git returned an invalid project prefix and exclude path"))?;
    let path = PathBuf::from(path.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Ok(Some((path, prefix.trim_end_matches('\r').to_owned())))
}

fn prepare_local_exclude(
    path: PathBuf,
    prefix: &str,
    skill_names: &BTreeSet<String>,
) -> Result<Update> {
    let original = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(Error::new(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };

    let mut lines = remove_managed_block(&original, &path, prefix)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !lines.is_empty() {
        lines.push(String::new());
    }

    let mut pattern = prefix.to_owned();
    for character in ['\\', '*', '?', '[', ']'] {
        pattern = pattern.replace(character, &format!("\\{character}"));
    }
    let mut block = vec![
        format!("{BEGIN_MARKER} {prefix}").trim_end().to_owned(),
        "# Generated by Bake Agent Context; do not edit this block manually.".to_owned(),
        format!("/{pattern}.agents/context/"),
    ];
    block.extend(
        skill_names
            .iter()
            .map(|name| format!("/{pattern}.agents/skills/{name}/")),
    );
    block.push(format!("{END_MARKER} {prefix}").trim_end().to_owned());

    lines.extend(block);
    let mut updated = lines.join("\n");
    updated.push('\n');

    Ok(Update {
        path,
        original,
        updated,
    })
}

fn remove_managed_block(contents: &str, path: &Path, prefix: &str) -> Result<String> {
    let begin_marker = format!("{BEGIN_MARKER} {prefix}");
    let end_marker = format!("{END_MARKER} {prefix}");
    let mut lines: Vec<String> = contents.lines().map(str::to_owned).collect();
    let begin_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == begin_marker.trim_end())
        .map(|(index, _)| index)
        .collect();
    let end_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == end_marker.trim_end())
        .map(|(index, _)| index)
        .collect();

    match (begin_positions.as_slice(), end_positions.as_slice()) {
        ([], []) => {}
        ([begin], [end]) if begin < end => {
            lines.drain(*begin..=*end);
        }
        _ => {
            return Err(Error::new(format!(
                "{} has an incomplete or duplicated bake-agent-context ignore block",
                path.display()
            )));
        }
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        Ok(String::new())
    } else {
        let mut output = lines.join("\n");
        output.push('\n');
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn update_apply_is_idempotent_and_reports_parent_and_write_errors() {
        let directory = tempdir().unwrap();
        let unchanged = directory.path().join("unchanged");
        Update {
            path: unchanged.clone(),
            original: "same".to_owned(),
            updated: "same".to_owned(),
        }
        .apply()
        .unwrap();
        assert!(!unchanged.exists());

        let parent_file = directory.path().join("file");
        fs::write(&parent_file, "not a directory").unwrap();
        let error = Update {
            path: parent_file.join("exclude"),
            original: String::new(),
            updated: "generated\n".to_owned(),
        }
        .apply()
        .unwrap_err();
        assert!(error.to_string().contains("cannot create"));

        let destination = directory.path().join("directory");
        fs::create_dir(&destination).unwrap();
        let error = Update {
            path: destination.clone(),
            original: "old".to_owned(),
            updated: "new".to_owned(),
        }
        .apply()
        .unwrap_err();
        assert!(error.to_string().contains("cannot update"));

        let error = Update {
            path: PathBuf::new(),
            original: "old".to_owned(),
            updated: "new".to_owned(),
        }
        .apply()
        .unwrap_err();
        assert!(error.to_string().contains("cannot update"));
    }

    #[test]
    fn restores_previous_exclusions_and_reports_restore_errors() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("exclude");
        fs::write(&path, "user rule\n").unwrap();
        let update = prepare_local_exclude(path.clone(), "", &BTreeSet::new()).unwrap();
        update.apply().unwrap();
        update.restore().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "user rule\n");
        let _failure = fs::fail_once(fs::Operation::Write, move |candidate| candidate == path);
        assert!(
            update
                .restore()
                .unwrap_err()
                .to_string()
                .contains("cannot restore")
        );
    }

    #[test]
    fn resolves_git_output_and_rejects_invalid_utf8() {
        let directory = tempdir().unwrap();
        let root = directory.path();

        assert_eq!(
            exclude_path_from_output(root, false, b"ignored").unwrap(),
            None
        );
        assert_eq!(
            exclude_path_from_output(root, true, b"\n.git/info/exclude\n").unwrap(),
            Some((root.join(".git/info/exclude"), String::new()))
        );
        let absolute = root.join("exclude");
        assert_eq!(
            exclude_path_from_output(
                root,
                true,
                format!("app/\n{}\n", absolute.display()).as_bytes()
            )
            .unwrap(),
            Some((absolute, "app/".to_owned()))
        );
        assert!(exclude_path_from_output(root, true, &[0xff]).is_err());
        assert!(exclude_path_from_output(root, true, b"missing newline").is_err());
    }

    #[test]
    fn prepares_and_applies_managed_rules_without_losing_user_rules() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("exclude");
        let original = "# user rule\n*.local\n\n";
        fs::write(&path, original).unwrap();

        let update = prepare_local_exclude(
            path.clone(),
            "",
            &BTreeSet::from(["zeta-skill".to_owned(), "alpha-skill".to_owned()]),
        )
        .unwrap();
        update.apply().unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.starts_with("# user rule\n*.local\n\n# BEGIN bake-agent-context\n"));
        assert!(result.find("alpha-skill").unwrap() < result.find("zeta-skill").unwrap());

        let repeated = prepare_local_exclude(
            path.clone(),
            "",
            &BTreeSet::from(["alpha-skill".to_owned(), "zeta-skill".to_owned()]),
        )
        .unwrap();
        repeated.apply().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), result);
    }

    #[test]
    fn handles_empty_exclude_files_and_rejects_broken_managed_blocks() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("exclude");

        let empty = prepare_local_exclude(path.clone(), "", &BTreeSet::new()).unwrap();
        empty.apply().unwrap();
        let just_block = fs::read_to_string(&path).unwrap();
        assert!(just_block.starts_with(BEGIN_MARKER));

        for contents in [
            BEGIN_MARKER.to_owned(),
            END_MARKER.to_owned(),
            format!("{END_MARKER}\n{BEGIN_MARKER}\n"),
            format!("{BEGIN_MARKER}\n{END_MARKER}\n{BEGIN_MARKER}\n{END_MARKER}\n"),
        ] {
            assert!(
                remove_managed_block(&contents, &path, "").is_err(),
                "{contents:?}"
            );
        }
        assert_eq!(
            remove_managed_block(&format!("{BEGIN_MARKER}\n{END_MARKER}\n"), &path, "").unwrap(),
            ""
        );
        assert_eq!(
            remove_managed_block("user\n\n", &path, "").unwrap(),
            "user\n"
        );
    }

    #[test]
    fn scopes_exclusions_to_each_project_in_a_repository() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        for relative in ["", "apps/web [dev]", "apps/worker"] {
            let project = root.join(relative);
            fs::create_dir_all(&project).unwrap();
            prepare(&project, ["provider-old".to_owned()])
                .unwrap()
                .unwrap()
                .apply()
                .unwrap();
        }
        prepare(&root.join("apps/web [dev]"), ["provider-new".to_owned()])
            .unwrap()
            .unwrap()
            .apply()
            .unwrap();
        prepare(root, ["provider-old".to_owned()])
            .unwrap()
            .unwrap()
            .apply()
            .unwrap();

        for (path, ignored) in [
            (".agents/skills/provider-old/SKILL.md", true),
            ("apps/worker/.agents/skills/provider-old/SKILL.md", true),
            ("apps/web [dev]/.agents/context/index.md", true),
            ("apps/web [dev]/.agents/skills/provider-new/SKILL.md", true),
            ("apps/web [dev]/.agents/skills/provider-old/SKILL.md", false),
            (
                "apps/web [dev]/.agents/skills/project-local/SKILL.md",
                false,
            ),
            ("apps/web d/.agents/skills/provider-new/SKILL.md", false),
        ] {
            let status = Command::new("git")
                .args(["check-ignore", "--quiet", path])
                .current_dir(root)
                .status()
                .unwrap();
            assert_eq!(status.success(), ignored, "{path}");
        }
    }

    #[test]
    fn reports_exclude_read_errors() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("exclude-directory");
        fs::create_dir(&path).unwrap();

        let error = prepare_local_exclude(path, "", &BTreeSet::new())
            .err()
            .unwrap();
        assert!(error.to_string().contains("cannot read"));
    }

    #[test]
    fn propagates_git_lookup_and_managed_block_errors_from_prepare() {
        let directory = tempdir().unwrap();
        #[cfg(unix)]
        {
            let executable = directory.path().join("not-executable");
            fs::write(&executable, "not a command").unwrap();
            let error = prepare_with_executable(
                directory.path(),
                Vec::<String>::new(),
                executable.as_os_str(),
            )
            .err()
            .unwrap();
            assert!(error.to_string().contains("cannot locate Git exclude file"));
        }

        let error = prepare_with_locator(directory.path(), Vec::<String>::new(), |_| {
            Err(Error::new("injected Git lookup failure"))
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains("injected Git lookup failure"));

        let repository = tempdir().unwrap();
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        fs::write(repository.path().join(".git/info/exclude"), BEGIN_MARKER).unwrap();
        assert!(prepare(repository.path(), Vec::<String>::new()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn handles_git_command_start_errors() {
        let directory = tempdir().unwrap();
        assert_eq!(
            local_exclude_path_with(directory.path(), OsStr::new("missing-git-executable"))
                .unwrap(),
            None
        );
        let error = local_exclude_path_with(directory.path(), directory.path().as_os_str())
            .err()
            .unwrap();
        assert!(error.to_string().contains("cannot locate Git exclude file"));
    }
}
