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
    tags/<name>.md
    notes/<uuid>.md
  cache/                    # ignored redb index, recovery file, operation lock
```

For example, `tags/tooling.md`:

```markdown
---
topic: Tooling and development workflow
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

[All usage guides](README.md).
