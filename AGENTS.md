# Working on robostickies

Robostickies stores repository notes in Markdown with YAML frontmatter. The
Markdown files are authoritative; the redb index is disposable and ignored by
Git. Keep private definitions and notes in their separate `private/` namespace.
Public review must never include private notes, tags, counts, or diagnostics.

## Session orientation

Install this checkout with `cargo install --path . --locked` and use the CLI or
installed MCP server while developing robostickies itself.

At the start of work, run `robostickies context` or call MCP `get_context`.
Read tag topics and the full guidance for startup tags, and confirm the selected
repository in local context. Record concrete friction with `papercut` and useful
successes with `paperclip` while continuing the task. Publish a sanitized public
copy of a private observation before promoting it into tracked guidance.

Track the shared `.robostickies` configuration, guidance, public tag definitions,
and reviewed public findings in Git, just as in any other repository. Keep the
private namespace and cache ignored with the built-in exclusions. When asked to
share findings, review and sanitize the prose, preview and publish the public
copy, and retain the private original.

## Self-drive verification

Run new features and behavior changes through the affected
[self-drive-testing](.agents/skills/self-drive-testing/SKILL.md) workflows before
finalizing them, alongside regression tests. Exercise the changed feature through
the installed CLI or MCP connection. Keep papercuts and provisional findings
private until their evidence has been finalized for review.

Installed MCP code changes need verification through a newly started server.
You may launch the skill in a separate fresh agent process with its own MCP
server bound to this checkout while the current session continues. This
satisfies the restart requirement for verification without restarting the current
session. If using the current session for those checks, wait for the user's
restart confirmation first. Restart an older MCP process before using it for
behavior changed by the installation.

## Test data

Self-drive observations, test fixtures, and run records are private by default.
Keep run records in `.robostickies/private/runs/`. Create public fixtures only
when a test requires public visibility, and record their IDs and paths for cleanup.
Before any commit, remove synthetic notes and public test output, including
copies made to test publishing and temporary copies used to promote guidance.
Delete unreferenced test tag definitions from both namespaces. Archiving fixtures
or leaving them unstaged is not cleanup. Keep genuine findings private by default,
separate from fixture tags; reviewed public findings selected for sharing may
remain in Git. Durable lessons also belong in code, regression tests, or scoped
agent guidance.

## Rust CLI conventions

- Use `clap` derive for parsing and retain a `CommandFactory::debug_assert()`
  test for the root parser.
- Implement MCP directly with `rmcp`, using stdio. Do not use `clap-mcp`.
- Share behavior through the public library; keep process concerns in the binary.
- Use Tokio for async work and `tracing` for diagnostics. Honor `RUST_LOG` and
  write logs to stderr so stdout remains a clean MCP transport.
- Document public APIs, including errors and observable side effects.
- Use idiomatic Rust with typed errors. Prefer composition and straightforward functions.
- Test behavior and meaningful failure paths, especially privacy, publishing,
  concurrency, and source/index consistency. Avoid unsafe code.
- Before completion or a commit, run `cargo fmt --all`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
  `cargo test --workspace --all-features`. `cargo xtask check` runs these checks
  with formatting in check mode, plus compilation of all targets.

## Development tasks

Use `cargo xtask check` (also `cargo xtask ci`), `coverage` / `coverage-open`,
and `profile` / `profile-open` as the canonical development handles. Follow
[xtask/AGENTS.md](xtask/AGENTS.md) for work under `xtask/` and see the
[development guide](docs/usage/development.md) for command usage.
The `image` handle is omitted because this repository has no container image
workflow. The optional fresh-agent `mcp-test` evaluator is not adopted; add it
only by user request, alongside the existing self-drive workflow.

Run `cargo xtask coverage` locally as a merge request nears completion. Review
per-file coverage and meaningful failure paths; coverage is not a CI gate.
Linux is the supported platform. macOS and Windows checks are informative;
support for either platform requires explicit adoption.

Keep CI/workflow declarations and script callers aligned with canonical xtask
behavior when it changes. The xtask stays provider-agnostic and must not inspect
CI declarations. Prefer typed, portable `cargo xtask` commands for Cargo-aware
development orchestration over new Python wrappers. Inspect overlapping Python
scripts and propose a migration; obtain user approval before replacing mature
scripts or changing their callers. Retain Python when its libraries or
data-processing strengths are material.

## Agent documentation

Update the [usage documentation](docs/usage/README.md) whenever user-facing
behavior changes. Keep the root README's Features list and links aligned with
those guides.

Maintain this file according to the [AGENTS.md standard](https://agents.md/).
Keep guidance portable across agent clients and preserve the format of existing
agent instructions when implementing opt-in integration in other repositories.
Maintain project-local skills under `.agents/skills/` according to the
[Agent Skills specification](https://agentskills.io/specification).
For changes under `src/`, follow [src/AGENTS.md](src/AGENTS.md).
For tests, follow [tests/AGENTS.md](tests/AGENTS.md).
