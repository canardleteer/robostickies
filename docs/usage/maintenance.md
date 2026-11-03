# Diagnostics and cache recovery

Run `doctor` to validate Markdown sources and Git privacy protections. Run
`reindex` to rebuild the disposable redb index from those sources:

```sh
robostickies doctor
robostickies reindex
robostickies doctor --json
```

Context, configuration, and health reports include the shared `data_version`
and derived `upgrade_available` status. See [store upgrades](upgrades.md) to
preview and apply a migration.

Read the separate `upgrade` advice even when health is good and diagnostics are
empty. A safely usable legacy store can be healthy while an optional upgrade is
recommended. Current data needs no migration or routine rebuild after installation.

Git validation probes capture their output and receive closed input, keeping
their I/O separate from the stdio MCP transport.

A rebuild does not rewrite the authoritative Markdown files. If indexing fails
after a successful save, the operation still returns the saved note's ID with
warnings. Retain that ID and repair the cache before retrying discovery; repeating
creation would make another note.

See [shared editing](shared-editing.md) for lock contention and
[Markdown storage](storage.md) for the source layout.

## Errors and exit statuses

CLI errors use `invalid_input`, `invalid_document`, `not_found`, `not_initialized`,
`conflict`, `privacy`, `configuration`, `upgrade_required`, `busy`, `io`, `index`,
or `internal` codes.
Exit statuses are 2 for clap usage, 65 for invalid input/documents, 66 for missing
notes/stores, 74 for I/O, 75 for lock contention, 78 for configuration, and 1 for
other failures. Mandatory upgrades use `upgrade_required` with exit status 78.
Empty results succeed.

An undefined tag reference returns `not_found`; select an existing tag or create
its definition explicitly before using it.

[All usage guides](README.md).
