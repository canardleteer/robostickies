# robostickies

Brief repository notes for humans and agents, available through a `clap` CLI and
an `rmcp` stdio server. Capture what gets in the way (`papercut`), what helps
(`paperclip`), and general observations with your own tags.

Markdown with YAML frontmatter is the source of truth. Shared files can travel
in Git history; private definitions and notes live in an ignored namespace.
A disposable, pure Rust `redb` database indexes the local source files.

## Start

Requires Rust 1.90 or newer.

```sh
git clone https://github.com/canardleteer/robostickies.git
cd robostickies
cargo install --path . --locked
cd /path/to/repository
robostickies init
robostickies context
robostickies add "The workspace test helper saved several setup steps" --tag paperclip
robostickies add "Tests need the package directory as their working directory" --tag papercut
robostickies list
robostickies search "working directory"
robostickies review
```

Run commands from any directory inside a Git repository, or select an explicit
root with `--repo /path/to/repository`. Outside Git, initialization uses the
current directory. Every CLI operation supports `--json`; errors go to stderr
with stable codes. Pass `-` or `--file <path>` to read a note body from stdin or
a UTF-8 file. Text after `update` replaces the body; omitted fields remain intact.

```sh
robostickies tags create tooling --topic "Tooling and development workflow" \
  "Read the relevant tool's help and record concrete failures or useful helpers."
robostickies tags list
robostickies tags get tooling
robostickies add "The build helper avoids repeated environment setup" --tag paperclip --tag tooling
robostickies update <uuid> "Corrected observation" --if-match <revision>
robostickies archive <uuid>
robostickies unarchive <uuid>
```

Predefined tags are optional. Notes can use multiple defined tags or no
tags. Edit tag guidance directly in its Markdown file. Definitions require a
short YAML `topic`; `read_on_start: true` includes full guidance in session
context. Additional frontmatter is retained during note edits, including comments.

## Files

```text
.robostickies/
  config.toml
  README.md
  .gitignore
  tags/{papercut,paperclip,resolved,adopted,...}.md
  notes/<uuid>.md
  private/
    config.toml             # optional repository-local defaults
    tags/<name>.md
    notes/<uuid>.md
  cache/                    # ignored redb index, recovery file, operation lock
```

For example, `tags/tooling.md`:

```markdown
---
topic: Tooling and development workflow
read_on_start: true
---
Record the triggering command, observed behavior, and useful workaround.
```

A note is similarly readable:

```markdown
---
id: 019a1220-0000-7000-8000-000000000001
created_at: "2026-10-25T12:00:00Z"
updated_at: "2026-10-25T12:00:00Z"
tags: [papercut, tooling]
status: active
---
The test helper expects the package directory as its working directory.
```

Use the complete UUID as the filename and command identifier. New IDs use
UUIDv7. Notes support `active` and `archived` status. Git retains the history of
shared edits; the index is rebuilt from current files after edits or checkouts.

## Private observations and publishing

```sh
robostickies tags create personal --topic "Local observations" --private
robostickies add "An observation for local use" --tag papercut --tag private:personal
robostickies add "Another local note" --private
robostickies list --scope private
robostickies publish <uuid> --dry-run
robostickies publish <uuid> "Reviewed public wording"
```

Private tags always have explicit `private:<name>` identities. A shared tag
named `personal` and a private tag named `private:personal` can coexist. Any
private tag forces private storage. Removing that tag does not publish a note.
Adding a private tag to a public note moves it into private storage; its earlier
public content remains in Git history.

`publish` immediately creates a new public copy and keeps the private original.
Use `--dry-run` for a preview. The copy drops private tags and additional metadata,
and accepts replacement body text and public `--tag` values. `--clear-tags`
publishes an untagged copy. `--if-match` rejects a changed source. Review the
prose you choose to publish; publishing does not redact the supplied text.

By default, `.robostickies/.gitignore` excludes `private/` and `cache/`. To use
local Git rules instead, initialize with `--ignore-location local`; rules go in
Git's `info/exclude`, including in linked worktrees. Git ignore rules do not
remove existing tracked files. `doctor` identifies tracking and exclusion issues.

