use std::{collections::BTreeMap, fs, path::Path};

use robostickies::{
    Store,
    model::*,
    version::{CURRENT, SUPPORTED},
};
use serde_json::{Value, json};

fn inventory(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for directory in ["tags", "private/tags", "notes", "private/notes"] {
        for entry in fs::read_dir(root.join(directory)).unwrap() {
            let entry = entry.unwrap();
            files.insert(
                format!("{directory}/{}", entry.file_name().to_str().unwrap()),
                fs::read(entry.path()).unwrap(),
            );
        }
    }
    files.insert(
        "config.toml".into(),
        fs::read(root.join("config.toml")).unwrap(),
    );
    files
}

fn setup(root: &Path, version: &str) -> Store {
    let store = Store::with_user_config(root, None).unwrap();
    fs::create_dir(store.directory()).unwrap();
    fs::write(
        store.directory().join("config.toml"),
        format!("format_version = 1\ndata_version = '{version}'\n"),
    )
    .unwrap();
    store.init(InitOptions::default()).unwrap();
    store
}

#[test]
fn descriptions_are_optional_verbatim_nonblank_and_separate_from_usage_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let store = setup(temp.path(), CURRENT.as_str());
    for private in [false, true] {
        for (name, description) in [("name-only", None), ("described", Some("  Build steps  "))] {
            let tag = store
                .create_tag(CreateTag {
                    name: name.into(),
                    description: description.map(Into::into),
                    body: "Usage guidance sentinel".into(),
                    read_on_start: true,
                    private: Some(private),
                })
                .unwrap();
            let path = store.directory().join(format!(
                "{}tags/{name}.md",
                if private { "private/" } else { "" }
            ));
            let text = fs::read_to_string(path).unwrap();
            assert_eq!(text.contains("description:"), description.is_some());
            assert!(!text.contains("topic:"));
            let read = store.get_tag(&tag.name).unwrap();
            assert_eq!(read.description.as_deref(), description);
            assert_eq!(read.body, "Usage guidance sentinel\n");
            assert_eq!(
                serde_json::to_value(&read).unwrap()["description"],
                json!(description)
            );
            assert!(serde_json::to_value(&read).unwrap().get("topic").is_none());
        }
    }
    let public = store.context(Scope::Public).unwrap();
    assert!(
        public
            .tags
            .iter()
            .all(|tag| tag.visibility == Visibility::Public)
    );
    assert!(
        public
            .startup_tags
            .iter()
            .all(|tag| tag.visibility == Visibility::Public)
    );
    assert!(
        public
            .tags
            .iter()
            .any(|tag| tag.name == "name-only" && tag.description.is_none())
    );
    assert!(
        public
            .startup_tags
            .iter()
            .any(|tag| tag.name == "name-only" && tag.body.contains("sentinel"))
    );
    assert_eq!(
        store
            .list_tags(Scope::All, Some("NAME-ONLY"))
            .unwrap()
            .tags
            .len(),
        2
    );
    assert_eq!(
        store
            .list_tags(Scope::All, Some("BUILD STEPS"))
            .unwrap()
            .tags
            .len(),
        2
    );
    assert!(
        store
            .list_tags(Scope::All, Some("Usage guidance sentinel"))
            .unwrap()
            .tags
            .is_empty()
    );
    let before = inventory(&store.directory());
    for blank in ["", " ", "\r\n\t"] {
        let input =
            serde_json::from_value::<CreateTag>(json!({"name":"blank", "description":blank}))
                .unwrap();
        assert_eq!(store.create_tag(input).unwrap_err().code, "invalid_input");
        assert_eq!(inventory(&store.directory()), before);
    }
    for args in [
        json!({"name":"missing"}),
        json!({"name":"null", "description":null}),
    ] {
        let tag = store
            .create_tag(serde_json::from_value(args).unwrap())
            .unwrap();
        assert!(tag.description.is_none());
        assert!(tag.body.is_empty());
    }
    assert!(serde_json::from_value::<CreateTag>(json!({"name":"old", "topic":"Old API"})).is_err());
    for header in [
        "topic: old",
        "topic: null",
        "description: current\ntopic: old",
        "description: ''",
        "description: ' '",
    ] {
        fs::write(
            store.directory().join("tags/invalid.md"),
            format!("---\n{header}\n---\n"),
        )
        .unwrap();
        assert_eq!(
            store.get_tag("invalid").unwrap_err().code,
            "invalid_document"
        );
    }
    for header in ["description: null", "read_on_start: false", "{}", ""] {
        fs::write(
            store.directory().join("tags/valid.md"),
            format!("---\n{header}\n---\n"),
        )
        .unwrap();
        assert!(store.get_tag("valid").unwrap().description.is_none());
    }
}

