# Diagnostics and cache recovery

Run `doctor` to validate Markdown sources and Git privacy protections. Run
`reindex` to rebuild the disposable redb index from those sources:

```sh
robostickies doctor
robostickies reindex
robostickies doctor --json
```

A rebuild does not rewrite the authoritative Markdown files. If indexing fails
after a successful save, the operation still returns the saved note's ID with
warnings. Retain that ID and repair the cache before retrying discovery; repeating
creation would make another note.

See [shared editing](shared-editing.md) for lock contention and
[Markdown storage](storage.md) for the source layout.

## Errors and exit statuses

CLI errors use `invalid_input`, `invalid_document`, `not_found`, `not_initialized`,
`conflict`, `privacy`, `configuration`, `busy`, `io`, `index`, or `internal` codes.
Exit statuses are 2 for clap usage, 65 for invalid input/documents, 66 for missing
notes/stores, 74 for I/O, 75 for lock contention, 78 for configuration, and 1 for
other failures. Empty results succeed.

[All usage guides](README.md).
