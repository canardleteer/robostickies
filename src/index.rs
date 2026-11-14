use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use redb::{
    Database, MultimapTableDefinition, ReadableDatabase, ReadableTable, ReadableTableMetadata,
    TableDefinition,
};
use serde::{Deserialize, Serialize};

use crate::{document, model::*};

const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files_v1");
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes_v1");
const TAGS: TableDefinition<&str, &[u8]> = TableDefinition::new("tags_v1");
const FILTERS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("filters_v1");
const DATES: TableDefinition<&str, &str> = TableDefinition::new("dates_v1");
const METADATA: TableDefinition<&str, u64> = TableDefinition::new("metadata_v1");
const GENERATION: u64 = 3;

#[derive(Clone, Serialize, Deserialize)]
enum Entity {
    Note(Note),
    Tag(Tag),
}

#[derive(Clone, Serialize, Deserialize)]
struct Source {
    fingerprint: String,
    entity: Option<Entity>,
    diagnostic: Option<Diagnostic>,
}

/// Open only while the caller holds the repository operation lock.
pub(crate) struct Index {
    db: Database,
    pub diagnostics: Vec<Diagnostic>,
}

impl Index {
    pub fn refresh(root: &Path, force: bool) -> anyhow::Result<Self> {
        match Self::refresh_once(root, force) {
            Err(error)
                if (error.is::<redb::TableError>() || error.is::<redb::StorageError>())
                    && root.join("cache/index.redb").is_file() =>
            {
                tracing::warn!(%error, "Rebuilding incompatible or damaged index tables");
                quarantine(root)?;
                Self::refresh_once(root, false)
            }
            result => result,
        }
    }

    fn refresh_once(root: &Path, force: bool) -> anyhow::Result<Self> {
        let path = root.join("cache/index.redb");
        if force && path.exists() {
            fs::remove_file(&path)?;
        }
        let db = match Database::create(&path) {
            Ok(db) => db,
            Err(error @ redb::DatabaseError::DatabaseAlreadyOpen) => return Err(error.into()),
            Err(error) => {
                // This is a disposable cache; preserve the failed file for diagnosis.
                if path.is_file() {
                    tracing::warn!(%error, "Rebuilding unreadable cache");
                    quarantine(root)?;
                    Database::create(&path)?
                } else {
                    return Err(error.into());
                }
            }
        };
        let tx = db.begin_write()?;
        let outdated = tx
            .open_table(METADATA)?
            .get("generation")?
            .map(|v| v.value())
            != Some(GENERATION);
        let mut old = BTreeMap::new();
        if !outdated {
            let table = tx.open_table(FILES)?;
            for row in table.iter()? {
                let (key, value) = row?;
                if let Ok(source) = serde_json::from_slice::<Source>(value.value()) {
                    old.insert(key.value().to_owned(), source);
                }
            }
        }
        let Catalog {
            sources,
            notes,
            tags,
            diagnostics,
        } = read_sources(root, &old, false)?;
        let changed = force
            || outdated
            || old.len() != sources.len()
            || sources.iter().any(|(key, source)| {
                old.get(key)
                    .is_none_or(|old| old.fingerprint != source.fingerprint)
            });
        // Open empty tables on first use; transactions make a refresh all-or-nothing.
        let missing = tx.open_table(NOTES)?.is_empty()? && !notes.is_empty();
        if changed || missing {
            tx.delete_table(FILES)?;
            tx.delete_table(NOTES)?;
            tx.delete_table(TAGS)?;
            tx.delete_table(DATES)?;
            tx.delete_multimap_table(FILTERS)?;
            {
                let mut table = tx.open_table(FILES)?;
                for (key, value) in &sources {
                    let encoded = serde_json::to_vec(value)?;
                    table.insert(key.as_str(), encoded.as_slice())?;
                }
            }
            {
                let mut note_table = tx.open_table(NOTES)?;
                let mut tag_table = tx.open_table(TAGS)?;
                let mut filters = tx.open_multimap_table(FILTERS)?;
                let mut dates = tx.open_table(DATES)?;
                for note in notes.values() {
                    let encoded = serde_json::to_vec(note)?;
                    note_table.insert(note.id.as_str(), encoded.as_slice())?;
                    for filter in [
                        format!("scope:{}", note.visibility),
                        format!("status:{}", note.status.as_str()),
                    ]
                    .into_iter()
                    .chain(note.tags.iter().map(|tag| format!("tag:{tag}")))
                    {
                        filters.insert(filter.as_str(), note.id.as_str())?;
                    }
                    dates.insert(document::date_key(note)?.as_str(), note.id.as_str())?;
                }
                for tag in tags.values() {
                    let encoded = serde_json::to_vec(tag)?;
                    tag_table.insert(tag.name.as_str(), encoded.as_slice())?;
                }
            }
        } else {
            tx.open_table(TAGS)?;
            tx.open_table(DATES)?;
            tx.open_multimap_table(FILTERS)?;
        }
        tx.open_table(METADATA)?.insert("generation", GENERATION)?;
        tx.commit()?;
        Ok(Self { db, diagnostics })
    }

