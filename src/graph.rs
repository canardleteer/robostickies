use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{document, model::*, version::DataVersion};

pub(crate) fn task(note: &Note) -> Result<Option<TaskSpec>> {
    decode(note, "task")
}

pub(crate) fn sheet(note: &Note) -> Result<Option<SheetSpec>> {
    decode(note, "sheet")
}

fn decode<T: serde::de::DeserializeOwned>(note: &Note, key: &str) -> Result<Option<T>> {
    note.metadata
        .get(key)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                Error::new("invalid_input", format!("Invalid {key} metadata: {error}"))
            })
        })
        .transpose()
}

pub(crate) fn validate_metadata(note: &Note) -> Result<()> {
    let task = task(note)?;
    let sheet = sheet(note)?;
    if task.is_some() && sheet.is_some() {
        return Err(Error::new(
            "invalid_input",
            "task and sheet are mutually exclusive",
        ));
    }
    if let Some(task) = task {
        references(&task.requires, Some(&note.id))?;
        if task.state.terminal() && task.deferral.is_some() {
            return Err(Error::new(
                "invalid_input",
                "Terminal tasks cannot be deferred",
            ));
        }
        if let Some(Deferral::Until { until }) = task.deferral {
            document::timestamp(&until)?;
        }
    }
    if let Some(sheet) = sheet {
        references(&sheet.items, Some(&note.id))?;
    }
    Ok(())
}

fn references(ids: &[String], own: Option<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        document::note_id(id)?;
        if own == Some(id.as_str()) || !seen.insert(id) {
            return Err(Error::new(
                "invalid_input",
                "References must be unique and cannot refer to the note itself",
            ));
        }
    }
    Ok(())
}

fn normalize(task: &mut TaskSpec) -> Result<()> {
    if let Some(Deferral::Until { until }) = &mut task.deferral {
        *until = document::date_bound(until)?.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    }
    Ok(())
}

pub(crate) fn set_classification(
    note: &mut Note,
    mut task: Option<TaskSpec>,
    sheet: Option<SheetSpec>,
) -> Result<()> {
    if let Some(task) = &mut task {
        normalize(task)?;
    }
    for (key, value) in [
        ("task", task.map(serde_json::to_value).transpose()),
        ("sheet", sheet.map(serde_json::to_value).transpose()),
    ] {
        if let Some(value) = value.map_err(|e| Error::new("invalid_input", e.to_string()))? {
            note.metadata.insert(key.into(), value);
        }
    }
    validate_metadata(note)
}

fn patch_mapping(note: &mut Note, key: &str, spec: &impl Serialize, fields: &[&str]) -> Result<()> {
    let value = serde_json::to_value(spec)
        .map_err(|error| Error::new("invalid_input", error.to_string()))?;
    if let Some(existing) = note
        .metadata
        .get_mut(key)
        .and_then(serde_json::Value::as_object_mut)
    {
        for field in fields {
            if let Some(value) = value.get(*field) {
                existing.insert((*field).into(), value.clone());
            } else {
                existing.remove(*field);
            }
        }
    } else {
        note.metadata.insert(key.into(), value);
    }
    Ok(())
}

