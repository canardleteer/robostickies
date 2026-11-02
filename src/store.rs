//! Repository operations shared by CLI commands and MCP tools.
//!
//! Writes use an operation-scoped process lock and atomic file replacement.
//! Source files are never committed, staged, or removed from Git's index here.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    config::{self, Configuration},
    document,
    index::Index,
    model::*,
    version::{self, DataVersion, UpgradeResult},
};

/// Brief instructions supplied at MCP discovery, including before initialization.
pub const DISCOVERY_GUIDANCE: &str = "Call get_context at the start of work to read tag topics and startup guidance for this repository. Record concrete friction with papercut and useful successes with paperclip; other defined tags and general notes are welcome. Private tags use private:<name> and force ignored local storage. Public review includes only public notes; use publish_note to copy a private note into public storage. Read get_context for details and a portable AGENTS.md snippet. Use init_store if the repository is not initialized.";

const GUIDE: &str = r#"# Robostickies

At the start of work, read the tag catalog and the full guidance for tags marked
`read_on_start`. Use `robostickies context` or the MCP `get_context` tool.

Launch `robostickies mcp` from the agent's repository or working directory.
The process selects the containing Git root, or its working directory outside
Git, and keeps that selection for its lifetime. Use `--repo DIR` for an explicit
override; relative paths resolve from the startup working directory.

Record brief, concrete observations while working, then continue your task:

- `papercut`: friction, misleading guidance, broken tools, or avoidable rework.
  Explain the trigger, its effect, and what would have helped.
- `paperclip`: something that saved time or produced a useful result. Describe
  what worked and when to reuse it; routine success usually needs no note.
- Use any defined tags together, or leave a general note untagged.

For a verified fix, retain `papercut`, add `resolved`, record verification
evidence, and archive. Verify in the affected environment; MCP fixes require
installation and an agent restart followed by checks through the installed
connection. Archived status alone does not prove resolution.

For reusable guidance, retain `paperclip`, add `adopted` after the guidance exists
in the appropriately scoped AGENTS.md and has been reviewed, record its destination,
and archive. Publish a sanitized public copy of a private observation before
promoting it into tracked guidance. Keep other useful paperclips active without
`adopted`. Read the outcome tag's full guidance before applying it.

Outcome tags use the ordinary tag and update APIs. Retain the existing tags and
visibility. If a definition is missing, create it explicitly in the appropriate
namespace: a public definition for a public note, or a public/private definition
for a private note. A private outcome tag would move a public note into private
storage. Initialization adds missing predefined definitions without overwriting
custom guidance and honors tag privacy defaults.

Discover tags with `tags list` / `list_tags` and read details with `tags get` /
`get_tag`. Define tags in `tags/<name>.md`, using YAML `topic` and optionally
`read_on_start`, followed by Markdown guidance. Create them with `tags create` /
`create_tag`. Private definitions live separately in `private/tags/` and are
always addressed as `private:<name>`; public names cannot shadow them.

Notes live in `notes/<uuid>.md` or `private/notes/<uuid>.md`. YAML frontmatter
contains `id`, `created_at`, `updated_at` (RFC3339), `tags`, and `status`
(`active` or `archived`); the body is Markdown. Additional metadata is retained
when editing. The files are authoritative; the redb index is disposable.

Edits, archive/unarchive, and publishing (including previews) require the revision
from the last successful read or save. Use CLI `--if-match` or MCP
`expected_revision`. Missing or blank revisions are rejected. A stale revision
returns `conflict` without writing: re-read and reconcile the changes before
retrying. There is no overwrite bypass or automatic merge.

Cooperating CLI/MCP clients sharing one OS account on local disk use an exclusive
operation lock. Source reads, revision comparisons, and saves share that lock;
content replacement is atomic. Contention waits up to five seconds, then returns
`busy`. The OS releases the lock when a process exits; keep the lock file in place.
Concurrent direct editor/Git writes are outside this guarantee. After upgrading,
restart every MCP process using the checkout before relying on the new contract.

Any private tag makes a note private. `add --private` also keeps an otherwise
publicly tagged or untagged note local. Private notes stay private when tags
are removed. `publish <id>` / `publish_note` makes a new public copy, removes
private tags and additional metadata, and retains the private original.
Use `--dry-run` / `dry_run: true` to preview, and replace the body if necessary.
Publishing copies the supplied prose; review the text you choose to share.

Browse with visibility (`public`, `private`, `all`), tags, excluded tags, status,
text, and date filters. Tag filters require all selected tags; exclusions reject
any match. Search is a case-insensitive literal substring. Dates accept UTC
YYYY-MM-DD or RFC3339; `since` is inclusive and `until` is exclusive.
Review defaults to public notes. Local review can explicitly include private or
all notes. To include a private observation in public review, publish it first.
Review is deterministic material for you or an agent to summarize.
Review inputs have no limit or cursor; list/search retain pagination.
Find verified history with `review --status archived --tag resolved` or
`review --status archived --tag adopted`. In MCP use `review_notes` with
`status: "archived"` and `tags: ["resolved"]` or `["adopted"]`. Select scope and
qualified tag names explicitly for local private history.

If a public note ID also exists privately, public list/review retains the public
copy without exposing the conflict. Local diagnostics report duplicate IDs;
local reviews and ID-based reads/edits/publications reject the ambiguity. Repair
the copied source's UUID and filename manually. Index recovery does not edit notes.

Use `get_config` / `config show` to inspect effective configuration and sources.
User defaults come from the platform config directory's `robostickies.toml`.
`[tags] default_private = false` is the default; `private_patterns` accepts globs.
Precedence: explicit creation option, private/config.toml, user configuration,
config.toml, built-ins. The private namespace and cache are Git-ignored.
Ignore rules do not remove already tracked content from Git history.

