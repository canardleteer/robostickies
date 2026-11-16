//! Canonical clap CLI parsing and presentation over the shared repository API.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::{Store, model::*};

const TAG_HELP: &str = r#"Tag fields:
  NAME           Required tag name used with --tag, e.g. fresh-install.
  --description  Optional short explanation shown by tags list.
  BODY           Optional Markdown guidance on when and how to use the tag.

Examples:
  robostickies tags create fresh-install
  robostickies tags create onboarding --description "Setup and first-run checks" \
    "Record setup steps, missing tools, and first-run errors."
  robostickies add "Setup required an extra PATH entry" --tag fresh-install

Self-explanatory tags usually do not need a description.
Use tags get onboarding to read the description and guidance.
Name-only creation in a legacy store requires an explicit robostickies upgrade."#;

/// Repository notes for humans and agents.
#[derive(Debug, Parser)]
#[command(version, long_version = crate::build_info::long_version(), about, arg_required_else_help = true)]
pub struct Cli {
    /// Directory override; defaults to the Git root containing the startup CWD,
    /// or that CWD outside Git.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// Emit structured JSON instead of human-readable output.
    #[arg(long, global = true)]
    json: bool,
    /// Override the platform-standard user configuration file.
    #[arg(long, global = true, conflicts_with = "no_user_config")]
    user_config: Option<PathBuf>,
    /// Disable user configuration for reproducible automation.
    #[arg(long, global = true)]
    no_user_config: bool,
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    /// Whether error output should be structured JSON.
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scaffold .robostickies without overwriting existing notes or tags.
    Init {
        /// Add an idempotent pointer to the repository's existing AGENTS.md format.
        #[arg(long)]
        agents: bool,
        #[arg(long, value_enum, default_value = "store")]
        ignore_location: IgnoreLocation,
    },
    /// Record nonempty UTF-8 text. Markdown or plain text is preferred.
    Add {
        #[command(flatten)]
        text: TextInput,
        /// Advisory body hint: markdown, text, yaml, or another nonblank string.
        #[arg(long)]
        body_format: Option<String>,
        /// Defined tag name, e.g. fresh-install; repeat for multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[command(flatten)]
        visibility: VisibilityArgs,
        #[command(flatten)]
        classification: ClassificationArgs,
    },
    /// Read a note by its full UUID.
    Get { id: String },
    /// Edit selected note fields; unmentioned fields remain unchanged.
    Update {
        id: String,
        #[command(flatten)]
        text: TextInput,
        /// Set an advisory body hint; omitted hints remain unchanged.
        #[arg(long, conflicts_with = "clear_body_format")]
        body_format: Option<String>,
        /// Remove an existing string body hint.
        #[arg(long)]
        clear_body_format: bool,
        /// Replace all tags with this set; repeat the flag for multiple tags.
        #[arg(long = "tag", conflicts_with = "clear_tags")]
        tags: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
        #[arg(long, value_enum)]
        status: Option<NoteStatus>,
        #[command(flatten)]
        visibility: VisibilityArgs,
        /// Required last-read revision; re-read and reconcile after a conflict.
        #[arg(long)]
        if_match: String,
        #[command(flatten)]
        classification: ClassificationArgs,
        /// Remove task metadata after reconciling incoming references.
        #[arg(long, conflicts_with_all = ["task_options", "sheet_options", "clear_sheet"])]
        clear_task: bool,
        /// Remove saved sheet metadata.
        #[arg(long, conflicts_with_all = ["task_options", "sheet_options"])]
        clear_sheet: bool,
    },
    /// Archive a resolved or obsolete note.
    Archive {
        id: String,
        /// Required last-read revision.
        #[arg(long)]
        if_match: String,
    },
    /// Restore an archived note to active status.
    Unarchive {
        id: String,
        /// Required last-read revision.
        #[arg(long)]
        if_match: String,
    },
    /// Browse notes with optional filters.
    List {
        #[command(flatten)]
        filters: Filters,
        #[arg(long)]
        text: Option<String>,
    },
    /// Search note bodies for a case-insensitive literal substring.
    Search {
        text: String,
        #[command(flatten)]
        filters: Filters,
    },
    /// Produce complete review material; visibility defaults to public.
    Review {
        #[command(flatten)]
        filters: ReviewFilters,
        #[arg(long)]
        text: Option<String>,
    },
    /// Define tags for labeling notes, with optional descriptions and guidance.
    #[command(after_help = TAG_HELP)]
    Tags {
        #[command(subcommand)]
        command: TagCommand,
    },
    /// Discover task state, readiness, prerequisites, and deferral.
    Tasks {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Read saved task sheets in authored order.
    Sheets {
        #[command(subcommand)]
        command: SheetCommand,
    },
    /// Copy a private note into public storage, retaining the original.
    Publish {
        id: String,
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        text: TextInput,
        /// Set a public hint explicitly; the private source hint is never copied.
        #[arg(long)]
        body_format: Option<String>,
        /// Replace the public tag set; private tags are forbidden.
        #[arg(long = "tag", conflicts_with = "clear_tags")]
        tags: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
        /// Required source revision, including for --dry-run.
        #[arg(long)]
        if_match: String,
        #[command(flatten)]
        classification: ClassificationArgs,
    },
    /// Read session guidance, tag names and optional descriptions, startup details, and an AGENTS.md snippet.
    Context {
        #[arg(long, value_enum, default_value = "all")]
        scope: Scope,
    },
    /// Inspect effective configuration and its source files.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Validate source files and Git privacy protections.
    Doctor,
    /// Rebuild the disposable redb index from Markdown sources.
    Reindex,
    /// Upgrade the shared store data version, preserving notes and guidance.
    Upgrade {
        /// Validate and preview the migration without changing source files.
        #[arg(long)]
        dry_run: bool,
        /// Decline this optional target until the recommended target changes.
        #[arg(long, conflicts_with_all = ["dry_run", "keep_snapshot"])]
        defer: bool,
        /// Retain the private recovery snapshot after a successful upgrade.
        #[arg(long)]
        keep_snapshot: bool,
    },
    /// Serve MCP over stdio; stdout is exclusively protocol data.
    ///
    /// Existing servers keep their compiled build after installation. Restart or
    /// reconnect the client, then call the MCP version tool to identify that server.
    Mcp,
}

impl Command {
    fn read_scope(&self) -> Scope {
        match self {
            Self::Context { scope } => *scope,
            Self::List { filters, .. } | Self::Search { filters, .. } => {
                filters.review.scope.unwrap_or(Scope::All)
            }
            Self::Review { filters, .. } => filters.scope.unwrap_or(Scope::Public),
            Self::Tasks {
                command: TaskCommand::List { filters, .. },
            } => filters.review.scope.unwrap_or(Scope::All),
            Self::Sheets {
                command: SheetCommand::Render { scope, .. },
            } => *scope,
            Self::Tags {
                command: TagCommand::List { scope, .. },
            } => *scope,
            _ => Scope::All,
        }
    }
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    /// Define a tag name, with an optional description and usage guidance.
    #[command(after_help = TAG_HELP)]
    Create {
        /// Tag name used by --tag, e.g. fresh-install.
        name: String,
        /// Optional catalog explanation; self-explanatory tags usually need none.
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        text: TextInput,
        #[arg(long)]
        read_on_start: bool,
        #[command(flatten)]
        visibility: VisibilityArgs,
    },
    /// List tag names and any descriptions.
    List {
        #[arg(long, value_enum, default_value = "all")]
        scope: Scope,
        #[arg(long)]
        text: Option<String>,
    },
    /// Read a tag's optional description and usage guidance.
    Get {
        /// Identifier, qualified as private:<name> for private definitions.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Debug, Args)]
#[group(id = "task_options", multiple = true, conflicts_with = "sheet_options")]
struct TaskArgs {
    /// Classify as a task (new tasks default to todo).
    #[arg(long)]
    task: bool,
    #[arg(long, value_enum)]
    task_state: Option<TaskState>,
    /// Replace prerequisites with these task UUIDs; repeat for branching dependencies.
    #[arg(long, conflicts_with = "clear_requires")]
    requires: Vec<String>,
    #[arg(long)]
    clear_requires: bool,
    /// Wait until explicitly resumed.
    #[arg(long, conflicts_with_all = ["defer_until", "resume_task"])]
    defer: bool,
    /// Wait until a UTC date or RFC3339 instant.
    #[arg(long, conflicts_with = "resume_task")]
    defer_until: Option<String>,
    #[arg(long)]
    resume_task: bool,
}

#[derive(Debug, Args)]
#[group(id = "sheet_options", multiple = true)]
struct SheetArgs {
    /// Classify as a saved task sheet.
    #[arg(long)]
    sheet: bool,
    /// Replace authored membership with these task UUIDs, in this order.
    #[arg(long = "item", conflicts_with = "clear_items")]
    items: Vec<String>,
    #[arg(long)]
    clear_items: bool,
    #[arg(long, value_enum)]
    ordering: Option<SheetOrdering>,
}

#[derive(Debug, Args)]
struct ClassificationArgs {
    #[command(flatten)]
    task: TaskArgs,
    #[command(flatten)]
    sheet: SheetArgs,
}

impl ClassificationArgs {
    fn patches(self) -> (Option<TaskPatch>, Option<SheetPatch>) {
        let t = self.task;
        let s = self.sheet;
        let task = (t.task
            || t.task_state.is_some()
            || !t.requires.is_empty()
            || t.clear_requires
            || t.defer
            || t.defer_until.is_some()
            || t.resume_task)
            .then(|| TaskPatch {
                state: t.task_state,
                requires: (t.clear_requires || !t.requires.is_empty()).then_some(t.requires),
                deferral: if t.defer {
                    Some(Deferral::Manual)
                } else {
                    t.defer_until.map(|until| Deferral::Until { until })
                },
                clear_deferral: t.resume_task,
            });
        let sheet =
            (s.sheet || !s.items.is_empty() || s.clear_items || s.ordering.is_some()).then(|| {
                SheetPatch {
                    items: (s.clear_items || !s.items.is_empty()).then_some(s.items),
                    ordering: s.ordering,
                }
            });
        (task, sheet)
    }

    fn specs(self) -> (Option<TaskSpec>, Option<SheetSpec>) {
        let (task, sheet) = self.patches();
        (
            task.map(|patch| TaskSpec {
                state: patch.state.unwrap_or_default(),
                requires: patch.requires.unwrap_or_default(),
                deferral: patch.deferral,
            }),
            sheet.map(|patch| SheetSpec {
                items: patch.items.unwrap_or_default(),
                ordering: patch.ordering.unwrap_or_default(),
            }),
        )
    }
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// List active unfinished tasks by default. Filters do not hide prerequisite evidence.
    List {
        #[command(flatten)]
        filters: Filters,
        #[arg(long)]
        text: Option<String>,
        #[arg(long = "state", value_enum, conflicts_with = "all_task_states")]
        states: Vec<TaskState>,
        #[arg(long)]
        all_task_states: bool,
        #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
        ready: Option<bool>,
        #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
        blocked: Option<bool>,
        #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
        deferred: Option<bool>,
    },
}

#[derive(Debug, Subcommand)]
enum SheetCommand {
    /// Render a saved sheet as read-only Markdown; defaults to public scope.
    Render {
        id: String,
        #[arg(long, value_enum, default_value = "public")]
        scope: Scope,
    },
}

#[derive(Debug, Args)]
struct TextInput {
    /// UTF-8 text, or - to read stdin. Use Markdown for tag guidance.
    body: Option<String>,
    /// Read UTF-8 text from a file, or - for stdin.
    #[arg(long, conflicts_with = "body")]
    file: Option<PathBuf>,
}

impl TextInput {
    fn read(self) -> Result<Option<String>> {
        if self.body.as_deref() == Some("-")
            || self.file.as_deref() == Some(std::path::Path::new("-"))
        {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            return Ok(Some(text));
        }
        if let Some(path) = self.file {
            return std::fs::read_to_string(&path)
                .map(Some)
                .map_err(|e| Error::new("io", format!("Cannot read {}: {e}", path.display())));
        }
        Ok(self.body)
    }
}

#[derive(Debug, Args)]
struct VisibilityArgs {
    /// Keep the new tag or note in ignored private storage.
    #[arg(long, conflicts_with = "public")]
    private: bool,
    /// Select public creation; private note tags still require private storage.
    #[arg(long, alias = "shared")]
    public: bool,
}

impl VisibilityArgs {
    fn value(&self) -> Option<bool> {
        if self.private {
            Some(true)
        } else if self.public {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
struct Filters {
    #[command(flatten)]
    review: ReviewFilters,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
struct ReviewFilters {
    #[arg(long, value_enum)]
    scope: Option<Scope>,
    /// Require every specified tag.
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long = "exclude-tag")]
    exclude_tags: Vec<String>,
    #[arg(long, value_enum, default_value = "active")]
    status: StatusFilter,
    /// Inclusive creation-date bound (UTC YYYY-MM-DD or RFC3339).
    #[arg(long)]
    since: Option<String>,
    /// Exclusive creation-date bound (UTC YYYY-MM-DD or RFC3339).
    #[arg(long)]
    until: Option<String>,
}

impl Filters {
    fn query(self, text: Option<String>) -> Query {
        Query {
            limit: self.limit,
            cursor: self.cursor,
            ..self.review.query(text).into()
        }
    }
}

impl ReviewFilters {
    fn query(self, text: Option<String>) -> ReviewQuery {
        ReviewQuery {
            scope: self.scope,
            tags: self.tags,
            exclude_tags: self.exclude_tags,
            status: self.status,
            text,
            since: self.since,
            until: self.until,
        }
    }
}

/// Execute a parsed CLI request. Filesystem behavior lives in [`Store`].
pub async fn run(cli: Cli) -> Result<()> {
    let discovered = Store::discover(cli.repo.as_deref())?;
    let store = if cli.no_user_config || cli.user_config.is_some() {
        Store::with_user_config(discovered.repository(), cli.user_config)?
    } else {
        discovered
    };
    let json = cli.json;
    let (notice, local_warnings) =
        if !matches!(cli.command, Command::Mcp | Command::Upgrade { .. }) && store.initialized() {
            let config = store.configuration_for_scope(cli.command.read_scope())?;
            (
                config.upgrade.reminder().then_some(config.upgrade),
                config.warnings,
            )
        } else {
            (None, vec![])
        };
    let result = match cli.command {
        Command::Mcp => crate::mcp::serve(store).await,
        Command::Upgrade {
            dry_run,
            defer,
            keep_snapshot,
        } => {
            let result = store.upgrade_with_options(UpgradeOptions {
                dry_run,
                defer,
                keep_snapshot,
            })?;
            if let Some(snapshot) = &result.snapshot {
                eprintln!(
                    "Snapshot: {} ({})",
                    snapshot.path,
                    if snapshot.retained {
                        "retained"
                    } else {
                        "removed after validation"
                    }
                );
            }
            for warning in &result.warnings {
                eprintln!("Warning: {warning}");
            }
            emit(result, json, |result| {
                format!(
                    "{} -> {}: {}",
                    result.source_version,
                    result.target_version,
                    if result.deferred {
                        "upgrade deferred; sources unchanged"
                    } else if result.dry_run {
                        "dry run; no sources changed"
                    } else if result.wrote {
                        "upgraded"
                    } else {
                        "already current"
                    }
                )
            })
        }
        Command::Init {
            agents,
            ignore_location,
        } => emit(
            store.init(InitOptions {
                agents,
                ignore_location,
            })?,
            json,
            |result| {
                format!(
                    "Initialized .robostickies\n{}{}",
                    result.created.join("\n"),
                    warnings(&result.warnings)
                )
            },
        ),
        Command::Add {
            text,
            body_format,
            tags,
            visibility,
            classification,
        } => {
            let body = text.read()?.ok_or_else(|| {
                Error::new(
                    "invalid_input",
                    "Supply note text, - for stdin, or --file <path>",
                )
            })?;
            let (task, sheet) = classification.specs();
            emit(
                store.create_note(CreateNote {
                    body,
                    body_format,
                    tags,
                    private: visibility.value(),
                    task,
                    sheet,
                })?,
                json,
                saved_note,
            )
        }
        Command::Get { id } => emit(store.get_note(&id)?, json, format_note),
        Command::Update {
            id,
            text,
            body_format,
            clear_body_format,
            tags,
            clear_tags,
            status,
            visibility,
            if_match,
            classification,
            clear_task,
            clear_sheet,
        } => {
            let (task, sheet) = classification.patches();
            emit(
                store.update_note(UpdateNote {
                    id,
                    body: text.read()?,
                    body_format,
                    clear_body_format,
                    tags: if clear_tags || !tags.is_empty() {
                        Some(tags)
                    } else {
                        None
                    },
                    status,
                    private: visibility.value(),
                    expected_revision: if_match,
                    task,
                    sheet,
                    clear_task,
                    clear_sheet,
                })?,
                json,
                saved_note,
            )
        }
        Command::Archive { id, if_match } => emit(
            store.update_note(UpdateNote {
                id,
                status: Some(NoteStatus::Archived),
                expected_revision: if_match,
                ..UpdateNote::default()
            })?,
            json,
            saved_note,
        ),
        Command::Unarchive { id, if_match } => emit(
            store.update_note(UpdateNote {
                id,
                status: Some(NoteStatus::Active),
                expected_revision: if_match,
                ..UpdateNote::default()
            })?,
            json,
            saved_note,
        ),
        Command::List { filters, text } => {
            emit(store.list_notes(filters.query(text))?, json, format_page)
        }
        Command::Search { filters, text } => emit(
            store.list_notes(filters.query(Some(text)))?,
            json,
            format_page,
        ),
        Command::Tasks {
            command:
                TaskCommand::List {
                    filters,
                    text,
                    states,
                    all_task_states,
                    ready,
                    blocked,
                    deferred,
                },
        } => emit(
            store.list_tasks(TaskQuery {
                notes: filters.query(text),
                states,
                all_task_states,
                ready,
                blocked,
                deferred,
            })?,
            json,
            |page| {
                let mut text = format!(
                    "Tasks: {}. Evaluated at: {}.\n",
                    page.total, page.evaluated_at
                );
                for task in &page.tasks {
                    text.push_str(&format!(
                        "\n{} [{}; ready={}; blocked={}; deferred={}]\n{}\n",
                        task.note.id,
                        task.task.state.as_str(),
                        task.ready,
                        task.blocked,
                        task.active_deferral.is_some(),
                        task.note.body
                    ));
                    for blocker in &task.blockers {
                        text.push_str(&format!(
                            "  {}: {}\n",
                            blocker.reason,
                            blocker.path.join(" -> ")
                        ));
                    }
                }
                for diagnostic in &page.diagnostics {
                    text.push_str(&format!("\n{}: {}\n", diagnostic.path, diagnostic.message));
                }
                if let Some(cursor) = &page.next_cursor {
                    text.push_str(&format!("\nNext cursor: {cursor}\n"));
                }
                text
            },
        ),
        Command::Sheets {
            command: SheetCommand::Render { id, scope },
        } => emit(
            store.render_task_sheet(RenderTaskSheet {
                id,
                scope: Some(scope),
            })?,
            json,
            |sheet| sheet.markdown.clone(),
        ),
        Command::Review { filters, text } => {
            emit(store.review(filters.query(text).into())?, json, |review| {
                review.markdown.clone()
            })
        }
        Command::Tags { command } => match command {
            TagCommand::Create {
                name,
                description,
                text,
                read_on_start,
                visibility,
            } => emit(
                store.create_tag(CreateTag {
                    name,
                    description,
                    body: text.read()?.unwrap_or_default(),
                    read_on_start,
                    private: visibility.value(),
                })?,
                json,
                |tag| {
                    format!(
                        "Created tag {}\n",
                        tag_label(&tag.name, tag.description.as_deref())
                    )
                },
            ),
            TagCommand::List { scope, text } => {
                emit(store.list_tags(scope, text.as_deref())?, json, |list| {
                    let mut text = list
                        .tags
                        .iter()
                        .map(|tag| {
                            let name = format!(
                                "{}{}",
                                tag.name,
                                if tag.read_on_start {
                                    " [read on start]"
                                } else {
                                    ""
                                }
                            );
                            tag_label(&name, tag.description.as_deref())
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    for diagnostic in &list.diagnostics {
                        text.push_str(&format!("\n{}: {}", diagnostic.path, diagnostic.message));
                    }
                    text
                })
            }
            TagCommand::Get { name } => emit(store.get_tag(&name)?, json, |tag| {
                let mut text = format!("{}\n", tag_label(&tag.name, tag.description.as_deref()));
                if !tag.body.is_empty() {
                    text.push_str(&format!("\n{}", tag.body));
                }
                text
            }),
        },
        Command::Publish {
            id,
            dry_run,
            text,
            body_format,
            tags,
            clear_tags,
            if_match,
            classification,
        } => {
            let (task, sheet) = classification.specs();
            emit(
                store.publish_note(PublishNote {
                    id,
                    dry_run,
                    body: text.read()?,
                    body_format,
                    tags: if clear_tags || !tags.is_empty() {
                        Some(tags)
                    } else {
                        None
                    },
                    expected_revision: if_match,
                    task,
                    sheet,
                })?,
                json,
                |result| {
                    format!(
                        "{}\n{}{}",
                        if result.dry_run {
                            "Preview (no public copy saved)"
                        } else {
                            "Published public copy"
                        },
                        format_note(&result.note),
                        warnings(&result.warnings)
                    )
                },
            )
        }
        Command::Context { scope } => emit(store.context(scope)?, json, |context| {
            let mut text = context.guidance.clone();
            if let Some(root) = &context.repository_root {
                text.push_str(&format!("\nRepository: {root}\n"));
            }
            if !context.initialized {
                text.push_str("\nNot initialized. Run robostickies init or call init_store.\n");
            }
            if let Some(advice) = &context.upgrade
                && advice.requirement == crate::version::UpgradeRequirement::Must
            {
                text.push_str(&format!("\n{}\n", advice.message));
            }
            text.push_str("\n## Available tags\n\n");
            for tag in &context.tags {
                text.push_str(&format!(
                    "- {}\n",
                    tag_label(&tag.name, tag.description.as_deref())
                ));
            }
            for tag in &context.startup_tags {
                text.push_str(&format!("\n## {}\n\n{}\n", tag.name, tag.body));
            }
            text.push_str(&format!(
                "\n## Optional AGENTS.md paragraph\n\n{}",
                context.agents_md
            ));
            for diagnostic in &context.diagnostics {
                text.push_str(&format!("\n{}: {}\n", diagnostic.path, diagnostic.message));
            }
            text
        }),
        Command::Config {
            command: ConfigCommand::Show,
        } => emit(store.configuration()?, json, |config| {
            serde_json::to_string_pretty(config).unwrap_or_default()
        }),
        Command::Doctor | Command::Reindex => {
            let health = if matches!(cli.command, Command::Reindex) {
                store.reindex()?
            } else {
                store.doctor()?
            };
            let healthy = health.healthy;
            emit(health, json, |health| {
                let mut text = format!(
                    "{} notes, {} tags: {}\n",
                    health.notes,
                    health.tags,
                    if health.healthy {
                        "healthy"
                    } else {
                        "problems found"
                    }
                );
                if health.upgrade.requirement == crate::version::UpgradeRequirement::Must {
                    text.push_str(&format!("{}\n", health.upgrade.message));
                }
                for diagnostic in &health.diagnostics {
                    text.push_str(&format!(
                        "{} [{}]: {}\n",
                        diagnostic.path, diagnostic.code, diagnostic.message
                    ));
                }
                text
            })?;
            if healthy {
                Ok(())
            } else {
                Err(Error::new(
                    "invalid_document",
                    "Validation found problems; see the report",
                ))
            }
        }
    };
    if result.is_ok()
        && let Some(advice) = notice
    {
        eprintln!("Notice: {}", advice.message);
    }
    if result.is_ok() {
        for warning in local_warnings {
            eprintln!("Warning: {warning}");
        }
    }
    result
}

fn emit<T: Serialize>(value: T, json: bool, render: impl FnOnce(&T) -> String) -> Result<()> {
    let text = if json {
        serde_json::to_string(&value).map_err(|e| Error::new("internal", e.to_string()))?
    } else {
        render(&value)
    };
    let mut output = io::stdout().lock();
    writeln!(output, "{}", text.trim_end())?;
    Ok(())
}

fn tag_label(name: &str, description: Option<&str>) -> String {
    match description {
        Some(description) => format!("{name} — {description}"),
        None => name.into(),
    }
}

fn format_note(note: &Note) -> String {
    format!(
        "{} [{}; {}]\nTags: {}\nRevision: {}\n\n{}",
        note.id,
        note.visibility,
        note.status.as_str(),
        if note.tags.is_empty() {
            "untagged".into()
        } else {
            note.tags.join(", ")
        },
        note.revision,
        note.body
    )
}

fn warnings(warnings: &[String]) -> String {
    warnings
        .iter()
        .map(|w| format!("\nWarning: {w}\n"))
        .collect()
}
fn saved_note(result: &SavedNote) -> String {
    format!(
        "{}{}",
        format_note(&result.note),
        warnings(&result.warnings)
    )
}
fn format_page(page: &NotePage) -> String {
    let mut text = format!("{} matching notes\n", page.total);
    for note in &page.notes {
        text.push_str(&format!(
            "{} [{}] {} — {}\n",
            note.id,
            note.visibility,
            note.tags.join(","),
            note.body.lines().next().unwrap_or_default()
        ));
    }
    if let Some(cursor) = &page.next_cursor {
        text.push_str(&format!("Next cursor: {cursor}\n"));
    }
    for diagnostic in &page.diagnostics {
        text.push_str(&format!("{}: {}\n", diagnostic.path, diagnostic.message));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn root_parser_builds() {
        Cli::command().debug_assert();
    }

    #[test]
    fn visibility_overrides_conflict() {
        assert!(
            Cli::try_parse_from(["robostickies", "add", "note", "--private", "--public"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "robostickies",
                "tags",
                "create",
                "tooling",
                "--description",
                "Tools",
                "--private",
                "--shared"
            ])
            .is_err()
        );
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn serialization_failure_is_reported_before_writing_stdout() {
        struct Fails;
        impl Serialize for Fails {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("fixture encoding failure"))
            }
        }
        fn render(_: &Fails) -> String {
            "fixture rendered".into()
        }
        assert_eq!(emit(Fails, true, render).unwrap_err().code, "internal");
        emit(Fails, false, render).unwrap();
    }
}
