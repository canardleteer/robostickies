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
            if entry.path() == root.join(".robostickies/cache")
                || entry.path() == root.join(".robostickies/private/snapshots")
            {
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
    if !version.is_some_and(|value| value.contains("v1alpha4")) {
        for directory in ["tags", "private/tags"] {
            if let Ok(entries) = fs::read_dir(store.directory().join(directory)) {
                for entry in entries {
                    let path = entry.unwrap().path();
                    let text = fs::read_to_string(&path).unwrap();
                    fs::write(path, text.replacen("description:", "topic:", 1)).unwrap();
                }
            }
        }
    }
    let field = version
        .map(|v| format!("data_version = {v} # keep version comment\n"))
        .unwrap_or_default();
    fs::write(store.directory().join("config.toml"), format!("# Team settings\nformat_version = 1 # syntax\n{field}\n[tags] # keep table comment\ndefault_private = false\nprivate_patterns = ['shared-*']\n")).unwrap();
}

fn legacy_note(store: &Store, private: bool, tags: &str, newline: &str) -> Note {
    let note = store
        .create_note(CreateNote {
            body: "Migration fixture".into(),
            body_format: None,
            tags: vec![],
            private: Some(private),
        })
        .unwrap()
        .note;
    let source = format!("---\n# keep header\nid: {}\ncreated_at: '{}'\nupdated_at: '{}'\ntags:\n{tags} # keep tags\nstatus: active\nextra:\n  nested: [one, two] # keep metadata\n---\nBody without final newline",note.id,note.created_at,note.updated_at).replace('\n',newline);
    fs::write(store.directory().join(&note.path), source).unwrap();
    store.get_note(&note.id).unwrap()
}

#[test]
fn v1alpha3_repairs_only_versioned_sources_and_leaves_cache_values_and_unaffected_files_intact() {
    for cache in ["warm", "missing", "corrupt"] {
        for newline in ["\n", "\r\n"] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::with_user_config(temp.path(), None).unwrap();
            store.init(InitOptions::default()).unwrap();
            write_config(&store, Some("'v1alpha2'"));
            let mut notes = vec![];
            for private in [false, true] {
                for tags in ["[\"papercut\"]", "[]"] {
                    notes.push(legacy_note(&store, private, tags, newline));
                }
            }
            let valid = store
                .create_note(CreateNote {
                    body: "Unchanged valid fixture".into(),
                    body_format: None,
                    tags: vec![],
                    private: Some(true),
                })
                .unwrap()
                .note;
            store.reindex().unwrap();
            let index = store.directory().join("cache/index.redb");
            match cache {
                "missing" => fs::remove_file(&index).unwrap(),
                "corrupt" => fs::write(&index, b"broken cache").unwrap(),
                _ => {}
            }
            let cache_before = fs::read(&index).ok();
            let before = sources(temp.path());
            for note in &notes {
                assert_eq!(
                    store
                        .update_note(UpdateNote {
                            id: note.id.clone(),
                            expected_revision: note.revision.clone(),
                            body: Some("No implicit migration".into()),
                            ..UpdateNote::default()
                        })
                        .unwrap_err()
                        .code,
                    "invalid_document"
                );
            }
            assert_eq!(sources(temp.path()), before);
            let preview = store.upgrade(true).unwrap();
            assert_eq!(preview.source_version.as_str(), "v1alpha2");
            assert_eq!(preview.target_version.as_str(), "v1alpha4");
            assert!(preview.dry_run && !preview.wrote && preview.snapshot.is_none());
            assert_eq!(sources(temp.path()), before);
            assert_eq!(fs::read(&index).ok(), cache_before);
            let mut expected = before.clone();
            for (path, bytes) in &mut expected {
                if path.starts_with(".robostickies/tags")
                    || path.starts_with(".robostickies/private/tags")
                {
                    *bytes = String::from_utf8(bytes.clone())
                        .unwrap()
                        .replacen("topic:", "description:", 1)
                        .into_bytes();
                }
            }
            let config = PathBuf::from(".robostickies/config.toml");
            let text = String::from_utf8(expected[&config].clone())
                .unwrap()
                .replace("'v1alpha2'", "\"v1alpha4\"");
            expected.insert(config, text.into_bytes());
            for note in &notes {
                let path = Path::new(".robostickies").join(&note.path);
                let text = String::from_utf8(expected[&path].clone())
                    .unwrap()
                    .replace(&format!("tags:{newline}["), &format!("tags:{newline}  ["));
                expected.insert(path, text.into_bytes());
            }
            let upgraded = store.upgrade(false).unwrap();
            assert!(upgraded.wrote && !upgraded.snapshot.unwrap().retained);
            assert_eq!(sources(temp.path()), expected);
            assert_eq!(fs::read(&index).ok(), cache_before);
            assert_eq!(store.get_note(&valid.id).unwrap().revision, valid.revision);
            for old in &notes {
                let read = store.get_note(&old.id).unwrap();
                assert_eq!(read.id, old.id);
                assert_eq!(read.created_at, old.created_at);
                assert_eq!(read.updated_at, old.updated_at);
                assert_eq!(read.tags, old.tags);
                assert_eq!(read.status, old.status);
                assert_eq!(read.metadata, old.metadata);
                assert_eq!(read.body, old.body);
                assert_eq!(read.visibility, old.visibility);
                assert_ne!(read.revision, old.revision);
                assert_eq!(
                    store
                        .update_note(UpdateNote {
                            id: old.id.clone(),
                            expected_revision: old.revision.clone(),
                            body: Some("stale".into()),
                            ..UpdateNote::default()
                        })
                        .unwrap_err()
                        .code,
                    "conflict"
                );
            }
            assert!(!store.upgrade(false).unwrap().wrote);
            assert_eq!(sources(temp.path()), expected);
            assert!(store.doctor().unwrap().healthy);
        }
    }
}

