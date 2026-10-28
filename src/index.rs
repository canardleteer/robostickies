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
const GENERATION: u64 = 2;

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
        {
            let table = tx.open_table(FILES)?;
            for row in table.iter()? {
                let (key, value) = row?;
                if let Ok(source) = serde_json::from_slice::<Source>(value.value()) {
                    old.insert(key.value().to_owned(), source);
                }
            }
        }
        let mut sources = BTreeMap::new();
        let mut diagnostics = vec![];
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
                is_tag,
                &old,
                &mut sources,
                &mut diagnostics,
            )?;
        }
        let changed = force
            || outdated
            || old.len() != sources.len()
            || sources.iter().any(|(key, source)| {
                old.get(key)
                    .is_none_or(|old| old.fingerprint != source.fingerprint)
            });
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
            let visibility = if copies.iter().any(|n| n.visibility == Visibility::Private) {
                Visibility::Private
            } else {
                Visibility::Public
            };
            diagnostics.push(Diagnostic {
                path: copies[0].path.clone(),
                code: "duplicate_id".into(),
                message: "Duplicate note ID; give manually copied notes a new UUID".into(),
                visibility,
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
                    tag.name.to_lowercase().contains(q) || tag.topic.to_lowercase().contains(q)
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

fn quarantine(root: &Path) -> std::io::Result<()> {
    let backup = root.join("cache/index.failed.redb");
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(root.join("cache/index.redb"), backup)
}

fn scan(
    root: &Path,
    directory: &str,
    visibility: Visibility,
    is_tag: bool,
    old: &BTreeMap<String, Source>,
    sources: &mut BTreeMap<String, Source>,
    diagnostics: &mut Vec<Diagnostic>,
) -> anyhow::Result<()> {
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
                let fingerprint = document::fingerprint(&text);
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
                    document::parse_tag(&text, &name).map(Entity::Tag)
                } else {
                    document::parse_note(&text, &path, visibility).map(Entity::Note)
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
