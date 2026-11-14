//! Repository data and request types for the shared library.

use std::{collections::BTreeMap, fmt};

use clap::ValueEnum;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An operation failure with a stable machine-readable code.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Error {
    /// Stable category, such as `invalid_input`, `conflict`, or `busy`.
    pub code: String,
    /// Explanation of the failure and how to recover; excludes note payloads.
    pub message: String,
    /// Present only for a mandatory safety boundary; ordinary errors retain their shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<crate::version::UpgradeAdvice>,
    /// Retained recovery copy after a failed migration or source validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<crate::version::SnapshotResult>,
}

impl Error {
    /// Construct a failure at a library boundary.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            upgrade: None,
            snapshot: None,
        }
    }

    pub(crate) fn with_upgrade(mut self, advice: crate::version::UpgradeAdvice) -> Self {
        self.upgrade = Some(advice);
        self
    }

    pub(crate) fn with_snapshot(mut self, snapshot: crate::version::SnapshotResult) -> Self {
        self.message.push_str(&format!(
            "; recovery snapshot retained at {}",
            snapshot.path
        ));
        self.snapshot = Some(snapshot);
        self
    }

    /// Exit status used by the CLI.
    pub fn exit_code(&self) -> i32 {
        match self.code.as_str() {
            "invalid_input" | "invalid_document" => 65,
            "not_found" | "not_initialized" => 66,
            "busy" => 75,
            "configuration" | "upgrade_required" => 78,
            "io" => 74,
            _ => 1,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new("io", error.to_string())
    }
}

/// Result type used by repository operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Storage namespace. Public means eligible for sharing through Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which namespaces a read operation may include.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Public,
    Private,
    #[default]
    All,
}

impl Scope {
    pub(crate) fn includes(self, visibility: Visibility) -> bool {
        self == Self::All
            || matches!(
                (self, visibility),
                (Self::Public, Visibility::Public) | (Self::Private, Visibility::Private)
            )
    }
}

/// Notes can be archived without deleting their history.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum NoteStatus {
    #[default]
    Active,
    Archived,
}

impl NoteStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

/// Status selection for browsing and review.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum StatusFilter {
    #[default]
    Active,
    Archived,
    All,
}

/// A note read from its authoritative Markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
    pub status: NoteStatus,
    pub body: String,
    /// Computed from storage location and private tag references.
    pub visibility: Visibility,
    /// Content hash for optimistic concurrency checks.
    pub revision: String,
    /// Path relative to `.robostickies`.
    pub path: String,
    /// Additional YAML fields retained during edits.
    pub metadata: BTreeMap<String, Value>,
}

/// A user-defined tag and its complete Markdown guidance.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tag {
    /// Scope-qualified name, e.g. `tooling` or `private:tooling`.
    pub name: String,
    /// Optional catalog explanation. Self-explanatory names usually need none.
    /// Usage instructions belong in `body`; absent descriptions serialize as null.
    pub description: Option<String>,
    /// Include this tag's complete guidance in session startup context.
    pub read_on_start: bool,
    /// Optional Markdown usage instructions, separate from the description.
    pub body: String,
    pub visibility: Visibility,
    pub revision: String,
    pub metadata: BTreeMap<String, Value>,
}

/// Compact tag information suitable for indexing and discovery.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagSummary {
    /// Scope-qualified tag name used when attaching it to notes.
    pub name: String,
    /// Optional catalog explanation; self-explanatory names usually need none.
    pub description: Option<String>,
    /// The complete guidance is included in session startup context.
    pub read_on_start: bool,
    pub visibility: Visibility,
}

impl From<&Tag> for TagSummary {
    fn from(tag: &Tag) -> Self {
        Self {
            name: tag.name.clone(),
            description: tag.description.clone(),
            read_on_start: tag.read_on_start,
            visibility: tag.visibility,
        }
    }
}

/// A source problem surfaced without silently serving stale cached content.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub path: String,
    pub code: String,
    pub message: String,
    pub visibility: Visibility,
}

/// Common filters. Date bounds are UTC dates or RFC3339 timestamps; `until` is exclusive.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    /// Defaults to all for browsing, and public for review.
    pub scope: Option<Scope>,
    /// Every listed tag must be attached to the note.
    pub tags: Vec<String>,
    /// Exclude notes with any of these tags.
    pub exclude_tags: Vec<String>,
    pub status: StatusFilter,
    /// Case-insensitive literal substring of the note body.
    pub text: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    /// Page size, between 1 and 1000.
    pub limit: usize,
    /// Cursor returned by a previous response with the same filters.
    pub cursor: Option<String>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            scope: None,
            tags: vec![],
            exclude_tags: vec![],
            status: StatusFilter::Active,
            text: None,
            since: None,
            until: None,
            limit: 50,
            cursor: None,
        }
    }
}

