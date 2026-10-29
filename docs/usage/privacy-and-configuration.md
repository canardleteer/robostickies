# Private notes and configuration

## Private notes and publication

```sh
robostickies tags create personal --topic "Local observations" --private
robostickies add "An observation for local use" --tag papercut --tag private:personal
robostickies add "Another local note" --private
robostickies list --scope private
robostickies get <uuid>
robostickies publish <uuid> --dry-run --if-match <revision>
robostickies publish <uuid> "Reviewed public wording" --if-match <revision>
```

Private tags always have explicit `private:<name>` identities. A shared tag
named `personal` and a private tag named `private:personal` can coexist. Any
private tag forces private storage. Removing that tag does not publish a note.
Adding a private tag to a public note moves it into private storage; its earlier
public content remains in Git history.

`publish` immediately creates a new public copy and keeps the private original.
Use `--dry-run` for a preview. The copy drops private tags and additional metadata,
and accepts replacement body text and public `--tag` values. `--clear-tags`
publishes an untagged copy. Both preview and copy require `--if-match`; a preview
does not change the source revision. A changed source is rejected. Review the
prose you choose to publish; publishing does not redact the supplied text.

By default, `.robostickies/.gitignore` excludes `private/` and `cache/`. To use
local Git rules instead, initialize with `--ignore-location local`; rules go in
Git's `info/exclude`, including in linked worktrees. Git ignore rules do not
remove existing tracked files. `doctor` identifies tracking and exclusion issues.

## Configuration

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

[All usage guides](README.md).
