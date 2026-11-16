use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use redb::{
    Database, MultimapTableDefinition, ReadableDatabase, ReadableMultimapTable, ReadableTable,
    ReadableTableMetadata, TableDefinition,
};
use serde::{Deserialize, Serialize};

use crate::{
    document,
    graph::{self, Evaluator, Graph},
    model::*,
};

const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files_v1");
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes_v1");
const TAGS: TableDefinition<&str, &[u8]> = TableDefinition::new("tags_v1");
const FILTERS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("filters_v1");
const DATES: TableDefinition<&str, &str> = TableDefinition::new("dates_v1");
const METADATA: TableDefinition<&str, u64> = TableDefinition::new("metadata_v1");
const GENERATION: u64 = 4;
const TASKS: TableDefinition<&str, &[u8]> = TableDefinition::new("tasks_v1");
const SHEETS: TableDefinition<&str, &[u8]> = TableDefinition::new("sheets_v1");
const IDENTITIES: TableDefinition<&str, &[u8]> = TableDefinition::new("identities_v1");
const TASK_FILTERS: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("task_filters_v1");
const REQUIRES: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("requires_v1");
const REQUIRED_BY: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("required_by_v1");
const MEMBER_OF: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("member_of_v1");
const DEFERRALS: TableDefinition<&str, &str> = TableDefinition::new("deferrals_v1");

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
    /// Load graph records without reading note bodies.
    pub fn graph(&self) -> anyhow::Result<Graph> {
        let tx = self.db.begin_read()?;
        let tasks = tx.open_table(TASKS)?;
        let sheets = tx.open_table(SHEETS)?;
        let identities = tx.open_table(IDENTITIES)?;
        let mut graph = Graph::default();
        for row in tasks.iter()? {
            let (key, value) = row?;
            graph
                .tasks
                .insert(key.value().into(), serde_json::from_slice(value.value())?);
        }
        for row in sheets.iter()? {
            let (key, value) = row?;
            graph
                .sheets
                .insert(key.value().into(), serde_json::from_slice(value.value())?);
        }
        for row in identities.iter()? {
            let (key, value) = row?;
            graph
                .identities
                .insert(key.value().into(), serde_json::from_slice(value.value())?);
        }
        for (definition, incoming) in [
            (REQUIRED_BY, &mut graph.incoming_tasks),
            (MEMBER_OF, &mut graph.incoming_sheets),
        ] {
            for row in tx.open_multimap_table(definition)?.iter()? {
                let (key, values) = row?;
                let sources = values
                    .map(|value| value.map(|value| value.value().to_owned()))
                    .collect::<std::result::Result<BTreeSet<_>, _>>()?;
                incoming.insert(key.value().into(), sources);
            }
        }
        Ok(graph)
    }

    fn note(&self, id: &str) -> anyhow::Result<Option<Note>> {
        let tx = self.db.begin_read()?;
        tx.open_table(NOTES)?
            .get(id)?
            .map(|value| serde_json::from_slice(value.value()).map_err(Into::into))
            .transpose()
    }

    pub fn list_tasks(
        &self,
        input: &TaskQuery,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<TaskPage> {
        let query = &input.notes;
        validate_query(query)?;
        if input.all_task_states && !input.states.is_empty() {
            return Err(Error::new("invalid_input", "states and all_task_states conflict").into());
        }
        let graph = self.graph()?;
        let scope = query.scope.unwrap_or(Scope::All);
        let tx = self.db.begin_read()?;
        let filters = tx.open_multimap_table(TASK_FILTERS)?;
        let select = |key: &str| -> anyhow::Result<BTreeSet<String>> {
            filters
                .get(key)?
                .map(|row| row.map(|v| v.value().to_owned()).map_err(Into::into))
                .collect()
        };
        let states = if input.all_task_states {
            vec![
                TaskState::Todo,
                TaskState::InProgress,
                TaskState::Done,
                TaskState::Cancelled,
            ]
        } else if input.states.is_empty() {
            vec![TaskState::Todo, TaskState::InProgress]
        } else {
            input.states.clone()
        };
        let mut candidates = BTreeSet::new();
        for state in states {
            candidates.extend(select(&format!("state:{}", state.as_str()))?);
        }
        let mut required = query
            .tags
            .iter()
            .map(|t| format!("tag:{t}"))
            .collect::<Vec<_>>();
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
        for filter in required {
            candidates = candidates
                .intersection(&select(&filter)?)
                .cloned()
                .collect();
        }
        for tag in &query.exclude_tags {
            candidates = candidates
                .difference(&select(&format!("tag:{tag}"))?)
                .cloned()
                .collect();
        }
        if let Some(deferred) = input.deferred {
            let mut waiting = select("deferral:manual")?;
            let lower = format!("{}/~", at.format("%Y-%m-%dT%H:%M:%S%.9fZ"));
            for row in tx.open_table(DEFERRALS)?.range(lower.as_str()..)? {
                waiting.insert(row?.1.value().into());
            }
            candidates = if deferred {
                candidates.intersection(&waiting).cloned().collect()
            } else {
                candidates.difference(&waiting).cloned().collect()
            };
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
        let needle = query.text.as_ref().map(|s| s.to_lowercase());
        let mut evaluator = Evaluator::new(&graph, at);
        let mut matched = Vec::new();
        for identity in candidates {
            let Some(record) = graph.tasks.get(&identity) else {
                continue;
            };
            let public = scope == Scope::Public || record.visibility == Visibility::Public;
            if scope != Scope::Public
                && record.visibility == Visibility::Private
                && graph
                    .identities
                    .get(&record.id)
                    .is_some_and(|ids| ids.len() != 1)
            {
                continue;
            }
            let created = document::timestamp(&record.created_at)?;
            if since.is_some_and(|bound| created < bound)
                || until.is_some_and(|bound| created >= bound)
            {
                continue;
            }
            let summary = evaluator.summary(&record.id, public);
            let deferred = record.task.deferral.as_ref().is_some_and(|d| match d {
                Deferral::Manual => true,
                Deferral::Until { until } => {
                    document::timestamp(until).is_ok_and(|until| at < until)
                }
            });
            let blocked = !summary.blockers.is_empty();
            let ready = record.status == NoteStatus::Active
                && !record.task.state.terminal()
                && !deferred
                && !blocked;
            if input.ready.is_some_and(|v| v != ready)
                || input.blocked.is_some_and(|v| v != blocked)
                || input.deferred.is_some_and(|v| v != deferred)
            {
                continue;
            }
            // Bodies are only needed for literal text matching and the returned page.
            if let Some(needle) = &needle
                && self
                    .note(&record.id)?
                    .is_none_or(|note| !note.body.to_lowercase().contains(needle))
            {
                continue;
            }
            matched.push((
                format!("{}/{}", created.format("%Y-%m-%dT%H:%M:%S%.9fZ"), record.id),
                record,
            ));
        }
        matched.sort_by(|a, b| b.0.cmp(&a.0));
        let total = matched.len();
        let mut page = matched
            .into_iter()
            .filter(|(key, _)| query.cursor.as_ref().is_none_or(|cursor| key < cursor))
            .take(query.limit + 1)
            .collect::<Vec<_>>();
        let next_cursor = if page.len() > query.limit {
            page.truncate(query.limit);
            page.last().map(|(key, _)| key.clone())
        } else {
            None
        };
        let mut tasks = Vec::new();
        for (_, record) in page {
            if let Some(note) = self.note(&record.id)? {
                tasks.push(evaluator.view(
                    note,
                    scope == Scope::Public || record.visibility == Visibility::Public,
                )?);
            }
        }
        Ok(TaskPage {
            tasks,
            total,
            next_cursor,
            diagnostics: self
                .diagnostics
                .iter()
                .filter(|d| scope.includes(d.visibility))
                .cloned()
                .collect(),
            evaluated_at: at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        })
    }

    pub fn render_task_sheet(
        &self,
        id: &str,
        scope: Scope,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<TaskSheet> {
        let graph = self.graph()?;
        if scope != Scope::Public && graph.identities.get(id).is_some_and(|ids| ids.len() > 1) {
            return Err(
                Error::new("conflict", "Multiple files have this note ID; run doctor").into(),
            );
        }
        let note = self
            .note(id)?
            .filter(|note| scope.includes(note.visibility))
            .ok_or_else(|| Error::new("not_found", "Sheet not found in the requested scope"))?;
        let spec = graph
            .sheets
            .get(&graph::stored_key(&note.path, id))
            .map(|record| record.sheet.clone())
            .ok_or_else(|| Error::new("invalid_input", "The selected note is not a sheet"))?;
        let public = note.visibility == Visibility::Public;
        let mut evaluator = Evaluator::new(&graph, at);
        let mut ancestors = BTreeSet::new();
        let mut rows = Vec::new();
        let mut diagnostics = Vec::new();
        for id in &spec.items {
            let row = self.sheet_row(id, scope, public, &graph, &mut evaluator)?;
            if row.task.is_some() {
                ancestors.extend(evaluator.summary(id, public).ancestors);
            }
            rows.push(row);
        }
        let mut context = Vec::new();
        for id in ancestors
            .iter()
            .filter(|id| !rows.iter().any(|row| row.id == **id && row.task.is_some()))
        {
            // Local prerequisite evidence may be public even in a private-only row view.
            context.push(self.sheet_row(
                id,
                if public { Scope::Public } else { Scope::All },
                public,
                &graph,
                &mut evaluator,
            )?);
        }
        let mut relevant = BTreeSet::from([note.path.clone()]);
        let mut unavailable_members = BTreeSet::new();
        for row in rows.iter().chain(&context) {
            if let Some(task) = &row.task {
                relevant.insert(task.note.path.clone());
            }
            if let Some(reason) = &row.unavailable
                && unavailable_members.insert(row.id.as_str())
            {
                diagnostics.push(Diagnostic {
                    path: note.path.clone(),
                    code: reason.clone(),
                    message: format!("Sheet item {} is unavailable ({reason})", row.id),
                    visibility: note.visibility,
                });
            }
        }
        // Scoped rows already diagnose unavailable members, including members
        // also present in prerequisite context. Keep other source/graph errors
        // without repeating the cached sheet-level resolution diagnostics.
        diagnostics.extend(
            self.diagnostics
                .iter()
                .filter(|d| {
                    relevant.contains(&d.path)
                        && (scope != Scope::Public || d.visibility == Visibility::Public)
                        && !(d.path == note.path && d.code == "unresolved_reference")
                })
                .cloned(),
        );
        let complete =
            diagnostics.is_empty() && rows.iter().chain(&context).all(|r| r.task.is_some());
        let next_task = (complete && spec.ordering == SheetOrdering::Preferred)
            .then(|| {
                rows.iter()
                    .find(|r| r.task.as_ref().is_some_and(|t| t.ready))
                    .map(|r| r.id.clone())
            })
            .flatten();
        let mut result = TaskSheet {
            note,
            sheet: spec,
            scope,
            evaluated_at: at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            rows,
            context,
            diagnostics,
            complete,
            next_task,
            markdown: String::new(),
        };
        result.markdown = render_sheet(&result);
        Ok(result)
    }

    fn sheet_row(
        &self,
        id: &str,
        scope: Scope,
        public: bool,
        graph: &Graph,
        evaluator: &mut Evaluator<'_>,
    ) -> anyhow::Result<SheetRow> {
        let record = graph.lookup(id, public);
        let reason = match record {
            None => Some("unresolved_reference"),
            Some(record) if !scope.includes(record.visibility) => Some("outside_scope"),
            _ => None,
        };
        let task = if reason.is_none() {
            self.note(id)?
                .map(|note| {
                    let public = public || note.visibility == Visibility::Public;
                    evaluator.view(note, public)
                })
                .transpose()?
        } else {
            None
        };
        Ok(SheetRow {
            id: id.into(),
            task,
            unavailable: reason.map(str::to_owned),
        })
    }

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
            graph,
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
        let missing_graph = (tx.open_table(TASKS)?.is_empty()? && !graph.tasks.is_empty())
            || (tx.open_table(SHEETS)?.is_empty()? && !graph.sheets.is_empty())
            || !cached_records_match(&tx, TASKS, &graph.tasks)?
            || !cached_records_match(&tx, SHEETS, &graph.sheets)?
            || !cached_records_match(&tx, IDENTITIES, &graph.identities)?
            || !cached_relationships_match(&tx, &graph)?;
        if changed || missing || missing_graph {
            tx.delete_table(FILES)?;
            tx.delete_table(NOTES)?;
            tx.delete_table(TAGS)?;
            tx.delete_table(DATES)?;
            tx.delete_multimap_table(FILTERS)?;
            tx.delete_table(TASKS)?;
            tx.delete_table(SHEETS)?;
            tx.delete_table(IDENTITIES)?;
            tx.delete_table(DEFERRALS)?;
            tx.delete_multimap_table(TASK_FILTERS)?;
            tx.delete_multimap_table(REQUIRES)?;
            tx.delete_multimap_table(REQUIRED_BY)?;
            tx.delete_multimap_table(MEMBER_OF)?;
            {
                let mut tasks = tx.open_table(TASKS)?;
                let mut sheets = tx.open_table(SHEETS)?;
                let mut identities = tx.open_table(IDENTITIES)?;
                let mut filters = tx.open_multimap_table(TASK_FILTERS)?;
                let mut requires = tx.open_multimap_table(REQUIRES)?;
                let mut required_by = tx.open_multimap_table(REQUIRED_BY)?;
                let mut member_of = tx.open_multimap_table(MEMBER_OF)?;
                let mut deferrals = tx.open_table(DEFERRALS)?;
                for (identity, record) in &graph.tasks {
                    tasks.insert(identity.as_str(), serde_json::to_vec(record)?.as_slice())?;
                    for filter in [
                        format!("state:{}", record.task.state.as_str()),
                        format!("scope:{}", record.visibility),
                        format!("status:{}", record.status.as_str()),
                    ]
                    .into_iter()
                    .chain(record.tags.iter().map(|t| format!("tag:{t}")))
                    {
                        filters.insert(filter.as_str(), identity.as_str())?;
                    }
                    for target in &record.task.requires {
                        let target = graph.reference_key(target, record.visibility);
                        requires.insert(identity.as_str(), target.as_str())?;
                    }
                    if record.task.deferral == Some(Deferral::Manual) {
                        filters.insert("deferral:manual", identity.as_str())?;
                    }
                    if let Some(Deferral::Until { until }) = &record.task.deferral {
                        let date = document::timestamp(until)?.format("%Y-%m-%dT%H:%M:%S%.9fZ");
                        deferrals
                            .insert(format!("{date}/{identity}").as_str(), identity.as_str())?;
                    }
                }
                for (identity, record) in &graph.sheets {
                    sheets.insert(identity.as_str(), serde_json::to_vec(record)?.as_slice())?;
                }
                for (target, sources) in &graph.incoming_tasks {
                    for source in sources {
                        required_by.insert(target.as_str(), source.as_str())?;
                    }
                }
                for (target, sources) in &graph.incoming_sheets {
                    for source in sources {
                        member_of.insert(target.as_str(), source.as_str())?;
                    }
                }
                for (id, entries) in &graph.identities {
                    identities.insert(id.as_str(), serde_json::to_vec(entries)?.as_slice())?;
                }
            }
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
            tx.open_table(TASKS)?;
            tx.open_table(SHEETS)?;
            tx.open_table(IDENTITIES)?;
            tx.open_table(DEFERRALS)?;
            tx.open_multimap_table(TASK_FILTERS)?;
            tx.open_multimap_table(REQUIRES)?;
            tx.open_multimap_table(REQUIRED_BY)?;
            tx.open_multimap_table(MEMBER_OF)?;
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
    graph: Graph,
}

fn cached_records_match<T: Serialize>(
    tx: &redb::WriteTransaction,
    definition: TableDefinition<&str, &[u8]>,
    expected: &BTreeMap<String, T>,
) -> anyhow::Result<bool> {
    let table = tx.open_table(definition)?;
    if table.len()? != expected.len() as u64 {
        return Ok(false);
    }
    for (key, value) in expected {
        let encoded = serde_json::to_vec(value)?;
        if table
            .get(key.as_str())?
            .is_none_or(|row| row.value() != encoded)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn cached_relationships_match(tx: &redb::WriteTransaction, graph: &Graph) -> anyhow::Result<bool> {
    let mut filters: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut requires: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut deferrals = BTreeMap::new();
    for (identity, record) in &graph.tasks {
        for filter in [
            format!("state:{}", record.task.state.as_str()),
            format!("scope:{}", record.visibility),
            format!("status:{}", record.status.as_str()),
        ]
        .into_iter()
        .chain(record.tags.iter().map(|tag| format!("tag:{tag}")))
        {
            filters.entry(filter).or_default().insert(identity.clone());
        }
        for target in &record.task.requires {
            requires
                .entry(identity.clone())
                .or_default()
                .insert(graph.reference_key(target, record.visibility));
        }
        match &record.task.deferral {
            Some(Deferral::Manual) => {
                filters
                    .entry("deferral:manual".into())
                    .or_default()
                    .insert(identity.clone());
            }
            Some(Deferral::Until { until }) => {
                let date = document::timestamp(until)?.format("%Y-%m-%dT%H:%M:%S%.9fZ");
                deferrals.insert(format!("{date}/{identity}"), identity.clone());
            }
            None => {}
        }
    }
    for (definition, expected) in [
        (TASK_FILTERS, &filters),
        (REQUIRES, &requires),
        (REQUIRED_BY, &graph.incoming_tasks),
        (MEMBER_OF, &graph.incoming_sheets),
    ] {
        let table = tx.open_multimap_table(definition)?;
        let mut actual = BTreeMap::new();
        for row in table.iter()? {
            let (key, values) = row?;
            actual.insert(
                key.value().to_owned(),
                values
                    .map(|v| v.map(|v| v.value().to_owned()))
                    .collect::<std::result::Result<BTreeSet<_>, _>>()?,
            );
        }
        if actual != *expected {
            return Ok(false);
        }
    }
    let table = tx.open_table(DEFERRALS)?;
    let actual = table
        .iter()?
        .map(|row| row.map(|(k, v)| (k.value().to_owned(), v.value().to_owned())))
        .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
    Ok(actual == deferrals)
}

fn validate_query(query: &Query) -> crate::Result<()> {
    if !(1..=1000).contains(&query.limit) {
        return Err(Error::new(
            "invalid_input",
            "limit must be between 1 and 1000",
        ));
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
        ));
    }
    for tag in query.tags.iter().chain(&query.exclude_tags) {
        document::tag_name(tag)?;
    }
    if let Some(cursor) = &query.cursor {
        let (date, id) = cursor
            .split_once('/')
            .ok_or_else(|| Error::new("invalid_input", "Invalid pagination cursor"))?;
        document::timestamp(date)?;
        document::note_id(id)?;
    }
    Ok(())
}

fn render_sheet(sheet: &TaskSheet) -> String {
    use std::fmt::Write;
    let ordering = if sheet.sheet.ordering == SheetOrdering::Display {
        "display"
    } else {
        "preferred"
    };
    let mut markdown = format!(
        "# Robostickies task sheet\n\nEvaluated at: {}. Scope: {:?}. Ordering: {ordering}. Complete: {}.\n\nCheckboxes are a read-only view; edit source notes by UUID and revision.\nLinks are relative to the .robostickies directory.\n\n{}\n\nSheet: {}. Revision: {}. [Source]({}).\n",
        sheet.evaluated_at,
        sheet.scope,
        if sheet.complete { "yes" } else { "no" },
        sheet.note.body,
        sheet.note.id,
        sheet.note.revision,
        sheet.note.path,
    );
    if let Some(id) = &sheet.next_task {
        let _ = writeln!(markdown, "\nNext suggested task: {id}.");
    }
    for (title, rows) in [
        ("Tasks", &sheet.rows),
        ("Prerequisite context", &sheet.context),
    ] {
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(markdown, "\n## {title}\n");
        for (index, row) in rows.iter().enumerate() {
            let indent = " ".repeat((index + 1).to_string().len() + 2);
            if let Some(view) = &row.task {
                let state = match view.task.state {
                    TaskState::Done => "Done",
                    TaskState::Cancelled => "Cancelled",
                    _ if view.ready => "Ready",
                    _ if view.note.status == NoteStatus::Archived => "Archived",
                    _ if view.active_deferral.is_some() => "Deferred",
                    _ => "Blocked",
                };
                let _ = writeln!(
                    markdown,
                    "{}. [{}] **{state} · {}** ({}) — {}",
                    index + 1,
                    if view.task.state == TaskState::Done {
                        "x"
                    } else {
                        " "
                    },
                    view.task.state.as_str(),
                    view.note.status.as_str(),
                    view.note.id
                );
                let _ = writeln!(
                    markdown,
                    "\n{indent}Revision: {}. [Source]({}).\n",
                    view.note.revision, view.note.path
                );
                for line in view.note.body.lines() {
                    let _ = writeln!(markdown, "{indent}{line}");
                }
                if let Some(deferral) = &view.active_deferral {
                    let reason = match deferral {
                        Deferral::Manual => "Deferred manually; resume explicitly.".into(),
                        Deferral::Until { until } => format!("Deferred until {until}."),
                    };
                    let _ = writeln!(markdown, "\n{indent}{reason}");
                }
                for blocker in &view.blockers {
                    let _ = writeln!(
                        markdown,
                        "\n{indent}Prerequisite {}: {} ({}).",
                        blocker.id,
                        blocker.path.join(" → "),
                        blocker.reason
                    );
                }
            } else {
                let _ = writeln!(
                    markdown,
                    "{}. [ ] **Unavailable** — {} ({}).",
                    index + 1,
                    row.id,
                    row.unavailable.as_deref().unwrap_or("unresolved_reference")
                );
            }
        }
    }
    if !sheet.diagnostics.is_empty() {
        markdown.push_str("\n## Diagnostics\n\n");
        for diagnostic in &sheet.diagnostics {
            // Escape generated diagnostic messages before embedding them in Markdown.
            let escaped: String = diagnostic
                .message
                .chars()
                .flat_map(|c| {
                    if matches!(c, '\\' | '[' | ']' | '*' | '_' | '`' | '<' | '>') {
                        vec!['\\', c]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            let _ = writeln!(markdown, "- {escaped}");
        }
    }
    markdown
}

/// Source-only fallback for graph validation while the disposable cache is unavailable.
pub(crate) fn source_graph(root: &Path) -> crate::Result<Graph> {
    read_sources(root, &BTreeMap::new(), false)
        .map(|catalog| catalog.graph)
        .map_err(|error| Error::new("io", error.to_string()))
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
    let graph = Graph::from_notes(groups.values().flatten(), version)?;
    diagnostics.extend(graph.diagnostics(Scope::All));
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
        graph,
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
mod task_index_tests {
    use super::*;
    use crate::Store;

    fn setup() -> (tempfile::TempDir, Store) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(root.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        (root, store)
    }

    fn create(store: &Store, body: &str, spec: TaskSpec, private: bool) -> Note {
        store
            .create_note(CreateNote {
                body: body.into(),
                task: Some(spec),
                private: Some(private),
                ..CreateNote::default()
            })
            .unwrap()
            .note
    }

    #[test]
    fn dated_deferral_expires_exactly_at_the_captured_instant_without_writes() {
        let (_root, store) = setup();
        let until = "2030-01-02T03:04:05.123456789Z";
        let task = create(
            &store,
            "Timed",
            TaskSpec {
                deferral: Some(Deferral::Until {
                    until: until.into(),
                }),
                ..TaskSpec::default()
            },
            false,
        );
        let manual = create(
            &store,
            "Manual",
            TaskSpec {
                deferral: Some(Deferral::Manual),
                ..TaskSpec::default()
            },
            false,
        );
        let sheet = store
            .create_note(CreateNote {
                body: "Timed work".into(),
                sheet: Some(SheetSpec {
                    items: vec![task.id.clone(), manual.id.clone()],
                    ordering: SheetOrdering::Preferred,
                }),
                ..CreateNote::default()
            })
            .unwrap()
            .note;
        let bytes = fs::read(store.directory().join(&task.path)).unwrap();
        let index = Index::refresh(&store.directory(), false).unwrap();
        let at = document::timestamp(until).unwrap();
        for (instant, expected) in [
            (at - chrono::Duration::nanoseconds(1), false),
            (at, true),
            (at + chrono::Duration::nanoseconds(1), true),
        ] {
            let page = index
                .list_tasks(
                    &TaskQuery {
                        ready: Some(true),
                        ..TaskQuery::default()
                    },
                    instant,
                )
                .unwrap();
            assert_eq!(page.total, usize::from(expected));
            let waiting = index
                .list_tasks(
                    &TaskQuery {
                        deferred: Some(true),
                        ..TaskQuery::default()
                    },
                    instant,
                )
                .unwrap();
            assert_eq!(waiting.total, if expected { 1 } else { 2 });
            let eligible = index
                .list_tasks(
                    &TaskQuery {
                        deferred: Some(false),
                        ..TaskQuery::default()
                    },
                    instant,
                )
                .unwrap();
            assert_eq!(eligible.total, usize::from(expected));
            let report = index
                .render_task_sheet(&sheet.id, Scope::Public, instant)
                .unwrap();
            assert_eq!(report.next_task, expected.then(|| task.id.clone()));
            assert_eq!(report.evaluated_at, page.evaluated_at);
            assert_eq!(report.rows[0].task.as_ref().unwrap().ready, expected);
        }
        assert_eq!(fs::read(store.directory().join(&task.path)).unwrap(), bytes);
        assert_eq!(store.get_note(&task.id).unwrap().revision, task.revision);
        let tx = index.db.begin_read().unwrap();
        let bytes = tx
            .open_table(TASKS)
            .unwrap()
            .get(graph::key(Visibility::Public, &task.id).as_str())
            .unwrap()
            .unwrap()
            .value()
            .to_vec();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(record.get("ready").is_none() && record.get("body").is_none());
        assert_eq!(record["task"]["deferral"]["until"], until);
    }

    #[test]
    fn task_indexes_refresh_reverse_edges_membership_and_recover_damaged_derived_data() {
        let (_root, store) = setup();
        let a = create(&store, "A", TaskSpec::default(), false);
        let b = create(
            &store,
            "B",
            TaskSpec {
                requires: vec![a.id.clone()],
                deferral: Some(Deferral::Manual),
                ..TaskSpec::default()
            },
            true,
        );
        let sheet = store
            .create_note(CreateNote {
                body: "Order".into(),
                private: Some(true),
                sheet: Some(SheetSpec {
                    items: vec![b.id.clone(), a.id.clone()],
                    ordering: SheetOrdering::Preferred,
                }),
                ..CreateNote::default()
            })
            .unwrap()
            .note;
        let inspect = || {
            let index = Index::refresh(&store.directory(), false).unwrap();
            let graph = index.graph().unwrap();
            assert_eq!(
                graph.incoming_tasks[&graph::key(Visibility::Public, &a.id)],
                BTreeSet::from([graph::key(Visibility::Private, &b.id)])
            );
            assert_eq!(
                graph.incoming_sheets[&graph::key(Visibility::Private, &b.id)],
                BTreeSet::from([graph::key(Visibility::Private, &sheet.id)])
            );
            assert_eq!(
                graph.sheets[&graph::key(Visibility::Private, &sheet.id)]
                    .sheet
                    .items,
                vec![b.id.clone(), a.id.clone()]
            );
            assert_eq!(
                index
                    .list_tasks(
                        &TaskQuery {
                            deferred: Some(true),
                            ..TaskQuery::default()
                        },
                        chrono::Utc::now()
                    )
                    .unwrap()
                    .total,
                1
            );
        };
        inspect();
        for damage in [
            "reverse",
            "membership",
            "filters",
            "requires",
            "deferrals",
            "record",
            "identity",
            "generation",
        ] {
            let db = Database::open(store.directory().join("cache/index.redb")).unwrap();
            let tx = db.begin_write().unwrap();
            match damage {
                "reverse" => {
                    tx.delete_multimap_table(REQUIRED_BY).unwrap();
                }
                "membership" => {
                    tx.delete_multimap_table(MEMBER_OF).unwrap();
                }
                "filters" => {
                    tx.delete_multimap_table(TASK_FILTERS).unwrap();
                }
                "requires" => {
                    tx.delete_multimap_table(REQUIRES).unwrap();
                }
                "deferrals" => {
                    tx.open_table(DEFERRALS)
                        .unwrap()
                        .insert("bad", "bad")
                        .unwrap();
                }
                "record" => {
                    tx.open_table(TASKS)
                        .unwrap()
                        .insert(
                            graph::key(Visibility::Public, &a.id).as_str(),
                            b"invalid".as_slice(),
                        )
                        .unwrap();
                }
                "identity" => {
                    tx.open_table(IDENTITIES)
                        .unwrap()
                        .insert(a.id.as_str(), b"invalid".as_slice())
                        .unwrap();
                }
                "generation" => {
                    tx.open_table(METADATA)
                        .unwrap()
                        .insert("generation", 3)
                        .unwrap();
                }
                _ => unreachable!(),
            }
            tx.commit().unwrap();
            drop(db);
            inspect();
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
        }
        let b = store
            .update_note(UpdateNote {
                id: b.id.clone(),
                expected_revision: b.revision.clone(),
                task: Some(TaskPatch {
                    requires: Some(vec![]),
                    clear_deferral: true,
                    ..TaskPatch::default()
                }),
                ..UpdateNote::default()
            })
            .unwrap()
            .note;
        let index = Index::refresh(&store.directory(), false).unwrap();
        assert!(index.graph().unwrap().incoming_tasks.is_empty());
        assert_eq!(
            index
                .list_tasks(
                    &TaskQuery {
                        deferred: Some(true),
                        ..TaskQuery::default()
                    },
                    chrono::Utc::now()
                )
                .unwrap()
                .total,
            0
        );
        drop(index);
        fs::remove_file(store.directory().join(&b.path)).unwrap();
        let index = Index::refresh(&store.directory(), false).unwrap();
        assert_eq!(index.graph().unwrap().tasks.len(), 1);
        assert!(
            !index
                .render_task_sheet(&sheet.id, Scope::All, chrono::Utc::now())
                .unwrap()
                .complete
        );
    }

    #[test]
    fn task_candidate_filters_paginate_without_filtering_prerequisite_evidence() {
        let (_root, store) = setup();
        let done = create(
            &store,
            "Prerequisite evidence",
            TaskSpec {
                state: TaskState::Done,
                ..TaskSpec::default()
            },
            false,
        );
        let done = store
            .update_note(UpdateNote {
                id: done.id,
                expected_revision: done.revision,
                status: Some(NoteStatus::Archived),
                ..UpdateNote::default()
            })
            .unwrap()
            .note;
        let mut selected = Vec::new();
        for index in 0..4 {
            let note = create(
                &store,
                &format!("Selected {index}"),
                TaskSpec {
                    requires: vec![done.id.clone()],
                    ..TaskSpec::default()
                },
                index % 2 == 0,
            );
            selected.push(
                store
                    .update_note(UpdateNote {
                        id: note.id,
                        expected_revision: note.revision,
                        tags: Some(vec!["paperclip".into()]),
                        ..UpdateNote::default()
                    })
                    .unwrap()
                    .note,
            );
        }
        let index = Index::refresh(&store.directory(), false).unwrap();
        let at = chrono::Utc::now();
        let mut query = TaskQuery {
            ready: Some(true),
            notes: Query {
                tags: vec!["paperclip".into()],
                since: Some("2000-01-01".into()),
                until: Some("2100-01-01".into()),
                text: Some("SELECTED".into()),
                limit: 2,
                ..Query::default()
            },
            ..TaskQuery::default()
        };
        let page = index.list_tasks(&query, at).unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(page.tasks.len(), 2);
        query.notes.cursor = page.next_cursor;
        let next = index.list_tasks(&query, at).unwrap();
        assert_eq!(next.tasks.len(), 2);
        assert!(next.next_cursor.is_none());
        assert!(
            next.tasks
                .iter()
                .all(|t| !page.tasks.iter().any(|old| old.note.id == t.note.id))
        );
        for (scope, total) in [(Scope::Private, 2), (Scope::Public, 2), (Scope::All, 4)] {
            assert_eq!(
                index
                    .list_tasks(
                        &TaskQuery {
                            notes: Query {
                                scope: Some(scope),
                                ..Query::default()
                            },
                            ..TaskQuery::default()
                        },
                        at
                    )
                    .unwrap()
                    .total,
                total
            );
        }
        assert_eq!(
            index
                .list_tasks(
                    &TaskQuery {
                        notes: Query {
                            exclude_tags: vec!["paperclip".into()],
                            ..Query::default()
                        },
                        ..TaskQuery::default()
                    },
                    at
                )
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            index
                .list_tasks(
                    &TaskQuery {
                        states: vec![TaskState::Done],
                        notes: Query {
                            status: StatusFilter::Archived,
                            ..Query::default()
                        },
                        ..TaskQuery::default()
                    },
                    at
                )
                .unwrap()
                .total,
            1
        );
        for input in [
            TaskQuery {
                notes: Query {
                    limit: 1001,
                    ..Query::default()
                },
                ..TaskQuery::default()
            },
            TaskQuery {
                notes: Query {
                    cursor: Some("bad/id".into()),
                    ..Query::default()
                },
                ..TaskQuery::default()
            },
            TaskQuery {
                notes: Query {
                    tags: vec!["bad tag".into()],
                    ..Query::default()
                },
                ..TaskQuery::default()
            },
        ] {
            assert!(index.list_tasks(&input, at).is_err());
        }
        drop(selected);
    }
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
                task: None,
                sheet: None,
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
                task: None,
                sheet: None,
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
                task: None,
                sheet: None,
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
