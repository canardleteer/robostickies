# robostickies

Enables taking brief repository notes for humans and agents, during development
and available through a CLI interface and an MCP `stdio` server. Capture what
gets in the way (`papercut`), what helps (`paperclip`), and general
observations or todo tasks with your own tags.

Markdown with YAML frontmatter is the source of truth. Shared files can travel
in Git history; private definitions and notes live in an ignored namespace.
A disposable, pure Rust `redb` database indexes the local source files.

Inspired by [papercuts](https://github.com/treygoff24/papercuts) and
[paperclips](https://github.com/dlorp/paperclips).

> [!WARNING]
> Clanker generated code, without commitment to a stable API yet.

## Quick Start

Requires Rust 1.90 or newer. Install from Git and initialize the target repository:

```sh
cargo install --git https://github.com/canardleteer/robostickies.git --locked
cd /path/to/repository
robostickies init
robostickies context
```

Register with one agent:

```sh
# Codex
codex mcp add robostickies -- robostickies mcp

# Claude Code
claude mcp add --transport stdio --scope local robostickies -- robostickies mcp
```

Start that agent in the repository and use `/mcp` to check the connection. See
[MCP setup and tool approvals](docs/usage/mcp.md) for details, or
[keep the entire store local](docs/usage/privacy-and-configuration.md#keep-the-entire-store-local)
to use it without adding tracked integration files.

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
- [Development tasks](docs/usage/development.md) provide workspace checks,
  coverage reports, and profiling against synthetic notes.

## Development

The public library shares operations between the CLI and MCP server. Integration
tests use Cargo's built binary and isolated stores.

```sh
cargo fmt --all
cargo xtask check
```

`cargo xtask ci` runs the same checks. Use `--only=fmt,clippy,test` or
`--exclude=clippy` to select checks. The
[development guide](docs/usage/development.md) covers feature selection,
coverage, profiling, and optional tool installation. No container engine is
required.

Agent development and verification instructions are in [AGENTS.md](AGENTS.md).
Integration test conventions are in [tests/AGENTS.md](tests/AGENTS.md).