Use `doctor` to validate sources and Git exclusions, and `reindex` to rebuild
the index. Never edit the binary index. `init --agents` optionally inserts a
small pointer into AGENTS.md while preserving the rest of that file.
"#;

/// Repository handle; construction does not create files or open the index.
#[derive(Debug, Clone)]
pub struct Store {
    repo: PathBuf,
    user_config: Option<PathBuf>,
}

/// Session orientation and a snippet suitable for existing agent instructions.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Context {
    pub initialized: bool,
    pub data_version: Option<DataVersion>,
    pub upgrade_available: bool,
    /// Canonical root selected when the handle was constructed. Omitted in public context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_root: Option<String>,
    pub guidance: String,
    pub agents_md: String,
    pub tags: Vec<TagSummary>,
    pub startup_tags: Vec<Tag>,
    /// Omitted for public-only context so personal defaults are not exported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Configuration>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Guard {
    _file: File,
}

fn cache_error(error: anyhow::Error) -> Error {
    error.downcast_ref::<Error>().cloned().unwrap_or_else(|| {
        Error::new(
            "index",
            format!("Index operation failed: {error}; run robostickies reindex"),
        )
    })
}

impl Store {
    /// Discover the Git root from the current directory, or use an explicit root.
    /// Outside Git, the current directory is used. The selected directory must exist.
    pub fn discover(repo: Option<&Path>) -> Result<Self> {
        let root = if let Some(path) = repo {
            path.to_path_buf()
        } else {
            let cwd = std::env::current_dir()?;
            match Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(&cwd)
                .output()
            {
                Ok(output) if output.status.success() => {
                    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end())
                }
                _ => cwd,
            }
        };
        Self::with_user_config(root, config::user_config_path())
    }

    /// Open a root with an explicitly selected user configuration file.
    /// Passing None disables user configuration, useful for reproducible automation.
    pub fn with_user_config(repo: impl AsRef<Path>, user_config: Option<PathBuf>) -> Result<Self> {
        let repo = repo.as_ref().canonicalize()?;
        if !repo.is_dir() {
            return Err(Error::new(
                "invalid_input",
                "Repository root must be a directory",
            ));
        }
        Ok(Self { repo, user_config })
    }

    /// Selected repository root.
    pub fn repository(&self) -> &Path {
        &self.repo
    }

    /// Root of authoritative source files and the disposable cache.
    pub fn directory(&self) -> PathBuf {
        self.repo.join(".robostickies")
    }

    fn checked(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let relative = Path::new(".robostickies").join(relative);
        document::safe_path(&self.repo, &relative)?;
        Ok(self.repo.join(relative))
    }

    pub(crate) fn initialized(&self) -> bool {
        self.directory().join("config.toml").is_file()
    }

    fn lock(&self, initializing: bool) -> Result<Guard> {
        self.lock_for_scope(initializing, Scope::All)
    }

    fn lock_for_scope(&self, initializing: bool, scope: Scope) -> Result<Guard> {
        let config = self.checked("config.toml")?;
        if !initializing && !config.try_exists()? {
            return Err(Error::new(
                "not_initialized",
                "Run robostickies init or call init_store first",
            ));
        }
        let cache = self.checked("cache")?;
        fs::create_dir_all(&cache)?;
        let path = self.checked("cache/store.lock")?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    let guard = Guard { _file: file };
                    // Validate while holding the same lock used by upgrades and writers.
                    self.configuration_for_scope_unlocked(scope)?;
                    return Ok(guard);
                }
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(Error::new(
                        "busy",
                        "Repository is busy; retry the operation",
                    ));
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }

    fn index(&self, force: bool) -> Result<Index> {
        self.checked("cache/index.redb")?;
        self.checked("cache/index.failed.redb")?;
        self.assert_ignored("cache/index.redb")?;
        self.assert_ignored("cache/index.failed.redb")?;
        Index::refresh(&self.directory(), force).map_err(cache_error)
    }

    fn refresh_warning(&self) -> Vec<String> {
        match self.index(false) {
            Ok(_) => vec![],
            Err(error) => vec![format!("Source saved successfully; {error}")],
        }
    }

    fn assert_untracked(&self, relative: &str) -> Result<()> {
        let path = format!(".robostickies/{relative}");
        if let Ok(output) = Command::new("git")
            .args(["ls-files", "--error-unmatch", "--", &path])
            .current_dir(&self.repo)
            .output()
            && output.status.success()
        {
            return Err(Error::new(
                "privacy",
                format!(
                    "{path} is already tracked by Git; ignore rules do not untrack it. Review its tracking before using it for private data or cache"
                ),
            ));
        }
        Ok(())
    }

    fn assert_ignored(&self, relative: &str) -> Result<()> {
        self.assert_untracked(relative)?;
        let path = format!(".robostickies/{relative}");
        if let Ok(output) = Command::new("git")
            .args(["check-ignore", "--no-index", "-q", "--", &path])
            .current_dir(&self.repo)
            .output()
            && output.status.code() == Some(1)
        {
            return Err(Error::new(
                "privacy",
                format!(
                    "{path} is not ignored by Git; rerun robostickies init to restore private/cache exclusions before saving data there"
                ),
            ));
        }
        Ok(())
    }

    fn tag_path(&self, name: &str) -> Result<String> {
        let (visibility, bare) = document::tag_name(name)?;
        Ok(format!(
            "{}tags/{bare}.md",
            if visibility == Visibility::Private {
                "private/"
            } else {
                ""
            }
        ))
    }

    fn read_tag(&self, name: &str) -> Result<Tag> {
        let path = self.checked(self.tag_path(name)?)?;
        let text = fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    "not_found",
                    format!("Undefined tag {name:?}; use tags list or create_tag"),
                )
            } else {
                e.into()
            }
        })?;
        document::parse_tag(&text, name)
    }

    fn validate_tags(&self, tags: &mut Vec<String>) -> Result<bool> {
        tags.sort();
        tags.dedup();
        let mut private = false;
        for tag in tags.iter() {
            private |= self.read_tag(tag)?.visibility == Visibility::Private;
        }
        Ok(private)
    }

    fn read_note(&self, id: &str) -> Result<(Note, String)> {
        document::note_id(id)?;
        let mut found = None;
        for (prefix, physical) in [("", Visibility::Public), ("private/", Visibility::Private)] {
            let relative = format!("{prefix}notes/{id}.md");
            let path = self.checked(&relative)?;
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if found.is_some() {
                return Err(Error::new(
                    "conflict",
                    "Multiple files have this note ID; run doctor and give manually copied notes a new UUID",
                ));
            }
            found = Some((document::parse_note(&text, &relative, physical)?, text));
        }
        found.ok_or_else(|| {
            Error::new(
                "not_found",
                "Note not found; use list or search to discover IDs",
            )
        })
    }

    fn new_note(
        &self,
        body: String,
        tags: Vec<String>,
        visibility: Visibility,
        status: NoteStatus,
    ) -> Result<Note> {
        if body.trim().is_empty() {
            return Err(Error::new("invalid_input", "A note body cannot be empty"));
        }
        let id = Uuid::now_v7().to_string();
        let time = document::now();
        let path = format!(
            "{}notes/{id}.md",
            if visibility == Visibility::Private {
                "private/"
            } else {
                ""
            }
        );
        Ok(Note {
            id,
            created_at: time.clone(),
            updated_at: time,
            tags,
            status,
            body,
            visibility,
            revision: String::new(),
            path,
            metadata: BTreeMap::new(),
        })
    }

    fn save_new(&self, note: Note) -> Result<Note> {
        let path = self.checked(&note.path)?;
        if note.visibility == Visibility::Private {
            self.assert_ignored(&note.path)?;
        }
        let text = document::render_note(&note)?;
        let note = document::parse_note(&text, &note.path, note.visibility)?;
        document::atomic_write(&path, &text, false)?;
        Ok(note)
    }

    /// Scaffold a store. Existing source files are preserved; agent integration is opt-in.
    /// Returns configuration, filesystem, or lock failures without altering Git's index.
    pub fn init(&self, options: InitOptions) -> Result<InitResult> {
        self.checked("config.toml")?;
        fs::create_dir_all(self.directory())?;
        let _guard = self.lock(true)?;
        // Install exclusions before creating private files or populating the index.
        match options.ignore_location {
            IgnoreLocation::Store => {
                let path = self.checked(".gitignore")?;
                write_block(&path, "robostickies ignores", "/private/\n/cache/\n")?;
            }
            IgnoreLocation::Local => {
                let output = Command::new("git")
                    .args([
                        "rev-parse",
                        "--path-format=absolute",
                        "--git-path",
                        "info/exclude",
                    ])
                    .current_dir(&self.repo)
                    .output()?;
                if !output.status.success() {
                    return Err(Error::new(
                        "configuration",
                        "Local excludes require a Git repository; initialize Git or use --ignore-location store",
                    ));
                }
                let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end());
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err(Error::new("io", "Refusing a symlinked Git exclude file"));
                }
                let prefix = Command::new("git")
                    .args(["rev-parse", "--show-prefix"])
                    .current_dir(&self.repo)
                    .output()?;
                if !prefix.status.success() {
                    return Err(Error::new(
                        "configuration",
                        "Cannot determine the repository-relative store path",
                    ));
                }
                let prefix = String::from_utf8_lossy(&prefix.stdout);
                let prefix = prefix.trim_end_matches(['\r', '\n']);
                let escaped: String = prefix
                    .chars()
                    .flat_map(|c| {
                        if matches!(c, '\\' | '*' | '?' | '[' | ']' | '!' | '#' | ' ') {
                            vec!['\\', c]
                        } else {
                            vec![c]
                        }
                    })
                    .collect();
                let name = if prefix.is_empty() {
                    "robostickies ignores".to_owned()
                } else {
                    format!("robostickies ignores {}", document::fingerprint(prefix))
                };
                let rules =
                    format!("/{escaped}.robostickies/private/\n/{escaped}.robostickies/cache/\n");
                write_block(&path, &name, &rules)?;
            }
        }
        let mut result = InitResult {
            created: vec![],
            agents_updated: false,
            warnings: vec![],
        };
        let new_config = format!(
            "format_version = 1\ndata_version = \"{}\"\n\n[tags]\ndefault_private = false\nprivate_patterns = []\n",
            version::CURRENT
        );
        for (relative, body) in [("config.toml", new_config.as_str()), ("README.md", GUIDE)] {
            let path = self.checked(relative)?;
            if !path.exists() {
                document::atomic_write(&path, body, false)?;
                result.created.push(format!(".robostickies/{relative}"));
            }
        }
        for directory in ["tags", "notes", "private/tags", "private/notes"] {
            fs::create_dir_all(self.checked(directory)?)?;
        }
        let defaults = self.configuration_unlocked()?;
        for (name, topic, body) in [
            (
                "papercut",
                "Problems that waste time or cause avoidable rework.",
                "Record a concrete trigger, what went wrong, its cost, and what would have helped. Capture it while context is fresh, then continue your task. Avoid speculation and duplicate reports; search when useful.\n",
            ),
            (
                "paperclip",
                "What works well and is worth reusing.",
                "Record what saved time or produced a useful result, the circumstances, and how to reuse it. Prefer concrete value over routine success or generic praise.\n",
            ),
            (
                "resolved",
                "Fixes verified in the affected environment.",
                "Keep the papercut tag, add resolved after verifying the fix in its affected environment, record evidence, then archive. For MCP fixes, install the binary and wait for the user's agent restart before verifying through the installed connection. Archived status alone does not prove resolution. Preserve the note's visibility when selecting or creating an outcome definition.\n",
            ),
            (
                "adopted",
                "Reusable guidance reviewed in scoped agent instructions.",
                "Keep the paperclip tag, add adopted after reusable guidance exists in the appropriately scoped AGENTS.md and has been reviewed, record its destination, then archive. Publish a sanitized public copy of a private observation before promoting it into tracked guidance. Keep other useful paperclips active without adopted. Preserve the note's visibility when selecting or creating an outcome definition.\n",
            ),
        ] {
            if self.checked(format!("tags/{name}.md"))?.exists()
                || self.checked(format!("private/tags/{name}.md"))?.exists()
            {
                continue;
            }
            let tag = self.create_tag_unlocked(CreateTag {
                name: name.into(),
                topic: topic.into(),
                body: body.into(),
                read_on_start: true,
                private: Some(defaults.tags.is_private(name, None)?),
            })?;
            result
                .created
                .push(format!(".robostickies/{}", self.tag_path(&tag.name)?));
        }
        if options.agents {
            let path = self.repo.join("AGENTS.md");
            document::safe_path(&self.repo, Path::new("AGENTS.md"))?;
            result.agents_updated = write_block(&path, "robostickies", &self.agents_snippet())?;
        }
        result.warnings = self.refresh_warning();
        Ok(result)
    }

    fn configuration_unlocked(&self) -> Result<Configuration> {
        document::safe_path(&self.repo, Path::new(".robostickies/private/config.toml"))?;
        config::load(&self.directory(), self.user_config.as_deref())
    }

    fn configuration_for_scope_unlocked(&self, scope: Scope) -> Result<Configuration> {
        if scope == Scope::Public {
            config::load_shared(&self.directory())
        } else {
            self.configuration_unlocked()
        }
    }

    /// Inspect effective defaults and the configuration files that supplied them.
    pub fn configuration(&self) -> Result<Configuration> {
        self.configuration_for_scope(Scope::All)
    }

    pub(crate) fn configuration_for_scope(&self, scope: Scope) -> Result<Configuration> {
        self.checked("config.toml")?;
        let _guard = if self.initialized() {
            Some(self.lock_for_scope(false, scope)?)
        } else {
            None
        };
        self.configuration_for_scope_unlocked(scope)
    }

    /// Upgrade both namespaces by migrating the shared configuration atomically.
    /// A dry run validates the same inputs without writing sources. Existing
    /// initialization never upgrades a store; repeated upgrades are idempotent.
    /// Notes, tags, guidance, and cache files are not migrated by this step.
    ///
    /// # Errors
    /// Returns `not_initialized` for an absent store, `configuration` for invalid
    /// or unsupported versions or forbidden overrides, and lock or I/O failures.
    /// Atomic replacement prevents partially written configuration files.
    pub fn upgrade(&self, dry_run: bool) -> Result<UpgradeResult> {
        let _guard = self.lock(false)?;
        let source_version = self.configuration_unlocked()?.data_version;
        let wrote = source_version.upgrade_available() && !dry_run;
        if source_version.upgrade_available() {
            let path = self.checked("config.toml")?;
            let original = fs::read_to_string(&path)?;
            let migrated = version::migrate(&original, source_version)?;
            if wrote {
                document::atomic_write(&path, &migrated, true)?;
            }
        }
        Ok(UpgradeResult {
            source_version,
            target_version: version::CURRENT,
            wrote,
            dry_run,
        })
    }

    fn create_tag_unlocked(&self, input: CreateTag) -> Result<Tag> {
        document::slug(&input.name)?;
        if input.topic.trim().is_empty() {
            return Err(Error::new("invalid_input", "Tag topic cannot be empty"));
        }
        let private = self
            .configuration_unlocked()?
            .tags
            .is_private(&input.name, input.private)?;
        let name = format!("{}{}", if private { "private:" } else { "" }, input.name);
        let relative = self.tag_path(&name)?;
        let path = self.checked(&relative)?;
        if path.exists() {
            return Err(Error::new(
                "conflict",
                format!("Tag {name:?} already exists; edit its Markdown definition directly"),
            ));
        }
        if private {
            self.assert_ignored(&relative)?;
        }
        let text = document::render_tag(&input)?;
        let tag = document::parse_tag(&text, &name)?;
        document::atomic_write(&path, &text, false)?;
        Ok(tag)
    }

    /// Create a tag in its selected namespace; existing definitions are never overwritten.
    pub fn create_tag(&self, input: CreateTag) -> Result<Tag> {
        let _guard = self.lock(false)?;
        let tag = self.create_tag_unlocked(input)?;
        // Discovery refreshes lazily, so a cache failure cannot turn a saved tag into a failed creation.
        Ok(tag)
    }

    /// Read complete guidance for one qualified tag directly from its source.
    pub fn get_tag(&self, name: &str) -> Result<Tag> {
        let _guard = self.lock(false)?;
        self.read_tag(name)
    }

    /// Discover short tag topics, optionally matching a name or topic substring.
    pub fn list_tags(&self, scope: Scope, text: Option<&str>) -> Result<TagList> {
        let _guard = self.lock_for_scope(false, scope)?;
        let index = self.index(false)?;
        let tags = index
            .tags(scope, text)
            .map_err(cache_error)?
            .iter()
            .map(TagSummary::from)
            .collect();
        Ok(TagList {
            tags,
            diagnostics: index
                .diagnostics
                .into_iter()
                .filter(|d| scope.includes(d.visibility))
                .collect(),
        })
    }

    /// Record a note, routing all private-tagged content into private storage.
    pub fn create_note(&self, mut input: CreateNote) -> Result<SavedNote> {
        let _guard = self.lock(false)?;
        let private_tag = self.validate_tags(&mut input.tags)?;
        if private_tag && input.private == Some(false) {
            return Err(Error::new(
                "privacy",
                "A private tag requires private storage; remove the tag or omit the public override",
            ));
        }
        let visibility = if private_tag || input.private == Some(true) {
            Visibility::Private
        } else {
            Visibility::Public
        };
        let note = self.save_new(self.new_note(
            input.body,
            input.tags,
            visibility,
            NoteStatus::Active,
        )?)?;
        Ok(SavedNote {
            note,
            warnings: self.refresh_warning(),
        })
    }

    /// Read one note directly, without relying on the cache. Full UUIDs are required.
    pub fn get_note(&self, id: &str) -> Result<Note> {
        let _guard = self.lock(false)?;
        self.read_note(id).map(|(note, _)| note)
    }

    /// Edit selected fields and preserve additional YAML metadata and comments.
    /// Privacy can increase here; decreasing it requires an explicit public copy.
    /// Requires the last-read revision; blank revisions return `invalid_input` and stale
    /// revisions return `conflict`. The comparison and save share the operation lock.
    pub fn update_note(&self, mut input: UpdateNote) -> Result<SavedNote> {
        let _guard = self.lock(false)?;
        let (mut note, original) = self.read_note(&input.id)?;
        check_revision(&note, &input.expected_revision)?;
        if note.visibility == Visibility::Private && input.private == Some(false) {
            return Err(Error::new(
                "privacy",
                "Use publish to create a public copy of a private note",
            ));
        }
        let old_path = note.path.clone();
        if let Some(tags) = &mut input.tags {
            let has_private = self.validate_tags(tags)?;
            if has_private && input.private == Some(false) {
                return Err(Error::new(
                    "privacy",
                    "A private tag requires private storage",
                ));
            }
            if has_private {
                note.visibility = Visibility::Private;
            }
            note.tags = tags.clone();
        }
        if let Some(body) = &input.body {
            if body.trim().is_empty() {
                return Err(Error::new("invalid_input", "A note body cannot be empty"));
            }
            note.body = body.clone();
        }
        if let Some(status) = input.status {
            note.status = status;
        }
        if input.private == Some(true) {
            note.visibility = Visibility::Private;
        }
        note.updated_at = document::now();
        note.path = format!(
            "{}notes/{}.md",
            if note.visibility == Visibility::Private {
                "private/"
            } else {
                ""
            },
            note.id
        );
        let text = document::patch_note(&original, &note, &input)?;
        let updated = document::parse_note(&text, &note.path, note.visibility)?;
        let target = self.checked(&note.path)?;
        if note.visibility == Visibility::Private {
            self.assert_ignored(&note.path)?;
        }
        if old_path != note.path {
            if target.exists() {
                return Err(Error::new(
                    "conflict",
                    "A note already exists at the private destination",
                ));
            }
            fs::create_dir_all(self.checked("private/notes")?)?;
            // Move the old public content first: new private content is never written publicly.
            fs::rename(self.checked(old_path)?, &target)?;
        }
        document::atomic_write(&target, &text, true)?;
        Ok(SavedNote {
            note: updated,
            warnings: self.refresh_warning(),
        })
    }

    /// Copy a private note into public storage, optionally returning a preview only.
    /// The original is retained. Extra metadata and private references are not copied.
    /// Both previews and copies require the last-read revision. Blank revisions return
    /// `invalid_input`; stale revisions return `conflict` without creating a copy.
    pub fn publish_note(&self, input: PublishNote) -> Result<Publication> {
        let _guard = self.lock(false)?;
        let (source, _) = self.read_note(&input.id)?;
        check_revision(&source, &input.expected_revision)?;
        if source.visibility != Visibility::Private {
            return Err(Error::new(
                "invalid_input",
                "Only private notes need publishing; this note is already public",
            ));
        }
        let mut tags = input.tags.unwrap_or_else(|| {
            source
                .tags
                .iter()
                .filter(|t| !t.starts_with("private:"))
                .cloned()
                .collect()
        });
        if self.validate_tags(&mut tags)? {
            return Err(Error::new(
                "privacy",
                "A public copy cannot have private tags",
            ));
        }
        let note = self.new_note(
            input.body.unwrap_or(source.body),
            tags,
            Visibility::Public,
            source.status,
        )?;
        let (note, warnings) = if input.dry_run {
            let text = document::render_note(&note)?;
            (
                document::parse_note(&text, &note.path, Visibility::Public)?,
                vec![],
            )
        } else {
            let note = self.save_new(note)?;
            (note, self.refresh_warning())
        };
        Ok(Publication {
            dry_run: input.dry_run,
            source_revision: source.revision,
            note,
            warnings,
        })
    }

    /// Browse indexed notes. Sources are synchronized before every query.
    pub fn list_notes(&self, query: Query) -> Result<NotePage> {
        let _guard = self.lock_for_scope(false, query.scope.unwrap_or(Scope::All))?;
        self.index(false)?.query(&query).map_err(cache_error)
    }

    /// Build complete deterministic review material. Default visibility is public.
    /// Pagination is consumed internally; malformed selected sources prevent an incomplete report.
    pub fn review(&self, mut query: Query) -> Result<Review> {
        let scope = query.scope.unwrap_or(Scope::Public);
        let _guard = self.lock_for_scope(false, scope)?;
        query.scope = Some(scope);
        if query.cursor.is_some() {
            return Err(Error::new(
                "invalid_input",
                "Review covers all matching notes; omit the pagination cursor",
            ));
        }
        let index = self.index(false)?;
        query.limit = 1000;
        let mut notes = vec![];
        let first = index.query(&query).map_err(cache_error)?;
        let diagnostics = first.diagnostics;
        if !diagnostics.is_empty() {
            return Err(Error::new(
                "invalid_document",
                "Review has invalid or unresolved source files in the selected scope; run doctor before publishing a complete report",
            ));
        }
        let total = first.total;
        notes.extend(first.notes);
        query.cursor = first.next_cursor;
        while query.cursor.is_some() {
            let page = index.query(&query).map_err(cache_error)?;
            notes.extend(page.notes);
            query.cursor = page.next_cursor;
        }
        let mut markdown = format!(
            "# Robostickies review\n\nVisibility: {}. Matching notes: {total}.\n",
            match scope {
                Scope::Public => "public",
                Scope::Private => "private",
                Scope::All => "all (local review)",
            }
        );
        for note in &notes {
            markdown.push_str(&format!(
                "\n## {}\n\n{} · {} · {}\n\n{}\n",
                note.id,
                note.created_at,
                note.status.as_str(),
                if note.tags.is_empty() {
                    "untagged".into()
                } else {
                    note.tags.join(", ")
                },
                note.body.trim_end()
            ));
        }
        Ok(Review {
            scope,
            total,
            notes,
            markdown,
            diagnostics,
        })
    }

    /// Explain capabilities, tag topics, startup guidance, and optional agent integration.
    /// This operation is available even when the repository has not been initialized.
    pub fn context(&self, scope: Scope) -> Result<Context> {
        let mut context = Context {
            initialized: self.initialized(),
            data_version: None,
            upgrade_available: false,
            repository_root: (scope != Scope::Public)
                .then(|| self.repo.to_string_lossy().into_owned()),
            guidance: GUIDE.into(),
            agents_md: self.agents_snippet(),
            tags: vec![],
            startup_tags: vec![],
            configuration: None,
            diagnostics: vec![],
        };
        if !context.initialized {
            return Ok(context);
        }
        let _guard = self.lock_for_scope(false, scope)?;
        let configuration = self.configuration_for_scope_unlocked(scope)?;
        context.data_version = Some(configuration.data_version);
        context.upgrade_available = configuration.upgrade_available;
        let index = self.index(false)?;
        let tags = index.tags(scope, None).map_err(cache_error)?;
        context.tags = tags.iter().map(TagSummary::from).collect();
        context.startup_tags = tags.into_iter().filter(|t| t.read_on_start).collect();
        context.diagnostics = index
            .diagnostics
            .into_iter()
            .filter(|d| scope.includes(d.visibility))
            .collect();
        if scope != Scope::Public {
            context.configuration = Some(configuration);
        }
        Ok(context)
    }

    /// Portable paragraph callers may place into their own AGENTS.md format.
    pub fn agents_snippet(&self) -> String {
        "At the start of work, run `robostickies context` or call the MCP `get_context` tool to read tag topics and startup guidance. Record brief, concrete friction and useful wins with the defined tags while continuing your task. See [.robostickies/README.md](.robostickies/README.md) for details. Keep private observations in the private namespace; publish an explicit public copy before including them in public review.\n".into()
    }

    /// Rebuild the entire disposable index and return source validation results.
    pub fn reindex(&self) -> Result<Health> {
        let _guard = self.lock(false)?;
        let index = self.index(true)?;
        self.health(index)
    }

    /// Validate source files and configuration. Check Git tracking and ignore rules.
    pub fn doctor(&self) -> Result<Health> {
        let _guard = self.lock(false)?;
        let index = self.index(false)?;
        self.health(index)
    }

    fn health(&self, index: Index) -> Result<Health> {
        let notes = index
            .query(&Query {
                status: StatusFilter::All,
                ..Query::default()
            })
            .map_err(cache_error)?
            .total;
        let tags = index.tags(Scope::All, None).map_err(cache_error)?.len();
        let mut diagnostics = index.diagnostics.clone();
        let configuration = self.configuration_unlocked()?;
        if let Ok(output) = Command::new("git")
            .args([
                "ls-files",
                "-z",
                "--",
                ".robostickies/private",
                ".robostickies/cache",
            ])
            .current_dir(&self.repo)
            .output()
            && output.status.success()
        {
            for path in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                diagnostics.push(Diagnostic { path: String::from_utf8_lossy(path).into_owned(), code: "tracked_private_data".into(), message: "Git is already tracking this ignored path; review and remove it from tracking before storing private data".into(), visibility: Visibility::Private });
            }
            for path in [
                ".robostickies/private/ignore-probe",
                ".robostickies/cache/ignore-probe",
            ] {
                let ignored = Command::new("git")
                    .args(["check-ignore", "-q", "--", path])
                    .current_dir(&self.repo)
                    .status()?;
                if !ignored.success() {
                    diagnostics.push(Diagnostic { path: path.into(), code: "missing_ignore".into(), message: "Private/cache paths are not ignored; rerun init to install ignore rules".into(), visibility: Visibility::Private });
                }
            }
        }
        Ok(Health {
            data_version: configuration.data_version,
            upgrade_available: configuration.upgrade_available,
            healthy: diagnostics.is_empty(),
            notes,
            tags,
            diagnostics,
        })
    }
}