    pub fn tags(&self, scope: Scope, text: Option<&str>) -> anyhow::Result<Vec<Tag>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(TAGS)?;
        let needle = text.map(str::to_lowercase);
        let mut tags = vec![];
        for entry in table.iter()? {
            let (_, data) = entry?;
            let tag: Tag = serde_json::from_slice(data.value())?;
            if scope.includes(tag.visibility)
                && needle.as_ref().is_none_or(|q| {
                    tag.name.to_lowercase().contains(q)
                        || tag
                            .description
                            .as_ref()
                            .is_some_and(|text| text.to_lowercase().contains(q))
                })
            {
                tags.push(tag);
            }
        }
        Ok(tags)
    }

    pub fn query(&self, query: &Query) -> anyhow::Result<NotePage> {
        let scope = query.scope.unwrap_or(Scope::All);
        if !(1..=1000).contains(&query.limit) {
            return Err(Error::new("invalid_input", "limit must be between 1 and 1000").into());
        }
        let since = query
            .since
            .as_deref()
            .map(document::date_bound)
            .transpose()?;
        let until = query
            .until
            .as_deref()
            .map(document::date_bound)
            .transpose()?;
        if since.zip(until).is_some_and(|(a, b)| a >= b) {
            return Err(Error::new(
                "invalid_input",
                "since must be before the exclusive until bound",
            )
            .into());
        }
        if let Some(cursor) = &query.cursor {
            let (date, id) = cursor
                .split_once('/')
                .ok_or_else(|| Error::new("invalid_input", "Invalid pagination cursor"))?;
            document::timestamp(date)?;
            document::note_id(id)?;
        }
        let tx = self.db.begin_read()?;
        let filters = tx.open_multimap_table(FILTERS)?;
        let select = |key: &str| -> anyhow::Result<BTreeSet<String>> {
            filters
                .get(key)?
                .map(|v| v.map(|v| v.value().to_owned()).map_err(Into::into))
                .collect()
        };
        let mut candidates: Option<BTreeSet<String>> = None;
        let mut required = vec![];
        if scope != Scope::All {
            required.push(format!(
                "scope:{}",
                if scope == Scope::Public {
                    "public"
                } else {
                    "private"
                }
            ));
        }
        if query.status != StatusFilter::All {
            required.push(format!(
                "status:{}",
                if query.status == StatusFilter::Active {
                    "active"
                } else {
                    "archived"
                }
            ));
        }
        for tag in &query.tags {
            document::tag_name(tag)?;
            required.push(format!("tag:{tag}"));
        }
        for filter in required {
            let matches = select(&filter)?;
            candidates = Some(match candidates {
                Some(previous) => previous.intersection(&matches).cloned().collect(),
                None => matches,
            });
        }
        let mut excluded = BTreeSet::new();
        for tag in &query.exclude_tags {
            document::tag_name(tag)?;
            excluded.extend(select(&format!("tag:{tag}"))?);
        }
        let notes = tx.open_table(NOTES)?;
        let dates = tx.open_table(DATES)?;
        let lower = since
            .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.9fZ").to_string())
            .unwrap_or_default();
        let upper = until
            .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.9fZ").to_string())
            .unwrap_or_else(|| "~".into());
        let needle = query.text.as_ref().map(|s| s.to_lowercase());
        let mut matches = vec![];
        let mut total = 0;
        for entry in dates.range(lower.as_str()..upper.as_str())?.rev() {
            let (key, id) = entry?;
            if candidates
                .as_ref()
                .is_some_and(|set| !set.contains(id.value()))
                || excluded.contains(id.value())
            {
                continue;
            }
            let Some(data) = notes.get(id.value())? else {
                continue;
            };
            let note: Note = serde_json::from_slice(data.value())?;
            if needle
                .as_ref()
                .is_some_and(|needle| !note.body.to_lowercase().contains(needle))
            {
                continue;
            }
            total += 1;
            if query
                .cursor
                .as_ref()
                .is_some_and(|cursor| key.value() >= cursor.as_str())
            {
                continue;
            }
            if matches.len() <= query.limit {
                matches.push(note);
            }
        }
        let next_cursor = if matches.len() > query.limit {
            matches.truncate(query.limit);
            matches.last().map(document::date_key).transpose()?
        } else {
            None
        };
        Ok(NotePage {
            notes: matches,
            total,
            next_cursor,
            diagnostics: self
                .diagnostics
                .iter()
                .filter(|d| scope.includes(d.visibility))
                .cloned()
                .collect(),
        })
    }
}

