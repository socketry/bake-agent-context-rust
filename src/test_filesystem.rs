// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

//! Deterministic filesystem failures for exercising error handling in tests.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::ThreadId;

pub(crate) use std::fs::{Metadata, Permissions, remove_file, set_permissions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    Copy,
    CreateDirectory,
    CreateDirectoryTree,
    FileType,
    Inspect,
    Read,
    ReadDirectory,
    ReadDirectoryEntry,
    RemoveDirectory,
    RemoveDirectoryTree,
    RenameSource,
    RenameDestination,
    Write,
}

struct Failure {
    operation: Operation,
    thread: ThreadId,
    matches: Box<dyn Fn(&Path) -> bool + Send + Sync>,
}

static FAILURES: Mutex<Vec<Failure>> = Mutex::new(Vec::new());

pub(crate) struct FailureGuard {
    thread: ThreadId,
}

impl Drop for FailureGuard {
    fn drop(&mut self) {
        FAILURES
            .lock()
            .unwrap()
            .retain(|failure| failure.thread != self.thread);
    }
}

pub(crate) fn fail_once(
    operation: Operation,
    matches: impl Fn(&Path) -> bool + Send + Sync + 'static,
) -> FailureGuard {
    let thread = std::thread::current().id();
    FAILURES.lock().unwrap().push(Failure {
        operation,
        thread,
        matches: Box::new(matches),
    });
    FailureGuard { thread }
}

fn check_failure(operation: Operation, path: &Path) -> io::Result<()> {
    if take_failure(operation, path) {
        Err(io::Error::other("injected filesystem failure"))
    } else {
        Ok(())
    }
}

fn take_failure(operation: Operation, path: &Path) -> bool {
    let thread = std::thread::current().id();
    let mut failures = FAILURES.lock().unwrap();
    let failure = failures.iter().position(|failure| {
        failure.thread == thread && failure.operation == operation && (failure.matches)(path)
    });
    failure.is_some_and(|index| {
        failures.remove(index);
        true
    })
}

pub(crate) fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    check_failure(Operation::CreateDirectory, path.as_ref())?;
    std::fs::create_dir(path)
}

pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    check_failure(Operation::CreateDirectoryTree, path.as_ref())?;
    std::fs::create_dir_all(path)
}

pub(crate) fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    check_failure(Operation::Inspect, path.as_ref())?;
    std::fs::symlink_metadata(path)
}

pub(crate) fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    check_failure(Operation::Read, path.as_ref())?;
    std::fs::read_to_string(path)
}

pub(crate) struct DirectoryEntry(std::fs::DirEntry);

impl DirectoryEntry {
    pub(crate) fn file_type(&self) -> io::Result<std::fs::FileType> {
        check_failure(Operation::FileType, &self.0.path())?;
        self.0.file_type()
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.0.path()
    }

    pub(crate) fn file_name(&self) -> std::ffi::OsString {
        self.0.file_name()
    }
}

pub(crate) struct ReadDir {
    inner: std::fs::ReadDir,
    fail_next_entry: bool,
}

impl Iterator for ReadDir {
    type Item = io::Result<DirectoryEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.fail_next_entry {
            self.fail_next_entry = false;
            return Some(Err(io::Error::other("injected filesystem failure")));
        }

        self.inner.next().map(|result| result.map(DirectoryEntry))
    }
}

pub(crate) fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
    let path = path.as_ref();
    check_failure(Operation::ReadDirectory, path)?;
    let fail_next_entry = take_failure(Operation::ReadDirectoryEntry, path);
    std::fs::read_dir(path).map(|inner| ReadDir {
        inner,
        fail_next_entry,
    })
}

pub(crate) fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    check_failure(Operation::RemoveDirectory, path.as_ref())?;
    std::fs::remove_dir(path)
}

pub(crate) fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    check_failure(Operation::RemoveDirectoryTree, path.as_ref())?;
    std::fs::remove_dir_all(path)
}

pub(crate) fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    check_failure(Operation::Read, path.as_ref())?;
    std::fs::read(path)
}

pub(crate) fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    check_failure(Operation::RenameSource, from.as_ref())?;
    check_failure(Operation::RenameDestination, to.as_ref())?;
    std::fs::rename(from, to)
}

pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    check_failure(Operation::Write, path.as_ref())?;
    std::fs::write(path, contents)
}

pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    check_failure(Operation::Copy, from.as_ref())?;
    std::fs::copy(from, to)
}
