use std::{collections::BTreeMap, fs};

use robostickies::{Store, model::*, version::DataVersion};
use serde_json::{Value, json};

fn setup() -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(root.path(), None).unwrap();
    store.init(InitOptions::default()).unwrap();
    (root, store)
}

fn task(store: &Store, body: &str, requires: &[&Note], private: bool) -> Note {
    store
        .create_note(CreateNote {
            body: body.into(),
            private: Some(private),
            task: Some(TaskSpec {
                requires: requires.iter().map(|n| n.id.clone()).collect(),
                ..TaskSpec::default()
            }),
            ..CreateNote::default()
        })
        .unwrap()
        .note
}

fn edit(store: &Store, note: &Note, patch: Value) -> Note {
    let mut input = json!({"id":note.id,"expected_revision":note.revision});
    input
        .as_object_mut()
        .unwrap()
        .extend(patch.as_object().unwrap().clone());
    store
        .update_note(serde_json::from_value(input).unwrap())
        .unwrap()
        .note
}

fn sheet(store: &Store, items: &[&Note], ordering: SheetOrdering, private: bool) -> Note {
    store
        .create_note(CreateNote {
            body: "Plan context\n\nFollow the source instructions.".into(),
            private: Some(private),
            sheet: Some(SheetSpec {
                items: items.iter().map(|n| n.id.clone()).collect(),
                ordering,
            }),
            ..CreateNote::default()
        })
        .unwrap()
        .note
}

fn render(store: &Store, note: &Note, scope: Scope) -> TaskSheet {
    store
        .render_task_sheet(RenderTaskSheet {
            id: note.id.clone(),
            scope: Some(scope),
        })
        .unwrap()
}

fn all(store: &Store) -> BTreeMap<String, TaskView> {
    store
        .list_tasks(TaskQuery {
            all_task_states: true,
            notes: Query {
                status: StatusFilter::All,
                ..Query::default()
            },
            ..TaskQuery::default()
        })
        .unwrap()
        .tasks
        .into_iter()
        .map(|t| (t.note.id.clone(), t))
        .collect()
}

#[test]
fn branching_dependencies_use_the_complete_graph_and_allow_work_out_of_order() {
    let (_root, store) = setup();
    let ordinary = store
        .create_note(CreateNote {
            body: "Checkbox [ ] and todo tag do not classify a note".into(),
            ..CreateNote::default()
        })
        .unwrap()
        .note;
    let a = task(&store, "Schema", &[], false);
    let b = task(&store, "Migration", &[&a], false);
    let c = task(&store, "Guide branch", &[&a], false);
    let d = task(&store, "Fan in", &[&b, &c], false);
    let initial = all(&store);
    assert_eq!(initial.len(), 4);
    assert!(!initial.contains_key(&ordinary.id));
    assert!(initial[&a.id].ready);
    assert_eq!(initial[&d.id].unsatisfied_prerequisites.len(), 3);
    assert_eq!(
        initial[&d.id]
            .blockers
            .iter()
            .find(|b| b.id == a.id)
            .unwrap()
            .path,
        vec![d.id.clone(), b.id.clone().min(c.id.clone()), a.id.clone()]
    );
    let b = edit(&store, &b, json!({"task":{"state":"done"}}));
    let c = edit(&store, &c, json!({"task":{"state":"in_progress"}}));
    assert!(all(&store)[&c.id].blocked);
    assert_eq!(b.metadata["task"]["requires"], json!([a.id]));
    assert!(all(&store)[&d.id].unsatisfied_prerequisites.contains(&a.id));
    let selected = store
        .list_tasks(TaskQuery {
            ready: Some(false),
            notes: Query {
                text: Some("Fan in".into()),
                limit: 1,
                ..Query::default()
            },
            ..TaskQuery::default()
        })
        .unwrap();
    assert_eq!(selected.total, 1);
    assert_eq!(selected.tasks[0].unsatisfied_prerequisites.len(), 2);
    let a = edit(
        &store,
        &a,
        json!({"task":{"state":"done"},"status":"archived"}),
    );
    assert!(all(&store)[&c.id].ready);
    let c = edit(&store, &c, json!({"task":{"state":"cancelled"}}));
    assert_eq!(
        all(&store)[&d.id].unsatisfied_prerequisites,
        vec![c.id.clone()]
    );
    let c = edit(
        &store,
        &c,
        json!({"task":{"state":"done"},"status":"archived"}),
    );
    assert!(all(&store)[&d.id].ready);
    let a = edit(&store, &a, json!({"task":{"state":"todo"}}));
    assert!(!all(&store)[&a.id].ready);
    assert!(!all(&store)[&d.id].ready);
    assert_eq!(all(&store)[&d.id].unsatisfied_prerequisites, vec![a.id]);
    assert_eq!(c.status, NoteStatus::Archived);
}

