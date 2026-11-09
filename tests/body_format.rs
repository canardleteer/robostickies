use std::fs;

use robostickies::{Store, model::*};
use serde_json::json;

fn create(store: &Store, hint: Option<&str>) -> Note {
    store
        .create_note(CreateNote {
            body: "answer: 42\n".into(),
            body_format: hint.map(str::to_owned),
            tags: vec![],
            private: Some(true),
        })
        .unwrap()
        .note
}

fn patch(note: &Note) -> UpdateNote {
    UpdateNote {
        id: note.id.clone(),
        expected_revision: note.revision.clone(),
        ..UpdateNote::default()
    }
}

#[test]
fn hints_are_advisory_strings_and_edits_support_set_clear_and_revision_guards() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    for hint in [
        "markdown",
        "text",
        "yaml",
        "application/custom",
        "λ\ncontent",
    ] {
        let note = create(&store, Some(hint));
        assert_eq!(note.metadata["body_format"], hint);
        assert_eq!(note.body, "answer: 42\n");
    }
    let note = create(&store, None);
    assert!(note.metadata.is_empty());
    let updated = store
        .update_note(UpdateNote {
            body_format: Some("yaml".into()),
            ..patch(&note)
        })
        .unwrap()
        .note;
    assert_eq!(updated.metadata["body_format"], "yaml");
    let source = store.directory().join(&updated.path);
    let before = fs::read(&source).unwrap();
    for input in [
        UpdateNote {
            body_format: Some("text".into()),
            ..patch(&note)
        },
        UpdateNote {
            clear_body_format: true,
            ..patch(&note)
        },
    ] {
        assert_eq!(store.update_note(input).unwrap_err().code, "conflict");
        assert_eq!(fs::read(&source).unwrap(), before);
    }
    for input in [
        UpdateNote {
            body_format: Some(" ".into()),
            ..patch(&updated)
        },
        UpdateNote {
            body_format: Some("text".into()),
            clear_body_format: true,
            ..patch(&updated)
        },
    ] {
        assert_eq!(store.update_note(input).unwrap_err().code, "invalid_input");
        assert_eq!(fs::read(&source).unwrap(), before);
    }
    let cleared = store
        .update_note(UpdateNote {
            clear_body_format: true,
            ..patch(&updated)
        })
        .unwrap()
        .note;
    assert!(!cleared.metadata.contains_key("body_format"));
    let cleared_again = store
        .update_note(UpdateNote {
            clear_body_format: true,
            ..patch(&cleared)
        })
        .unwrap()
        .note;
    assert_eq!(cleared_again.metadata, cleared.metadata);
    let total = store.list_notes(Query::default()).unwrap().total;
    assert_eq!(
        store
            .create_note(CreateNote {
                body: "body".into(),
                body_format: Some("\n\t".into()),
                tags: vec![],
                private: Some(true)
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert_eq!(store.list_notes(Query::default()).unwrap().total, total);
}

#[test]
fn legacy_collisions_and_unrelated_metadata_survive_edits_in_both_newline_styles() {
    for newline in ["\n", "\r\n"] {
        for value in [
            "null",
            "42",
            "[one, two]",
            "{kind: custom}",
            "'yaml' # old hint",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::with_user_config(temp.path(), None).unwrap();
            store.init(InitOptions::default()).unwrap();
            let note = create(&store, None);
            let path = store.directory().join(&note.path);
            let extra = format!(
                "# custom comment\nbody_format: {value}\ncustom:\n  owner: local # retained\n"
            );
            let text = fs::read_to_string(&path)
                .unwrap()
                .replacen("---\n", &format!("---\n{extra}"), 1)
                .replace('\n', newline);
            fs::write(&path, &text).unwrap();
            let original = store.get_note(&note.id).unwrap();
            let updated = store
                .update_note(UpdateNote {
                    status: Some(NoteStatus::Archived),
                    ..patch(&original)
                })
                .unwrap()
                .note;
            assert_eq!(updated.metadata, original.metadata);
            let edited = fs::read_to_string(&path).unwrap();
            assert!(edited.contains(&extra.replace('\n', newline)));
            if value.starts_with('\'') {
                let cleared = store
                    .update_note(UpdateNote {
                        clear_body_format: true,
                        ..patch(&updated)
                    })
                    .unwrap()
                    .note;
                assert!(!cleared.metadata.contains_key("body_format"));
                assert_eq!(cleared.metadata["custom"], json!({"owner":"local"}));
                assert!(
                    fs::read_to_string(&path)
                        .unwrap()
                        .contains("owner: local # retained")
                );
            } else {
                for input in [
                    UpdateNote {
                        body_format: Some("text".into()),
                        ..patch(&updated)
                    },
                    UpdateNote {
                        clear_body_format: true,
                        ..patch(&updated)
                    },
                ] {
                    assert_eq!(store.update_note(input).unwrap_err().code, "invalid_input");
                    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
                }
            }
        }
    }
}

#[test]
fn clearing_hints_retains_comments_without_turning_scalar_data_into_comments() {
    for newline in ["\n", "\r\n"] {
        for hint in [
            "'hash # scalar data' # inline hint",
            "|- # scalar header\n  # scalar data",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::with_user_config(temp.path(), None).unwrap();
            store.init(InitOptions::default()).unwrap();
            let note = create(&store, None);
            let path = store.directory().join(&note.path);
            let original = fs::read_to_string(&path)
                .unwrap()
                .replacen(
                    "---\n",
                    &format!("---\n# hint heading\nbody_format: {hint}\n"),
                    1,
                )
                .trim_end_matches('\n')
                .replace('\n', newline);
            fs::write(&path, &original).unwrap();
            let note = store.get_note(&note.id).unwrap();
            let cleared = store
                .update_note(UpdateNote {
                    clear_body_format: true,
                    ..patch(&note)
                })
                .unwrap()
                .note;
            let edited = fs::read_to_string(&path).unwrap();
            assert!(cleared.metadata.is_empty());
            assert!(!edited.contains("scalar data"));
            for comment in ["# hint heading", "# inline hint", "# scalar header"] {
                assert_eq!(
                    edited.matches(comment).count(),
                    original.matches(comment).count()
                );
            }
            assert_eq!(cleared.body, note.body);
            assert_eq!(cleared.visibility, Visibility::Private);
            assert_eq!(store.get_note(&note.id).unwrap().revision, cleared.revision);
            if newline == "\r\n" {
                assert!(!edited.replace("\r\n", "").contains('\n'));
            }
        }
    }
}

#[test]
fn publication_strips_the_source_hint_and_accepts_only_an_explicit_public_hint() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    let note = create(&store, Some("private-hint-sentinel"));
    let source = fs::read(store.directory().join(&note.path)).unwrap();
    for dry_run in [true, false] {
        for hint in [None, Some("text")] {
            let publication = store
                .publish_note(PublishNote {
                    id: note.id.clone(),
                    dry_run,
                    body: Some("Reviewed UTF-8 prose ☕".into()),
                    body_format: hint.map(str::to_owned),
                    tags: None,
                    expected_revision: note.revision.clone(),
                })
                .unwrap();
            assert_eq!(publication.note.visibility, Visibility::Public);
            assert_eq!(
                publication.note.metadata.get("body_format"),
                hint.map(|v| json!(v)).as_ref()
            );
            assert_eq!(publication.note.metadata.len(), usize::from(hint.is_some()));
            assert_eq!(
                store.directory().join(&publication.note.path).exists(),
                !dry_run
            );
            assert_eq!(
                fs::read(store.directory().join(&note.path)).unwrap(),
                source
            );
        }
    }
    let review = serde_json::to_string(&store.review(Query::default()).unwrap()).unwrap();
    assert!(!review.contains("private-hint-sentinel"));
}