pub(crate) fn patch(note: &mut Note, input: &UpdateNote) -> Result<()> {
    if (input.task.is_some() && input.clear_task)
        || (input.sheet.is_some() && input.clear_sheet)
        || ((input.task.is_some() || input.clear_task)
            && (input.sheet.is_some() || input.clear_sheet))
    {
        return Err(Error::new(
            "invalid_input",
            "Conflicting task/sheet patches or clear flags",
        ));
    }
    if input.task.is_some() && note.metadata.contains_key("sheet")
        || input.sheet.is_some() && note.metadata.contains_key("task")
    {
        return Err(Error::new(
            "invalid_input",
            "Clear the existing classification in a separate edit before switching task/sheet kind",
        ));
    }
    if input.clear_task {
        note.metadata.remove("task");
    }
    if input.clear_sheet {
        note.metadata.remove("sheet");
    }
    if let Some(patch) = &input.task {
        if patch.clear_deferral && patch.deferral.is_some() {
            return Err(Error::new(
                "invalid_input",
                "deferral and clear_deferral conflict",
            ));
        }
        let mut spec = task(note)?.unwrap_or_default();
        if let Some(state) = patch.state {
            spec.state = state;
            if state.terminal() {
                spec.deferral = None;
            }
        }
        if let Some(requires) = &patch.requires {
            spec.requires = requires.clone();
        }
        if patch.clear_deferral {
            spec.deferral = None;
        }
        if let Some(deferral) = &patch.deferral {
            spec.deferral = Some(deferral.clone());
            normalize(&mut spec)?;
        }
        let mut fields = Vec::new();
        if patch.state.is_some() {
            fields.push("state");
        }
        if patch.requires.is_some() {
            fields.push("requires");
        }
        if patch.deferral.is_some()
            || patch.clear_deferral
            || patch.state.is_some_and(TaskState::terminal)
        {
            fields.push("deferral");
        }
        patch_mapping(note, "task", &spec, &fields)?;
    }
    if let Some(patch) = &input.sheet {
        let mut spec = sheet(note)?.unwrap_or_default();
        if let Some(items) = &patch.items {
            spec.items = items.clone();
        }
        if let Some(ordering) = patch.ordering {
            spec.ordering = ordering;
        }
        let mut fields = Vec::new();
        if patch.items.is_some() {
            fields.push("items");
        }
        if patch.ordering.is_some() {
            fields.push("ordering");
        }
        patch_mapping(note, "sheet", &spec, &fields)?;
    }
    validate_metadata(note)
}

pub(crate) fn key(visibility: Visibility, id: &str) -> String {
    format!("{visibility}/{id}")
}

pub(crate) fn stored_key(path: &str, id: &str) -> String {
    key(
        if path.starts_with("private/") {
            Visibility::Private
        } else {
            Visibility::Public
        },
        id,
    )
}

/// Graph records omit note bodies. Keys include the namespace to distinguish
/// duplicate IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskRecord {
    pub id: String,
    pub visibility: Visibility,
    pub path: String,
    pub status: NoteStatus,
    pub created_at: String,
    pub tags: Vec<String>,
    pub task: TaskSpec,
}