struct Catalog {
    sources: BTreeMap<String, Source>,
    notes: BTreeMap<String, Note>,
    tags: BTreeMap<String, Tag>,
    diagnostics: Vec<Diagnostic>,
}

/// Validate authoritative sources without opening or repairing the disposable cache.
/// Caller must already hold the operation lock.
pub(crate) fn validate_sources(root: &Path) -> crate::Result<()> {
    validated_sources(root, false).map(|_| ())
}

pub(crate) fn validated_sources(
    root: &Path,
    for_upgrade: bool,
) -> crate::Result<(Vec<Note>, Vec<Tag>)> {
    let catalog = read_sources(root, &BTreeMap::new(), for_upgrade)
        .map_err(|error| Error::new("io", error.to_string()))?;
    if let Some(diagnostic) = catalog.diagnostics.first() {
        return Err(Error::new(
            "invalid_document",
            format!(
                "Authoritative source validation failed at {}: {}; run robostickies doctor for local diagnostics",
                diagnostic.path, diagnostic.message
            ),
        ));
    }
    Ok((
        catalog.notes.into_values().collect(),
        catalog.tags.into_values().collect(),
    ))
}

fn read_sources(
    root: &Path,
    old: &BTreeMap<String, Source>,
    for_upgrade: bool,
) -> anyhow::Result<Catalog> {
    let mut sources = BTreeMap::new();
    let mut diagnostics = vec![];
    let version = crate::config::load_shared(root)?.data_version;
    for (directory, visibility, is_tag) in [
        ("tags", Visibility::Public, true),
        ("private/tags", Visibility::Private, true),
        ("notes", Visibility::Public, false),
        ("private/notes", Visibility::Private, false),
    ] {
        scan(
            root,
            directory,
            visibility,
            SourceKind {
                is_tag,
                version,
                for_upgrade,
            },
            old,
            &mut sources,
            &mut diagnostics,
        )?;
    }
    let mut groups: BTreeMap<String, Vec<Note>> = BTreeMap::new();
    let mut tags = BTreeMap::new();
    for source in sources.values() {
        if let Some(diagnostic) = &source.diagnostic {
            diagnostics.push(diagnostic.clone());
        }
        match &source.entity {
            Some(Entity::Note(note)) => {
                groups
                    .entry(note.id.clone())
                    .or_default()
                    .push(note.clone());
            }
            Some(Entity::Tag(tag)) => {
                tags.insert(tag.name.clone(), tag.clone());
            }
            None => {}
        }
    }
    let mut notes = BTreeMap::new();
    for (id, copies) in groups {
        if copies.len() == 1 {
            notes.insert(id, copies[0].clone());
            continue;
        }
        // A local conflict must not change public selection or expose private details.
        let public: Vec<_> = copies
            .iter()
            .filter(|note| note.visibility == Visibility::Public)
            .collect();
        diagnostics.push(Diagnostic {
            path: copies[0].path.clone(),
            code: "duplicate_id".into(),
            message: "Duplicate note ID; give manually copied notes a new UUID".into(),
            // Canonical filenames allow at most one file per namespace.
            // A duplicate therefore always involves private storage.
            visibility: Visibility::Private,
        });
        if public.len() == 1 {
            notes.insert(id, public[0].clone());
        }
    }
    for note in notes.values() {
        for tag in &note.tags {
            if !tags.contains_key(tag) {
                diagnostics.push(Diagnostic {
                    path: note.path.clone(),
                    code: "unknown_tag".into(),
                    message: format!(
                        "Undefined tag {tag:?}; create its definition or update this note"
                    ),
                    visibility: note.visibility,
                });
            }
        }
        if note.visibility == Visibility::Private && !note.path.starts_with("private/") {
            diagnostics.push(Diagnostic { path: note.path.clone(), code: "misplaced_private_note".into(),
                message: "This note has a private tag but is outside private/; move it into private/notes/".into(), visibility: Visibility::Private });
        }
    }
    Ok(Catalog {
        sources,
        notes,
        tags,
        diagnostics,
    })
}

