//! Typed task metadata, query filters, and read-only instruction sheet results.
//!
//! Task state is independent of note archival. Readiness is evaluated from current
//! sources and a single UTC instant; it is never written to frontmatter or the cache.

use clap::ValueEnum;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{Diagnostic, Note, Query, Scope};

/// Optional patch fields distinguish omission from an explicit, invalid null.
pub(crate) fn supplied<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Work state, independent of the containing note's active/archived status.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskState {
    #[default]
    Todo,
    InProgress,
    Done,
    Cancelled,
}

impl TaskState {
    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Waiting is separate from prerequisites. Date inputs accept UTC dates or RFC3339.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Deferral {
    Manual,
    Until { until: String },
}

impl<'de> Deserialize<'de> for Deferral {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde's internally tagged unit variants ignore surplus fields. A struct
        // variant enforces the same strict mapping contract as the until variant.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Input {
            Manual {},
            Until { until: String },
        }
        Ok(match Input::deserialize(deserializer)? {
            Input::Manual {} => Self::Manual,
            Input::Until { until } => Self::Until { until },
        })
    }
}

/// Complete task metadata. New callers may use `Default` to create a todo task.
/// Stored mappings must explicitly contain state; references retain authored order.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    #[serde(default)]
    pub state: TaskState,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Deferral")]
    pub deferral: Option<Deferral>,
}

/// Selected task changes. Omission preserves a field; an empty requires list clears it.
/// Terminal transitions clear deferral, and clear_deferral conflicts with deferral.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskPatch {
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "TaskState")]
    pub state: Option<TaskState>,
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Vec<String>")]
    pub requires: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Deferral")]
    pub deferral: Option<Deferral>,
    #[serde(default)]
    pub clear_deferral: bool,
}

/// Presentation order never creates prerequisites. Preferred order suggests ready work.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum SheetOrdering {
    #[default]
    Display,
    Preferred,
}

/// A saved, ordered list of task UUIDs. Membership may be empty.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetSpec {
    pub items: Vec<String>,
    #[serde(default)]
    pub ordering: SheetOrdering,
}

/// Selected sheet changes. Supplying items replaces the entire authored list.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetPatch {
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Vec<String>")]
    pub items: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "supplied",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "SheetOrdering")]
    pub ordering: Option<SheetOrdering>,
}

/// Note filters plus task state and independent readiness filters, combined with AND.
/// Empty states selects todo and in_progress; all_task_states includes terminal work.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TaskQuery {
    #[serde(flatten)]
    pub notes: Query,
    pub states: Vec<TaskState>,
    pub all_task_states: bool,
    pub ready: Option<bool>,
    pub blocked: Option<bool>,
    pub deferred: Option<bool>,
}

/// One deterministic relationship path from the selected task to an unsatisfied task.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Blocker {
    pub id: String,
    /// Includes the selected task and the blocker UUID.
    pub path: Vec<String>,
    /// unfinished, unresolved_reference, or cycle; no private lookup details in public views.
    pub reason: String,
}

/// A decoded task and the evaluation of its complete prerequisite closure.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TaskView {
    pub task: TaskSpec,
    pub ready: bool,
    pub blocked: bool,
    /// Present only while a manual or dated deferral is active.
    pub active_deferral: Option<Deferral>,
    pub unsatisfied_prerequisites: Vec<String>,
    pub blockers: Vec<Blocker>,
    pub note: Note,
}

/// A task page evaluated at one instant, including scoped source/graph diagnostics.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskPage {
    pub tasks: Vec<TaskView>,
    pub total: usize,
    pub next_cursor: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub evaluated_at: String,
}

/// Rendering defaults to public visibility. Explicit all enables a mixed local sheet.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderTaskSheet {
    pub id: String,
    pub scope: Option<Scope>,
}

/// An authored sheet position; unavailable members retain their position and UUID.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetRow {
    pub id: String,
    pub task: Option<TaskView>,
    pub unavailable: Option<String>,
}

/// Structured and Markdown views produced by the same evaluation.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskSheet {
    pub note: Note,
    pub sheet: SheetSpec,
    pub scope: Scope,
    pub evaluated_at: String,
    pub rows: Vec<SheetRow>,
    /// Transitive prerequisites outside membership, including completed evidence.
    pub context: Vec<SheetRow>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
    /// Only a complete preferred sheet suggests its first ready member.
    pub next_task: Option<String>,
    pub markdown: String,
}