#[test]
fn v1alpha3_revalidates_legacy_cached_parses_without_exporting_private_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, Some("'v1alpha2'"));
    let note = legacy_note(&store, true, "[]", "\n");
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
    write_config(&store, Some("'v1alpha3'"));
    assert_eq!(
        store.get_note(&note.id).unwrap_err().code,
        "invalid_document"
    );
    assert!(!store.doctor().unwrap().healthy);
    let public = store
        .list_notes(Query {
            scope: Some(Scope::Public),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(public.total, 0);
    assert!(public.diagnostics.is_empty());
}

#[cfg(unix)]
#[test]
fn failed_source_replacement_restores_prior_writes_and_keeps_a_verified_recovery_copy() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, Some("'v1alpha2'"));
    legacy_note(&store, false, "[\"papercut\"]", "\n");
    legacy_note(&store, true, "[]", "\r\n");
    let directory = store.directory().join("private/notes");
    let permissions = fs::metadata(&directory).unwrap().permissions();
    let before = sources(temp.path());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    assert!(!store.upgrade(true).unwrap().wrote);
    let result = store.upgrade(false);
    fs::set_permissions(directory, permissions).unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "io");
    assert!(error.message.contains("writes were restored"));
    assert_eq!(sources(temp.path()), before);
    let snapshot = error.snapshot.unwrap();
    assert!(snapshot.retained);
    assert_eq!(
        fs::read(temp.path().join(snapshot.path).join("config.toml")).unwrap(),
        fs::read(store.directory().join("config.toml")).unwrap()
    );
    assert!(store.upgrade(false).unwrap().wrote);
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
                        body_format: None,
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
            for (path, bytes) in &mut expected {
                if path.starts_with(".robostickies/tags")
                    || path.starts_with(".robostickies/private/tags")
                {
                    *bytes = String::from_utf8(bytes.clone())
                        .unwrap()
                        .replacen("topic:", "description:", 1)
                        .into_bytes();
                }
            }
            expected.remove(Path::new(".robostickies/config.toml"));
            assert_eq!(after, expected);
            assert!(migrated.contains("data_version = \"v1alpha4\""));
            assert_eq!(
                migrated.replace(&format!("data_version = \"v1alpha4\"{line_ending}"), ""),
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
                    body_format: None,
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
    for version in ["'v1alpha1'", "'v1alpha2'", "'v1alpha3'", "'v1alpha4'"] {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        fs::create_dir(store.directory()).unwrap();
        write_config(&store, Some(version));
        let before = fs::read_to_string(store.directory().join("config.toml")).unwrap();
        assert!(!store.upgrade(true).unwrap().wrote);
        let result = store.upgrade(false).unwrap();
        let after = fs::read_to_string(store.directory().join("config.toml")).unwrap();
        if !version.contains("alpha4") {
            assert!(result.wrote);
            assert_eq!(after, before.replace(version, "\"v1alpha4\""));
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
                        body_format: None,
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
                            body_format: None,
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

#[test]
fn deferrals_are_private_target_specific_and_do_not_change_sources_or_public_advice() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, None);
    let config_path = store.directory().join("config.toml");
    let original = fs::read(&config_path).unwrap();
    let public_before = serde_json::to_value(store.context(Scope::Public).unwrap()).unwrap();
    let deferred = store
        .upgrade_with_options(UpgradeOptions {
            defer: true,
            ..UpgradeOptions::default()
        })
        .unwrap();
    assert!(deferred.deferred && !deferred.wrote && deferred.snapshot.is_none());
    assert_eq!(
        deferred.upgrade.requirement,
        robostickies::version::UpgradeRequirement::Should
    );
    assert!(store.configuration().unwrap().upgrade.deferred);
    assert!(store.context(Scope::All).unwrap().upgrade.unwrap().deferred);
    assert!(store.doctor().unwrap().upgrade.deferred);
    assert_eq!(fs::read(&config_path).unwrap(), original);
    assert!(!store.directory().join("private/snapshots").exists());
    assert_eq!(
        serde_json::to_value(store.context(Scope::Public).unwrap()).unwrap(),
        public_before
    );
    let preference = store.directory().join("private/upgrade-preferences.toml");
    for previous in ["v1alpha1", "v1alpha2", "a-previous-target"] {
        fs::write(&preference, format!("deferred_target = '{previous}'\n")).unwrap();
        assert!(!store.configuration().unwrap().upgrade.deferred);
    }
    for malformed in [
        "",
        "deferred_target = 1",
        "deferred_target = ''",
        "unknown = true",
        "not toml",
    ] {
        fs::write(&preference, malformed).unwrap();
        let config = store.configuration().unwrap();
        assert!(!config.upgrade.deferred);
        assert_eq!(config.warnings.len(), 1);
        assert_eq!(
            serde_json::to_value(store.context(Scope::Public).unwrap()).unwrap(),
            public_before
        );
    }
    // Deferral is advisory: an explicit upgrade still runs every registered step.
    store
        .upgrade_with_options(UpgradeOptions {
            defer: true,
            ..UpgradeOptions::default()
        })
        .unwrap();
    assert!(store.upgrade(false).unwrap().wrote);
    assert_eq!(store.configuration().unwrap().data_version, CURRENT);
    assert!(!store.configuration().unwrap().upgrade.deferred);
    for options in [
        UpgradeOptions {
            defer: true,
            dry_run: true,
            keep_snapshot: false,
        },
        UpgradeOptions {
            defer: true,
            dry_run: false,
            keep_snapshot: true,
        },
    ] {
        assert_eq!(
            store.upgrade_with_options(options).unwrap_err().code,
            "invalid_input"
        );
    }
}

#[test]
fn snapshots_include_hidden_and_private_data_but_exclude_only_generated_subtrees() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, None);
    fs::create_dir_all(store.directory().join("private/nested/cache")).unwrap();
    fs::write(
        store.directory().join("private/nested/cache/.hidden"),
        b"private binary\0fixture",
    )
    .unwrap();
    fs::create_dir_all(store.directory().join("private/snapshots/old")).unwrap();
    fs::write(
        store.directory().join("private/snapshots/old/sentinel"),
        "do not copy old snapshots",
    )
    .unwrap();
    let original = fs::read(store.directory().join("config.toml")).unwrap();
    assert!(store.upgrade(true).unwrap().snapshot.is_none());
    let upgrade = store
        .upgrade_with_options(UpgradeOptions {
            keep_snapshot: true,
            ..UpgradeOptions::default()
        })
        .unwrap();
    let snapshot = upgrade.snapshot.unwrap();
    assert!(upgrade.wrote && snapshot.retained);
    let location = temp.path().join(snapshot.path);
    assert_eq!(fs::read(location.join("config.toml")).unwrap(), original);
    assert_eq!(
        fs::read(location.join("private/nested/cache/.hidden")).unwrap(),
        b"private binary\0fixture"
    );
    assert!(location.join(".gitignore").is_file());
    assert!(!location.join("cache").exists());
    assert!(!location.join("private/snapshots").exists());
    assert!(
        store
            .directory()
            .join("private/snapshots/old/sentinel")
            .is_file()
    );
    let count = fs::read_dir(store.directory().join("private/snapshots"))
        .unwrap()
        .count();
    assert!(store.upgrade(false).unwrap().snapshot.is_none());
    assert_eq!(
        fs::read_dir(store.directory().join("private/snapshots"))
            .unwrap()
            .count(),
        count
    );
}