#[test]
fn legacy_creation_preserves_representation_and_name_only_creation_never_writes() {
    for version in SUPPORTED.into_iter().filter(|version| *version != CURRENT) {
        let temp = tempfile::tempdir().unwrap();
        let store = setup(temp.path(), version.as_str());
        for private in [false, true] {
            let tag = store
                .create_tag(CreateTag {
                    name: "custom".into(),
                    description: Some("  Legacy explanation  ".into()),
                    body: "Usage".into(),
                    read_on_start: true,
                    private: Some(private),
                })
                .unwrap();
            let path = format!("{}tags/custom.md", if private { "private/" } else { "" });
            let text = fs::read_to_string(store.directory().join(path)).unwrap();
            assert!(text.contains("topic:"));
            assert!(!text.contains("description:"));
            assert_eq!(tag.description.as_deref(), Some("  Legacy explanation  "));
            let before = inventory(&store.directory());
            let error = store
                .create_tag(
                    serde_json::from_value(json!({"name":"without", "private":private})).unwrap(),
                )
                .unwrap_err();
            assert_eq!(error.code, "upgrade_required");
            for instruction in [
                "v1alpha4",
                "upgrade --dry-run",
                "upgrade_store",
                "explicitly",
            ] {
                assert!(error.message.contains(instruction));
            }
            assert_eq!(inventory(&store.directory()), before);
            assert!(store.get_tag(&tag.name).is_ok());
            assert!(store.context(Scope::All).unwrap().diagnostics.is_empty());
            assert!(store.doctor().unwrap().healthy);
            store.reindex().unwrap();
            assert_eq!(inventory(&store.directory()), before);
        }
    }
}

#[test]
fn every_upgrade_path_renames_only_tag_keys_and_preserves_values_and_source_bytes() {
    for version in SUPPORTED.into_iter().filter(|version| *version != CURRENT) {
        for newline in ["\n", "\r\n"] {
            let temp = tempfile::tempdir().unwrap();
            let store = setup(temp.path(), version.as_str());
            for directory in ["tags", "private/tags"] {
                for (name, header) in [
                    (
                        "plain",
                        "# heading\ntopic: plain # explain\nread_on_start: true\nextra: {nested: [one, two]} # metadata\n",
                    ),
                    ("quoted", "'topic': \"Keep quoting\" # inline\n"),
                    (
                        "anchored",
                        "topic: &explanation 'Keep anchor'\nother: *explanation\n",
                    ),
                    (
                        "alias",
                        "shared: &explanation 'Keep alias'\ntopic: *explanation # alias comment\n",
                    ),
                    (
                        "folded",
                        "topic: >- # block\n  Multiple lines\n  of explanation\n",
                    ),
                    ("flow", "{topic: 'Keep flow', extra: [one, two]} # flow\n"),
                ] {
                    fs::write(
                        store.directory().join(format!("{directory}/{name}.md")),
                        format!("---\n{header}---\nGuidance without final newline")
                            .replace('\n', newline),
                    )
                    .unwrap();
                }
            }
            let before = inventory(&store.directory());
            let tags = store.context(Scope::All).unwrap();
            let cache = fs::read(store.directory().join("cache/index.redb")).unwrap();
            assert!(!store.upgrade(true).unwrap().wrote);
            assert_eq!(inventory(&store.directory()), before);
            assert_eq!(
                fs::read(store.directory().join("cache/index.redb")).unwrap(),
                cache
            );
            let upgraded = store.upgrade(false).unwrap();
            assert!(upgraded.wrote);
            assert!(!upgraded.snapshot.unwrap().retained);
            assert_eq!(
                fs::read(store.directory().join("cache/index.redb")).unwrap(),
                cache
            );
            for (path, bytes) in &before {
                if path.ends_with(".md") {
                    let expected = String::from_utf8(bytes.clone()).unwrap().replacen(
                        "topic",
                        "description",
                        1,
                    );
                    assert_eq!(
                        fs::read_to_string(store.directory().join(path)).unwrap(),
                        expected,
                        "{path}"
                    );
                }
            }
            for old in tags.tags {
                let read = store.get_tag(&old.name).unwrap();
                assert_eq!(read.description, old.description);
                assert_eq!(read.visibility, old.visibility);
            }
            assert!(store.doctor().unwrap().healthy);
            let after = inventory(&store.directory());
            assert!(!store.upgrade(false).unwrap().wrote);
            assert_eq!(inventory(&store.directory()), after);
        }
    }
}