User configuration is `robostickies.toml` in the platform config directory:

| Platform | Location |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/robostickies.toml` or `~/.config/robostickies.toml` |
| macOS | `~/Library/Application Support/robostickies.toml` |
| Windows | `%APPDATA%\robostickies.toml` |

```toml
[tags]
default_private = false
private_patterns = ["personal-*", "local-*"]
```

Matching patterns make newly created tags private. Explicit `--private` or
`--public` overrides both patterns and defaults. Precedence, highest first:
explicit option, `.robostickies/private/config.toml`, user config, shared
`.robostickies/config.toml`, built-ins. Layers replace only specified fields;
pattern arrays replace the earlier array. Initialization honors these defaults
for the predefined tags too. Shared configuration requires `format_version = 1`.

Inspect resolution with `config show`. Use `--user-config <path>` to select a
different user file or `--no-user-config` for reproducible automation.

## Search and review

```sh
robostickies list --scope all --status all
robostickies search "test helper" --tag tooling --exclude-tag papercut
robostickies list --since 2026-10-01 --until 2026-11-01 --limit 20
robostickies list --limit 20 --cursor '<cursor from preceding response>'
robostickies review --scope public
robostickies review --scope all --status all
```

Browsing defaults to active notes across both namespaces. Tag filters require
all selected tags; exclusions reject any match. Text search is a case-insensitive
literal substring of the Markdown body. Results are newest first, then descending
ID. Date bounds accept UTC dates or RFC3339 timestamps: `since` is inclusive and
`until` exclusive. Reuse pagination cursors with the same filters.

Review defaults to public notes and covers all matching notes, without pagination.
CLI review and MCP `review_notes` reject `limit` and `cursor`; list/search keep
their paging inputs.
It emits deterministic Markdown, or structured material with `--json`, for you
or an agent to summarize. Public review excludes private notes and metadata
before computing counts or rendering. Publish selected private notes first if
you want their reviewed content in public material. Invalid sources in the
selected visibility prevent a complete review until corrected.

## Resolution and adoption

Initialization adds missing `resolved` and `adopted` definitions, including in
existing stores. It preserves customized definitions and honors tag privacy
defaults. Both tags provide startup guidance and use the existing tag/update
commands and APIs.

| Observation | Condition | Action |
| --- | --- | --- |
| Papercut | Fix verified in the affected environment | Retain `papercut`, add `resolved`, record evidence, archive |
| Paperclip for permanent guidance | Guidance exists in the appropriately scoped AGENTS.md and has been reviewed | Retain `paperclip`, add `adopted`, record the destination, archive |
| Other useful paperclip | Permanent instructions are unwarranted | Keep active without `adopted` |

MCP fixes require installation, an agent restart, and verification through the
installed connection before resolution. Archived status alone proves no outcome.
Publish a sanitized public copy of a private observation before promoting it
into tracked guidance.

Updates replace the entire tag set, so retain the original tags when adding an
outcome. Preserve the note's visibility: use a public outcome definition for a
public note; a private note can use public or private definitions. If the needed
definition is missing, create it with an explicit `--public`/`--private` or MCP
`private` value to override current defaults. Adding a private tag moves a public
note into private storage.

```sh
robostickies update <uuid> "Observation and verification evidence" --tag papercut --tag resolved --if-match <revision>
robostickies archive <uuid>
robostickies review --status archived --tag resolved
robostickies review --status archived --tag adopted
```

In MCP, use `update_note` with the retained tags plus the outcome and
`status: "archived"`. Query history with `review_notes` using
`{"status":"archived","tags":["resolved"]}` or
`{"status":"archived","tags":["adopted"]}`. Public/active defaults still apply;
select `scope` and qualified tag names explicitly for private history.

## MCP and agent orientation

Install the binary, then run `robostickies init` in the target repository.
Start the server from the directory the agent is working in:

```sh
robostickies mcp
```

Directory selection happens once. The process uses the Git root containing its
startup working directory, or that directory outside Git. The client controls
the launch directory, which should be the agent's repository or CWD.
Use `--repo DIR` to override selection; relative paths resolve from the startup
working directory. Start a separate process for each repository.

The binary must be on the client's PATH.

### Codex

Register the server with the Codex CLI:

```sh
codex mcp add robostickies -- robostickies mcp
codex mcp list
```

This reusable user-level configuration lets each launched server process
discover its repository from its working directory. Start Codex in the repository
and use `/mcp` to check the connection.

For project-scoped setup, put the equivalent entry in `.codex/config.toml`:

```toml
[mcp_servers.robostickies]
command = "robostickies"
args = ["mcp"]
```

Project configuration loads for trusted projects. See the
[Codex MCP documentation](https://developers.openai.com/codex/mcp/).

### Claude Code

From the target repository, register the server with local scope:

```sh
claude mcp add --transport stdio --scope local robostickies -- robostickies mcp
claude mcp list
```

Start Claude Code there and use `/mcp` to check the connection. Local scope keeps
the registration specific to that project and user. See the
[Claude Code MCP documentation](https://code.claude.com/docs/en/mcp).

### Using the server

Discovery includes brief startup instructions even before initialization. Agents
should call `get_context` to read topics, full startup guidance, capabilities,
and an optional AGENTS.md paragraph. Use `init_store` to scaffold through MCP.
Local all/private context includes the canonical `repository_root` before and
after initialization. The CLI displays it too. Public context omits the field
entirely, along with personal configuration.

| MCP tools | Purpose |
| --- | --- |
| `get_context`, `get_config` | Guidance and effective defaults |
| `create_note`, `get_note`, `update_note` | Record and maintain notes; archive through `status` |
| `list_notes` | Browse and search using shared filters |
| `review_notes` | Complete review, defaulting to public visibility |
| `create_tag`, `list_tags`, `get_tag` | Define tags and explore short/full guidance |
| `publish_note` | Immediately copy or preview a private note as public |
| `init_store`, `validate_store`, `rebuild_index` | Initialize and maintain the store |

Creation tools accept `private: true` or `false`; omission uses the relevant
defaults. Private tag references still force private note storage. `update_note`
and `publish_note` accept `expected_revision`; get/create/update responses supply
the current revision. `publish_note` defaults to immediate copying, with
`dry_run: true` for previews. Tools return structured results and tool errors.

Resources `robostickies://context`, `robostickies://guidance`,
`robostickies://tags`, and `robostickies://config` offer read access as well.
The server binds to one repository and logs exclusively to stderr. Set `RUST_LOG`
to adjust diagnostics. No HTTP transport is exposed.
The context resource includes the local repository root and matches unscoped
`get_context`; use the scoped tool for public output.

