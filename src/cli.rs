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
        } => {
            let body = text.read()?.ok_or_else(|| {
                Error::new(
                    "invalid_input",
                    "Supply note text, - for stdin, or --file <path>",
                )
            })?;
            emit(
                store.create_note(CreateNote {
                    body,
                    body_format,
                    tags,
                    private: visibility.value(),
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
        } => emit(
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
            })?,
            json,
            saved_note,
        ),
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
        } => emit(
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
        ),
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
