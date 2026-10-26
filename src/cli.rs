//! Canonical clap CLI parsing and presentation over the shared repository API.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::{Store, model::*};

/// Repository notes for humans and agents.
#[derive(Debug, Parser)]
#[command(version, about, arg_required_else_help = true)]
pub struct Cli {
    /// Repository root; otherwise discover the Git root from the current directory.
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
    /// Record a brief note. Pass - for stdin, or --file for Markdown text.
    Add {
        #[command(flatten)]
        text: TextInput,
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
        /// Replace all tags with this set; repeat the flag for multiple tags.
        #[arg(long = "tag", conflicts_with = "clear_tags")]
        tags: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
        #[arg(long, value_enum)]
        status: Option<NoteStatus>,
        #[command(flatten)]
        visibility: VisibilityArgs,
        /// Refuse the update unless this revision is still current.
        #[arg(long)]
        if_match: Option<String>,
    },
    /// Archive a resolved or obsolete note.
    Archive {
        id: String,
        #[arg(long)]
        if_match: Option<String>,
    },
    /// Restore an archived note to active status.
    Unarchive {
        id: String,
        #[arg(long)]
        if_match: Option<String>,
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
        filters: Filters,
        #[arg(long)]
        text: Option<String>,
    },
    /// Define tags or discover their short topics and complete guidance.
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
        /// Replace the public tag set; private tags are forbidden.
        #[arg(long = "tag", conflicts_with = "clear_tags")]
        tags: Vec<String>,
        #[arg(long)]
        clear_tags: bool,
        #[arg(long)]
        if_match: Option<String>,
    },
    /// Read session guidance, tag topics, startup details, and an AGENTS.md snippet.
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
    /// Serve MCP over stdio; stdout is exclusively protocol data.
    Mcp,
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    Create {
        name: String,
        #[arg(long)]
        topic: String,
        #[command(flatten)]
        text: TextInput,
        #[arg(long)]
        read_on_start: bool,
        #[command(flatten)]
        visibility: VisibilityArgs,
    },
    List {
        #[arg(long, value_enum, default_value = "all")]
        scope: Scope,
        #[arg(long)]
        text: Option<String>,
    },
    Get {
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Debug, Args)]
struct TextInput {
    /// Markdown text, or - to read stdin.
    body: Option<String>,
    /// Read Markdown from a UTF-8 file, or - for stdin.
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
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

impl Filters {
    fn query(self, text: Option<String>) -> Query {
        Query {
            scope: self.scope,
            tags: self.tags,
            exclude_tags: self.exclude_tags,
            status: self.status,
            text,
            since: self.since,
            until: self.until,
            limit: self.limit,
            cursor: self.cursor,
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
    match cli.command {
        Command::Mcp => crate::mcp::serve(store).await,
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
            tags,
            clear_tags,
            status,
            visibility,
            if_match,
        } => emit(
            store.update_note(UpdateNote {
                id,
                body: text.read()?,
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
            emit(store.review(filters.query(text))?, json, |review| {
                review.markdown.clone()
            })
        }
        Command::Tags { command } => match command {
            TagCommand::Create {
                name,
                topic,
                text,
                read_on_start,
                visibility,
            } => emit(
                store.create_tag(CreateTag {
                    name,
                    topic,
                    body: text.read()?.unwrap_or_default(),
                    read_on_start,
                    private: visibility.value(),
                })?,
                json,
                |tag| format!("Created tag {}\n{}\n", tag.name, tag.topic),
            ),
            TagCommand::List { scope, text } => {
                emit(store.list_tags(scope, text.as_deref())?, json, |list| {
                    let mut text = list
                        .tags
                        .iter()
                        .map(|tag| {
                            format!(
                                "{}{} — {}",
                                tag.name,
                                if tag.read_on_start {
                                    " [read on start]"
                                } else {
                                    ""
                                },
                                tag.topic
                            )
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
                format!("{}\n{}\n\n{}", tag.name, tag.topic, tag.body)
            }),
        },
        Command::Publish {
            id,
            dry_run,
            text,
            tags,
            clear_tags,
            if_match,
        } => emit(
            store.publish_note(PublishNote {
                id,
                dry_run,
                body: text.read()?,
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
            if !context.initialized {
                text.push_str("\nNot initialized. Run robostickies init or call init_store.\n");
            }
            text.push_str("\n## Available tags\n\n");
            for tag in &context.tags {
                text.push_str(&format!("- {}: {}\n", tag.name, tag.topic));
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
    }
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
                "--topic",
                "Tools",
                "--private",
                "--shared"
            ])
            .is_err()
        );
    }
}
