# Store data versions and upgrades

The shared `.robostickies/config.toml` is the only stored authority for the
repository's data version. It covers public and private notes and tag
definitions together. New stores write:

```toml
format_version = 1
data_version = "v1alpha2"
```

An absent `data_version` means `v1alpha1`. Both versions remain readable and
writable. Successful CLI commands print an upgrade notice on stderr for legacy stores;
JSON note responses retain their existing shape. MCP reports `data_version`
and `upgrade_available` in `get_context`, `get_config`, and `validate_store`.
The corresponding CLI commands are `context`, `config show`, and `doctor`.

Preview and apply an explicit upgrade:

```sh
robostickies upgrade --dry-run
robostickies upgrade
robostickies upgrade --json
```

In MCP, call `upgrade_store` with `{"dry_run": true}` for a preview, or `{}`
to apply it. Results contain `source_version`, `target_version`, `wrote`, and
`dry_run`. Repeating an upgrade returns `wrote: false` after the store reaches
the current version. Running `init` on an existing store does not upgrade it.

The `v1alpha1` to `v1alpha2` migration changes only the shared version field.
It preserves other configuration content and formatting, note and tag bytes,
IDs, revisions, nested metadata, comments, privacy settings, customized
guidance, and agent files. Cache rebuilding is independent of data migration.
Version validation and atomic replacement use the same operation lock as
ordinary readers and writers. Dry runs leave source files unchanged.

User configuration and `.robostickies/private/config.toml` cannot set
`data_version`, even to the shared value. Documents receive no version field.
Public reads validate the shared configuration and use its version status.
Local layer errors remain local; they cannot change public review results or
appear in public diagnostics. Local inspection and writes validate all layers.
The numeric `format_version` checks configuration syntax independently of the
data version; cache generations remain an internal index detail. Malformed or
unsupported versions fail with `configuration` before rewriting sources.

Upgrade an initialized store using a compatible executable. An absent store
must be initialized first. Restart existing MCP processes after installing new
code before relying on its behavior. See [shared editing](shared-editing.md)
for the limits of the operation lock.

[All usage guides](README.md).
