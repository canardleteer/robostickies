# Markdown storage

```text
.robostickies/
  config.toml
  README.md
  .gitignore
  tags/{papercut,paperclip,resolved,adopted,...}.md
  notes/<uuid>.md
  private/
    config.toml             # optional repository-local defaults
    upgrade-preferences.toml # optional local deferral, compatible with older tools
    snapshots/<unique-id>/  # temporary or explicitly retained recovery copies
    tags/<name>.md
    notes/<uuid>.md
  cache/                    # ignored redb index, recovery file, operation lock
```

For example, `tags/tooling.md`:

```markdown
---
description: Build tools and development workflow
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

Edits retain custom frontmatter metadata and comments. They preserve the
file's LF or CRLF convention, including replacement body text. Frontmatter-only
edits preserve body bytes, including the absence of a final newline.
Tag updates preserve sequence indentation and comments. Clearing tags writes
an empty YAML sequence. The explicit `v1alpha2` → `v1alpha3` store upgrade repairs
flow tag lists left unindented by older writers. Ordinary edits reject those
malformed sources; see [store upgrades](upgrades.md).

Comments displaced by replacing or clearing a YAML value are retained verbatim
at the end of the frontmatter, before its closing `---`, including repeated
comments. When working on that frontmatter, review those comments and relocate
or remove them when their intent is clear. Ask the user when it is uncertain.

Bodies contain nonempty UTF-8 text; Markdown or plain text is preferred. An
optional string `body_format: markdown` in frontmatter is an advisory hint,
exposed as `metadata.body_format`. It does not select a parser or execute the
body. Unknown strings and legacy non-string metadata remain readable. See
[body hints](getting-started.md#bodies-and-optional-format-hints) for editing rules.

The `tooling` filename is the required tag identifier. Its optional `description`
explains the name in the catalog, and its optional body supplies Markdown usage
guidance. Self-explanatory tags usually do not need a description:

```markdown
---
read_on_start: false
---
```

For example, that file can define `tags/fresh-install.md` using only its name.
Missing or null descriptions mean no explanation; empty or whitespace-only text
is invalid. New files omit absent descriptions, while JSON results use `null`.
Current-format files reject `topic`. Legacy stores require it and expose its
value as the runtime description without changing source bytes. The explicit
`v1alpha3` → `v1alpha4` [upgrade](upgrades.md) renames those keys in both namespaces.

The shared configuration holds the repository's sole `data_version`. Markdown
documents have no version field, and the disposable index uses a separate cache
generation. See [store versions and upgrades](upgrades.md).

[All usage guides](README.md).
