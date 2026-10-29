# robostickies

Brief repository notes for humans and agents, available through a `clap` CLI and
an `rmcp` stdio server. Capture what gets in the way (`papercut`), what helps
(`paperclip`), and general observations with your own tags.

Markdown with YAML frontmatter is the source of truth. Shared files can travel
in Git history; private definitions and notes live in an ignored namespace.
A disposable, pure Rust `redb` database indexes the local source files.

Inspired by [papercuts](https://github.com/treygoff24/papercuts) and
[paperclips](https://github.com/dlorp/paperclips).

## Features

- [CLI access](docs/usage/getting-started.md) works from any directory in a
  repository, with file or stdin input and optional JSON output.
- [Notes and tags](docs/usage/getting-started.md#notes-and-tags) capture friction
  and useful successes, with custom tags and startup guidance.
- [Markdown storage](docs/usage/storage.md) keeps notes in editable files that
  can be shared and tracked in Git.
- [Private notes and publishing](docs/usage/privacy-and-configuration.md#private-notes-and-publication)
  keep local observations ignored and publish reviewed copies.
- [Configuration](docs/usage/privacy-and-configuration.md#configuration) controls
  privacy defaults by combining user settings with public and private repository
  settings.
- [Search and review](docs/usage/search-and-review.md) filter notes and produce
  a complete review of every match.
- [Shared editing](docs/usage/shared-editing.md) uses revision checks and file
  locking to protect edits from competing CLI and MCP clients.
- [Resolution and adoption](docs/usage/outcomes.md) track verified fixes and
  reusable guidance with outcome tags.
- [MCP tools and resources](docs/usage/mcp.md) let agents manage notes and read
  repository context and tag guidance.
- [Diagnostics and cache recovery](docs/usage/maintenance.md) validate note
  sources and Git privacy protections, and rebuild the disposable index.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The public library shares operations between the CLI and MCP server.
Integration tests start fresh stdio servers from Cargo's built binary and use
isolated stores. A clean checkout needs no installed executable, temporary
runner, or existing notes. They cover resources, recovery, concurrent CLI writes,
frontmatter/publication, context selection, and complete review across pages.
Test-owned CLI/Git subprocesses use asynchronous I/O with a ten-second deadline
and bounded kill/reap cleanup. A Rust subprocess fixture exercises timeout and
lock-release behavior without shell scripts. Shared-edit races use two independent
MCP servers and CLI writers against the same isolated store.
Live installed-connection checks remain necessary after MCP changes; follow the
[self-drive-testing skill](.agents/skills/self-drive-testing/SKILL.md).

Self-drive observations and generated test data stay private by default. Save
run records under `.robostickies/private/runs/`. Public fixtures are permitted
when a test needs public visibility. Before committing, remove synthetic notes
and public test output, including copies made to test publishing and temporary
copies used to promote guidance. Delete unreferenced fixture tag definitions
from both namespaces. Archiving or leaving fixtures unstaged is insufficient.
Keep genuine findings private by default and separate from fixture tags.

This repository also shares robostickies findings in Git. When asked to publish
genuine findings, review and sanitize their prose, preview and publish public
copies, and retain the private originals. Commit selected public findings
alongside the shared `.robostickies` store files. Keep fixtures and run records
out of commits, along with private files and the disposable cache. Reusable
lessons also belong in code, regression tests, or scoped agent guidance.