impl TaskRecord {
    pub fn from_note(note: &Note) -> Result<Option<Self>> {
        Ok(task(note)?.map(|task| Self {
            id: note.id.clone(),
            visibility: note.visibility,
            path: note.path.clone(),
            status: note.status,
            created_at: note.created_at.clone(),
            tags: note.tags.clone(),
            task,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SheetRecord {
    pub id: String,
    pub visibility: Visibility,
    pub path: String,
    pub sheet: SheetSpec,
}

impl SheetRecord {
    pub fn from_note(note: &Note) -> Result<Option<Self>> {
        Ok(sheet(note)?.map(|sheet| Self {
            id: note.id.clone(),
            visibility: note.visibility,
            path: note.path.clone(),
            sheet,
        }))
    }
}

#[derive(Default)]
pub(crate) struct Graph {
    pub tasks: BTreeMap<String, TaskRecord>,
    pub sheets: BTreeMap<String, SheetRecord>,
    pub identities: BTreeMap<String, BTreeSet<String>>,
    pub incoming_tasks: BTreeMap<String, BTreeSet<String>>,
    pub incoming_sheets: BTreeMap<String, BTreeSet<String>>,
}

impl Graph {
    pub fn from_notes<'a>(
        notes: impl IntoIterator<Item = &'a Note>,
        version: DataVersion,
    ) -> Result<Self> {
        let mut graph = Self::default();
        for note in notes {
            let key = stored_key(&note.path, &note.id);
            graph
                .identities
                .entry(note.id.clone())
                .or_default()
                .insert(key.clone());
            if !version.tasks() {
                continue;
            }
            if let Some(task) = TaskRecord::from_note(note)? {
                graph.tasks.insert(key.clone(), task);
            }
            if let Some(sheet) = SheetRecord::from_note(note)? {
                graph.sheets.insert(key, sheet);
            }
        }
        for (identity, record) in &graph.tasks {
            for target in &record.task.requires {
                graph
                    .incoming_tasks
                    .entry(graph.reference_key(target, record.visibility))
                    .or_default()
                    .insert(identity.clone());
            }
        }
        for (identity, record) in &graph.sheets {
            for target in &record.sheet.items {
                graph
                    .incoming_sheets
                    .entry(graph.reference_key(target, record.visibility))
                    .or_default()
                    .insert(identity.clone());
            }
        }
        Ok(graph)
    }

    pub fn reference_key(&self, id: &str, from: Visibility) -> String {
        self.lookup(id, from == Visibility::Public)
            .map(|record| stored_key(&record.path, id))
            .unwrap_or_else(|| format!("unresolved/{id}"))
    }

    pub fn lookup(&self, id: &str, public: bool) -> Option<&TaskRecord> {
        if public {
            return self
                .tasks
                .get(&key(Visibility::Public, id))
                .filter(|record| record.visibility == Visibility::Public);
        }
        let identities = self.identities.get(id)?;
        if identities.len() != 1 {
            return None;
        }
        self.tasks.get(identities.first()?)
    }

    pub fn validate_edit(&mut self, before: Option<&Note>, after: &Note) -> Result<()> {
        let previous = before.map(task).transpose()?.flatten();
        let next = task(after)?;
        let moving_private = before.is_some_and(|n| n.visibility == Visibility::Public)
            && after.visibility == Visibility::Private;
        if previous.is_some() && (next.is_none() || moving_private) {
            let identity = before.map_or_else(
                || key(after.visibility, &after.id),
                |note| stored_key(&note.path, &note.id),
            );
            let incoming_task = self.incoming_tasks.get(&identity).is_some_and(|sources| {
                sources
                    .iter()
                    .any(|source| !moving_private || source.starts_with("public/"))
            });
            let incoming_sheet = self.incoming_sheets.get(&identity).is_some_and(|sources| {
                sources
                    .iter()
                    .any(|source| !moving_private || source.starts_with("public/"))
            });
            if incoming_task || incoming_sheet {
                return Err(Error::new(
                    if moving_private {
                        "privacy"
                    } else {
                        "invalid_input"
                    },
                    "Reconcile incoming task and sheet references before removing classification or moving this task to private storage",
                ));
            }
        }
        if let Some(before) = before {
            self.tasks.remove(&stored_key(&before.path, &before.id));
            self.identities.remove(&before.id);
        }
        let identity = key(after.visibility, &after.id);
        self.identities
            .entry(after.id.clone())
            .or_default()
            .insert(identity.clone());
        if let Some(record) = TaskRecord::from_note(after)? {
            self.tasks.insert(identity, record);
        }
        let relationships_changed = previous.as_ref().map(|t| &t.requires)
            != next.as_ref().map(|t| &t.requires)
            || before.is_some_and(|n| n.visibility != after.visibility);
        if next.is_some() && relationships_changed {
            self.validate_component(
                &after.id,
                after.visibility == Visibility::Public,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            )?;
        }
        let previous_sheet = before.map(sheet).transpose()?.flatten();
        if let Some(spec) = sheet(after)?
            && (previous_sheet.as_ref().map(|s| &s.items) != Some(&spec.items)
                || before.is_some_and(|n| n.visibility != after.visibility))
        {
            for id in spec.items {
                if self
                    .lookup(&id, after.visibility == Visibility::Public)
                    .is_none()
                {
                    return Err(unresolved());
                }
            }
        }
        Ok(())
    }

    fn validate_component(
        &self,
        id: &str,
        public: bool,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        if !visiting.insert(id.into()) {
            return Err(Error::new(
                "invalid_input",
                "Task prerequisites would contain a cycle",
            ));
        }
        if !visited.contains(id) {
            let record = self.lookup(id, public).ok_or_else(unresolved)?;
            for target in &record.task.requires {
                self.validate_component(
                    target,
                    public || record.visibility == Visibility::Public,
                    visiting,
                    visited,
                )?;
            }
            visited.insert(id.into());
        }
        visiting.remove(id);
        Ok(())
    }

    pub fn diagnostics(&self, scope: Scope) -> Vec<Diagnostic> {
        let mut evaluator = Evaluator::new(self, Utc::now());
        let mut diagnostics = Vec::new();
        for task in self
            .tasks
            .values()
            .filter(|task| scope.includes(task.visibility))
        {
            let summary = evaluator.summary(&task.id, task.visibility == Visibility::Public);
            for blocker in summary
                .blockers
                .values()
                .filter(|b| b.reason != "unfinished")
            {
                diagnostics.push(Diagnostic {
                    path: task.path.clone(),
                    code: blocker.reason.clone(),
                    message: format!(
                        "Task prerequisite graph has {} at {}",
                        blocker.reason,
                        blocker.path.join(" -> ")
                    ),
                    visibility: task.visibility,
                });
            }
        }
        for sheet in self
            .sheets
            .values()
            .filter(|sheet| scope.includes(sheet.visibility))
        {
            for id in &sheet.sheet.items {
                if self
                    .lookup(id, sheet.visibility == Visibility::Public)
                    .is_none()
                {
                    diagnostics.push(Diagnostic {
                        path: sheet.path.clone(),
                        code: "unresolved_reference".into(),
                        message: format!("Sheet item {id} is not an available task"),
                        visibility: sheet.visibility,
                    });
                }
            }
        }
        diagnostics
    }
}

fn unresolved() -> Error {
    Error::new(
        "invalid_input",
        "Unresolved task reference; every target must be an unambiguous task available in the source namespace",
    )
}

#[derive(Default, Clone)]
pub(crate) struct Summary {
    pub blockers: BTreeMap<String, Blocker>,
    pub ancestors: BTreeSet<String>,
}

pub(crate) struct Evaluator<'a> {
    graph: &'a Graph,
    at: DateTime<Utc>,
    memo: BTreeMap<(bool, String), Summary>,
    visiting: BTreeSet<(bool, String)>,
}

impl<'a> Evaluator<'a> {
    pub fn new(graph: &'a Graph, at: DateTime<Utc>) -> Self {
        Self {
            graph,
            at,
            memo: BTreeMap::new(),
            visiting: BTreeSet::new(),
        }
    }

    pub fn summary(&mut self, id: &str, public: bool) -> Summary {
        let identity = (public, id.to_owned());
        if let Some(cached) = self.memo.get(&identity) {
            return cached.clone();
        }
        if !self.visiting.insert(identity.clone()) {
            return Summary {
                blockers: BTreeMap::from([(
                    id.into(),
                    Blocker {
                        id: id.into(),
                        path: vec![id.into()],
                        reason: "cycle".into(),
                    },
                )]),
                ..Summary::default()
            };
        }
        let mut summary = Summary::default();
        if let Some(record) = self.graph.lookup(id, public) {
            // Sorting gives a stable explanation path independent of authored edge order.
            let mut requires = record.task.requires.clone();
            requires.sort();
            for target in requires {
                let target_public = public || record.visibility == Visibility::Public;
                summary.ancestors.insert(target.clone());
                let target_record = self.graph.lookup(&target, target_public);
                if target_record.is_none_or(|r| r.task.state != TaskState::Done) {
                    summary.blockers.entry(target.clone()).or_insert(Blocker {
                        id: target.clone(),
                        path: vec![id.into(), target.clone()],
                        reason: if target_record.is_none() {
                            "unresolved_reference"
                        } else {
                            "unfinished"
                        }
                        .into(),
                    });
                }
                if target_record.is_some() {
                    let nested = self.summary(&target, target_public);
                    summary.ancestors.extend(nested.ancestors);
                    for (blocker_id, mut blocker) in nested.blockers {
                        blocker.path.insert(0, id.into());
                        // A structural problem takes precedence over an unfinished state.
                        if blocker.reason != "unfinished"
                            || !summary.blockers.contains_key(&blocker_id)
                        {
                            summary.blockers.insert(blocker_id, blocker);
                        }
                    }
                }
            }
        }
        self.visiting.remove(&identity);
        self.memo.insert(identity, summary.clone());
        summary
    }

    pub fn view(&mut self, note: Note, public: bool) -> Result<TaskView> {
        let task = task(&note)?.ok_or_else(unresolved)?;
        let summary = self.summary(&note.id, public);
        let active_deferral = task.deferral.clone().filter(|d| match d {
            Deferral::Manual => true,
            Deferral::Until { until } => {
                document::timestamp(until).is_ok_and(|until| self.at < until)
            }
        });
        let blocked = !summary.blockers.is_empty();
        Ok(TaskView {
            ready: note.status == NoteStatus::Active
                && !task.state.terminal()
                && active_deferral.is_none()
                && !blocked,
            task,
            blocked,
            active_deferral,
            unsatisfied_prerequisites: summary.blockers.keys().cloned().collect(),
            blockers: summary.blockers.into_values().collect(),
            note,
        })
    }
}
