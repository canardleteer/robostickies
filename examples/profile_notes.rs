//! Representative local workload for `cargo xtask profile`.
//! All input is synthetic and lives in an automatically removed temporary store.

use std::{fs, hint::black_box};

use anyhow::{Result, ensure};
use robostickies::{
    Store,
    model::{InitOptions, Query, Scope},
};
use uuid::Uuid;

fn main() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let store = Store::with_user_config(temporary.path(), None)?;
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
    for _ in 0..8 {
        let health = store.reindex()?;
        ensure!(
            health.healthy && health.notes == 256,
            "synthetic store indexing failed"
        );
        let selected = store.list_notes(Query {
            scope: Some(Scope::Private),
            tags: vec!["papercut".into()],
            text: Some("synthetic".into()),
            limit: 1000,
            ..Query::default()
        })?;
        ensure!(selected.total == 128, "synthetic note filter failed");
        let review = store.review(Query {
            scope: Some(Scope::Private),
            ..Query::default()
        })?;
        ensure!(review.notes.len() == 256, "synthetic review was incomplete");
        black_box((health, selected, review));
    }
    temporary.close()?;
    eprintln!(
        "Profile workload completed: 256 synthetic private notes, eight index/query/review passes."
    );
    Ok(())
}
