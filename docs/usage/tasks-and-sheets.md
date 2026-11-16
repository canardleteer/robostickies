# Tasks and saved sheets

Tasks are notes with structured work state, prerequisites, and optional deferral.
Saved sheets hold ordered task UUIDs and a body with context and instructions.
Both use the ordinary note library, CLI, and MCP operations. Tags, checkboxes,
and numbers in note bodies do not classify work or create relationships.

New stores use `v1alpha5`. In an older store, preview and explicitly apply the
[store upgrade](upgrades.md) before writing task or sheet metadata. The preview
rejects existing custom `task` or `sheet` keys; relocate those values first.

## Create and discover work

```sh
robostickies add "Design the schema." --task --private
robostickies add "Write the migration." --task --requires "$SCHEMA_ID" --private
robostickies tasks list --ready --scope all --json
robostickies tasks list --blocked --scope all --json
```

Use actual UUIDs from successful responses. New tasks default to `todo`.
The four work states are `todo`, `in_progress`, `done`, and `cancelled`.
Note status (`active` or `archived`) remains independent.

Readiness requires an active, unfinished task, no active deferral, and every
transitive prerequisite in `done`. Only completion satisfies a prerequisite:
cancelled and unfinished archived tasks still block descendants. Archived done
tasks satisfy dependencies. If a task is completed before its prerequisites,
descendants still wait for the unfinished ancestors. Starting and completing
blocked work succeeds and retains its prerequisite relationships.

`tasks list` defaults to active `todo` and `in_progress` tasks across all
visibility. Combine ordinary note filters with repeated `--state`, or use
`--all-task-states`. `--ready`, `--blocked`, and `--deferred` accept an optional
`=true` or `=false`. All selected filters combine with AND. Queries return newest
creation time/UUID first; `--limit` and `--cursor` work as in note browsing.
Filters and pagination select rows without excluding prerequisite evidence.

Task results contain the source note and revision, decoded task metadata,
readiness, active deferral, unsatisfied prerequisite UUIDs, and one deterministic
relationship path per blocker. Each response includes `evaluated_at` and scoped
diagnostics. Readiness is a current view; re-evaluate before acting.

The MCP equivalents are `create_note` with a `task` specification and
`list_tasks` with the same filters:

```json
{"body": "Design the schema.", "private": true, "task": {}}
```

```json
{"scope": "all", "ready": true}
```

MCP task-state filters use `states: ["todo", "in_progress"]`. Use
`all_task_states: true` to include terminal states; it conflicts with `states`.

## Edit and defer tasks

Get the task's current revision with `get` / `get_note`. Every edit requires
`--if-match` / `expected_revision`. On conflict, re-read and reconcile.

```sh
robostickies update "$TASK_ID" --task-state in_progress --if-match "$REVISION"
robostickies update "$TASK_ID" --defer --if-match "$REVISION"
robostickies update "$TASK_ID" --defer-until 2026-11-20 --if-match "$REVISION"
robostickies update "$TASK_ID" --resume-task --if-match "$REVISION"
robostickies update "$TASK_ID" --task-state done --if-match "$REVISION"
```

Refresh `REVISION` after each successful save. `--defer` waits for an explicit
resume. Dates accept UTC `YYYY-MM-DD` or RFC3339; a date means midnight UTC.
A dated deferral is inactive exactly at its timestamp. Querying never changes
the note or its revision when the date passes. Deferral and prerequisite
blocking are independent and may both apply.

Done/cancelled transitions clear deferral in the same save. Reopening does not
restore an old deferral; deferring terminal work is invalid. `--defer`,
`--defer-until`, and `--resume-task` conflict with one another.

Repeated `--requires` replaces prerequisites; `--clear-requires` empties them.
MCP task patches use optional `state`, `requires`, `deferral`, and
`clear_deferral`. Omitted fields retain their values; null is invalid. An empty
`requires` array clears prerequisites. For example, pass this task patch along
with the task UUID and last-read revision to `update_note`:

```json
{"state": "in_progress", "deferral": {"kind": "until", "until": "2026-11-20"}}
```

Task and sheet mappings reject unknown fields, duplicate UUIDs, self-links,
non-task targets, unavailable targets, and cycles. Relationship edits validate
the current graph under the same lock as revision comparison and source saving.
Unrelated defects do not prevent work in another component. State/body edits
may record reality while existing relationship problems remain visible.

## Save and render a sheet

```sh
robostickies add "Prepare the storage change." --sheet --private \
  --item "$SCHEMA_ID" --item "$MIGRATION_ID" --ordering preferred
robostickies sheets render "$SHEET_ID" --scope all
```

Use `--json` for structured rows, prerequisite context, diagnostics, completeness,
and rendered Markdown from the same evaluation. MCP uses `create_note` with
`sheet: {"items": ["<task UUID>"], "ordering": "preferred"}` and
`render_task_sheet` with `{"id": "<sheet UUID>", "scope": "all"}`.

Both `display` (the default) and `preferred` retain authored membership order.
Only a complete preferred sheet suggests its first ready item. A blocked first
item can be followed by a ready recommendation. Membership positions never
create prerequisites; strict sequences require explicit `requires` edges.
A task can appear in several sheets with different orders.

Rendering includes full task bodies, source links, UUIDs, revisions, work and
archival state, blockers, and deferral reasons. Prerequisites outside the selected
rows appear as context. Links are relative to `.robostickies`; exported sheets
need that base to resolve them. Checked boxes mean done. Cancelled tasks remain
unchecked and labelled. Checkboxes are a read-only view; update source tasks by
UUID and revision, then render again.

Rendering defaults to public scope. Use explicit `--scope all` for mixed local
sheets, or `--scope private` for private rows. Public members outside a private
row scope remain placeholders; public prerequisite evidence can still explain
private work. Missing, malformed, ambiguous, or out-of-scope items make the
result incomplete and suppress the next-item recommendation.
Each unavailable member is diagnosed once per sheet. Prerequisite errors in task
sources are reported separately.

Sheet patches replace membership with repeated `--item`, or `--clear-items`;
`--ordering` changes presentation. MCP uses optional `items` and `ordering`.
Unmentioned fields retain their values; empty membership is valid.

## Classification, privacy, and publishing

Task/sheet fields remain in `metadata` in ordinary note responses. Adding typed
metadata to an ordinary note explicitly converts it while retaining its UUID
and body. `--clear-task` / `clear_task` and `--clear-sheet` / `clear_sheet` remove
classification. Reconcile incoming prerequisites and sheet membership before
clearing a task. To switch kinds, clear the existing classification in a
separate edit, then convert. Set and clear options conflict.

Public tasks and sheets reference only public tasks. Private sources can
reference either namespace. Moving a public task into private storage requires
reconciling incoming public references first, including moves caused by private
tags. No edit silently rewrites another note or removes a relationship.

Public views resolve public sources only. Private contents, duplicates, paths,
and diagnostics cannot change public graph results. A public hand-edited
reference to an unavailable UUID has the same unresolved result whether that
UUID exists privately or nowhere. `doctor` and graph views diagnose direct
source edits without repairing them.

Publication defaults to an ordinary public copy with a new UUID and stripped
extra metadata; the private original remains. Explicitly supply `--task` or
`--sheet` options (MCP `task` or `sheet` complete specifications) to publish
reviewed structured data. All supplied targets must be public tasks. Private
references are never copied, remapped, or dropped silently. Preview with
`--dry-run` / `dry_run: true`; both preview and publication require the current
source revision. Review prose and hand-written links too.

[All usage guides](README.md).