#[test]
fn sheets_preserve_preferred_and_display_order_and_include_external_prerequisites() {
    let (_root, store) = setup();
    let a = task(
        &store,
        "External prerequisite\n\nFull body and instructions",
        &[],
        false,
    );
    let b = task(&store, "Earlier but blocked", &[&a], false);
    let c = task(&store, "Later and ready", &[], false);
    let preferred = sheet(&store, &[&b, &c], SheetOrdering::Preferred, false);
    let display = sheet(&store, &[&c, &b], SheetOrdering::Display, false);
    let result = render(&store, &preferred, Scope::Public);
    assert!(result.complete);
    assert_eq!(result.next_task, Some(c.id.clone()));
    assert_eq!(
        result.rows.iter().map(|r| &r.id).collect::<Vec<_>>(),
        vec![&b.id, &c.id]
    );
    assert_eq!(result.context[0].id, a.id);
    assert!(result.markdown.contains("Full body and instructions"));
    for needle in [
        &b.revision,
        "read-only",
        "relative to the .robostickies",
        "Next suggested task",
        "Prerequisite context",
    ] {
        assert!(result.markdown.contains(needle), "missing {needle}");
    }
    assert!(render(&store, &display, Scope::Public).next_task.is_none());
    let preferred = edit(&store, &preferred, json!({"sheet":{"items":[c.id,b.id]}}));
    assert_eq!(render(&store, &preferred, Scope::All).rows[0].id, c.id);
    assert_eq!(store.get_note(&b.id).unwrap().revision, b.revision);
    let empty = edit(&store, &preferred, json!({"sheet":{"items":[]}}));
    let result = render(&store, &empty, Scope::All);
    assert!(result.rows.is_empty() && result.complete && result.next_task.is_none());
    assert_eq!(
        store
            .render_task_sheet(RenderTaskSheet {
                id: b.id,
                scope: None
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
}

#[test]
fn deferral_transitions_preserve_prerequisites_and_terminal_states_clear_waiting() {
    let (_root, store) = setup();
    let a = task(&store, "Prerequisite", &[], false);
    let b = task(&store, "Waiting", &[&a], false);
    let b = edit(&store, &b, json!({"task":{"deferral":{"kind":"manual"}}}));
    let view = &all(&store)[&b.id];
    assert!(view.blocked && view.active_deferral.is_some() && !view.ready);
    let b = edit(&store, &b, json!({"task":{"clear_deferral":true}}));
    assert!(all(&store)[&b.id].active_deferral.is_none());
    let b = edit(
        &store,
        &b,
        json!({"task":{"deferral":{"kind":"until","until":"2040-01-01"}}}),
    );
    assert!(
        b.metadata["task"]["deferral"]["until"]
            .as_str()
            .unwrap()
            .starts_with("2040-01-01T00:00:00")
    );
    let plan = sheet(&store, &[&b], SheetOrdering::Display, false);
    assert!(
        render(&store, &plan, Scope::Public)
            .markdown
            .contains("Deferred until")
    );
    for terminal in ["done", "cancelled"] {
        let b = store.get_note(&b.id).unwrap();
        let b = edit(&store, &b, json!({"task":{"state":terminal}}));
        assert!(b.metadata["task"].get("deferral").is_none());
        assert_eq!(b.metadata["task"]["requires"], json!([a.id]));
        assert_eq!(
            store
                .update_note(UpdateNote {
                    id: b.id.clone(),
                    expected_revision: b.revision.clone(),
                    task: Some(TaskPatch {
                        deferral: Some(Deferral::Manual),
                        ..TaskPatch::default()
                    }),
                    ..UpdateNote::default()
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        let reopened = edit(&store, &b, json!({"task":{"state":"todo"}}));
        assert!(all(&store)[&reopened.id].active_deferral.is_none());
        // Continue with a refreshed source for the next terminal transition.
        if terminal == "done" {
            let _ = edit(
                &store,
                &reopened,
                json!({"task":{"deferral":{"kind":"until","until":"2040-01-01"}}}),
            );
        }
    }
}

#[test]
fn classification_reference_and_cycle_validation_rejects_without_source_changes() {
    let (_root, store) = setup();
    let ordinary = store
        .create_note(CreateNote {
            body: "Ordinary".into(),
            ..CreateNote::default()
        })
        .unwrap()
        .note;
    let a = task(&store, "A", &[], false);
    let b = task(&store, "B", &[&a], false);
    for patch in [
        json!({"task":{"requires":[a.id]}}),
        json!({"task":{"requires":[b.id,b.id]}}),
        json!({"task":{"requires":[b.id]}}),
        json!({"task":{"requires":[ordinary.id]}}),
        json!({"task":{"requires":["00000000-0000-0000-0000-000000000000"]}}),
        json!({"sheet":{"items":[]}}),
        json!({"clear_task":true}),
        json!({"task":{"state":"done"},"clear_task":true}),
        json!({"task":{"deferral":{"kind":"manual"},"clear_deferral":true}}),
    ] {
        let bytes = fs::read(store.directory().join(&a.path)).unwrap();
        let mut input = json!({"id":a.id,"expected_revision":a.revision});
        input
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(
            store
                .update_note(serde_json::from_value(input).unwrap())
                .unwrap_err()
                .code,
            "invalid_input",
            "{patch}"
        );
        assert_eq!(fs::read(store.directory().join(&a.path)).unwrap(), bytes);
    }
    let plan = sheet(&store, &[&a], SheetOrdering::Preferred, false);
    let b = edit(&store, &b, json!({"task":{"requires":[]}}));
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: a.id.clone(),
                expected_revision: a.revision.clone(),
                clear_task: true,
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    let _plan = edit(&store, &plan, json!({"sheet":{"items":[]}}));
    let a = edit(&store, &a, json!({"clear_task":true}));
    assert!(!a.metadata.contains_key("task"));
    let a = edit(&store, &a, json!({"sheet":{"items":[b.id]}}));
    assert!(a.metadata.contains_key("sheet"));
    let a = edit(&store, &a, json!({"clear_sheet":true}));
    let a = edit(&store, &a, json!({"task":{}}));
    assert_eq!(a.metadata["task"]["state"], "todo");
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: a.id.clone(),
                expected_revision: ordinary.revision,
                task: Some(TaskPatch::default()),
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "conflict"
    );
    for value in [
        json!({"body":"Bad","task":null}),
        json!({"body":"Bad","task":{"state":"wat"}}),
        json!({"body":"Bad","task":{"state":"todo","extra":true}}),
        json!({"body":"Bad","task":{"state":"todo","requires":null}}),
        json!({"body":"Bad","sheet":{"items":[],"ordering":"strict"}}),
        json!({"body":"Bad","sheet":null}),
        json!({"body":"Bad","task":{"state":"todo","deferral":{"kind":"manual","until":"2030-01-01"}}}),
    ] {
        assert!(
            serde_json::from_value::<CreateNote>(value.clone()).is_err(),
            "{value}"
        );
    }
    for patch in [
        json!({"task":null}),
        json!({"task":{"state":null}}),
        json!({"task":{"deferral":null}}),
        json!({"sheet":{"items":null}}),
        json!({"sheet":{"ordering":null}}),
    ] {
        let mut input = json!({"id":a.id,"expected_revision":a.revision});
        input
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(
            serde_json::from_value::<UpdateNote>(input).is_err(),
            "{patch}"
        );
    }
}

#[test]
fn public_incoming_edges_prevent_privatization_and_private_only_rows_need_local_scope() {
    let (_root, store) = setup();
    let a = task(&store, "Public prerequisite", &[], false);
    let b = task(&store, "Public dependent", &[&a], false);
    let private = task(&store, "Private dependent", &[&a], true);
    let plan = sheet(&store, &[&a, &private], SheetOrdering::Preferred, true);
    assert_eq!(
        store
            .render_task_sheet(RenderTaskSheet {
                id: plan.id.clone(),
                scope: None
            })
            .unwrap_err()
            .code,
        "not_found"
    );
    let partial = render(&store, &plan, Scope::Private);
    assert!(!partial.complete && partial.next_task.is_none());
    assert_eq!(
        partial.rows[0].unavailable.as_deref(),
        Some("outside_scope")
    );
    assert_eq!(partial.context[0].id, a.id);
    assert!(render(&store, &plan, Scope::All).complete);
    for request in [
        UpdateNote {
            id: a.id.clone(),
            expected_revision: a.revision.clone(),
            private: Some(true),
            ..UpdateNote::default()
        },
        UpdateNote {
            id: a.id.clone(),
            expected_revision: a.revision.clone(),
            tags: Some(vec!["private:local".into()]),
            ..UpdateNote::default()
        },
    ] {
        if request.tags.is_some() {
            store
                .create_tag(CreateTag {
                    name: "local".into(),
                    description: None,
                    body: String::new(),
                    read_on_start: false,
                    private: Some(true),
                })
                .unwrap();
        }
        assert_eq!(store.update_note(request).unwrap_err().code, "privacy");
        assert!(store.directory().join(&a.path).exists());
    }
    let _b = edit(&store, &b, json!({"task":{"requires":[]}}));
    let public_sheet = sheet(&store, &[&a], SheetOrdering::Display, false);
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: a.id.clone(),
                expected_revision: a.revision.clone(),
                private: Some(true),
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "privacy"
    );
    let _public_sheet = edit(&store, &public_sheet, json!({"sheet":{"items":[]}}));
    let a = edit(&store, &a, json!({"private":true}));
    assert!(a.path.starts_with("private/"));
    assert!(store.get_note(&private.id).is_ok());
    assert_eq!(
        store
            .create_note(CreateNote {
                body: "Reject private target".into(),
                task: Some(TaskSpec {
                    requires: vec![private.id],
                    ..TaskSpec::default()
                }),
                ..CreateNote::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
}

#[test]
fn sheet_member_diagnostics_are_unique_and_retain_distinct_graph_problems() {
    for private in [false, true] {
        let (_root, store) = setup();
        let missing = task(&store, "Deleted member", &[], private);
        let malformed = task(&store, "Malformed member", &[], private);
        let blocked = task(&store, "Dependent member", &[&missing], private);
        let plan = sheet(
            &store,
            &[&missing, &malformed, &blocked],
            SheetOrdering::Preferred,
            private,
        );
        let original_plan = fs::read(store.directory().join(&plan.path)).unwrap();
        let original_blocked = fs::read(store.directory().join(&blocked.path)).unwrap();
        fs::remove_file(store.directory().join(&missing.path)).unwrap();
        fs::write(
            store.directory().join(&malformed.path),
            "Malformed source\n",
        )
        .unwrap();

        let result = render(
            &store,
            &plan,
            if private { Scope::All } else { Scope::Public },
        );
        assert!(!result.complete && result.next_task.is_none());
        assert!(result.rows[0].task.is_none() && result.rows[1].task.is_none());
        assert!(result.context.iter().any(|row| row.id == missing.id));
        let member_diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.path == plan.path)
            .collect();
        assert_eq!(member_diagnostics.len(), 2);
        for member in [&missing, &malformed] {
            assert_eq!(
                member_diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.message.contains(&member.id))
                    .count(),
                1
            );
            assert_eq!(
                result
                    .markdown
                    .matches(&format!("Sheet item {}", member.id))
                    .count(),
                1
            );
        }
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.path == blocked.path
                && diagnostic.code == "unresolved_reference"
                && diagnostic.message.contains(&missing.id)
        }));
        assert_eq!(
            fs::read(store.directory().join(&plan.path)).unwrap(),
            original_plan
        );
        assert_eq!(
            fs::read(store.directory().join(&blocked.path)).unwrap(),
            original_blocked
        );
    }
}

#[test]
fn direct_edits_deletions_and_duplicate_ids_have_scoped_diagnostics_and_incomplete_sheets() {
    let (_root, store) = setup();
    let a = task(&store, "Public A", &[], false);
    let b = task(&store, "Public B", &[&a], false);
    let plan = sheet(&store, &[&b], SheetOrdering::Preferred, false);
    let before = serde_json::to_value(render(&store, &plan, Scope::Public)).unwrap();
    let original_a = fs::read_to_string(store.directory().join(&a.path)).unwrap();
    fs::write(
        store.directory().join(format!("private/notes/{}.md", a.id)),
        original_a.replace("Public A", "PRIVATE DUPLICATE SENTINEL"),
    )
    .unwrap();
    let mut after = serde_json::to_value(render(&store, &plan, Scope::Public)).unwrap();
    after["evaluated_at"] = before["evaluated_at"].clone();
    after["markdown"] = json!(after["markdown"].as_str().unwrap().replace(
        after["evaluated_at"].as_str().unwrap(),
        before["evaluated_at"].as_str().unwrap()
    ));
    // Compare content after normalizing the separately captured evaluation time.
    for field in ["rows", "context", "diagnostics", "complete", "next_task"] {
        assert_eq!(after[field], before[field], "{field}");
    }
    assert!(!after.to_string().contains("PRIVATE DUPLICATE SENTINEL"));
    assert_eq!(store.get_note(&a.id).unwrap_err().code, "conflict");
    let local = store.list_tasks(TaskQuery::default()).unwrap();
    assert!(
        local
            .tasks
            .iter()
            .any(|task| task.note.id == a.id && task.note.visibility == Visibility::Public)
    );
    assert_eq!(local.total, local.tasks.len());
    assert!(
        store
            .doctor()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "duplicate_id")
    );
    fs::remove_file(store.directory().join(format!("private/notes/{}.md", a.id))).unwrap();
    fs::remove_file(store.directory().join(&a.path)).unwrap();
    let result = render(&store, &plan, Scope::Public);
    assert!(!result.complete && result.next_task.is_none());
    assert!(result.context[0].task.is_none());
    assert_eq!(
        all(&store)[&b.id].blockers[0].reason,
        "unresolved_reference"
    );
    fs::write(
        store.directory().join(&a.path),
        original_a.replace("requires: []", &format!("requires: [{}]", b.id)),
    )
    .unwrap();
    assert!(
        store
            .doctor()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "cycle")
    );
    let b = edit(
        &store,
        &b,
        json!({"task":{"state":"done"},"body":"Work completed despite invalid graph"}),
    );
    assert!(all(&store)[&b.id].blocked);
    let independent = task(&store, "Independent repair", &[], false);
    assert!(all(&store)[&independent.id].ready);
    let a = store.get_note(&a.id).unwrap();
    let _a = edit(&store, &a, json!({"task":{"requires":[]}}));
    assert!(store.doctor().unwrap().healthy);
}

