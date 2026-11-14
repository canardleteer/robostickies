<p align="center">
  <img src="docs/assets/robostickies-desk.png" alt="A robot holding a sticky note at a warmly lit desk covered with notes, beside a code editor and a sleeping cat." width="100%">
</p>

# robostickies

[![crates.io](https://img.shields.io/crates/v/robostickies.svg)](https://crates.io/crates/robostickies)
[![Workspace checks](https://github.com/canardleteer/robostickies/actions/workflows/check.yml/badge.svg?branch=main&event=push)](https://github.com/canardleteer/robostickies/actions/workflows/check.yml)

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

Requires Rust 1.90 or newer. Install robostickies and initialize the target repository:

```sh
cargo install robostickies --locked
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
- [Build identification](docs/usage/getting-started.md#version-and-build-identification)
  reports compiled version, commit, source state, target, compiler, and profile
  through the CLI and callable MCP `version` tool.
- [Notes and tags](docs/usage/getting-started.md#notes-and-tags) capture friction
  and useful successes, with custom tag names, optional descriptions and usage
  guidance, and CLI help examples.
- [Markdown storage](docs/usage/storage.md) keeps notes in editable files that
  can be shared and tracked in Git, preserving YAML metadata and comments during edits.
- [Body hints](docs/usage/getting-started.md#bodies-and-optional-format-hints)
  describe nonempty UTF-8 text with optional advisory metadata.
- [Private notes and publishing](docs/usage/privacy-and-configuration.md#private-notes-and-publication)
  keep local observations ignored and publish reviewed copies.
- [Configuration](docs/usage/privacy-and-configuration.md#configuration) controls
  privacy defaults by combining TOML 1.1 user settings with public and private
  repository settings.
- [Search and review](docs/usage/search-and-review.md) filter notes and produce
  a complete review of every match.
- [Shared editing](docs/usage/shared-editing.md) uses revision checks and file
  locking to protect edits from competing CLI and MCP clients.
- [Resolution and adoption](docs/usage/outcomes.md) track verified fixes and
  reusable guidance with outcome tags.
- [MCP tools and resources](docs/usage/mcp.md) let agents manage notes and read
  repository context, startup upgrade advice, and tag guidance.
- [Diagnostics and cache recovery](docs/usage/maintenance.md) validate note
  sources and Git privacy protections, and rebuild the disposable index.
- [Store versions and upgrades](docs/usage/upgrades.md) report legacy formats
  with structured advice and optional deferral, and migrate explicitly with
  verified private recovery snapshots, including versioned YAML repairs.
- [Development tasks](docs/usage/development.md) provide workspace checks,
  unused-dependency checks, coverage reports, and profiling against synthetic notes.

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

Agent development and verification instructions are in [AGENTS.md](https://github.com/canardleteer/robostickies/blob/main/AGENTS.md).
Integration test conventions are in [tests/AGENTS.md](https://github.com/canardleteer/robostickies/blob/main/tests/AGENTS.md).
The [maintainer guide](https://github.com/canardleteer/robostickies/blob/main/MAINTAINERS.md)
covers release setup and publishing.

Licensed under the [MIT License](LICENSE).