#[test]
fn failed_source_validation_retains_a_snapshot_and_never_repairs_the_cache() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    write_config(&store, None);
    fs::write(
        store.directory().join("notes/invalid.md"),
        "malformed synthetic source",
    )
    .unwrap();
    let index = store.directory().join("cache/index.redb");
    fs::write(&index, "invalid disposable cache").unwrap();
    let config = fs::read(store.directory().join("config.toml")).unwrap();
    assert_eq!(store.upgrade(true).unwrap_err().code, "invalid_document");
    assert!(!store.directory().join("private/snapshots").exists());
    let error = store.upgrade(false).unwrap_err();
    assert_eq!(error.code, "invalid_document");
    let snapshot = error.snapshot.unwrap();
    assert!(snapshot.retained);
    assert_eq!(
        fs::read(temp.path().join(snapshot.path).join("notes/invalid.md")).unwrap(),
        b"malformed synthetic source"
    );
    assert_eq!(
        fs::read(store.directory().join("config.toml")).unwrap(),
        config
    );
    assert_eq!(fs::read(&index).unwrap(), b"invalid disposable cache");
}

#[cfg(unix)]
#[test]
fn unsafe_snapshot_sources_and_destinations_fail_before_configuration_changes() {
    use std::os::unix::fs::symlink;
    for hazard in ["source-link", "destination-link", "hard-link"] {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        write_config(&store, None);
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), "must stay unchanged").unwrap();
        match hazard {
            "source-link" => symlink(&outside, store.directory().join(".hidden-link")).unwrap(),
            "destination-link" => {
                symlink(&outside, store.directory().join("private/snapshots")).unwrap()
            }
            _ => fs::hard_link(
                outside.join("sentinel"),
                store.directory().join(".hard-link"),
            )
            .unwrap(),
        }
        let original = fs::read(store.directory().join("config.toml")).unwrap();
        assert_eq!(store.upgrade(false).unwrap_err().code, "io");
        assert_eq!(
            fs::read(store.directory().join("config.toml")).unwrap(),
            original
        );
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"must stay unchanged"
        );
    }
}
