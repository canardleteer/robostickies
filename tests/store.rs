mod support;

use support::CommandOutput;
use tokio::process::Command;

use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use robostickies::{Store, model::*};
use tempfile::TempDir;

fn fresh() -> (TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    (temp, store)
}

fn tag(store: &Store, name: &str, private: bool) -> Tag {
    store
        .create_tag(CreateTag {
            name: name.into(),
            topic: format!("Topic {name}"),
            body: "Detailed guidance".into(),
            read_on_start: false,
            private: Some(private),
        })
        .unwrap()
}

fn note(store: &Store, body: &str, tags: &[&str]) -> Note {
    store
        .create_note(CreateNote {
            body_format: None,
            body: body.into(),
            tags: tags.iter().map(|t| (*t).into()).collect(),
            private: None,
        })
        .unwrap()
        .note
}

fn git(root: &Path, args: &[&str]) -> String {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let output = runtime
        .block_on(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=Robostickies test",
                    "-c",
                    "user.email=tests@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(root)
                .bounded_output(),
        )
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn publish(source: &Note, dry_run: bool) -> PublishNote {
    PublishNote {
        body_format: None,
        id: source.id.clone(),
        dry_run,
        body: None,
        tags: None,
        expected_revision: source.revision.clone(),
    }
}

#[test]
fn initialization_is_idempotent_and_agent_integration_preserves_format() {
    let temp = tempfile::tempdir().unwrap();
    let original = "# Team conventions\n\nKeep our custom format.\n";
    fs::write(temp.path().join("AGENTS.md"), original).unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    assert!(!store.context(Scope::All).unwrap().initialized);
    store.init(InitOptions::default()).unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("AGENTS.md")).unwrap(),
        original
    );
    let cut = store.get_tag("papercut").unwrap();
    let definition = fs::read(store.directory().join("tags/papercut.md")).unwrap();
    assert!(cut.read_on_start);
    assert!(
        store
            .init(InitOptions::default())
            .unwrap()
            .created
            .is_empty()
    );
    assert_eq!(
        fs::read(store.directory().join("tags/papercut.md")).unwrap(),
        definition
    );
    let options = InitOptions {
        agents: true,
        ..InitOptions::default()
    };
    assert!(store.init(options.clone()).unwrap().agents_updated);
    let updated = fs::read_to_string(temp.path().join("AGENTS.md")).unwrap();
    assert!(updated.starts_with(original));
    assert!(updated.contains("robostickies context"));
    assert!(!store.init(options).unwrap().agents_updated);
    assert_eq!(
        fs::read_to_string(temp.path().join("AGENTS.md")).unwrap(),
        updated
    );
    let context = store.context(Scope::All).unwrap();
    assert_eq!(context.startup_tags.len(), 4);
    assert_eq!(context.tags.len(), 4);
    assert!(context.configuration.is_some());
}

#[test]
fn configuration_patterns_precedence_and_scope_qualified_tags() {
    let (temp, store) = fresh();
    let user = temp.path().join("user.toml");
    fs::write(&user, "[tags]\nprivate_patterns = ['personal-*']\n").unwrap();
    let store = Store::with_user_config(store.repository(), Some(user.clone())).unwrap();
    let create = |name: &str, private| CreateTag {
        name: name.into(),
        topic: "A topic".into(),
        body: String::new(),
        read_on_start: false,
        private,
    };
    assert_eq!(
        store
            .create_tag(create("personal-work", None))
            .unwrap()
            .name,
        "private:personal-work"
    );
    assert_eq!(
        store
            .create_tag(create("personal-work", Some(false)))
            .unwrap()
            .name,
        "personal-work"
    );
    assert_eq!(
        store.get_tag("private:personal-work").unwrap().visibility,
        Visibility::Private
    );
    fs::write(
        &user,
        "[tags]\ndefault_private = true\nprivate_patterns = []\n",
    )
    .unwrap();
    assert_eq!(
        store.create_tag(create("local", None)).unwrap().visibility,
        Visibility::Private
    );
    fs::write(
        store.directory().join("private/config.toml"),
        "[tags]\ndefault_private = false\n",
    )
    .unwrap();
    assert_eq!(
        store.create_tag(create("shared", None)).unwrap().visibility,
        Visibility::Public
    );
    assert_eq!(
        store
            .create_tag(create("override", Some(true)))
            .unwrap()
            .visibility,
        Visibility::Private
    );
    assert_eq!(store.configuration().unwrap().sources.len(), 3);
    fs::write(&user, "[tags]\nprivate_patterns = ['[invalid']\n").unwrap();
    assert_eq!(store.configuration().unwrap_err().code, "configuration");
}

