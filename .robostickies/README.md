# Robostickies

At the start of work, read the tag catalog and the full guidance for tags marked
`read_on_start`. Use `robostickies context` or the MCP `get_context` tool.

Inspect the returned `upgrade.requirement`, `upgrade.message`, and `warnings`
before deciding whether maintenance is needed. Healthy validation and empty
diagnostics do not mean that the store data version is current:

- `none`: no data migration is needed. A new executable alone does not require
  migration or an index rebuild.
- `should`: tell the user an optional upgrade is recommended, unless
  `upgrade.deferred` is true. Offer a preview, explicit upgrade, or deferral;
  respect a saved deferral instead of repeating the reminder.
- `must`: explain the required migration before ordinary work. Status and
  supported migration operations remain available.

Use `upgrade_store` with `dry_run: true` to preview, without those options to
apply, or with `defer: true` to decline the optional target. CLI equivalents are
`upgrade --dry-run`, `upgrade`, and `upgrade --defer`. Do not migrate automatically.
An uninitialized store needs initialization rather than an upgrade.

For MCP, call `version` to identify the running server's compiled build. CLI
`--version` identifies that CLI process, not an existing MCP server. After an
installation, restart/reconnect older servers or use a fresh server before
verifying changed behavior. A missing or mismatched MCP version/capability is a
verification gap; do not substitute the installed CLI's identity.

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
installation and checks through a newly started agent MCP connection. A separate
fresh testing agent can supply that connection while the current session continues.
Archived status alone does not prove resolution.

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
(`active` or `archived`); the body is nonempty UTF-8 text, preferably Markdown
or plain text. Optional `body_format` metadata is advisory and never interprets
the body. Additional metadata is retained when editing. The files are authoritative; the redb index is disposable.

Legacy YAML repairs run only through an explicit store upgrade. Ordinary edits
reject malformed legacy frontmatter instead of silently migrating it. Preview an
older store's upgrade or correct the source before editing.
After upgrading, read notes again before editing; repaired files have new revisions.

YAML edits preserve comments. Comments displaced by replacing or clearing a value
are retained at the end of the frontmatter, before its closing `---`. When working
on that frontmatter, review displaced comments: relocate or remove them when their
intent is clear, and ask the user when it is uncertain.

Edits, archive/unarchive, and publishing (including previews) require the revision
from the last successful read or save. Use CLI `--if-match` or MCP
`expected_revision`. Missing or blank revisions are rejected. A stale revision
returns `conflict` without writing: re-read and reconcile the changes before
retrying. There is no overwrite bypass or automatic merge.

Cooperating CLI/MCP clients sharing one OS account on local disk use an exclusive
operation lock. Source reads, revision comparisons, and saves share that lock;
content replacement is atomic. Contention waits up to five seconds, then returns
`busy`. The OS releases the lock when a process exits; keep the lock file in place.
Concurrent direct editor/Git writes are outside this guarantee. Store data
migrations and executable installation are separate. Running MCP processes keep
their compiled build. Use a newly started server to verify installed code, or
restart an older server before relying on its changed behavior.

Any private tag makes a note private. `add --private` also keeps an otherwise
publicly tagged or untagged note local. Private notes stay private when tags
are removed. `publish <id>` / `publish_note` makes a new public copy, removes
private tags and additional metadata, and retains the private original.
Use `--dry-run` / `dry_run: true` to preview, and replace the body if necessary.
Supply an explicit public body hint with `body_format` (CLI `--body-format`) if needed; source hints are removed.
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
