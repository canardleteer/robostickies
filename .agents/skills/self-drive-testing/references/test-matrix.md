# Feature matrix

Build the case list from the tools and resources advertised by the server.
Record results under `.robostickies/runs/`; keep this reference reusable.
For each case, retain its input and expected outcome with the observed result.
Use channels `installed connection`, `child process`, or `automated test` and
statuses `passed`, `failed`, or `pending`.

## Ordinary workflows

Use the installed connection and synthetic content. Inspect the matching source
files after writes. Tag fixtures and give each run a unique body marker; keep a
list of owned IDs because clearing tags is part of the lifecycle checks.

| Surface | Cases and expected behavior |
| --- | --- |
| `get_context` | Works before init; returns short topics and full startup bodies after init; public scope omits private tags and personal config |
| `init_store` | Creates files in this checkout; repeat call preserves sources; leaves root agent instructions alone by default |
| `create_tag`, `get_tag`, `list_tags` | Short topics; full bodies; startup flag; case-insensitive topic/name search; public/private definitions with the same bare name |
| `get_config` | Effective defaults and source paths match the files used by this process |
| `create_note`, `get_note` | Untagged and multiple tags; required UUID; returned path matches disk; private tags route locally; explicit public conflict is rejected |
| `update_note` | Omitted fields stay intact; empty tag list clears tags; latest revision succeeds; stale revision fails; archive/restore affects queries |
| `list_notes` | Literal case-insensitive body search; AND tags; exclusions; all scopes/statuses; newest-first order; date boundaries; pagination without duplicates |
| `review_notes` | Public by default; complete despite small page size; deterministic Markdown; rejects cursor; private content/counts/diagnostics are absent |
| `publish_note` | Dry run writes no source; actual copy has a new ID; original remains; private tags and extra metadata disappear; supplied body is retained |
| `validate_store`, `rebuild_index` | Healthy sources produce a healthy report; rebuild leaves Markdown unchanged |
| Resources | Read `robostickies://context`, `robostickies://guidance`, `robostickies://tags`, `robostickies://config` as JSON before and after init; compare with corresponding tools |
| Invalid calls | Unknown arguments; invalid IDs/tag names; empty bodies; unknown tags; invalid limits/dates/cursors; useful errors without unintended writes |

For publishing, add synthetic extra frontmatter to an owned private fixture,
then read its latest revision. Confirm preview leaves no file at its returned
path. Use replacement public prose and inspect both source files afterward.
Remove a private tag in a separate edit to verify that this does not publish.

## Temporary stores and automated coverage

Keep disruptive cases out of the repository's working notes. Use fresh stdio
processes bound to temporary directories; inspect actual responses and files.

| Area | Cases and expected behavior |
| --- | --- |
| Configuration | Isolated user file; shared/user/private precedence; privacy patterns; explicit tag visibility override; malformed configuration |
| Sources | Manual edits refresh search and topics; deletions disappear; metadata/comments survive edits; duplicate IDs and unresolved tags are diagnosed |
| Privacy | Malformed private sources and misplaced private-tagged notes do not enter public output; malformed public sources prevent an incomplete review |
| Cache | Missing/corrupt/incompatible index rebuilds from Markdown; failed cache is disposable; cache failure after a save returns its ID with warnings |
| Concurrency | CLI writes while MCP stays alive; concurrent writers lose no notes; stale edits are rejected |
| Repository | Nested startup CWD selects Git root; non-Git CWD works; explicit relative root overrides discovery; selection stays fixed per process |
| Integration | Optional agent paragraph preserves existing content and is idempotent; store/local ignore modes work, including linked worktrees |
| Filesystem | Invalid paths and symlinks cannot escape; missing private/cache exclusions prevent unsafe writes; tracked private/cache paths are reported |

Existing tests in `tests/store.rs` and `tests/interfaces.rs` cover these boundaries.
Run targeted regressions for fixes, then the full required checks. Record an
automated pass as automated coverage, even when the same feature also passed
through a live connection.

## Resume after installation

Keep a checkpoint in the local run ledger with the source revision or diff,
installed executable identity, note IDs, and remaining live calls. After the
user confirms restart, rediscover schemas and repeat the affected cases. For
argument validation, send an unsupported field to each tool with otherwise
valid inputs and check rejection before any mutation. Also verify that tools
without parameters still accept an omitted arguments object.