fn check_revision(note: &Note, expected: &str) -> Result<()> {
    if expected.trim().is_empty() {
        return Err(Error::new(
            "invalid_input",
            "Read the note and supply its revision before editing or publishing",
        ));
    }
    if expected != note.revision {
        return Err(Error::new(
            "conflict",
            "Note changed since it was read; read it again before applying the update",
        ));
    }
    Ok(())
}

fn write_block(path: &Path, name: &str, body: &str) -> Result<bool> {
    let original = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let (start, end) = if name.starts_with("robostickies ignores") {
        (format!("# BEGIN {name}"), format!("# END {name}"))
    } else {
        (
            format!("<!-- BEGIN {name} -->"),
            format!("<!-- END {name} -->"),
        )
    };
    let block = format!("{start}\n{}{end}\n", body);
    let replacement = if let Some(begin) = original.find(&start) {
        let finish = original[begin..].find(&end).ok_or_else(|| {
            Error::new(
                "configuration",
                format!(
                    "Unclosed managed block in {}; repair its end marker",
                    path.display()
                ),
            )
        })? + begin
            + end.len();
        let finish = finish + usize::from(original[finish..].starts_with('\n'));
        format!("{}{}{}", &original[..begin], block, &original[finish..])
    } else if original.contains(&end) {
        return Err(Error::new(
            "configuration",
            "Managed end marker has no matching start",
        ));
    } else {
        format!(
            "{}{}{}",
            original,
            if original.is_empty() || original.ends_with("\n\n") {
                ""
            } else if original.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            },
            block
        )
    };
    if replacement == original {
        return Ok(false);
    }
    document::atomic_write(path, &replacement, path.exists())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_blocks_preserve_surrounding_text_and_reject_broken_markers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("AGENTS.md");
        for prefix in ["", "Team rules", "Team rules\n", "Team rules\n\n"] {
            fs::write(&path, prefix).unwrap();
            assert!(write_block(&path, "robostickies", "First\n").unwrap());
            let first = fs::read_to_string(&path).unwrap();
            assert!(first.starts_with(prefix));
            assert!(!write_block(&path, "robostickies", "First\n").unwrap());
            assert!(write_block(&path, "robostickies", "Second\n").unwrap());
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                first.replace("First", "Second")
            );
        }
        for broken in [
            "<!-- BEGIN robostickies -->\nmissing end",
            "<!-- END robostickies -->\n",
        ] {
            fs::write(&path, broken).unwrap();
            assert_eq!(
                write_block(&path, "robostickies", "replacement\n")
                    .unwrap_err()
                    .code,
                "configuration"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            write_block(&path, "robostickies", "body").unwrap_err().code,
            "io"
        );
    }

    #[test]
    fn incomplete_files_and_non_git_local_exclusions_have_typed_errors() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        fs::write(&file, "content").unwrap();
        assert_eq!(
            Store::with_user_config(&file, None).unwrap_err().code,
            "invalid_input"
        );
        let store = Store::with_user_config(temp.path(), None).unwrap();
        assert_eq!(
            store
                .init(InitOptions {
                    agents: false,
                    ignore_location: IgnoreLocation::Local
                })
                .unwrap_err()
                .code,
            "configuration"
        );
        store.init(InitOptions::default()).unwrap();
        let missing = "00000000-0000-0000-0000-000000000001";
        assert_eq!(store.get_note(missing).unwrap_err().code, "not_found");
        fs::create_dir(store.directory().join(format!("notes/{missing}.md"))).unwrap();
        assert_eq!(store.get_note(missing).unwrap_err().code, "io");
        fs::create_dir(store.directory().join("tags/broken.md")).unwrap();
        assert_eq!(store.get_tag("broken").unwrap_err().code, "io");
        assert_eq!(
            store
                .create_tag(CreateTag {
                    name: "empty".into(),
                    topic: " ".into(),
                    body: String::new(),
                    read_on_start: false,
                    private: None
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn operations_on_absent_stores_fail_without_creating_sources() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        assert!(!store.configuration().unwrap().upgrade_available);
        let create = CreateNote {
            body: "Absent store fixture".into(),
            tags: vec![],
            private: Some(true),
        };
        let errors = [
            store.create_note(create).unwrap_err(),
            store.get_note("invalid").unwrap_err(),
            store.update_note(UpdateNote::default()).unwrap_err(),
            store
                .publish_note(PublishNote {
                    id: "invalid".into(),
                    body: None,
                    tags: None,
                    dry_run: true,
                    expected_revision: "revision".into(),
                })
                .unwrap_err(),
            store.list_notes(Query::default()).unwrap_err(),
            store.review(Query::default()).unwrap_err(),
            store.get_tag("tools").unwrap_err(),
            store.list_tags(Scope::All, None).unwrap_err(),
            store
                .create_tag(CreateTag {
                    name: "tools".into(),
                    topic: "Tools".into(),
                    body: String::new(),
                    read_on_start: false,
                    private: None,
                })
                .unwrap_err(),
            store.doctor().unwrap_err(),
            store.reindex().unwrap_err(),
            store.upgrade(false).unwrap_err(),
        ];
        assert!(errors.iter().all(|e| e.code == "not_initialized"));
        assert!(!store.directory().exists());
    }

    #[test]
    fn invalid_edits_publications_and_review_cursors_do_not_change_sources() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        let tag = store
            .create_tag(CreateTag {
                name: "local".into(),
                topic: "Local".into(),
                body: "Guidance".into(),
                read_on_start: false,
                private: Some(true),
            })
            .unwrap();
        let public = store
            .create_note(CreateNote {
                body: "Public fixture".into(),
                tags: vec![],
                private: Some(false),
            })
            .unwrap()
            .note;
        let local = store
            .create_note(CreateNote {
                body: "Local fixture".into(),
                tags: vec![tag.name.clone()],
                private: Some(true),
            })
            .unwrap()
            .note;
        for patch in [
            UpdateNote {
                body: Some(" ".into()),
                ..UpdateNote::default()
            },
            UpdateNote {
                tags: Some(vec![tag.name.clone()]),
                private: Some(false),
                ..UpdateNote::default()
            },
        ] {
            assert!(
                store
                    .update_note(UpdateNote {
                        id: public.id.clone(),
                        expected_revision: public.revision.clone(),
                        ..patch
                    })
                    .is_err()
            );
            assert_eq!(
                store.get_note(&public.id).unwrap().revision,
                public.revision
            );
        }
        let request = |note: &Note, tags| PublishNote {
            id: note.id.clone(),
            body: None,
            tags,
            dry_run: false,
            expected_revision: note.revision.clone(),
        };
        assert_eq!(
            store.publish_note(request(&public, None)).unwrap_err().code,
            "invalid_input"
        );
        assert_eq!(
            store
                .publish_note(request(&local, Some(vec![tag.name])))
                .unwrap_err()
                .code,
            "privacy"
        );
        assert_eq!(
            store
                .review(Query {
                    cursor: Some("anything".into()),
                    ..Query::default()
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        assert_eq!(store.list_notes(Query::default()).unwrap().total, 2);
        assert_eq!(store.get_note(&local.id).unwrap().revision, local.revision);
    }
}

#[cfg(test)]
mod input_failure_tests {
    use super::*;

    #[test]
    fn initialized_operations_reject_invalid_ids_tags_revisions_and_empty_publications() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(temp.path(), None).unwrap();
        store.init(InitOptions::default()).unwrap();
        assert_eq!(
            store
                .create_note(CreateNote {
                    body: " ".into(),
                    tags: vec![],
                    private: Some(true)
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        assert_eq!(
            store
                .create_note(CreateNote {
                    body: "Rejected tag fixture".into(),
                    tags: vec!["undefined".into()],
                    private: Some(true)
                })
                .unwrap_err()
                .code,
            "not_found"
        );
        assert_eq!(
            store
                .create_tag(CreateTag {
                    name: "Invalid".into(),
                    topic: "Tools".into(),
                    body: String::new(),
                    read_on_start: false,
                    private: None
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        let note = store
            .create_note(CreateNote {
                body: "Guarded fixture".into(),
                tags: vec![],
                private: Some(true),
            })
            .unwrap()
            .note;
        let before = fs::read(store.directory().join(&note.path)).unwrap();
        for (id, revision, code) in [
            ("invalid", "revision", "invalid_input"),
            (
                "00000000-0000-0000-0000-000000000001",
                "revision",
                "not_found",
            ),
            (note.id.as_str(), " ", "invalid_input"),
            (note.id.as_str(), "stale", "conflict"),
        ] {
            assert_eq!(
                store
                    .update_note(UpdateNote {
                        id: id.into(),
                        expected_revision: revision.into(),
                        ..UpdateNote::default()
                    })
                    .unwrap_err()
                    .code,
                code
            );
            assert_eq!(
                store
                    .publish_note(PublishNote {
                        id: id.into(),
                        expected_revision: revision.into(),
                        dry_run: true,
                        body: None,
                        tags: None
                    })
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert_eq!(
            store
                .update_note(UpdateNote {
                    id: note.id.clone(),
                    expected_revision: note.revision.clone(),
                    tags: Some(vec!["undefined".into()]),
                    ..UpdateNote::default()
                })
                .unwrap_err()
                .code,
            "not_found"
        );
        for (body, tags, code) in [
            (Some(" ".into()), None, "invalid_input"),
            (None, Some(vec!["undefined".into()]), "not_found"),
        ] {
            assert_eq!(
                store
                    .publish_note(PublishNote {
                        id: note.id.clone(),
                        expected_revision: note.revision.clone(),
                        dry_run: true,
                        body,
                        tags
                    })
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert_eq!(
            store
                .review(Query {
                    until: Some("invalid".into()),
                    ..Query::default()
                })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        assert_eq!(
            fs::read(store.directory().join(&note.path)).unwrap(),
            before
        );
        let index = store.directory().join("cache/index.redb");
        fs::remove_file(&index).unwrap();
        fs::create_dir(&index).unwrap();
        assert_eq!(store.list_tags(Scope::All, None).unwrap_err().code, "index");
        assert_eq!(store.review(Query::default()).unwrap_err().code, "index");
        assert_eq!(store.context(Scope::All).unwrap_err().code, "index");
        assert_eq!(
            fs::read(store.directory().join(&note.path)).unwrap(),
            before
        );
    }
}

#[cfg(all(test, unix))]
mod filesystem_failure_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn failed_initialization_and_cache_hazards_preserve_existing_files() {
        let readonly = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(readonly.path(), None).unwrap();
        let permissions = fs::metadata(readonly.path()).unwrap().permissions();
        fs::set_permissions(readonly.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let failed = store.init(InitOptions::default());
        fs::set_permissions(readonly.path(), permissions).unwrap();
        assert_eq!(failed.unwrap_err().code, "io");
        assert!(!store.directory().exists());

        fs::create_dir_all(store.directory().join(".gitignore")).unwrap();
        assert_eq!(store.init(InitOptions::default()).unwrap_err().code, "io");
        assert!(!store.directory().join("config.toml").exists());
        fs::remove_dir(store.directory().join(".gitignore")).unwrap();
        store.init(InitOptions::default()).unwrap();
        let agents = readonly.path().join("AGENTS.md");
        let broken = "<!-- BEGIN robostickies -->\nKeep this incomplete block.\n";
        fs::write(&agents, broken).unwrap();
        assert_eq!(
            store
                .init(InitOptions {
                    agents: true,
                    ..InitOptions::default()
                })
                .unwrap_err()
                .code,
            "configuration"
        );
        assert_eq!(fs::read_to_string(&agents).unwrap(), broken);
        let note = store
            .create_note(CreateNote {
                body: "Filesystem fixture".into(),
                tags: vec![],
                private: Some(true),
            })
            .unwrap()
            .note;
        let lock = store.directory().join("cache/store.lock");
        let permissions = fs::metadata(&lock).unwrap().permissions();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o444)).unwrap();
        let failed = store.doctor();
        fs::set_permissions(&lock, permissions).unwrap();
        assert_eq!(failed.unwrap_err().code, "io");
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "Outside fixture").unwrap();
        symlink(
            outside.path(),
            store.directory().join("cache/index.failed.redb"),
        )
        .unwrap();
        assert_eq!(store.list_notes(Query::default()).unwrap_err().code, "io");
        assert_eq!(store.get_note(&note.id).unwrap().revision, note.revision);
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            "Outside fixture"
        );

        let linked = tempfile::tempdir().unwrap();
        let store = Store::with_user_config(linked.path(), None).unwrap();
        fs::create_dir(store.directory()).unwrap();
        symlink(outside.path(), store.directory().join("config.toml")).unwrap();
        assert_eq!(store.init(InitOptions::default()).unwrap_err().code, "io");
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            "Outside fixture"
        );
    }
}
