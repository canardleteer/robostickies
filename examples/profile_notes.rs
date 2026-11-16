//! Representative local workload for `cargo xtask profile`.
//! All input is synthetic and lives in an automatically removed temporary store.

use std::{
    fs,
    hint::black_box,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use robostickies::{
    Store,
    model::{
        InitOptions, NotePage, Query, RenderTaskSheet, Review, Scope, TaskPatch, TaskQuery,
        UpdateNote,
    },
};
use uuid::Uuid;

fn main() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let store = Store::with_user_config(temporary.path(), None)?;
    seed(&store)?;
    let cold = Instant::now();
    let health = store.reindex()?;
    ensure!(
        health.healthy && health.notes == 256,
        "cold graph indexing failed"
    );
    let cold = cold.elapsed();
    let mut warm = Duration::ZERO;
    let mut render = Duration::ZERO;
    let mut edits = Duration::ZERO;
    for pass in 0..8 {
        let health = store.reindex()?;
        ensure!(
            health.healthy && health.notes == 256,
            "synthetic store indexing failed"
        );
        black_box((health, selected_notes(&store)?, complete_review(&store)?));
        let start = Instant::now();
        let tasks = store.list_tasks(TaskQuery {
            ready: Some(true),
            ..TaskQuery::default()
        })?;
        ensure!(tasks.total == 24, "task graph query failed");
        warm += start.elapsed();
        black_box(tasks);
        let start = Instant::now();
        let sheet = store.render_task_sheet(RenderTaskSheet {
            id: Uuid::from_u128(256).to_string(),
            scope: Some(Scope::All),
        })?;
        ensure!(
            sheet.complete && sheet.rows.len() == 192 && sheet.next_task.is_some(),
            "task sheet rendering failed"
        );
        render += start.elapsed();
        black_box(sheet);
        let start = Instant::now();
        let note = store.get_note(&Uuid::from_u128(192).to_string())?;
        let saved = store.update_note(UpdateNote {
            id: note.id,
            expected_revision: note.revision,
            task: Some(TaskPatch {
                requires: Some(vec![
                    Uuid::from_u128(if pass % 2 == 0 { 190 } else { 191 }).to_string(),
                ]),
                ..TaskPatch::default()
            }),
            ..UpdateNote::default()
        })?;
        ensure!(
            saved.warnings.is_empty(),
            "relationship edit cache refresh failed"
        );
        edits += start.elapsed();
        black_box(saved);
    }
    temporary.close()?;
    eprintln!(
        "Profile workload completed: 256 synthetic private notes, 192 tasks, one sheet; eight index/query/review/task/render/edit passes.\nCold indexing: {cold:?}; warm task queries: {warm:?}; sheets: {render:?}; relationship edits: {edits:?}."
    );
    Ok(())
}

fn seed(store: &Store) -> Result<()> {
    store.init(InitOptions::default())?;
    // Seed Markdown directly to measure a cold index and repeatable source scans.
    for index in 0..256_u128 {
        let id = Uuid::from_u128(index + 1);
        let tag = if index % 2 == 0 {
            "papercut"
        } else {
            "paperclip"
        };
        fs::write(
            store.directory().join(format!("private/notes/{id}.md")),
            format!(
                "---\nid: {id}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\ntags: [{tag}]\nstatus: active\n{}---\nSynthetic profiling note {index}: Markdown scanning, indexing, filtering, and review.\n",
                graph_metadata(index)
            ),
        )?;
    }
    Ok(())
}

fn graph_metadata(index: u128) -> String {
    if index < 192 {
        let state = if index % 8 < 4 { "done" } else { "todo" };
        let requires = if index.is_multiple_of(8) {
            String::new()
        } else {
            Uuid::from_u128(index).to_string()
        };
        let deferral = if index % 16 == 7 {
            "  deferral: {kind: manual}\n"
        } else if index % 16 == 15 {
            "  deferral: {kind: until, until: '2099-01-01T00:00:00Z'}\n"
        } else {
            ""
        };
        format!("task:\n  state: {state}\n  requires: [{requires}]\n{deferral}")
    } else if index == 255 {
        let items = (1..=192)
            .rev()
            .map(|id| Uuid::from_u128(id).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("sheet:\n  ordering: preferred\n  items: [{items}]\n")
    } else {
        String::new()
    }
}

fn selected_notes(store: &Store) -> Result<NotePage> {
    let selected = store.list_notes(Query {
        scope: Some(Scope::Private),
        tags: vec!["papercut".into()],
        text: Some("synthetic".into()),
        limit: 1000,
        ..Query::default()
    })?;
    ensure!(selected.total == 128, "synthetic note filter failed");
    Ok(selected)
}

fn complete_review(store: &Store) -> Result<Review> {
    let review = store.review(Query {
        scope: Some(Scope::Private),
        ..Query::default()
    })?;
    ensure!(review.notes.len() == 256, "synthetic review was incomplete");
    Ok(review)
}

#[test]
fn synthetic_workload_completes() {
    main().unwrap();
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

#[test]
fn workload_rejects_incomplete_inputs_and_cannot_overwrite_a_seed_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::with_user_config(temp.path(), None).unwrap();
    assert!(selected_notes(&store).is_err());
    assert!(complete_review(&store).is_err());
    store.init(InitOptions::default()).unwrap();
    assert!(
        selected_notes(&store)
            .unwrap_err()
            .to_string()
            .contains("filter failed")
    );
    assert!(
        complete_review(&store)
            .unwrap_err()
            .to_string()
            .contains("incomplete")
    );
    let path = store
        .directory()
        .join(format!("private/notes/{}.md", Uuid::from_u128(1)));
    fs::create_dir(&path).unwrap();
    assert!(seed(&store).is_err());
    assert!(path.is_dir());
    fs::remove_dir(&path).unwrap();
    fs::write(store.directory().join("config.toml"), "invalid = [").unwrap();
    assert!(seed(&store).is_err());
}

#[test]
#[ignore = "child process with an invalid temporary directory"]
fn missing_temporary_directory_fixture() {
    assert!(main().is_err());
}

#[tokio::test]
async fn temporary_directory_failure_is_bounded_and_creates_no_store() {
    use support::CommandOutput;
    let temp = tempfile::tempdir().unwrap();
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "not a directory").unwrap();
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "missing_temporary_directory_fixture",
        ])
        .env("TMPDIR", &blocked)
        .env("TMP", &blocked)
        .env("TEMP", &blocked)
        .bounded_output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}
