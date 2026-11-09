//! Prepared source replacements for explicit, snapshotted store upgrades.

use std::{fs, path::PathBuf};

use crate::{Error, Result, document};

pub(crate) struct Change {
    path: PathBuf,
    before: String,
    after: String,
    permissions: fs::Permissions,
}

impl Change {
    pub fn new(path: PathBuf, before: String, after: String) -> Result<Self> {
        let permissions = fs::metadata(&path)?.permissions();
        Ok(Self {
            path,
            before,
            after,
            permissions,
        })
    }
}

fn save(change: &Change, text: &str) -> Result<()> {
    document::atomic_write(&change.path, text, true)?;
    fs::set_permissions(&change.path, change.permissions.clone())?;
    Ok(())
}

/// Apply prepared files in order, with config.toml last. The caller owns the
/// operation lock and a verified recovery snapshot. Restore our writes on error;
/// never overwrite an intervening direct-editor change during recovery.
pub(crate) fn commit(changes: &[Change], validate: impl FnOnce() -> Result<()>) -> Result<()> {
    commit_with(changes, validate, save)
}

fn commit_with(
    changes: &[Change],
    validate: impl FnOnce() -> Result<()>,
    mut write: impl FnMut(&Change, &str) -> Result<()>,
) -> Result<()> {
    let mut attempted = 0;
    let result = (|| {
        for change in changes {
            if fs::read_to_string(&change.path)? != change.before {
                return Err(Error::new(
                    "conflict",
                    "Migration source changed; no overwrite was attempted",
                ));
            }
            // A writer can replace the file before returning a sync/permission
            // error, so include the attempt before calling it.
            attempted += 1;
            write(change, &change.after)?;
        }
        for change in changes {
            if fs::read_to_string(&change.path)? != change.after {
                return Err(Error::new(
                    "conflict",
                    "Migration source changed before final validation",
                ));
            }
        }
        validate()
    })();
    if let Err(mut error) = result {
        let mut restored = true;
        for change in changes[..attempted].iter().rev() {
            match fs::read_to_string(&change.path) {
                Ok(current) if current == change.before => {}
                Ok(current) if current == change.after => {
                    restored &= write(change, &change.before).is_ok();
                }
                _ => restored = false,
            }
        }
        error.message.push_str(if attempted == 0 {
            "; migration made no replacements; the recovery snapshot is retained"
        } else if restored {
            "; migration writes were restored; the recovery snapshot is retained"
        } else {
            "; recovery is incomplete; inspect the retained snapshot before proceeding"
        });
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture_changes(root: &Path) -> Vec<Change> {
        ["first.md", "second.md", "config.toml"]
            .into_iter()
            .map(|name| {
                let path = root.join(name);
                fs::write(&path, "old").unwrap();
                Change::new(path, "old".into(), "new".into()).unwrap()
            })
            .collect()
    }

    #[test]
    fn successful_commit_validates_all_replacements_and_preserves_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let changes = fixture_changes(temp.path());
        let permissions: Vec<_> = changes
            .iter()
            .map(|change| fs::metadata(&change.path).unwrap().permissions())
            .collect();
        let mut validated = false;
        commit(&changes, || {
            for change in &changes {
                assert_eq!(fs::read_to_string(&change.path).unwrap(), "new");
            }
            validated = true;
            Ok(())
        })
        .unwrap();
        assert!(validated);
        for (change, expected) in changes.iter().zip(permissions) {
            assert_eq!(fs::metadata(&change.path).unwrap().permissions(), expected);
        }
    }

    #[test]
    fn failed_writes_and_validation_restore_sources_including_the_version_marker() {
        for failure in 0..=3 {
            let temp = tempfile::tempdir().unwrap();
            let changes = fixture_changes(temp.path());
            let mut calls = 0;
            let error = commit_with(
                &changes,
                || Err(Error::new("invalid_document", "validation failed")),
                |change, text| {
                    let call = calls;
                    calls += 1;
                    if text == "new" && change.path.file_name().unwrap() != "config.toml" {
                        assert_eq!(
                            fs::read_to_string(temp.path().join("config.toml")).unwrap(),
                            "old"
                        );
                    }
                    if failure < 3 && call == failure {
                        return Err(Error::new("io", "injected write failure"));
                    }
                    save(change, text)
                },
            )
            .unwrap_err();
            assert!(error.message.contains("writes were restored"));
            for change in &changes {
                assert_eq!(fs::read_to_string(&change.path).unwrap(), "old");
            }
        }
    }

    #[test]
    fn changes_during_writes_and_failed_recovery_never_commit_the_version_marker() {
        for external in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let changes = fixture_changes(temp.path());
            let error = commit_with(
                &changes,
                || Ok(()),
                |change, text| {
                    if !external
                        && (change.path.file_name().unwrap() == "config.toml" || text == "old")
                    {
                        return Err(Error::new("io", "injected recovery failure"));
                    }
                    save(change, text)?;
                    if external
                        && text == "new"
                        && change.path.file_name().unwrap() == "config.toml"
                    {
                        fs::write(&changes[0].path, "external")?;
                    }
                    Ok(())
                },
            )
            .unwrap_err();
            assert!(error.message.contains("recovery is incomplete"));
            assert_eq!(fs::read_to_string(&changes[2].path).unwrap(), "old");
            if external {
                assert_eq!(error.code, "conflict");
            }
        }
    }

    #[test]
    fn conflicts_and_failed_recovery_keep_external_edits_and_report_the_snapshot_requirement() {
        let temp = tempfile::tempdir().unwrap();
        let changes = fixture_changes(temp.path());
        fs::write(&changes[0].path, "external").unwrap();
        let error = commit(&changes, || Ok(())).unwrap_err();
        assert_eq!(error.code, "conflict");
        assert!(error.message.contains("made no replacements"));
        assert_eq!(fs::read_to_string(&changes[0].path).unwrap(), "external");
        assert_eq!(fs::read_to_string(&changes[2].path).unwrap(), "old");

        let changes = fixture_changes(temp.path());
        let error = commit_with(
            &changes,
            || {
                fs::write(&changes[0].path, "external").unwrap();
                Err(Error::new("invalid_document", "validation failed"))
            },
            save,
        )
        .unwrap_err();
        assert!(error.message.contains("recovery is incomplete"));
        assert_eq!(fs::read_to_string(&changes[0].path).unwrap(), "external");
        assert_eq!(fs::read_to_string(&changes[2].path).unwrap(), "old");
    }

    #[test]
    fn conflicts_preserve_matching_external_edits_to_unattempted_files() {
        for external_index in 0..2 {
            let temp = tempfile::tempdir().unwrap();
            let changes = fixture_changes(temp.path());
            if external_index == 0 {
                fs::write(&changes[0].path, "new").unwrap();
            }
            let error = commit_with(
                &changes,
                || panic!("a conflicting migration must not validate"),
                |change, text| {
                    save(change, text)?;
                    if external_index == 1 && change.path == changes[0].path && text == "new" {
                        fs::write(&changes[1].path, "new")?;
                    }
                    Ok(())
                },
            )
            .unwrap_err();
            assert_eq!(error.code, "conflict");
            for (index, change) in changes.iter().enumerate() {
                assert_eq!(
                    fs::read_to_string(&change.path).unwrap(),
                    if index == external_index {
                        "new"
                    } else {
                        "old"
                    }
                );
            }
        }
    }

    #[test]
    fn failed_write_that_replaced_the_file_is_restored() {
        let temp = tempfile::tempdir().unwrap();
        let changes = fixture_changes(temp.path());
        let error = commit_with(
            &changes,
            || panic!("a failed migration must not validate"),
            |change, text| {
                save(change, text)?;
                if text == "new" {
                    return Err(Error::new("io", "injected post-save failure"));
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.message.contains("writes were restored"));
        for change in &changes {
            assert_eq!(fs::read_to_string(&change.path).unwrap(), "old");
        }
    }
}
