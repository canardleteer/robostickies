---
name: self-drive-testing
description: Exercise robostickies through its installed MCP server while improving this repository. Use for agent-driven feature testing, recording papercuts and paperclips, reproducing defects, and verifying a batch of MCP changes after restart.
---

# Self-drive testing

Use the installed MCP connection as a user would. Read the repository instructions
and [feature matrix](references/test-matrix.md). Refine this skill from observed
behavior; keep run-specific results in the local store.

## Start and record evidence

Call `get_context` first. Read tag topics and full startup guidance, inspect
configuration, and search existing observations before adding another. If needed,
call `init_store(agents=false)` and verify its files appear in this checkout.
Each MCP process keeps its startup repository selection; changing the agent's
working directory does not retarget it.

Keep every generated `.robostickies/` file and run result out of commits at this
development stage. Retain the built-in private/cache exclusions. Add no broader
ignore rules or Git exclusions.

Discover the current tools and resources to catch newly added features. Use a
unique run marker and a defined testing tag for synthetic notes. Save a ledger
under `.robostickies/runs/` with returned IDs and revisions. Keep copied private
responses in `private/runs/`; use outcomes in the public run summary. For each case, record
the expected result before checking it. Identify the execution channel and keep
failed or pending checks visible; child-process success does not verify the
agent's installed connection.

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

Use the revision from the latest successful read or edit for
`expected_revision`. Re-read after a conflict. A save with cache warnings still
created a note: retain its ID and repair the cache before retrying discovery.
Repeating creation would make a duplicate.

Omit `private` to derive visibility from tags. Explicit `private=false` conflicts
with a private tag; it does not overrule that tag. Private notes remain private
after tag removal. Publishing creates a new public copy and retains the original;
preview with `dry_run=true` first. Metadata removal does not redact the body.
Use synthetic markers for privacy tests.

Browsing defaults to all visibility; review defaults to public. Test explicit
scopes and compare public results with the public fixture set, including counts
and diagnostics. Context/config resources and health reports are local inspection
material. Use the scoped tools when preparing public output.

Text search matches a case-insensitive literal substring of the body. Tag filters
require every selected tag; exclusions reject any match. `since` is inclusive and
`until` exclusive. Keep filters unchanged while following cursors. Review consumes
all matches even with a small page size and rejects a supplied cursor.

## Turn observations into fixes

A `papercut` explains how to reproduce a problem and what should have happened.
Describe the actual result and its cost, then suggest a correction if known.
A `paperclip` explains what helped and when to reuse it. Search before adding
another observation and update findings when evidence changes.

Keep synthetic fixtures distinct from findings about this codebase. Archive only
the fixture IDs owned by the run after their lifecycle tests finish. Leave an
unverified fix pending rather than archiving its papercut.

Reproduce a defect before changing code. Add a regression test for observable
behavior and batch related fixes. Review new prose through slop-guard, validate
the skill, and run the repository quality checks. Before committing, stage
explicit source or skill paths and inspect the staged changes for generated data.

## Install and wait

When a change affects MCP, finish the batch and verify it with fresh child
processes. Save a local checkpoint describing completed cases and the next live
calls. Run `cargo install --path . --locked --force`, verify the installation,
and tell the user it is ready.

Stop until the user confirms the agent restart.

The existing server process continues running the old binary. After restart,
rediscover tools and read context. Repeat affected checks through the installed
connection before recording verification or closing the findings.