#[test]
fn structured_publication_is_explicit_revision_guarded_and_keeps_the_original() {
    let (_root, store) = setup();
    let a = task(&store, "Public evidence", &[], false);
    let private = task(&store, "Reviewed prose", &[&a], true);
    let bytes = fs::read(store.directory().join(&private.path)).unwrap();
    for structured in [false, true] {
        for dry_run in [true, false] {
            let published = store
                .publish_note(PublishNote {
                    id: private.id.clone(),
                    expected_revision: private.revision.clone(),
                    dry_run,
                    task: structured.then(|| TaskSpec {
                        requires: vec![a.id.clone()],
                        ..TaskSpec::default()
                    }),
                    ..PublishNote::default()
                })
                .unwrap();
            assert_ne!(published.note.id, private.id);
            assert_eq!(published.note.metadata.contains_key("task"), structured);
            assert_eq!(
                store.directory().join(&published.note.path).exists(),
                !dry_run
            );
            assert_eq!(
                fs::read(store.directory().join(&private.path)).unwrap(),
                bytes
            );
        }
    }
    for dry_run in [true, false] {
        assert_eq!(
            store
                .publish_note(PublishNote {
                    id: private.id.clone(),
                    expected_revision: private.revision.clone(),
                    dry_run,
                    task: Some(TaskSpec {
                        requires: vec![private.id.clone()],
                        ..TaskSpec::default()
                    }),
                    ..PublishNote::default()
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
    }
    let plan = sheet(&store, &[&a], SheetOrdering::Preferred, true);
    let public = store
        .publish_note(PublishNote {
            id: plan.id.clone(),
            expected_revision: plan.revision.clone(),
            sheet: Some(SheetSpec {
                items: vec![a.id.clone()],
                ordering: SheetOrdering::Display,
            }),
            ..PublishNote::default()
        })
        .unwrap()
        .note;
    assert_eq!(render(&store, &public, Scope::Public).rows[0].id, a.id);
    assert_eq!(
        store
            .publish_note(PublishNote {
                id: private.id,
                expected_revision: "stale".into(),
                ..PublishNote::default()
            })
            .unwrap_err()
            .code,
        "conflict"
    );
}

#[test]
fn nested_task_edits_preserve_comments_metadata_bodies_and_newline_conventions() {
    for newline in ["\n", "\r\n"] {
        let (_root, store) = setup();
        let a = task(&store, "Prerequisite", &[], true);
        let b = task(&store, "No final newline", &[&a], true);
        let source = format!("---\n# header\nid: {}\ncreated_at: '{}'\nupdated_at: '{}'\ntags: []\nstatus: active\ntask: # task heading\n  state: todo # state\n  requires: # requires\n    - {} # edge\n  deferral: # waiting\n    kind: until\n    until: '2040-01-01T00:00:00Z' # date\ncustom:\n  nested: [one, two] # untouched\n---\nNo final newline",b.id,b.created_at,b.updated_at,a.id).replace('\n',newline);
        fs::write(store.directory().join(&b.path), &source).unwrap();
        let b = store.get_note(&b.id).unwrap();
        let b = edit(
            &store,
            &b,
            json!({"task":{"state":"in_progress","requires":[],"clear_deferral":true}}),
        );
        assert_eq!(b.body, "No final newline");
        assert_eq!(b.metadata["custom"], json!({"nested":["one","two"]}));
        let source = fs::read_to_string(store.directory().join(&b.path)).unwrap();
        for comment in [
            "# header",
            "# task heading",
            "# state",
            "# requires",
            "# edge",
            "# waiting",
            "# date",
            "# untouched",
        ] {
            assert!(source.contains(comment), "missing {comment}: {source}");
        }
        assert_eq!(source.contains("\r\n"), newline == "\r\n");
        let b = edit(&store, &b, json!({"clear_task":true}));
        assert_eq!(b.body, "No final newline");
        assert!(!b.metadata.contains_key("task"));
        assert!(store.reindex().unwrap().healthy);
    }
}

#[test]
fn task_writes_survive_cache_failure_and_relationship_validation_has_a_source_fallback() {
    let (_root, store) = setup();
    let a = task(&store, "Prerequisite", &[], false);
    fs::remove_file(store.directory().join("cache/index.redb")).unwrap();
    fs::create_dir(store.directory().join("cache/index.redb")).unwrap();
    let saved = store
        .create_note(CreateNote {
            body: "Source survives cache failure".into(),
            task: Some(TaskSpec {
                requires: vec![a.id.clone()],
                ..TaskSpec::default()
            }),
            ..CreateNote::default()
        })
        .unwrap();
    assert!(!saved.warnings.is_empty());
    assert!(store.get_note(&saved.note.id).is_ok());
    assert_eq!(
        store
            .update_note(UpdateNote {
                id: a.id.clone(),
                expected_revision: a.revision.clone(),
                task: Some(TaskPatch {
                    requires: Some(vec![saved.note.id.clone()]),
                    ..TaskPatch::default()
                }),
                ..UpdateNote::default()
            })
            .unwrap_err()
            .code,
        "invalid_input"
    );
    let done = store
        .update_note(UpdateNote {
            id: saved.note.id,
            expected_revision: saved.note.revision,
            task: Some(TaskPatch {
                state: Some(TaskState::Done),
                ..TaskPatch::default()
            }),
            ..UpdateNote::default()
        })
        .unwrap();
    assert!(!done.warnings.is_empty());
    fs::remove_dir(store.directory().join("cache/index.redb")).unwrap();
    assert_eq!(all(&store).len(), 2);
    assert!(store.reindex().unwrap().healthy);
}

#[test]
fn v1alpha5_upgrade_reserves_metadata_without_reinterpreting_legacy_notes() {
    for namespace in ["notes", "private/notes"] {
        for key in ["task", "sheet"] {
            let (_root, store) = setup();
            let note = task(
                &store,
                "Legacy custom metadata",
                &[],
                namespace.starts_with("private"),
            );
            let config = store.directory().join("config.toml");
            fs::write(
                &config,
                fs::read_to_string(&config)
                    .unwrap()
                    .replace("v1alpha5", "v1alpha4"),
            )
            .unwrap();
            let path = store.directory().join(&note.path);
            let source = fs::read_to_string(&path)
                .unwrap()
                .replace("task:", &format!("{key}:"));
            fs::write(&path, &source).unwrap();
            assert!(store.get_note(&note.id).is_ok());
            assert_eq!(store.list_tasks(TaskQuery::default()).unwrap().total, 0);
            for dry in [true, false] {
                let error = store.upgrade(dry).unwrap_err();
                assert!(error.message.contains("collision"));
                assert_eq!(fs::read_to_string(&path).unwrap(), source);
                assert_eq!(
                    store.configuration().unwrap().data_version,
                    DataVersion::V1Alpha4
                );
            }
            assert_eq!(
                store
                    .create_note(CreateNote {
                        body: "Requires upgrade".into(),
                        task: Some(TaskSpec::default()),
                        ..CreateNote::default()
                    })
                    .unwrap_err()
                    .code,
                "upgrade_required"
            );
            assert_eq!(
                store
                    .update_note(UpdateNote {
                        id: note.id.clone(),
                        expected_revision: store.get_note(&note.id).unwrap().revision,
                        clear_task: true,
                        ..UpdateNote::default()
                    })
                    .unwrap_err()
                    .code,
                "upgrade_required"
            );
            let migrated = source.replace(&format!("{key}:"), "custom:");
            fs::write(&path, &migrated).unwrap();
            assert!(!store.upgrade(true).unwrap().wrote);
            assert_eq!(fs::read_to_string(&path).unwrap(), migrated);
            assert!(store.upgrade(false).unwrap().wrote);
            assert!(!store.upgrade(false).unwrap().wrote);
            assert_eq!(fs::read_to_string(&path).unwrap(), migrated);
            assert_eq!(
                store.configuration().unwrap().data_version,
                DataVersion::V1Alpha5
            );
        }
    }
}

#[test]
fn dated_deferral_writers_quote_rfc3339_and_preserve_unmentioned_source_spelling() {
    for newline in ["\n", "\r\n"] {
        let (_root, store) = setup();
        let note = store
            .create_note(CreateNote {
                body: "Dated task".into(),
                task: Some(TaskSpec {
                    deferral: Some(Deferral::Until {
                        until: "2040-01-01".into(),
                    }),
                    ..TaskSpec::default()
                }),
                ..CreateNote::default()
            })
            .unwrap()
            .note;
        let path = store.directory().join(&note.path);
        let source = fs::read_to_string(&path).unwrap();
        let quoted = source
            .lines()
            .find(|line| line.trim_start().starts_with("until:"))
            .unwrap()
            .trim_start();
        assert!(
            quoted.starts_with("until: \"") || quoted.starts_with("until: '"),
            "{source}"
        );
        let source = source.replace('\n', newline);
        fs::write(&path, &source).unwrap();
        let note = store.get_note(&note.id).unwrap();
        let note = edit(
            &store,
            &note,
            json!({"task":{"deferral":{"kind":"until","until":"2040-02-03T01:02:03+02:00"}}}),
        );
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            source.contains("\"2040-02-02T23:02:03.000000000Z\"")
                || source.contains("'2040-02-02T23:02:03.000000000Z'")
        );
        assert_eq!(source.contains("\r\n"), newline == "\r\n");
        let until = source
            .lines()
            .find(|line| line.trim_start().starts_with("until:"))
            .unwrap()
            .to_owned();
        let note = edit(&store, &note, json!({"task":{"state":"in_progress"}}));
        assert!(fs::read_to_string(&path).unwrap().contains(&until));
        assert_eq!(
            note.metadata["task"]["deferral"]["until"],
            "2040-02-02T23:02:03.000000000Z"
        );
        let original = fs::read_to_string(&path).unwrap().replace(
            "2040-02-02T23:02:03.000000000Z",
            "2040-02-03T01:02:03+02:00",
        );
        fs::write(&path, &original).unwrap();
        let note = store.get_note(&note.id).unwrap();
        let note = edit(&store, &note, json!({"task":{"state":"todo"}}));
        assert_eq!(
            note.metadata["task"]["deferral"]["until"],
            "2040-02-03T01:02:03+02:00"
        );
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("2040-02-03T01:02:03+02:00")
        );
    }
}

#[test]
fn patches_preserve_omitted_default_fields_in_authored_mappings() {
    let (_root, store) = setup();
    let task = task(&store, "Task with omitted defaults", &[], false);
    let path = store.directory().join(&task.path);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("  requires: []\n", ""),
    )
    .unwrap();
    let task = store.get_note(&task.id).unwrap();
    assert!(task.metadata["task"].get("requires").is_none());
    let task = edit(&store, &task, json!({"task":{"state":"in_progress"}}));
    assert!(task.metadata["task"].get("requires").is_none());
    let sheet = sheet(&store, &[&task], SheetOrdering::Display, false);
    let path = store.directory().join(&sheet.path);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("  ordering: display\n", ""),
    )
    .unwrap();
    let sheet = store.get_note(&sheet.id).unwrap();
    assert!(sheet.metadata["sheet"].get("ordering").is_none());
    let sheet = edit(&store, &sheet, json!({"sheet":{"items":[]}}));
    assert!(sheet.metadata["sheet"].get("ordering").is_none());
    assert_eq!(
        render(&store, &sheet, Scope::Public).sheet.ordering,
        SheetOrdering::Display
    );
}

