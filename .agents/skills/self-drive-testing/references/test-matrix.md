# Feature matrix

Build the case list from the tools and resources advertised by the server.
Cover each new feature or behavior change with its affected workflows and failure
paths.
Record results under `.robostickies/private/runs/<run>/`; keep this reference
reusable.
For each case, retain its input and expected outcome with the observed result.
Use channels `installed connection`, `child process`, or `automated test` and
statuses `passed`, `failed`, or `pending`.

## Ordinary workflows

Use the installed connection and synthetic content. Keep test notes and tag
definitions private by default; request public visibility only when the case needs
it. Inspect source files after writes. Record every owned ID/path in the private
ledger because clearing tags and publishing copies are lifecycle checks. Delete
synthetic fixtures from both namespaces after the run and before any commit.
Exercise archive/restore, then delete the fixture sources.

| Surface | Cases and expected behavior |
| --- | --- |
| Startup discovery | An ordinary fresh-agent orientation question surfaces applicable maintenance advice using only product-provided instructions; keep the expected fields out of its task. Test current, legacy, and deferred states separately before targeted checks |
| `version` | Actual running process matches all seven checkpoint fields, including before init; empty arguments accepted, unknown arguments rejected, no repository reads or store creation |
| `get_context` | Works before init; local all/private scopes include the canonical repository root; returns short topics and full startup bodies after init; public scope omits the root field, private tags, and personal config |
| `init_store` | Creates files in this checkout; adds missing outcome definitions in new/existing stores under current privacy defaults; repeat call preserves customized definitions; leaves root agent instructions alone by default |
| `create_tag`, `get_tag`, `list_tags` | Short topics; full bodies; startup flag; case-insensitive topic/name search; public/private definitions with the same bare name |
| `get_config` | Effective defaults and source paths match the files used by this process |
| Store versions | `get_context`, `get_config`, and `validate_store` agree on `data_version` and `upgrade_available`; public context exposes only shared version status; `upgrade_store` previews without source writes and repeated upgrades are idempotent |
| `create_note`, `get_note` | Untagged and multiple tags; required UUID; returned path matches disk; private tags route locally; explicit public conflict is rejected |
| Body hints | Recommended and arbitrary nonblank values; set/omit/clear; reject blank/conflicting/stale edits; preserve comments, nested metadata and newline style; legacy non-string hints survive unrelated edits and reject replacement/clear |
| `update_note` | Revision is required; missing/null/blank/stale guards reject without source writes; omitted patch fields stay intact; empty tag list clears tags; current revision succeeds; archive/restore affects queries |
| `list_notes` | Literal case-insensitive body search; AND tags; exclusions; all scopes/statuses; newest-first order; date boundaries; pagination without duplicates |
| `review_notes` | Public/active by default; schema omits paging; rejects limit/cursor; filters work across multiple internal pages; deterministic Markdown; private content/counts/diagnostics are absent |
| Outcome lifecycle | Retain papercut/paperclip and visibility; resolved requires affected-environment verification; adopted requires reviewed scoped guidance and a recorded destination; edits and archive/read retain outcome tags; archived-tag queries find history |
| `publish_note` | Source revision is required for preview and copy; stale guards create no copy; dry run writes no source; actual copy has a new ID; original remains; private tags and extra metadata disappear |
| Public body hint | Source hint disappears with private metadata; only an explicitly supplied public hint reaches the new copy; preview has no source file |
| `validate_store`, `rebuild_index` | Healthy sources produce a healthy report; rebuild leaves Markdown unchanged |
| Resources | Read `robostickies://context`, `robostickies://guidance`, `robostickies://tags`, `robostickies://config` as JSON before and after init; compare with corresponding tools |
| Invalid calls | Unknown arguments; invalid IDs/tag names; empty bodies; unknown tags; invalid limits/dates/cursors; useful errors without unintended writes |
| Cleanup | Ledger-owned synthetic notes and unreferenced fixture definitions deleted from both namespaces; no public test fixtures or public run output before commit; original findings and retained evidence stay private |
| Sharing findings | When asked to share genuine findings, preview and publish reviewed prose; retain the private original; commit selected public copies with shared configuration and public definitions; keep fixture ownership separate |

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
| Upgrades | New stores use the current version; missing version means legacy; legacy operations continue with CLI notices on stderr; explicit dry run and upgrade preserve both namespaces and customized guidance byte for byte except the shared version field; reject malformed/future versions and user/private overrides; exercise failed writes, cache states, and concurrent upgrades/writers |
| Advice and deferral | Healthy/empty diagnostics can coexist with SHOULD/MAY advice; current/no-store states do not recommend migration; optional target deferral changes no source version, never weakens mandatory safety, and leaves public responses independent; changed targets and malformed preferences restore recommendations |
| Recovery snapshots | Hidden/private inclusion, cache/prior-snapshot exclusions, unsafe-entry/copy/integrity failures, retained failure copies, requested retention/default deletion/cleanup warnings; no snapshots for previews, current stores, or deferrals; ordered source validation is independent of disposable cache |
| Sources | Manual edits/deletions refresh results; nested metadata/comments, spacing, and CRLF framing survive edits and lifecycle changes; replacement bodies follow the source newline style; cross-namespace duplicate IDs preserve public selection while local diagnostics and ambiguous ID operations report conflicts; legacy warm indexes refresh automatically |
| Privacy | Malformed private sources and misplaced private-tagged notes do not enter public output; malformed public sources prevent an incomplete review |
| Cache | A running MCP server rebuilds missing/corrupt/incompatible indexes from Markdown; failed cache is disposable; cache failure after a save returns its ID with warnings; edits also remain successful |
| Concurrency | Two MCP servers plus CLI writers retain independent creations; same-revision edits have exactly one winner; status/privacy races preserve the winner's fields; competing tag creation does not overwrite; contention returns busy; process exit releases the lock |
| Repository | Nested startup CWD selects Git root; non-Git CWD works; explicit relative root overrides discovery; selection stays fixed per process |
| Integration | Optional agent paragraph preserves existing content and is idempotent; store/local ignore modes work, including linked worktrees |
| Filesystem | Invalid paths and symlinks cannot escape; missing private/cache exclusions prevent unsafe writes; tracked private/cache paths are reported |

Existing tests in `tests/store.rs` and `tests/interfaces.rs` cover these boundaries.
They run under ordinary `cargo test` with Cargo's built binary and isolated
stores; no installed binary, temporary runner, or existing notes are required.
Use the bounded asynchronous subprocess helper for test-owned CLI/Git commands;
a stalled child must time out and be reaped. Keep whole-test timeouts as well.
Run targeted regressions for fixes, then the full required checks. Record an
automated pass as automated coverage, even when the same feature also passed
through a live connection.

## Resume after installation

Keep a checkpoint in the private run ledger with the source revision or diff,
installed executable identity, note IDs, and remaining live calls. Use either a
user-confirmed restart of the current session or a separate fresh agent process
with its own newly started MCP server. The current session may continue during
separate verification. Once the chosen connection is ready, rediscover schemas
and repeat the affected cases. For argument validation, send an unsupported field
to each tool with otherwise
valid inputs and check rejection before any mutation. Also verify that tools
without parameters still accept an omitted arguments object.

After verification, remove all owned synthetic notes and unused fixture definitions,
including temporary public copies. Rebuild and validate the index. Check public
queries with all statuses and the private ownership ledger before committing;
Git staging alone cannot detect public test artifacts left on disk.
