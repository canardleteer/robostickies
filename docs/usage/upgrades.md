# Store data versions and upgrades

The shared `.robostickies/config.toml` is the only stored authority for the
repository's data version. It covers both namespaces. New stores write:

```toml
format_version = 1
data_version = "v1alpha2"
```

An absent `data_version` means `v1alpha1`. Both versions remain readable and
writable. Installing, opening, or initializing an existing store never migrates
it silently. Optional body hints use existing additional metadata and require
neither a version bump nor a bulk rewrite.

## Upgrade advice and local deferral

`context`, `config show`, `doctor`, and `upgrade` (and their MCP counterparts)
return structured `upgrade` advice with source/target versions, `requirement`,
`can_upgrade`, `deferred`, and a message:

| Requirement | Meaning |
| --- | --- |
| `none` | The store is current. |
| `should` | The older representation is safely usable. Users SHOULD upgrade and MAY invoke the upgrade tool. |
| `must` | A recognized representation cannot be used safely. Users MUST upgrade before ordinary operations; status and supported migration operations remain available. |

Both representations registered in this RC are safely usable. A future
upgrade-only representation must declare its safety boundary and migration.
Unsupported newer formats require compatible newer tooling: they are never
reinterpreted as legacy or downgraded.

Successful ordinary CLI commands retain upgrade notices on stderr. Ordinary
note-response envelopes stay unchanged. To decline the current optional target:

```sh
robostickies upgrade --defer
```

In MCP, call `upgrade_store` with `{"defer": true}`. This records only
`deferred_target` in the separate ignored `private/upgrade-preferences.toml`;
it adds no configuration keys that older executables would reject. It suppresses
optional local reminders for that target. A different target restores the
recommendation. The actual source version and `upgrade_available` stay unchanged.
An explicit upgrade still runs, including every required intermediate migration.
Deferral never suppresses mandatory safety errors. It conflicts with `dry_run`
and `keep_snapshot`.

Malformed or unreadable preferences produce a local warning and leave
recommendations enabled. Public context and public queries ignore private
preferences entirely, including their warnings.

## Explicit migration and recovery snapshots

```sh
robostickies upgrade --dry-run
robostickies upgrade
robostickies upgrade --keep-snapshot --json
```

MCP `upgrade_store` accepts `dry_run`, `defer`, and `keep_snapshot`, all defaulting
to false. Results retain `source_version`, `target_version`, `wrote`, and `dry_run`,
and add `deferred`, `upgrade`, `snapshot`, and `warnings`. `snapshot` is null when
no copy was needed; otherwise it contains a repository-relative `path` and final
`retained` outcome. CLI snapshot announcements and cleanup warnings use stderr.

Before an upgrade changes authoritative data, it copies the store to a unique
`.robostickies/private/snapshots/<unique-id>/`. Hidden files and private contents
are included. Only the root disposable `cache/` and existing `private/snapshots/`
subtree are excluded; similarly named directories elsewhere are copied. Git
exclusions are checked before copying. Symlinks, special files, and hard-linked
files are refused. The copy's inventory and file hashes are validated against
unchanged sources before migration starts.

Current stores need no snapshot. Previewing or deferring an upgrade also creates none. Successful
migration and source validation remove the snapshot unless `keep_snapshot` was
requested. A snapshot failure leaves authoritative data unchanged; partial copies
are reported as incomplete or unverified and retained for inspection. Migration or
validation failure stops and retains the recovery copy; errors report its path.
There is no automatic rollback. Inspect that copy and the reported error before
manually restoring authoritative files under the operation lock. If cleanup
fails, migration remains successful and reports a retained-snapshot warning.

Migrations run in registered order and validate every step under the existing
operation lock, without reacquiring it. Authoritative source validation does not
open or repair redb; cache damage cannot decide migration success. Repair the
disposable cache separately with `reindex`.

The existing `v1alpha1` → `v1alpha2` step changes only the shared version marker
through atomic configuration replacement. It preserves other configuration
bytes, note/tag bytes, IDs, privacy, revisions, custom metadata, comments, newline
conventions, customized guidance, and agent files. This RC introduces no
multi-file data-format migration. Future incompatible transformations must
provide fail-closed recovery before registration.

User configuration and `private/config.toml` cannot set `data_version`, even to
the shared value. Public reads validate only the shared configuration; local
layer errors remain local. The numeric `format_version` governs configuration
interpretation separately, and cache generations remain internal index details.
Malformed or unsupported versions fail before rewriting sources.

Upgrade an initialized store using compatible tooling. Restart MCP processes
after installing new code before relying on its behavior. See
[shared editing](shared-editing.md) for operation-lock limits.

## Format version policy

`format_version` identifies how configuration files are interpreted. The current
value is `1`. Change it when existing valid configuration needs transformation
or would otherwise acquire a different meaning, including:

- Renaming or removing existing keys or tables without compatibility handling.
- Changing existing value types, units, or representation.
- Changing precedence or defaults so unchanged configuration behaves differently.
- Replacing the configuration grammar or document structure incompatibly.

Do not change it for:

- New optional settings whose absence preserves existing behavior.
- Additional validation messages or implementation changes.
- Documentation, CLI or MCP interfaces, or cache changes.

Unknown keys remain errors. A new optional setting can require a newer executable
while remaining in the same configuration format family. Older applications may
reject that setting; this does not require a repository data migration.

## Data version policy

`data_version` identifies how authoritative repository data is interpreted and
safely preserved. The current value is `v1alpha2`. Change it when existing stored
sources need migration, or readers and writers for the current version cannot
safely interpret and preserve the new representation, including:

- Required frontmatter field changes, incompatible field types, or newly required
  values.
- Incompatible note or tag layouts or namespace paths.
- Changes to stored identity, revision, lifecycle, or privacy semantics.
- New authoritative information that older writers could discard or handle
  unsafely.

Do not change it for:

- Optional metadata with safe defaults that remains losslessly preserved.
- New commands, queries, MCP responses, or features that leave stored contracts
  intact.
- Bug fixes restoring documented behavior, documentation changes, or cache and
  index generations.
- A package release by itself.

The existing `v1alpha1` → `v1alpha2` bootstrap migration introduces the shared
version marker without changing the note or tag representation. Future changes
follow the policy above. Version declarations stay centralized in `src/version.rs`.
A stored-data boundary change requires an ordered migration and regression tests
that verify preservation of authoritative sources and privacy across versions.

[All usage guides](README.md).
