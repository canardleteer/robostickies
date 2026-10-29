# Search and review

```sh
robostickies list --scope all --status all
robostickies search "test helper" --tag tooling --exclude-tag papercut
robostickies list --since 2026-10-01 --until 2026-11-01 --limit 20
robostickies list --limit 20 --cursor '<cursor from preceding response>'
robostickies review --scope public
robostickies review --scope all --status all
```

Browsing defaults to active notes across both namespaces. Tag filters require
all selected tags; exclusions reject any match. Text search is a case-insensitive
literal substring of the Markdown body. Results are newest first, then descending
ID. Date bounds accept UTC dates or RFC3339 timestamps: `since` is inclusive and
`until` exclusive. Reuse pagination cursors with the same filters.

Review defaults to public notes and covers all matching notes, without pagination.
CLI review and MCP `review_notes` reject `limit` and `cursor`; list/search keep
their paging inputs.
It emits deterministic Markdown, or structured material with `--json`, for you
or an agent to summarize. Public review excludes private notes and metadata
before computing counts or rendering. Publish selected private notes first if
you want their reviewed content in public material. Invalid sources in the
selected visibility prevent a complete review until corrected.

Duplicate IDs across public/private files do not change public list/review:
the public copy remains visible and conflict details stay local. Local diagnostics
report the duplicate, local reviews fail, and ID-based reads/edits/publications
reject the ambiguity. Repair the copied file's UUID and filename manually.
Cache recovery never rewrites the authoritative sources.

[All usage guides](README.md).