#[test]
fn legacy_sheet_shaped_metadata_remains_opaque_to_rendering() {
    for private in [false, true] {
        let (_root, store) = setup();
        let saved = sheet(&store, &[], SheetOrdering::Preferred, private);
        let config = store.directory().join("config.toml");
        fs::write(
            &config,
            fs::read_to_string(&config)
                .unwrap()
                .replace("v1alpha5", "v1alpha4"),
        )
        .unwrap();
        let source = fs::read(store.directory().join(&saved.path)).unwrap();
        let read = store.get_note(&saved.id).unwrap();
        assert_eq!(read.metadata, saved.metadata);
        for scope in [
            Scope::All,
            if private {
                Scope::Private
            } else {
                Scope::Public
            },
        ] {
            assert_eq!(
                store
                    .render_task_sheet(RenderTaskSheet {
                        id: saved.id.clone(),
                        scope: Some(scope)
                    })
                    .unwrap_err()
                    .code,
                "invalid_input"
            );
        }
        assert!(
            store
                .upgrade(true)
                .unwrap_err()
                .message
                .contains("collision")
        );
        assert_eq!(
            fs::read(store.directory().join(&saved.path)).unwrap(),
            source
        );
        assert_eq!(
            store.configuration().unwrap().data_version,
            DataVersion::V1Alpha4
        );
    }
}

