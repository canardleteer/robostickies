//! Private, verified recovery copies for explicit store upgrades.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{Error, Result, version::SnapshotResult};

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File { hash: blake3::Hash, len: u64 },
}

pub(crate) struct Snapshot {
    directory: PathBuf,
    pub result: SnapshotResult,
}

impl Snapshot {
    /// Copy all store files, including hidden/private data, except the root cache
    /// and existing snapshots. Verify the copy and unchanged input before returning.
    pub fn create(root: &Path, excluded: impl Fn(&Path) -> Result<()>) -> Result<Self> {
        Self::create_with(root, excluded, |source, target| {
            let mut input = fs::File::open(source)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            std::io::copy(&mut input, &mut output)?;
            output.flush()?;
            output.sync_all()?;
            fs::set_permissions(target, input.metadata()?.permissions())?;
            Ok(())
        })
    }

    fn create_with(
        root: &Path,
        excluded: impl Fn(&Path) -> Result<()>,
        copy: impl Fn(&Path, &Path) -> Result<()>,
    ) -> Result<Self> {
        let mut original = inventory(root, true)?;
        // A config-only legacy store may not have a private directory yet.
        original
            .entry(PathBuf::from("private"))
            .or_insert(Entry::Directory);
        let id = Uuid::now_v7().to_string();
        let relative = Path::new("private/snapshots").join(&id);
        crate::document::safe_path(root, &relative)?;
        excluded(&relative)?;
        for path in original.keys() {
            excluded(&relative.join(path))?;
        }
        let directory = root.join(&relative);
        fs::create_dir_all(
            directory
                .parent()
                .ok_or_else(|| Error::new("io", "Missing snapshot parent"))?,
        )?;
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&directory)?;
        let snapshot = Self {
            directory,
            result: SnapshotResult {
                path: format!(".robostickies/private/snapshots/{id}"),
                retained: true,
            },
        };
        let copied = (|| {
            for (path, kind) in &original {
                let target = snapshot.directory.join(path);
                match kind {
                    Entry::Directory => fs::create_dir(&target)?,
                    Entry::File { .. } => copy(&root.join(path), &target)?,
                }
            }
            if inventory(&snapshot.directory, false)? != original
                || inventory(root, true)? != original
            {
                return Err(Error::new(
                    "io",
                    "Snapshot validation failed or source files changed during copying",
                ));
            }
            Ok(())
        })();
        copied.map_err(|mut error: Error| {
            error.message.push_str(
                "; snapshot is incomplete or unverified; inspect it before attempting recovery",
            );
            error.with_snapshot(snapshot.result.clone())
        })?;
        Ok(snapshot)
    }

    /// Called only after migration and source validation succeed. Cleanup failure
    /// does not change migration success and leaves a reported recovery copy.
    pub fn finish(mut self, keep: bool) -> (SnapshotResult, Vec<String>) {
        let mut warnings = vec![];
        if !keep {
            match fs::remove_dir_all(&self.directory) {
                Ok(()) => self.result.retained = false,
                Err(_) => warnings.push("Migration succeeded; snapshot cleanup failed. Inspect the retained path before relying on the recovery copy; some files may already have been removed.".into()),
            }
        }
        (self.result, warnings)
    }
}

fn inventory(root: &Path, exclude_generated: bool) -> Result<BTreeMap<PathBuf, Entry>> {
    fn visit(
        root: &Path,
        relative: &Path,
        entries: &mut BTreeMap<PathBuf, Entry>,
        exclude_generated: bool,
    ) -> Result<()> {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            if exclude_generated
                && (path == Path::new("cache") || path == Path::new("private/snapshots"))
            {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            let kind = if metadata.is_dir() {
                Entry::Directory
            } else if metadata.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() != 1 {
                        return Err(Error::new("io", "Refusing a hard-linked snapshot source"));
                    }
                }
                let bytes = fs::read(entry.path())?;
                Entry::File {
                    hash: blake3::hash(&bytes),
                    len: bytes.len() as u64,
                }
            } else {
                return Err(Error::new(
                    "io",
                    "Snapshot sources must be regular files or directories; symlinks and special files are refused",
                ));
            };
            let directory = kind == Entry::Directory;
            entries.insert(path.clone(), kind);
            if directory {
                visit(root, &path, entries, exclude_generated)?;
            }
        }
        Ok(())
    }
    let mut entries = BTreeMap::new();
    visit(root, Path::new(""), &mut entries, exclude_generated)?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_integrity_failures_keep_sources_and_report_the_incomplete_snapshot() {
        for failure in ["copy", "corrupt", "source-race"] {
            let temp = tempfile::tempdir().unwrap();
            fs::write(temp.path().join("config.toml"), "original").unwrap();
            let result = Snapshot::create_with(
                temp.path(),
                |_| Ok(()),
                |source, target| {
                    if failure == "copy" {
                        return Err(Error::new("io", "synthetic copy failure"));
                    }
                    fs::copy(source, target)?;
                    if failure == "corrupt" {
                        fs::write(target, "corrupted copy")?;
                    }
                    if failure == "source-race" {
                        fs::write(source, "external edit")?;
                    }
                    Ok(())
                },
            );
            let error = result.err().unwrap();
            assert_eq!(error.code, "io");
            assert!(error.message.contains("incomplete or unverified"));
            let snapshot = error.snapshot.unwrap();
            assert!(snapshot.retained);
            assert!(
                temp.path()
                    .join(snapshot.path.strip_prefix(".robostickies/").unwrap())
                    .is_dir()
            );
            assert_eq!(
                fs::read_to_string(temp.path().join("config.toml")).unwrap(),
                if failure == "source-race" {
                    "external edit"
                } else {
                    "original"
                }
            );
        }
    }

    #[test]
    fn exclusion_failures_do_not_create_a_snapshot_and_successful_cleanup_removes_it() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("config.toml"), "original").unwrap();
        assert!(
            Snapshot::create(temp.path(), |_| Err(Error::new(
                "privacy",
                "synthetic missing exclusion"
            )))
            .is_err()
        );
        assert!(!temp.path().join("private").exists());
        let snapshot = Snapshot::create(temp.path(), |_| Ok(())).unwrap();
        let path = snapshot.directory.clone();
        let (result, warnings) = snapshot.finish(false);
        assert!(!result.retained && warnings.is_empty());
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_failure_is_a_success_warning_with_a_retained_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("config.toml"), "original").unwrap();
        let snapshot = Snapshot::create(temp.path(), |_| Ok(())).unwrap();
        let parent = snapshot.directory.parent().unwrap().to_owned();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).unwrap();
        let (result, warnings) = snapshot.finish(false);
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.retained);
        assert_eq!(warnings.len(), 1);
    }
}
