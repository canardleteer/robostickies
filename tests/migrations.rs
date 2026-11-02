use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use robostickies::{
    Store,
    model::*,
    version::{CURRENT, LEGACY},
};

fn sources(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == "cache" {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn write_config(store: &Store, version: Option<&str>) {
    let field = version
        .map(|v| format!("data_version = {v} # keep version comment\n"))
        .unwrap_or_default();
    fs::write(store.directory().join("config.toml"), format!("# Team settings\nformat_version = 1 # syntax\n{field}\n[tags] # keep table comment\ndefault_private = false\nprivate_patterns = ['shared-*']\n")).unwrap();
}

#[test]
fn upgrades_preserve_sources_revisions_privacy_and_customizations_with_all_cache_states() {
    for cache in ["warm", "missing", "corrupt"] {
        for line_ending in ["\n", "\r\n"] {
            let temp = tempfile::tempdir().unwrap();
            let user = temp.path().join("user.toml");
            fs::write(
                &user,
                "[tags]\ndefault_private = true\nprivate_patterns = ['user-*']\n",
            )
            .unwrap();
            let store = Store::with_user_config(temp.path(), Some(user)).unwrap();
            // Seed definitions publicly, then apply layered private defaults.
            let seed = Store::with_user_config(temp.path(), None).unwrap();
            seed.init(InitOptions::default()).unwrap();
            write_config(&store, None);
            fs::write(
                store.directory().join("private/config.toml"),
                "# Local defaults\n[tags]\nprivate_patterns = ['local-*']\n",
            )
            .unwrap();
            fs::write(
                store.directory().join("README.md"),
                "# Customized guidance\r\nKeep this text.\r\n",
            )
            .unwrap();
            fs::write(
                temp.path().join("AGENTS.md"),
                "Custom agent format with no managed block.\r\n",
            )
            .unwrap();
            let mut notes = vec![];
            for private in [false, true] {
                let saved = store
                    .create_note(CreateNote {
                        body: "Migration fixture".into(),
                        tags: vec!["papercut".into()],
                        private: Some(private),
                    })
                    .unwrap()
                    .note;
                let path = store.directory().join(&saved.path);
                let text = fs::read_to_string(&path).unwrap().replace("status: active", "status: active # keep status\nextra:\n  nested: [1, {key: value}] # keep nested metadata");
                fs::write(path, text.replace('\n', line_ending)).unwrap();
                notes.push(store.get_note(&saved.id).unwrap());
            }
            let config = store.directory().join("config.toml");
            let original_config = fs::read_to_string(&config)
                .unwrap()
                .replace('\n', line_ending);
            fs::write(&config, &original_config).unwrap();
            store.reindex().unwrap();
            let index = store.directory().join("cache/index.redb");
            match cache {
                "missing" => fs::remove_file(&index).unwrap(),
                "corrupt" => fs::write(&index, b"damaged cache").unwrap(),
                _ => {}
            }
            let before = sources(temp.path());
            for scope in [Scope::All, Scope::Private, Scope::Public] {
                let context = store.context(scope).unwrap();
                assert_eq!(context.data_version, Some(LEGACY));
                assert!(context.upgrade_available);
                if scope == Scope::Public {
                    assert!(context.configuration.is_none());
                    assert!(context.repository_root.is_none());
                }
            }
            assert!(store.doctor().unwrap().upgrade_available);
            // Existing initialization must preserve the legacy version too.
            store.init(InitOptions::default()).unwrap();
            assert_eq!(store.configuration().unwrap().data_version, LEGACY);
            assert_eq!(sources(temp.path()), before);
            // Orientation and init refresh the index; restore the selected
            // cache state immediately before the migration itself.
            match cache {
                "missing" => fs::remove_file(&index).unwrap(),
                "corrupt" => fs::write(&index, b"damaged cache").unwrap(),
                _ => {}
            }
            let cache_before = fs::read(&index).ok();
            let preview = store.upgrade(true).unwrap();
            assert_eq!(preview.source_version, LEGACY);
            assert_eq!(preview.target_version, CURRENT);
            assert!(preview.dry_run && !preview.wrote);
            assert_eq!(sources(temp.path()), before);
            assert_eq!(fs::read(&index).ok(), cache_before);
            let upgraded = store.upgrade(false).unwrap();
            assert!(upgraded.wrote && !upgraded.dry_run);
            assert_eq!(fs::read(&index).ok(), cache_before);
            let mut after = sources(temp.path());
            let migrated = String::from_utf8(
                after
                    .remove(Path::new(".robostickies/config.toml"))
                    .unwrap(),
            )
            .unwrap();
            let mut expected = before.clone();
            expected.remove(Path::new(".robostickies/config.toml"));
            assert_eq!(after, expected);
            assert!(migrated.contains("data_version = \"v1alpha2\""));
            assert_eq!(
                migrated.replace(&format!("data_version = \"v1alpha2\"{line_ending}"), ""),
                original_config
            );
            for old in &notes {
                let read = store.get_note(&old.id).unwrap();
                assert_eq!(read.revision, old.revision);
                assert_eq!(read.metadata, old.metadata);
                assert_eq!(read.visibility, old.visibility);
                let edited = store
                    .update_note(UpdateNote {
                        id: read.id,
                        body: Some("Edited after upgrade".into()),
                        expected_revision: read.revision,
                        ..UpdateNote::default()
                    })
                    .unwrap()
                    .note;
                assert_eq!(edited.metadata, old.metadata);
            }
            let original = store.get_note(&notes[1].id).unwrap();
            let publication = store
                .publish_note(PublishNote {
                    id: original.id.clone(),
                    body: Some("Reviewed migration fixture".into()),
                    tags: None,
                    dry_run: false,
                    expected_revision: original.revision.clone(),
                })
                .unwrap();
            assert!(publication.note.metadata.is_empty());
            assert_eq!(
                store.get_note(&original.id).unwrap().revision,
                original.revision
            );
            assert_eq!(store.list_notes(Query::default()).unwrap().total, 3);
            let defaults = store.configuration().unwrap();
            assert_eq!(defaults.data_version, CURRENT);
            assert!(!defaults.upgrade_available);
            assert!(defaults.tags.default_private);
            assert_eq!(defaults.tags.private_patterns, ["local-*"]);
            assert!(store.doctor().unwrap().healthy);
            let stable = sources(temp.path());
            for dry in [false, true] {
                let repeated = store.upgrade(dry).unwrap();
                assert_eq!(repeated.source_version, CURRENT);
                assert!(!repeated.wrote);
                assert_eq!(sources(temp.path()), stable);
            }
        }
    }
}

#[test]
fn explicit_versions_and_config_only_stores_upgrade_without_scaffolding() {
    for version in ["'v1alpha1'", "'v1alpha2'"] {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        fs::create_dir(store.directory()).unwrap();
        write_config(&store, Some(version));
        let before = fs::read_to_string(store.directory().join("config.toml")).unwrap();
        assert!(!store.upgrade(true).unwrap().wrote);
        let result = store.upgrade(false).unwrap();
        let after = fs::read_to_string(store.directory().join("config.toml")).unwrap();
        if version.contains("alpha1") {
            assert!(result.wrote);
            assert_eq!(after, before.replace("'v1alpha1'", "\"v1alpha2\""));
        } else {
            assert!(!result.wrote);
            assert_eq!(before, after);
        }
        assert!(!store.directory().join("README.md").exists());
        assert!(!store.directory().join("notes").exists());
    }
}

#[test]
fn absent_incomplete_invalid_and_future_stores_do_not_rewrite_sources() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    assert_eq!(store.upgrade(false).unwrap_err().code, "not_initialized");
    assert!(!store.directory().exists());
    fs::create_dir_all(store.directory().join("notes")).unwrap();
    assert_eq!(store.upgrade(true).unwrap_err().code, "not_initialized");
    for field in [
        "data_version = 'v2'",
        "data_version = ' V1alpha1 '",
        "data_version = 2",
        "data_version = []",
        "data_version = ''",
        "data_version = {nested = 'v1alpha1'}",
        "data_version = ",
    ] {
        fs::write(
            store.directory().join("config.toml"),
            format!("format_version = 1\n{field}\n"),
        )
        .unwrap();
        let before = sources(temp.path());
        for dry in [true, false] {
            assert_eq!(store.upgrade(dry).unwrap_err().code, "configuration");
            assert_eq!(sources(temp.path()), before);
        }
        assert_eq!(
            store.init(InitOptions::default()).unwrap_err().code,
            "configuration"
        );
        assert_eq!(sources(temp.path()), before);
    }
    for text in [
        "",
        "format_version = 2\n",
        "format_version = '1'\n",
        "[tags\n",
    ] {
        fs::write(store.directory().join("config.toml"), text).unwrap();
        assert_eq!(store.upgrade(false).unwrap_err().code, "configuration");
        assert_eq!(
            fs::read_to_string(store.directory().join("config.toml")).unwrap(),
            text
        );
    }
}

#[test]
fn user_and_private_version_overrides_are_forbidden_even_when_equal() {
    for namespace in ["user", "private"] {
        for value in ["v1alpha1", "v1alpha2"] {
            let temp = tempfile::tempdir().unwrap();
            let seed = Store::with_user_config(temp.path(), None).unwrap();
            seed.init(InitOptions::default()).unwrap();
            write_config(&seed, None);
            let path = if namespace == "user" {
                temp.path().join("user.toml")
            } else {
                seed.directory().join("private/config.toml")
            };
            fs::write(&path, format!("data_version = '{value}'\n")).unwrap();
            let store = Store::with_user_config(temp.path(), (namespace == "user").then_some(path))
                .unwrap();
            let before = sources(temp.path());
            assert_eq!(store.configuration().unwrap_err().code, "configuration");
            assert_eq!(store.upgrade(false).unwrap_err().code, "configuration");
            assert_eq!(
                store
                    .create_note(CreateNote {
                        body: "Rejected write".into(),
                        tags: vec![],
                        private: Some(true)
                    })
                    .unwrap_err()
                    .code,
                "configuration"
            );
            assert_eq!(sources(temp.path()), before);
        }
    }
}

#[test]
fn concurrent_upgrades_and_writers_share_the_operation_lock() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, None);
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                if i % 2 == 0 {
                    store.upgrade(false).unwrap().wrote
                } else {
                    store
                        .create_note(CreateNote {
                            body: format!("Concurrent migration writer {i}"),
                            tags: vec![],
                            private: Some(true),
                        })
                        .unwrap();
                    false
                }
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap().then_some(()))
            .count(),
        1
    );
    assert_eq!(store.configuration().unwrap().data_version, CURRENT);
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 4);
}

#[cfg(unix)]
#[test]
fn failed_atomic_upgrade_leaves_config_intact_and_dry_run_still_works() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, None);
    let before = sources(temp.path());
    let permissions = fs::metadata(store.directory()).unwrap().permissions();
    fs::set_permissions(store.directory(), fs::Permissions::from_mode(0o555)).unwrap();
    assert!(!store.upgrade(true).unwrap().wrote);
    let result = store.upgrade(false);
    fs::set_permissions(store.directory(), permissions).unwrap();
    assert_eq!(result.unwrap_err().code, "io");
    assert_eq!(sources(temp.path()), before);
    assert!(store.upgrade(false).unwrap().wrote);
}
