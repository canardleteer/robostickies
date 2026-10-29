---
name: self-drive-testing
description: Exercise robostickies through its installed MCP server while improving this repository. Use for agent-driven feature testing, recording papercuts and paperclips, reproducing defects, and verifying a batch of MCP changes after restart.
---

# Self-drive testing

Use the installed MCP connection as a user would. Read the repository instructions
and [feature matrix](references/test-matrix.md). Refine this skill from observed
behavior; keep run-specific results in the ignored private namespace.

## Start and record evidence

Call `get_context` first. Read tag topics and full startup guidance, inspect
configuration, and search existing observations before adding another. If needed,
call `init_store(agents=false)` and verify its files appear in this checkout.
Confirm `repository_root` in local context. Each MCP process keeps its startup
repository selection; changing the agent's working directory does not retarget
it. Public context omits the root entirely.

Self-drive observations, synthetic notes, fixture tag definitions, and run records
are private by default. Use MCP `private: true` or CLI `--private` when creating
test data. Use public fixtures only when the case requires public visibility.
Record the reason before creation, then save the returned IDs and paths for
cleanup. Test omitted-privacy defaults in isolated stores, without changing this
checkout's configuration. Keep genuine findings separate from fixture tags.

Retain the built-in private/cache exclusions. Add no broader ignore rules or Git
exclusions. Private files, cache data, fixtures, and run records stay out of
commits. Delete public test artifacts before any commit. Shared configuration,
guidance, and public tag definitions can accompany reviewed public findings.

Discover the current tools and resources to catch newly added features. Use a
unique run marker and a private testing tag for synthetic notes. Save the ledger
and all copied responses under `.robostickies/private/runs/<run>/` in the checkout.
Record returned IDs, revisions, current source paths, and fixture ownership,
including untagged notes and publication copies. For each case, record the expected
result before checking it. Identify the execution channel and keep failed or
pending checks visible; child-process success does not verify the agent's
installed connection.

Exercise ordinary workflows through MCP. Use temporary stores for malformed
sources, cache damage, filesystem hazards, configuration layers, and agent-file
integration. Start reproducible child processes with `--no-user-config`, or pass
an isolated `--user-config` file when testing precedence. Preserve this checkout's
configuration and existing observations.

## Use the tools accurately

Check `isError` before reading a tool result. Domain failures carry stable codes;
schema errors may be text-only. Successful results use `structuredContent`.
Creation and update return `note` inside an envelope, while `get_note` returns the
note itself. Inspect Markdown to verify paths and frontmatter.

Edits and publication, including previews, require `expected_revision` from the
latest successful read or save; CLI update/archive/unarchive/publish require
`--if-match`. There is no force override. On conflict, re-read and reconcile;
never attach a newly fetched revision to an unreviewed stale replacement.
A save with cache warnings still
created a note: retain its ID and repair the cache before retrying discovery.
Repeating creation would make a duplicate.

When explicitly testing visibility derivation, omit `private` and use a private
tag. Otherwise request private storage explicitly. Use `private=false` only for
an intentional public test case. It conflicts with a private tag; it does not
overrule that tag. Private notes remain private after tag removal. Publishing
creates a new public copy and retains the original;
preview with `dry_run=true` first. Metadata removal does not redact the body.
Use synthetic markers for privacy tests.

Browsing defaults to all visibility; review defaults to public. Test explicit
scopes and compare public results with the public fixture set, including counts
and diagnostics. Context/config resources and health reports are local inspection
material. Use the scoped tools when preparing public output.

Text search matches a case-insensitive literal substring of the body. Tag filters
require every selected tag; exclusions reject any match. `since` is inclusive and
`until` exclusive. Keep filters unchanged while following list/search cursors.
Review consumes all matches and accepts neither `limit` nor `cursor`.

Use separate MCP processes and CLI writers to test the shared operation lock.
Writers using one last-read revision must produce one successful edit and
`conflict` for the others. Check body, tags, status, visibility, and source paths
for unintended changes. Use temporary stores for lock contention and duplicate
IDs. A private duplicate must leave public list/review unchanged while local
inspection reports the conflict and ambiguous ID-based operations fail.

## Turn observations into fixes