#[test]
fn collisions_and_unsupported_renames_report_the_source_without_overwriting() {
    for directory in ["tags", "private/tags"] {
        for header in [
            "topic: Explanation\ndescription: Existing metadata\n",
            "topic: Explanation\ndescription: null\n",
            "defaults: &defaults {topic: Inherited}\n<<: *defaults\n",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = setup(temp.path(), "v1alpha3");
            let relative = format!("{directory}/collision.md");
            fs::write(
                store.directory().join(&relative),
                format!("---\n{header}---\nKeep guidance\n"),
            )
            .unwrap();
            assert!(store.doctor().unwrap().healthy);
            let before = inventory(&store.directory());
            for dry_run in [true, false] {
                let error = store.upgrade(dry_run).unwrap_err();
                assert_eq!(error.code, "invalid_document");
                assert!(error.message.contains(&relative), "{error}");
                assert_eq!(inventory(&store.directory()), before);
                assert_eq!(error.snapshot.is_some(), !dry_run);
            }
            let public = store.context(Scope::Public).unwrap();
            assert!(public.diagnostics.is_empty());
            assert!(
                public
                    .tags
                    .iter()
                    .all(|tag| !tag.name.starts_with("private:"))
            );
        }
    }
}

#[test]
fn interrupted_tag_upgrades_are_retryable_but_ordinary_legacy_readers_stay_strict() {
    for version in SUPPORTED.into_iter().filter(|version| *version != CURRENT) {
        let temp = tempfile::tempdir().unwrap();
        let store = setup(temp.path(), version.as_str());
        for (directory, name, header) in [
            ("tags", "papercut", "description: Already renamed"),
            ("private/tags", "name-only", "read_on_start: true"),
        ] {
            fs::write(
                store.directory().join(format!("{directory}/{name}.md")),
                format!("---\n{header}\n---\nKeep guidance\n"),
            )
            .unwrap();
        }
        assert_eq!(
            store.get_tag("papercut").unwrap_err().code,
            "invalid_document"
        );
        assert_eq!(
            store.get_tag("private:name-only").unwrap_err().code,
            "invalid_document"
        );
        let before = inventory(&store.directory());
        assert!(!store.upgrade(true).unwrap().wrote);
        assert_eq!(inventory(&store.directory()), before);
        assert!(store.upgrade(false).unwrap().wrote);
        for path in ["tags/papercut.md", "private/tags/name-only.md"] {
            assert_eq!(
                fs::read(store.directory().join(path)).unwrap(),
                before[path]
            );
        }
        assert!(
            store
                .get_tag("private:name-only")
                .unwrap()
                .description
                .is_none()
        );
        assert!(store.doctor().unwrap().healthy);
    }
}

#[test]
fn old_cache_generations_discard_stale_tag_entities_before_reparsing() {
    use redb::{ReadableTable, TableDefinition};
    let temp = tempfile::tempdir().unwrap();
    let store = setup(temp.path(), "v1alpha3");
    let expected = store.get_tag("papercut").unwrap().description.unwrap();
    for generation in [None, Some(1), Some(2)] {
        store.reindex().unwrap();
        let before = inventory(&store.directory());
        let db = redb::Database::create(store.directory().join("cache/index.redb")).unwrap();
        let tx = db.begin_write().unwrap();
        {
            let mut table = tx
                .open_table(TableDefinition::<&str, &[u8]>::new("files_v1"))
                .unwrap();
            let mut source: Value =
                serde_json::from_slice(table.get("tags/papercut.md").unwrap().unwrap().value())
                    .unwrap();
            let tag = source["entity"]["Tag"].as_object_mut().unwrap();
            tag.remove("description");
            tag.insert("topic".into(), json!(expected));
            table
                .insert(
                    "tags/papercut.md",
                    serde_json::to_vec(&source).unwrap().as_slice(),
                )
                .unwrap();
        }
        tx.delete_table(TableDefinition::<&str, u64>::new("metadata_v1"))
            .unwrap();
        if let Some(generation) = generation {
            tx.open_table(TableDefinition::<&str, u64>::new("metadata_v1"))
                .unwrap()
                .insert("generation", generation)
                .unwrap();
        }
        tx.commit().unwrap();
        drop(db);
        let tags = store.list_tags(Scope::Public, Some(&expected)).unwrap();
        assert_eq!(tags.tags.len(), 1);
        assert_eq!(tags.tags[0].description.as_deref(), Some(expected.as_str()));
        assert_eq!(inventory(&store.directory()), before);
    }
}

#[cfg(unix)]
#[test]
fn failed_private_tag_replacement_rolls_back_public_renames_and_version_marker() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let store = setup(temp.path(), "v1alpha3");
    store
        .create_tag(
            serde_json::from_value(
                json!({"name":"local", "description":"Keep local", "private":true}),
            )
            .unwrap(),
        )
        .unwrap();
    let directory = store.directory().join("private/tags");
    let permissions = fs::metadata(&directory).unwrap().permissions();
    let before = inventory(&store.directory());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    assert!(!store.upgrade(true).unwrap().wrote);
    let result = store.upgrade(false);
    fs::set_permissions(directory, permissions).unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "io");
    assert!(error.message.contains("writes were restored"));
    assert!(error.snapshot.unwrap().retained);
    assert_eq!(inventory(&store.directory()), before);
    assert!(store.upgrade(false).unwrap().wrote);
}