fn quarantine(root: &Path) -> std::io::Result<()> {
    let backup = root.join("cache/index.failed.redb");
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(root.join("cache/index.redb"), backup)
}

#[derive(Clone, Copy)]
struct SourceKind {
    is_tag: bool,
    version: crate::version::DataVersion,
    for_upgrade: bool,
}

fn scan(
    root: &Path,
    directory: &str,
    visibility: Visibility,
    kind: SourceKind,
    old: &BTreeMap<String, Source>,
    sources: &mut BTreeMap<String, Source>,
    diagnostics: &mut Vec<Diagnostic>,
) -> anyhow::Result<()> {
    let SourceKind {
        is_tag,
        version,
        for_upgrade,
    } = kind;
    if let Err(error) = document::safe_path(root, Path::new(directory)) {
        diagnostics.push(Diagnostic {
            path: directory.into(),
            code: error.code,
            message: error.message,
            visibility,
        });
        return Ok(());
    }
    let entries = match fs::read_dir(root.join(directory)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            diagnostics.push(Diagnostic {
                path: directory.into(),
                code: "io".into(),
                message: e.to_string(),
                visibility,
            });
            return Ok(());
        }
    };
    for entry in entries {
        let entry = entry?;
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            continue;
        };
        if !filename.ends_with(".md") {
            continue;
        }
        let path = format!("{directory}/{filename}");
        let text = if entry.file_type()?.is_file() {
            fs::read_to_string(entry.path())
        } else {
            Err(std::io::Error::other(
                "Only regular Markdown files are indexed; symlinks are not followed",
            ))
        };
        let source = match text {
            Ok(text) => {
                // A version change must revalidate cached source interpretations.
                let fingerprint = format!("{version}:{}", document::fingerprint(&text));
                if let Some(cached) = old
                    .get(&path)
                    .filter(|cached| cached.fingerprint == fingerprint)
                {
                    sources.insert(path, cached.clone());
                    continue;
                }
                let parsed = if is_tag {
                    let name = format!(
                        "{}{}",
                        if visibility == Visibility::Private {
                            "private:"
                        } else {
                            ""
                        },
                        filename.trim_end_matches(".md")
                    );
                    let parsed = document::parse_tag_for_version(&text, &name, version);
                    // Only migration preparation accepts already-renamed sources
                    // while the shared marker still names the old representation.
                    let parsed = if for_upgrade && !version.optional_tag_descriptions() {
                        parsed.or_else(|_| {
                            document::parse_tag_for_version(&text, &name, crate::version::CURRENT)
                        })
                    } else {
                        parsed
                    };
                    parsed.map(Entity::Tag)
                } else {
                    document::parse_note_for_version(&text, &path, visibility, version)
                        .map(Entity::Note)
                };
                match parsed {
                    Ok(entity) => Source {
                        fingerprint,
                        entity: Some(entity),
                        diagnostic: None,
                    },
                    Err(error) => Source {
                        fingerprint,
                        entity: None,
                        diagnostic: Some(Diagnostic {
                            path: path.clone(),
                            code: "invalid_document".into(),
                            message: error.to_string(),
                            visibility: if visibility == Visibility::Private
                                || text.contains("private:")
                            {
                                Visibility::Private
                            } else {
                                Visibility::Public
                            },
                        }),
                    },
                }
            }
            Err(error) => Source {
                fingerprint: format!("unreadable:{error}"),
                entity: None,
                diagnostic: Some(Diagnostic {
                    path: path.clone(),
                    code: "io".into(),
                    message: error.to_string(),
                    visibility,
                }),
            },
        };
        sources.insert(path, source);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn locked_damaged_and_unreadable_cache_paths_recover_without_source_edits() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        let note = store
            .create_note(CreateNote {
                body_format: None,
                body: "Cache recovery fixture".into(),
                tags: vec![],
                private: Some(true),
            })
            .unwrap()
            .note;
        let source = fs::read(store.directory().join(&note.path)).unwrap();
        let index = store.directory().join("cache/index.redb");
        let held = Database::open(&index).unwrap();
        assert!(Index::refresh(&store.directory(), false).is_err());
        drop(held);
        // A malformed derived row must not be trusted as a source snapshot.
        let db = Database::open(&index).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(FILES)
            .unwrap()
            .insert(note.path.as_str(), b"invalid".as_slice())
            .unwrap();
        tx.commit().unwrap();
        drop(db);
        assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
        for _ in 0..2 {
            fs::write(&index, "invalid redb").unwrap();
            assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
        }
        let failed = store.directory().join("cache/index.failed.redb");
        fs::remove_file(&failed).unwrap();
        fs::create_dir(&failed).unwrap();
        fs::write(&index, "invalid redb").unwrap();
        assert!(Index::refresh(&store.directory(), false).is_err());
        fs::remove_dir(&failed).unwrap();
        assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
        assert_eq!(
            fs::read(store.directory().join(&note.path)).unwrap(),
            source
        );
    }

    #[test]
    fn scans_report_filesystem_failures_and_missing_directories_without_private_leaks() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        fs::remove_dir(store.directory().join("notes")).unwrap();
        assert_eq!(store.list_notes(Query::default()).unwrap().total, 0);
        fs::write(store.directory().join("notes"), "not a directory").unwrap();
        let page = store
            .list_notes(Query {
                scope: Some(Scope::Public),
                ..Query::default()
            })
            .unwrap();
        assert_eq!(page.diagnostics[0].path, "notes");
        fs::remove_file(store.directory().join("notes")).unwrap();
        fs::create_dir(store.directory().join("notes")).unwrap();
        fs::write(store.directory().join("notes/ignored.txt"), "ignored").unwrap();
        fs::write(store.directory().join("private/notes/binary.md"), [0xff]).unwrap();
        fs::write(
            store.directory().join("notes/broken.md"),
            "---\ninvalid: [\n---\nBody",
        )
        .unwrap();
        fs::create_dir(store.directory().join("tags/directory.md")).unwrap();
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStringExt;
            fs::write(
                store
                    .directory()
                    .join("notes")
                    .join(std::ffi::OsString::from_vec(b"\xff.md".to_vec())),
                "invalid name",
            )
            .unwrap();
        }
        let public = store
            .list_notes(Query {
                scope: Some(Scope::Public),
                ..Query::default()
            })
            .unwrap();
        assert!(
            public
                .diagnostics
                .iter()
                .all(|d| d.visibility == Visibility::Public)
        );
        assert!(
            public
                .diagnostics
                .iter()
                .any(|d| d.path == "notes/broken.md")
        );
        assert!(!public.diagnostics.iter().any(|d| d.path.contains("binary")));
        assert!(
            store
                .list_notes(Query::default())
                .unwrap()
                .diagnostics
                .iter()
                .any(|d| d.path.contains("binary"))
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::Store;

    #[test]
    fn incompatible_table_types_are_rebuilt_and_invalid_queries_fail_consistently() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        let note = store
            .create_note(CreateNote {
                body_format: None,
                body: "Table recovery fixture".into(),
                tags: vec!["papercut".into()],
                private: Some(true),
            })
            .unwrap()
            .note;
        let path = store.directory().join("cache/index.redb");
        let source = fs::read(store.directory().join(&note.path)).unwrap();
        for name in [
            "metadata_v1",
            "files_v1",
            "notes_v1",
            "tags_v1",
            "dates_v1",
            "filters_v1",
        ] {
            let db = Database::open(&path).unwrap();
            let tx = db.begin_write().unwrap();
            if name == "filters_v1" {
                tx.delete_multimap_table(FILTERS).unwrap();
            } else {
                match name {
                    "metadata_v1" => {
                        tx.delete_table(METADATA).unwrap();
                    }
                    "files_v1" => {
                        tx.delete_table(FILES).unwrap();
                    }
                    "notes_v1" => {
                        tx.delete_table(NOTES).unwrap();
                    }
                    "tags_v1" => {
                        tx.delete_table(TAGS).unwrap();
                    }
                    _ => {
                        assert_eq!(name, "dates_v1");
                        tx.delete_table(DATES).unwrap();
                    }
                }
            }
            tx.open_table(TableDefinition::<u64, u64>::new(name))
                .unwrap()
                .insert(1, 1)
                .unwrap();
            tx.commit().unwrap();
            drop(db);
            assert_eq!(store.list_notes(Query::default()).unwrap().total, 1);
            assert_eq!(
                fs::read(store.directory().join(&note.path)).unwrap(),
                source
            );
            assert!(store.doctor().unwrap().healthy);
        }
        for query in [
            Query {
                since: Some("invalid".into()),
                ..Query::default()
            },
            Query {
                until: Some("invalid".into()),
                ..Query::default()
            },
            Query {
                cursor: Some("2026-01-01T00:00:00Z/invalid".into()),
                ..Query::default()
            },
            Query {
                cursor: Some("invalid/00000000-0000-0000-0000-000000000001".into()),
                ..Query::default()
            },
            Query {
                tags: vec!["Invalid".into()],
                ..Query::default()
            },
            Query {
                exclude_tags: vec!["Invalid".into()],
                ..Query::default()
            },
            Query {
                limit: 0,
                ..Query::default()
            },
        ] {
            assert_eq!(store.list_notes(query).unwrap_err().code, "invalid_input");
        }
        // A removed definition must produce a diagnostic in the note's visibility.
        fs::remove_file(store.directory().join("tags/papercut.md")).unwrap();
        let page = store.list_notes(Query::default()).unwrap();
        assert!(page.diagnostics.iter().any(|d| d.code == "unknown_tag"));
        assert_eq!(page.diagnostics[0].visibility, Visibility::Private);
        assert!(
            store
                .list_notes(Query {
                    scope: Some(Scope::Public),
                    ..Query::default()
                })
                .unwrap()
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn damaged_derived_json_and_dangling_dates_require_cache_repair_without_source_changes() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        let note = store
            .create_note(CreateNote {
                body_format: None,
                body: "Derived rows fixture".into(),
                tags: vec![],
                private: Some(true),
            })
            .unwrap()
            .note;
        let path = store.directory().join("cache/index.redb");
        let source = fs::read(store.directory().join(&note.path)).unwrap();
        for table in ["notes_v1", "tags_v1"] {
            let db = Database::open(&path).unwrap();
            let tx = db.begin_write().unwrap();
            let definition = TableDefinition::<&str, &[u8]>::new(table);
            let key = if table == "notes_v1" {
                note.id.as_str()
            } else {
                "papercut"
            };
            tx.open_table(definition)
                .unwrap()
                .insert(key, b"invalid json".as_slice())
                .unwrap();
            tx.commit().unwrap();
            drop(db);
            if table == "notes_v1" {
                assert_eq!(
                    store.list_notes(Query::default()).unwrap_err().code,
                    "index"
                );
            } else {
                assert_eq!(store.list_tags(Scope::All, None).unwrap_err().code, "index");
            }
            assert_eq!(store.doctor().unwrap_err().code, "index");
            store.reindex().unwrap();
            assert_eq!(
                fs::read(store.directory().join(&note.path)).unwrap(),
                source
            );
        }
        let db = Database::open(&path).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(DATES)
            .unwrap()
            .insert(
                "2026-01-01T00:00:00.000000000Z/00000000-0000-0000-0000-000000000001",
                "00000000-0000-0000-0000-000000000001",
            )
            .unwrap();
        tx.commit().unwrap();
        drop(db);
        assert_eq!(
            store
                .list_notes(Query {
                    status: StatusFilter::All,
                    ..Query::default()
                })
                .unwrap()
                .total,
            1
        );
        assert!(store.reindex().unwrap().healthy);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_source_directories_are_diagnosed_without_following_them() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        fs::rename(
            store.directory().join("tags"),
            store.directory().join("saved-tags"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), store.directory().join("tags")).unwrap();
        let health = store.doctor().unwrap();
        assert!(!health.healthy);
        assert!(
            health
                .diagnostics
                .iter()
                .any(|d| d.path == "tags" && d.code == "io")
        );
        assert!(
            store
                .list_tags(Scope::Public, None)
                .unwrap()
                .tags
                .is_empty()
        );
    }
}