/// Filters for a complete review. Defaults to active public notes; paging is internal.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewQuery {
    /// Omit to review public notes only.
    pub scope: Option<Scope>,
    /// Every listed tag must be attached to the note.
    pub tags: Vec<String>,
    /// Exclude notes with any of these tags.
    pub exclude_tags: Vec<String>,
    pub status: StatusFilter,
    /// Case-insensitive literal substring of the note body.
    pub text: Option<String>,
    /// Inclusive creation-date bound (UTC date or RFC3339 timestamp).
    pub since: Option<String>,
    /// Exclusive creation-date bound (UTC date or RFC3339 timestamp).
    pub until: Option<String>,
}

impl From<ReviewQuery> for Query {
    fn from(review: ReviewQuery) -> Self {
        Self {
            scope: review.scope,
            tags: review.tags,
            exclude_tags: review.exclude_tags,
            status: review.status,
            text: review.text,
            since: review.since,
            until: review.until,
            ..Self::default()
        }
    }
}

/// A page of notes, with the count calculated after all filters.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NotePage {
    pub notes: Vec<Note>,
    pub total: usize,
    pub next_cursor: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Input for creating a brief note.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNote {
    /// Nonempty UTF-8 text. Markdown or plain text is preferred.
    pub body: String,
    /// Advisory metadata.body_format; markdown, text, and yaml are recommended.
    /// Other nonblank strings are accepted without interpreting the body.
    pub body_format: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Omit to derive privacy from tags. False cannot override a private tag.
    pub private: Option<bool>,
}

/// Patch a note. Unspecified fields retain their current values.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNote {
    pub id: String,
    pub body: Option<String>,
    /// Set an advisory string hint. Omit to preserve existing metadata exactly.
    pub body_format: Option<String>,
    /// Remove a string hint; conflicts with body_format. Non-string legacy values
    /// must be reconciled in the source rather than overwritten by this interface.
    #[serde(default)]
    pub clear_body_format: bool,
    pub tags: Option<Vec<String>>,
    pub status: Option<NoteStatus>,
    /// True moves a public note into private storage. Publishing is separate.
    pub private: Option<bool>,
    /// Required last-read revision. A stale value rejects the edit without changing sources.
    pub expected_revision: String,
}

/// Define a tag. `name` is an unqualified filename slug.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTag {
    /// Required bare filename slug, e.g. fresh-install; used as the note tag.
    pub name: String,
    /// Optional short catalog explanation. Self-explanatory tags usually need none.
    /// Omit or use null for no description; blank text is invalid. Text is preserved.
    pub description: Option<String>,
    /// Optional Markdown usage instructions, separate from the catalog description.
    #[serde(default)]
    pub body: String,
    /// Include the full guidance in startup context.
    #[serde(default)]
    pub read_on_start: bool,
    /// Override configured privacy defaults; private tags use private:<name>.
    pub private: Option<bool>,
}

/// Options for copying a private note into public storage.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishNote {
    pub id: String,
    #[serde(default)]
    pub dry_run: bool,
    /// Replace the body when preparing the public copy.
    pub body: Option<String>,
    /// Explicit public hint. The source hint is removed with other extra metadata.
    pub body_format: Option<String>,
    /// Replace the public tag set. Private tags are always removed or rejected.
    pub tags: Option<Vec<String>>,
    /// Required last-read source revision, including for previews.
    pub expected_revision: String,
}

/// A saved note remains successful even if refreshing the disposable index fails.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SavedNote {
    pub note: Note,
    pub warnings: Vec<String>,
}

/// Publishing can return a preview without writing any source files.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Publication {
    pub dry_run: bool,
    pub source_revision: String,
    pub note: Note,
    pub warnings: Vec<String>,
}

/// Where initialization installs ignore rules.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum IgnoreLocation {
    #[default]
    Store,
    Local,
}

/// Initialization is idempotent and does not overwrite existing definitions.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct InitOptions {
    pub agents: bool,
    pub ignore_location: IgnoreLocation,
}

/// Initialization result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitResult {
    pub created: Vec<String>,
    pub agents_updated: bool,
    pub warnings: Vec<String>,
}

/// Tag discovery includes parse diagnostics within the requested scope.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TagList {
    pub tags: Vec<TagSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Deterministic review material for a human or an agent to summarize.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Review {
    pub scope: Scope,
    pub total: usize,
    pub notes: Vec<Note>,
    pub markdown: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Complete validation result. Validation does not change source files.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Health {
    pub data_version: crate::version::DataVersion,
    pub upgrade_available: bool,
    pub upgrade: crate::version::UpgradeAdvice,
    /// Local advisory preference warnings, separate from source health.
    pub warnings: Vec<String>,
    pub healthy: bool,
    pub notes: usize,
    pub tags: usize,
    pub diagnostics: Vec<Diagnostic>,
}

/// Options for an explicit store migration; dry runs never change source files.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct UpgradeOptions {
    pub dry_run: bool,
    /// Remember a declined optional target without changing the store version.
    pub defer: bool,
    /// Retain the recovery snapshot after a successful migration.
    pub keep_snapshot: bool,
}
