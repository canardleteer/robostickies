//! Representative local workload for `cargo xtask profile`.
//! All input is synthetic and lives in an automatically removed temporary store.

use std::{fs, hint::black_box};

use anyhow::{Result, ensure};
use robostickies::{
    Store,
    model::{InitOptions, NotePage, Query, Review, Scope},
};
use uuid::Uuid;

fn main() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let store = Store::with_user_config(temporary.path(), None)?;
    seed(&store)?;
    for _ in 0..8 {
        let health = store.reindex()?;
        ensure!(
            health.healthy && health.notes == 256,
            "synthetic store indexing failed"
        );
        black_box((health, selected_notes(&store)?, complete_review(&store)?));
    }
    temporary.close()?;
    eprintln!(
        "Profile workload completed: 256 synthetic private notes, eight index/query/review passes."
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
                "---\nid: {id}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\ntags: [{tag}]\nstatus: active\n---\nSynthetic profiling note {index}: Markdown scanning, indexing, filtering, and review.\n"
            ),
        )?;
    }
    Ok(())
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
