# Shared editing

## Read before editing

Read a note before editing it, then pass its returned revision with CLI
`--if-match` or MCP `expected_revision`. Saves return the next revision to use.
The same guard applies to archive/unarchive and to publication previews or copies.

If another writer changes the note first, your stale revision returns `conflict`
without modifying the source. Re-read the note and reconcile the intervening
changes before retrying; attaching a new revision to an unreviewed stale body
could still discard another writer's work. There is no force override or
automatic merge.

| Revision | Result |
| --- | --- |
| Missing, or null in MCP | Argument parsing fails |
| Empty or whitespace | `invalid_input` |
| Stale | `conflict` with the source unchanged |

See the [note commands](getting-started.md#notes-and-tags) for edit examples.

## File locking

Cooperating CLI processes and MCP servers that share one OS account on local disk
use one OS file lock. It covers the source read through the revision check and
save, while atomic replacement prevents partial files. New note IDs remain
unique; competing tag creation cannot overwrite an existing definition.

Lock contention waits up to five seconds before returning `busy` (CLI status 75).
The OS releases the lock when a process exits, so leave `cache/store.lock` in
place. Direct editor and Git writes do not use this lock.

## After an upgrade

Start a new MCP server after installing an upgrade to use the installed binary.
Existing servers retain their old behavior until restarted. The disposable index
rebuilds older derived data automatically. Markdown files need no migration.

[All usage guides](README.md).