A `papercut` explains how to reproduce a problem and what should have happened.
Describe the actual result and its cost, then suggest a correction if known.
A `paperclip` explains what helped and when to reuse it. Search before adding
another observation and update findings when evidence changes.

Keep synthetic fixtures distinct from genuine findings about this codebase.
Archive/restore are lifecycle test cases; afterward delete the synthetic fixtures.
Record genuine findings as private notes, and leave an unverified fix pending
rather than deleting or archiving its papercut.

When asked to share genuine findings, review and sanitize the prose, preview and
publish a public copy, and retain the private original. Record which public
findings are selected for Git history separately from the fixture ledger. Keep
private paths, personal configuration, and raw test responses out of public prose.

After a fix passes in the affected environment, retain `papercut`, add `resolved`,
record evidence, and archive. MCP fixes require installation, user-confirmed agent
restart, and checks through the installed connection first. Archived status alone
does not establish resolution.

For paperclips that warrant permanent instructions, place the reusable guidance
in the appropriately scoped AGENTS.md and review it. Retain `paperclip`, add
`adopted`, record the destination, and archive. Publish a sanitized public copy of
a private observation before promoting it into tracked guidance. Keep other
useful paperclips active without `adopted`.

Use existing tag/update APIs. Retain all existing tags and visibility; updates
replace the tag set. If an outcome definition is missing, create it explicitly
in the appropriate namespace. Public notes need a public definition even if
privacy defaults favor private tags. Private notes can use either namespace.
Repeat initialization to add missing predefined definitions without overwriting
custom guidance; initialization honors privacy defaults.

Find outcome history with `review_notes` and
`{"status":"archived","tags":["resolved"]}` or
`{"status":"archived","tags":["adopted"]}`. CLI equivalents are
`review --status archived --tag resolved` and
`review --status archived --tag adopted`. Select `scope` and qualified tag names
explicitly for private history.

Reproduce a defect before changing code. Add a regression test for observable
behavior and batch related fixes. Review new prose through slop-guard, validate
the skill, and run the repository quality checks. Before committing, stage
explicit source or skill paths and inspect the staged changes for generated data.

## Clean up before a commit

Delete all ledger-owned synthetic notes from both namespaces after the run,
including untagged notes, notes whose tags were cleared, and publication copies.
Delete owned fixture tag definitions only when no remaining notes reference them.
Keep original findings as private notes and retain needed evidence only under
`.robostickies/private/runs/`.

Remove synthetic notes and public test output before any commit, including
copies made to test publishing and temporary copies used to promote guidance.
Archiving a fixture or leaving it unstaged/untracked does not satisfy this rule.
Retain reviewed public findings selected for sharing, along with their public
definitions and shared configuration. Reusable lessons also belong in code,
regression tests, or appropriately scoped agent guidance.

Use the ownership ledger rather than tags alone to select deletions. Re-read
changed sources and reconcile revisions. If no deletion API is available, remove
only owned Markdown files under the existing store OS lock after checking their
last-read revisions; never delete the lock file. Rebuild and validate the index.
Verify that ledger-owned fixture sources are gone and public queries across all
statuses contain no test artifacts. Inspect the staged paths before committing.

## Install and wait

When a change affects MCP, finish the batch and verify it with fresh child
processes. Save a local checkpoint describing completed cases and the next live
calls, source revision/diff, fixture IDs, completed checks, and pending findings.
Run `cargo install --path . --locked --force` once for the batch, verify the
installed executable, record its hash, and mark the checkpoint awaiting restart.
Tell the user installation is ready. Keep the checkpoint private and default
fixtures retained across the restart to private storage. Public exceptions still
require cleanup before a commit.

Stop until the user confirms that every MCP process sharing this checkout has
restarted.

The existing server process continues running the old binary. After restart,
rediscover tools and read context. Repeat affected checks through the installed
connection before recording verification or closing the findings.
Confirm root selection/public omission, review schema and CLI behavior, then
initialize idempotently and check outcome topics, startup guidance, and resource
agreement. Repeat affected workflows and record evidence before applying outcomes.
If verification fails, keep the findings pending and repeat the fix/install/wait
cycle. After verification passes, complete fixture cleanup before committing
or pushing. No temporary public test data may remain at that point.