#[test]
fn any_private_tag_wins_and_public_names_never_shadow_private_names() {
    let (_temp, store) = fresh();
    tag(&store, "tooling", true);
    let local = note(
        &store,
        "Local workflow observation",
        &["papercut", "private:tooling"],
    );
    assert_eq!(local.visibility, Visibility::Private);
    assert!(
        store
            .directory()
            .join(&local.path)
            .starts_with(store.directory().join("private"))
    );
    tag(&store, "tooling", false);
    assert_eq!(
        store.get_note(&local.id).unwrap().visibility,
        Visibility::Private
    );
    let public = note(&store, "Shared tooling observation", &["tooling"]);
    assert_eq!(public.visibility, Visibility::Public);
    assert_eq!(
        store
            .list_notes(Query {
                scope: Some(Scope::Public),
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .list_notes(Query {
                scope: Some(Scope::Private),
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    let invalid = CreateNote {
        body_format: None,
        body: "Must remain private".into(),
        tags: vec!["private:tooling".into()],
        private: Some(false),
    };
    assert_eq!(store.create_note(invalid).unwrap_err().code, "privacy");
    assert_eq!(
        store
            .create_note(CreateNote {
                body_format: None,
                body: "Unknown".into(),
                tags: vec!["missing".into()],
                private: None
            })
            .unwrap_err()
            .code,
        "not_found"
    );
}

#[test]
fn publishing_supports_preview_or_immediate_copy_and_discards_private_metadata() {
    let (_temp, store) = fresh();
    tag(&store, "personal", true);
    let source = note(
        &store,
        "Original private prose",
        &["paperclip", "private:personal"],
    );
    let path = store.directory().join(&source.path);
    let text = fs::read_to_string(&path).unwrap().replacen(
        "---\n",
        "---\nprivate_email: private@example.invalid\n",
        1,
    );
    fs::write(&path, text).unwrap();
    let source = store.get_note(&source.id).unwrap();
    let mut request = publish(&source, true);
    request.expected_revision = source.revision.clone();
    let preview = store.publish_note(request.clone()).unwrap();
    assert!(!store.directory().join(&preview.note.path).exists());
    assert_eq!(preview.note.tags, ["paperclip"]);
    assert!(preview.note.metadata.is_empty());
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
    request.dry_run = false;
    request.body = Some("Reviewed public prose".into());
    let published = store.publish_note(request).unwrap().note;
    assert_ne!(published.id, source.id);
    assert_eq!(published.visibility, Visibility::Public);
    assert!(published.metadata.is_empty());
    assert_eq!(store.get_note(&source.id).unwrap().body, source.body);
    let report = store.review(Query::default()).unwrap();
    assert_eq!(report.total, 1);
    let json = serde_json::to_string(&report).unwrap();
    for private in [
        "Original private prose",
        "private:personal",
        "private@example.invalid",
        source.id.as_str(),
    ] {
        assert!(!json.contains(private), "leaked {private}");
    }
    assert!(json.contains("Reviewed public prose"));
}

#[test]
fn edits_preserve_frontmatter_comments_metadata_and_detect_stale_revisions() {
    let (_temp, store) = fresh();
    let source = note(&store, "Original body", &["papercut"]);
    let path = store.directory().join(&source.path);
    let text = format!(
        "---\n# Keep this comment\nid: {}\ncreated_at: '{}'\nupdated_at: '{}'\ntags:\n  - papercut # keep this too\nstatus: active\ncustom:\n  owner: Ada # extra metadata\n---\nOriginal body\n",
        source.id, source.created_at, source.updated_at
    );
    fs::write(&path, &text).unwrap();
    let source = store.get_note(&source.id).unwrap();
    let archived = store
        .update_note(UpdateNote {
            id: source.id.clone(),
            status: Some(NoteStatus::Archived),
            expected_revision: source.revision.clone(),
            ..UpdateNote::default()
        })
        .unwrap()
        .note;
    let changed = fs::read_to_string(path).unwrap();
    assert!(changed.contains("# Keep this comment"));
    assert!(changed.contains("- papercut # keep this too"));
    assert!(changed.contains("owner: Ada # extra metadata"));
    assert_eq!(archived.metadata["custom"]["owner"], "Ada");
    assert_eq!(archived.body, source.body);
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 0);
    assert_eq!(
        store
            .list_notes(Query {
                status: StatusFilter::Archived,
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    let stale = store
        .update_note(UpdateNote {
            id: source.id.clone(),
            body: Some("Stale overwrite".into()),
            expected_revision: source.revision,
            ..UpdateNote::default()
        })
        .unwrap_err();
    assert_eq!(stale.code, "conflict");
    assert_eq!(
        store.get_note(&source.id).unwrap().status,
        NoteStatus::Archived
    );
}

#[test]
fn private_routing_on_edit_never_publishes_implicitly() {
    let (_temp, store) = fresh();
    tag(&store, "local", true);
    let public = note(&store, "Originally public", &["papercut"]);
    let moved = store
        .update_note(UpdateNote {
            id: public.id.clone(),
            expected_revision: public.revision.clone(),
            body: Some("Now private text".into()),
            tags: Some(vec!["private:local".into()]),
            ..UpdateNote::default()
        })
        .unwrap()
        .note;
    assert!(!store.directory().join(&public.path).exists());
    assert!(store.directory().join(&moved.path).exists());
    assert_eq!(store.review(Query::default()).unwrap().total, 0);
    let removed = store
        .update_note(UpdateNote {
            id: moved.id.clone(),
            expected_revision: moved.revision.clone(),
            tags: Some(vec![]),
            ..UpdateNote::default()
        })
        .unwrap()
        .note;
    assert_eq!(removed.visibility, Visibility::Private);
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: removed.id,
                expected_revision: removed.revision,
                private: Some(false),
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "privacy"
    );
}

#[test]
fn public_queries_exclude_private_content_topics_counts_and_diagnostics() {
    let (_temp, store) = fresh();
    note(&store, "Public text", &["paperclip"]);
    let before = serde_json::to_string(&store.review(Query::default()).unwrap()).unwrap();
    tag(&store, "secret-topic", true);
    note(&store, "Unshared text", &["private:secret-topic"]);
    fs::write(
        store.directory().join("private/tags/broken-secret.md"),
        "invalid YAML private payload",
    )
    .unwrap();
    fs::write(
        store.directory().join("private/notes/broken-secret.md"),
        "confidential malformed note",
    )
    .unwrap();
    let after = serde_json::to_string(&store.review(Query::default()).unwrap()).unwrap();
    assert_eq!(after, before);
    let page = store
        .list_notes(Query {
            scope: Some(Scope::Public),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(page.diagnostics.is_empty());
    let context = store.context(Scope::Public).unwrap();
    assert!(context.configuration.is_none());
    assert!(
        !serde_json::to_string(&context)
            .unwrap()
            .contains("secret-topic")
    );
    assert_eq!(store.list_tags(Scope::Public, None).unwrap().tags.len(), 4);
    assert!(!store.doctor().unwrap().healthy);
}

#[test]
fn local_duplicate_ids_leave_public_selection_unchanged_and_block_ambiguous_writes() {
    let (_temp, store) = fresh();
    let source = note(&store, "Public source", &[]);
    let before = serde_json::to_value(store.review(Query::default()).unwrap()).unwrap();
    let public_query = Query {
        scope: Some(Scope::Public),
        ..Query::default()
    };
    let before_list =
        serde_json::to_value(store.list_notes(public_query.clone()).unwrap()).unwrap();
    let original = fs::read_to_string(store.directory().join(&source.path)).unwrap();
    let duplicate = store
        .directory()
        .join(format!("private/notes/{}.md", source.id));
    fs::write(
        &duplicate,
        original.replace("Public source", "PRIVATE_DUPLICATE_SENTINEL"),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(store.review(Query::default()).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(store.list_notes(public_query).unwrap()).unwrap(),
        before_list
    );
    let health = store.doctor().unwrap();
    assert!(!health.healthy);
    assert!(
        health
            .diagnostics
            .iter()
            .any(|d| d.code == "duplicate_id" && d.visibility == Visibility::Private)
    );
    for scope in [Scope::All, Scope::Private] {
        assert_eq!(
            store
                .review(Query {
                    scope: Some(scope),
                    ..Query::default()
                })
                .unwrap_err()
                .code,
            "invalid_document"
        );
    }
    assert_eq!(store.get_note(&source.id).unwrap_err().code, "conflict");
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: source.id.clone(),
                body: Some("Must not replace a copy".into()),
                expected_revision: source.revision.clone(),
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        store
            .publish_note(publish(&source, false))
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        fs::read_to_string(store.directory().join(&source.path)).unwrap(),
        original
    );
    fs::remove_file(duplicate).unwrap();
    assert_eq!(
        store.get_note(&source.id).unwrap().revision,
        source.revision
    );
    assert_eq!(
        serde_json::to_value(store.review(Query::default()).unwrap()).unwrap(),
        before
    );
}

#[test]
fn ambiguous_private_copies_are_excluded_from_the_index() {
    let (_temp, store) = fresh();
    tag(&store, "local", true);
    let source = note(&store, "Private duplicate fixture", &["private:local"]);
    fs::copy(
        store.directory().join(&source.path),
        store.directory().join(format!("notes/{}.md", source.id)),
    )
    .unwrap();
    assert_eq!(store.review(Query::default()).unwrap().total, 0);
    let page = store.list_notes(Query::default()).unwrap();
    assert_eq!(page.total, 0);
    assert!(page.diagnostics.iter().any(|d| d.code == "duplicate_id"));
    assert_eq!(store.get_note(&source.id).unwrap_err().code, "conflict");
}

#[test]
fn cache_generation_rebuilds_legacy_derived_rows_without_source_changes() {
    let (_temp, store) = fresh();
    let source = note(&store, "Public duplicate cache fixture", &[]);
    note(&store, "Other cached note keeps the table nonempty", &[]);
    fs::copy(
        store.directory().join(&source.path),
        store
            .directory()
            .join(format!("private/notes/{}.md", source.id)),
    )
    .unwrap();
    assert_eq!(store.review(Query::default()).unwrap().total, 2);
    let source_bytes = fs::read(store.directory().join(&source.path)).unwrap();
    for generation in [None, Some(1)] {
        let db = redb::Database::create(store.directory().join("cache/index.redb")).unwrap();
        let tx = db.begin_write().unwrap();
        // Model the old policy's omission while preserving its unchanged source cache.
        tx.open_table(redb::TableDefinition::<&str, &[u8]>::new("notes_v1"))
            .unwrap()
            .remove(source.id.as_str())
            .unwrap();
        tx.delete_table(redb::TableDefinition::<&str, u64>::new("metadata_v1"))
            .unwrap();
        if let Some(generation) = generation {
            tx.open_table(redb::TableDefinition::<&str, u64>::new("metadata_v1"))
                .unwrap()
                .insert("generation", generation)
                .unwrap();
        }
        tx.commit().unwrap();
        drop(db);
        let public = store.review(Query::default()).unwrap();
        assert_eq!(public.total, 2);
        assert!(public.notes.iter().any(|n| n.id == source.id));
        assert_eq!(
            fs::read(store.directory().join(&source.path)).unwrap(),
            source_bytes
        );
    }
}

#[test]
fn updates_and_publications_require_current_nonblank_revisions() {
    let (_temp, store) = fresh();
    let source = store
        .create_note(CreateNote {
            body_format: None,
            body: "Private original".into(),
            tags: vec![],
            private: Some(true),
        })
        .unwrap()
        .note;
    let path = store.directory().join(&source.path);
    let bytes = fs::read(&path).unwrap();
    for (revision, code) in [
        ("", "invalid_input"),
        ("   ", "invalid_input"),
        ("stale", "conflict"),
    ] {
        assert_eq!(
            store
                .update_note(UpdateNote {
                    id: source.id.clone(),
                    expected_revision: revision.into(),
                    body: Some("Rejected edit".into()),
                    ..UpdateNote::default()
                })
                .unwrap_err()
                .code,
            code
        );
        for dry_run in [true, false] {
            let mut request = publish(&source, dry_run);
            request.expected_revision = revision.into();
            assert_eq!(store.publish_note(request).unwrap_err().code, code);
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            fs::read_dir(store.directory().join("notes"))
                .unwrap()
                .count(),
            0
        );
    }
    let updated = store
        .update_note(UpdateNote {
            id: source.id.clone(),
            expected_revision: source.revision.clone(),
            body: Some("Newly reviewed text".into()),
            ..UpdateNote::default()
        })
        .unwrap()
        .note;
    assert_eq!(
        store
            .publish_note(publish(&source, false))
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        store
            .publish_note(publish(&updated, true))
            .unwrap()
            .source_revision,
        updated.revision
    );
    assert_eq!(
        store
            .publish_note(publish(&updated, false))
            .unwrap()
            .note
            .body
            .trim(),
        "Newly reviewed text"
    );
    assert_eq!(
        store.get_note(&updated.id).unwrap().revision,
        updated.revision
    );
}

#[test]
fn sources_override_cache_after_manual_edits_deletions_and_corruption() {
    let (_temp, store) = fresh();
    let source = note(&store, "Before edit", &["papercut"]);
    let path = store.directory().join(&source.path);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("Before edit", "After edit");
    fs::write(&path, text).unwrap();
    let page = store
        .list_notes(Query {
            text: Some("AFTER EDIT".into()),
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    fs::write(
        store.directory().join("cache/index.redb"),
        b"broken binary cache",
    )
    .unwrap();
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
    assert!(store.directory().join("cache/index.failed.redb").exists());
    fs::remove_file(&path).unwrap();
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 0);
    fs::write(&path, "broken note").unwrap();
    let page = store.list_notes(Query::default()).unwrap();
    assert_eq!(page.total, 0);
    assert_eq!(page.diagnostics.len(), 1);
    assert_eq!(
        store.review(Query::default()).unwrap_err().code,
        "invalid_document"
    );
}

#[test]
fn a_saved_note_survives_index_failure_without_a_duplicate_retry() {
    let (_temp, store) = fresh();
    let path = store.directory().join("cache/index.redb");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let saved = store
        .create_note(CreateNote {
            body_format: None,
            body: "Save despite cache failure".into(),
            tags: vec![],
            private: None,
        })
        .unwrap();
    assert!(!saved.warnings.is_empty());
    assert!(store.directory().join(&saved.note.path).is_file());
    assert_eq!(
        store.get_note(&saved.note.id).unwrap().body.trim(),
        "Save despite cache failure"
    );
    fs::remove_dir(&path).unwrap();
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
}

#[test]
fn incompatible_cache_tables_are_rebuilt_from_sources() {
    let (_temp, store) = fresh();
    note(&store, "Recover from an incompatible cache", &["paperclip"]);
    let path = store.directory().join("cache/index.redb");
    fs::remove_file(&path).unwrap();
    let db = redb::Database::create(&path).unwrap();
    let tx = db.begin_write().unwrap();
    {
        let incompatible: redb::TableDefinition<&str, u64> = redb::TableDefinition::new("files_v1");
        tx.open_table(incompatible)
            .unwrap()
            .insert("wrong-version", 1)
            .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
    // Recovery can replace an older failed cache on all platforms.
    fs::write(&path, "broken again").unwrap();
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
}

#[test]
fn filters_pagination_and_dates_are_consistent() {
    let (_temp, store) = fresh();
    tag(&store, "tooling", false);
    for (i, tags) in [
        (0, vec!["papercut", "tooling"]),
        (1, vec!["paperclip", "tooling"]),
        (2, vec!["paperclip"]),
    ] {
        let source = note(&store, &format!("Rust observation {i}"), &tags);
        let path = store.directory().join(source.path);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace(&source.created_at, &format!("2026-01-0{}T00:00:00Z", i + 1));
        fs::write(path, text).unwrap();
    }
    let query = Query {
        limit: 1,
        text: Some("RUST".into()),
        ..Query::default()
    };
    let page = store.list_notes(query.clone()).unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.notes[0].body.trim(), "Rust observation 2");
    let second = store
        .list_notes(Query {
            cursor: page.next_cursor,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(second.notes[0].body.trim(), "Rust observation 1");
    let third = store
        .list_notes(Query {
            cursor: second.next_cursor,
            ..query
        })
        .unwrap();
    assert_eq!(third.notes[0].body.trim(), "Rust observation 0");
    assert!(third.next_cursor.is_none());
    assert_eq!(
        store
            .list_notes(Query {
                tags: vec!["tooling".into(), "paperclip".into()],
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .list_notes(Query {
                exclude_tags: vec!["tooling".into()],
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .list_notes(Query {
                since: Some("2026-01-02".into()),
                until: Some("2026-01-03".into()),
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .list_notes(Query {
                since: Some("not a date".into()),
                ..Query::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert_eq!(
        store
            .list_notes(Query {
                limit: 0,
                ..Query::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert_eq!(
        store
            .list_notes(Query {
                cursor: Some("bad".into()),
                ..Query::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
}

#[test]
fn concurrent_writers_keep_every_note_and_do_not_hold_the_index_open() {
    let (_temp, store) = fresh();
    let barrier = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let store = store.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for i in 0..3 {
                    note(&store, &format!("Worker {worker} note {i}"), &["papercut"]);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(store.list_notes(Query::default()).unwrap().total, 12);
    assert!(store.doctor().unwrap().healthy);
}

#[test]
fn git_excludes_private_data_and_cache_and_reports_already_tracked_private_files() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    tag(&store, "personal", true);
    let private = note(&store, "Not shared", &["private:personal"]);
    let shared = note(&store, "Shared note", &["paperclip"]);
    git(temp.path(), &["add", ".robostickies"]);
    let tracked = git(temp.path(), &["ls-files"]);
    assert!(tracked.contains(&shared.path));
    assert!(!tracked.contains("private/"));
    assert!(!tracked.contains("cache/"));
    assert!(store.doctor().unwrap().healthy);
    let private_path = format!(".robostickies/{}", private.path);
    git(temp.path(), &["add", "-f", &private_path]);
    let health = store.doctor().unwrap();
    assert!(!health.healthy);
    assert!(
        health
            .diagnostics
            .iter()
            .any(|d| d.code == "tracked_private_data")
    );
    let error = store
        .update_note(UpdateNote {
            id: private.id,
            expected_revision: private.revision,
            body: Some("new private data".into()),
            ..UpdateNote::default()
        })
        .unwrap_err();
    assert_eq!(error.code, "privacy");
}

#[test]
fn local_excludes_work_in_linked_git_worktrees() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("main");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "--allow-empty", "-qm", "fixture"]);
    let worktree = temp.path().join("worktree");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-qb",
            "test-worktree",
            worktree.to_str().unwrap(),
        ],
    );
    let store = Store::with_user_config(&worktree, None).unwrap();
    store
        .init(InitOptions {
            ignore_location: IgnoreLocation::Local,
            agents: false,
        })
        .unwrap();
    assert!(!store.directory().join(".gitignore").exists());
    let private = store
        .create_note(CreateNote {
            body_format: None,
            body: "Private in worktree".into(),
            tags: vec![],
            private: Some(true),
        })
        .unwrap()
        .note;
    git(
        &worktree,
        &["check-ignore", &format!(".robostickies/{}", private.path)],
    );
    assert!(store.doctor().unwrap().healthy);
}

#[test]
fn index_tracks_git_checkouts() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    let original = note(&store, "Base branch body", &["papercut"]);
    git(temp.path(), &["add", ".robostickies"]);
    git(temp.path(), &["commit", "-qm", "base"]);
    let base = git(temp.path(), &["rev-parse", "HEAD"]).trim().to_owned();
    git(temp.path(), &["checkout", "-qb", "changed"]);
    store
        .update_note(UpdateNote {
            id: original.id.clone(),
            expected_revision: original.revision.clone(),
            body: Some("Other branch body".into()),
            ..UpdateNote::default()
        })
        .unwrap();
    git(temp.path(), &["add", ".robostickies"]);
    git(temp.path(), &["commit", "-qm", "changed"]);
    assert_eq!(
        store
            .list_notes(Query {
                text: Some("Other branch".into()),
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
    git(temp.path(), &["checkout", "-q", &base]);
    assert_eq!(
        store
            .list_notes(Query {
                text: Some("Other branch".into()),
                ..Query::default()
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        store
            .list_notes(Query {
                text: Some("Base branch".into()),
                ..Query::default()
            })
            .unwrap()
            .total,
        1
    );
}

#[test]
fn manually_misplaced_private_notes_are_never_returned_as_public() {
    let (_temp, store) = fresh();
    tag(&store, "local", true);
    let local = note(&store, "Private despite bad location", &["private:local"]);
    fs::rename(
        store.directory().join(&local.path),
        store.directory().join(format!("notes/{}.md", local.id)),
    )
    .unwrap();
    assert_eq!(
        store
            .list_notes(Query {
                scope: Some(Scope::Public),
                ..Query::default()
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(store.review(Query::default()).unwrap().total, 0);
    assert!(
        store
            .doctor()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "misplaced_private_note")
    );
}

#[test]
fn invalid_paths_and_bodies_are_rejected_before_writes() {
    let (_temp, store) = fresh();
    assert_eq!(
        store
            .create_tag(CreateTag {
                name: "../escape".into(),
                topic: "bad".into(),
                body: String::new(),
                read_on_start: false,
                private: None
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert_eq!(
        store.get_note("../../escape").unwrap_err().code,
        "invalid_input"
    );
    assert_eq!(
        store
            .create_note(CreateNote {
                body_format: None,
                body: "  ".into(),
                tags: vec![],
                private: None
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
}

#[test]
fn missing_git_exclusions_prevent_private_writes_and_can_be_repaired() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    fs::remove_file(store.directory().join(".gitignore")).unwrap();
    let input = CreateNote {
        body_format: None,
        body: "Keep local".into(),
        tags: vec![],
        private: Some(true),
    };
    assert_eq!(
        store.create_note(input.clone()).unwrap_err().code,
        "privacy"
    );
    assert_eq!(
        fs::read_dir(store.directory().join("private/notes"))
            .unwrap()
            .count(),
        0
    );
    store.init(InitOptions::default()).unwrap();
    assert_eq!(
        store.create_note(input).unwrap().note.visibility,
        Visibility::Private
    );
}

#[test]
fn user_privacy_defaults_apply_to_initial_tag_definitions() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user.toml");
    fs::write(&user, "[tags]\ndefault_private = true\n").unwrap();
    let store = Store::with_user_config(temp.path(), Some(user)).unwrap();
    store.init(InitOptions::default()).unwrap();
    assert!(store.directory().join("private/tags/papercut.md").exists());
    assert!(!store.directory().join("tags/papercut.md").exists());
    assert_eq!(store.context(Scope::All).unwrap().startup_tags.len(), 4);
    assert_eq!(
        store.get_tag("private:papercut").unwrap().visibility,
        Visibility::Private
    );
}

#[test]
fn initialization_backfills_outcome_definitions_without_overwriting_customizations() {
    for config in [
        "[tags]\ndefault_private = false\n",
        "[tags]\ndefault_private = true\n",
        "[tags]\nprivate_patterns = ['resolved', 'adopted']\n",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user.toml");
        fs::write(&user, config).unwrap();
        let store = Store::with_user_config(temp.path(), Some(user)).unwrap();
        store.init(InitOptions::default()).unwrap();
        let private = config.contains("true") || config.contains("private_patterns");
        let prefix = if private { "private:" } else { "" };
        for name in ["resolved", "adopted"] {
            let definition = store.get_tag(&format!("{prefix}{name}")).unwrap();
            assert!(definition.read_on_start);
            assert!(!definition.topic.is_empty());
            assert!(!definition.body.is_empty());
            if name == "resolved" {
                assert!(definition.body.contains("user-confirmed restart"));
                assert!(definition.body.contains("separate fresh agent process"));
                assert!(
                    definition
                        .body
                        .contains("newly started agent MCP connection")
                );
            }
            assert_eq!(definition.visibility == Visibility::Private, private);
        }
        // Model an older store with no outcome definitions, then one customized definition.
        let namespace = if private { "private/tags" } else { "tags" };
        for name in ["resolved", "adopted"] {
            fs::remove_file(store.directory().join(format!("{namespace}/{name}.md"))).unwrap();
        }
        let custom = "---\ntopic: Team-specific resolution\nread_on_start: false\n---\nKeep this custom guidance.\n";
        // The definition in either namespace prevents init from adding the same bare name.
        let custom_path = store.directory().join("tags/resolved.md");
        fs::write(&custom_path, custom).unwrap();
        let result = store.init(InitOptions::default()).unwrap();
        assert_eq!(
            result.created,
            [format!(".robostickies/{namespace}/adopted.md")]
        );
        assert_eq!(fs::read_to_string(&custom_path).unwrap(), custom);
        assert_eq!(
            store.get_tag("resolved").unwrap().body,
            "Keep this custom guidance.\n"
        );
        assert_eq!(
            store
                .list_tags(Scope::All, Some("resolved"))
                .unwrap()
                .tags
                .len(),
            1
        );
        let adopted_path = store.directory().join(format!("{namespace}/adopted.md"));
        let adopted = fs::read(&adopted_path).unwrap();
        assert!(
            store
                .init(InitOptions::default())
                .unwrap()
                .created
                .is_empty()
        );
        assert_eq!(fs::read(adopted_path).unwrap(), adopted);
    }
}

#[test]
fn outcome_tags_preserve_visibility_and_survive_edits_and_archiving() {
    let (_temp, store) = fresh();
    // Existing notes must keep their namespace even if tag privacy defaults change.
    let public = note(&store, "Public defect", &["papercut"]);
    let private = store
        .create_note(CreateNote {
            body_format: None,
            body: "Local reusable guidance".into(),
            tags: vec!["paperclip".into()],
            private: Some(true),
        })
        .unwrap()
        .note;
    fs::remove_file(store.directory().join("tags/resolved.md")).unwrap();
    fs::remove_file(store.directory().join("tags/adopted.md")).unwrap();
    fs::write(
        store.directory().join("private/config.toml"),
        "[tags]\ndefault_private = true\n",
    )
    .unwrap();
    // Use the existing tag API with an explicit namespace when a definition is missing.
    tag(&store, "resolved", false);
    tag(&store, "adopted", true);
    for (source, outcome) in [(public, "resolved"), (private, "private:adopted")] {
        let mut tags = source.tags.clone();
        tags.push(outcome.into());
        let applied = store
            .update_note(UpdateNote {
                id: source.id.clone(),
                tags: Some(tags),
                expected_revision: source.revision,
                ..UpdateNote::default()
            })
            .unwrap()
            .note;
        assert_eq!(applied.visibility, source.visibility);
        assert_eq!(applied.path, source.path);
        let archived = store
            .update_note(UpdateNote {
                id: source.id.clone(),
                body: Some("Evidence and guidance destination recorded".into()),
                status: Some(NoteStatus::Archived),
                expected_revision: applied.revision,
                ..UpdateNote::default()
            })
            .unwrap()
            .note;
        let read = store.get_note(&source.id).unwrap();
        assert_eq!(read.tags, applied.tags);
        assert_eq!(read.status, NoteStatus::Archived);
        assert_eq!(read.visibility, source.visibility);
        assert_eq!(read.revision, archived.revision);
        let review = store
            .review(Query {
                scope: Some(Scope::All),
                status: StatusFilter::Archived,
                tags: vec![outcome.into()],
                ..Query::default()
            })
            .unwrap();
        assert_eq!(review.total, 1);
        assert_eq!(review.notes[0].id, source.id);
    }
}

#[test]
fn local_excludes_support_explicit_nested_roots_with_git_pattern_characters() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    let nested = temp.path().join("nested [project]");
    fs::create_dir(&nested).unwrap();
    let store = Store::with_user_config(&nested, None).unwrap();
    store
        .init(InitOptions {
            agents: false,
            ignore_location: IgnoreLocation::Local,
        })
        .unwrap();
    assert_eq!(
        store
            .create_note(CreateNote {
                body_format: None,
                body: "Nested private note".into(),
                tags: vec![],
                private: Some(true)
            })
            .unwrap()
            .note
            .visibility,
        Visibility::Private
    );
    assert!(store.doctor().unwrap().healthy);
}

#[cfg(unix)]
#[test]
fn symlinks_do_not_escape_the_repository_or_enter_public_reports() {
    let (_temp, store) = fresh();
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("outside.md");
    fs::write(&source, "external content").unwrap();
    std::os::unix::fs::symlink(&source, store.directory().join("notes/linked.md")).unwrap();
    let page = store.list_notes(Query::default()).unwrap();
    assert_eq!(page.total, 0);
    assert_eq!(page.diagnostics.len(), 1);
    fs::remove_dir(store.directory().join("private/notes")).unwrap();
    std::os::unix::fs::symlink(outside.path(), store.directory().join("private/notes")).unwrap();
    assert!(
        store
            .create_note(CreateNote {
                body_format: None,
                body: "must not escape".into(),
                tags: vec![],
                private: Some(true)
            })
            .is_err()
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}