#[test]
fn long_sheet_bodies_follow_the_width_of_the_ordered_list_marker() {
    let (_root, store) = setup();
    let tasks = (1..=10)
        .map(|index| {
            task(
                &store,
                &format!("Instructions for task {index}."),
                &[],
                false,
            )
        })
        .collect::<Vec<_>>();
    let sheet = sheet(
        &store,
        &tasks.iter().collect::<Vec<_>>(),
        SheetOrdering::Display,
        false,
    );
    let result = render(&store, &sheet, Scope::Public);
    let tenth = result.markdown.split("10. [ ]").nth(1).unwrap();
    assert!(tenth.contains("\n    Revision:"), "{tenth}");
    assert!(tenth.contains("\n    Instructions for task 10."), "{tenth}");
}

#[test]
fn malformed_targets_and_misplaced_private_duplicates_fail_closed_in_graph_views() {
    let (_root, store) = setup();
    let a = task(&store, "Evidence", &[], false);
    let b = task(&store, "Dependent", &[&a], false);
    let source = fs::read_to_string(store.directory().join(&a.path)).unwrap();
    for corrupt in [
        source.replace("state: todo", "state: invalid"),
        source.replace("state: todo", "extra: invalid"),
        source.replace("state: todo", "state: todo\n  extra: true"),
    ] {
        fs::write(store.directory().join(&a.path), corrupt).unwrap();
        assert_eq!(store.get_note(&a.id).unwrap_err().code, "invalid_document");
        let rows = all(&store);
        assert!(!rows[&b.id].ready && rows[&b.id].blocked);
        assert!(
            store
                .doctor()
                .unwrap()
                .diagnostics
                .iter()
                .any(|d| d.code == "invalid_document")
        );
    }
    fs::write(store.directory().join(&a.path), &source).unwrap();
    store
        .create_tag(CreateTag {
            name: "hidden".into(),
            description: None,
            body: String::new(),
            read_on_start: false,
            private: Some(true),
        })
        .unwrap();
    let private = task(&store, "PRIVATE SENTINEL", &[], true);
    let private_source = fs::read_to_string(store.directory().join(&private.path))
        .unwrap()
        .replace("tags: []", "tags: [private:hidden]");
    fs::write(store.directory().join(&private.path), &private_source).unwrap();
    fs::write(
        store.directory().join(format!("notes/{}.md", private.id)),
        &private_source,
    )
    .unwrap();
    let local = store.list_tasks(TaskQuery::default()).unwrap();
    assert_eq!(local.total, local.tasks.len());
    assert!(local.tasks.iter().all(|task| task.note.id != private.id));
    let public = store
        .list_tasks(TaskQuery {
            notes: Query {
                scope: Some(Scope::Public),
                ..Query::default()
            },
            ..TaskQuery::default()
        })
        .unwrap();
    assert!(!serde_json::to_string(&public).unwrap().contains("SENTINEL"));
    assert!(public.diagnostics.is_empty());
    assert!(
        store
            .create_note(CreateNote {
                body: "Conflicting classifications".into(),
                task: Some(TaskSpec::default()),
                sheet: Some(SheetSpec::default()),
                ..CreateNote::default()
            })
            .is_err()
    );
    assert!(
        store
            .create_note(CreateNote {
                body: "Unavailable sheet target".into(),
                sheet: Some(SheetSpec {
                    items: vec![private.id],
                    ..SheetSpec::default()
                }),
                ..CreateNote::default()
            })
            .is_err()
    );
}