`init --agents` optionally appends a small managed pointer to `AGENTS.md`; it
preserves other content and is idempotent. You can instead obtain the paragraph
from context and place it in your preferred format. Guidance follows the
[AGENTS.md standard](https://agents.md/).

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The library shares all operations between CLI and MCP. Operation-scoped locking
allows multiple CLI processes and MCP servers to use the same store. Atomic
writes preserve source files; saved notes still return their ID if index refresh
fails. `reindex` regenerates the cache, and `doctor` diagnoses source problems.

Integration tests start fresh stdio servers from Cargo's built binary and use
isolated stores. A clean checkout needs no installed executable, temporary
runner, or existing notes. They cover resources, recovery, concurrent CLI writes,
frontmatter/publication, context selection, and complete review across pages.
Live installed-connection checks remain necessary after MCP changes; follow the
[self-drive-testing skill](.agents/skills/self-drive-testing/SKILL.md).

CLI errors use `invalid_input`, `invalid_document`, `not_found`, `not_initialized`,
`conflict`, `privacy`, `configuration`, `busy`, `io`, `index`, or `internal` codes.
Exit statuses are 2 for clap usage, 65 for invalid input/documents, 66 for missing
notes/stores, 74 for I/O, 75 for lock contention, 78 for configuration, and 1 for
other failures. Empty results succeed.

Inspired by [papercuts](https://github.com/treygoff24/papercuts) and
[paperclips](https://github.com/dlorp/paperclips).
