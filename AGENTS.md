# Working on robostickies

Robostickies stores repository notes in Markdown with YAML frontmatter. The
Markdown files are authoritative; the redb index is disposable and ignored by
Git. Keep private definitions and notes in their separate `private/` namespace.
Public review must never include private notes, tags, counts, or diagnostics.

## Session orientation

At the start of work, run `robostickies context` or call MCP `get_context`.
Read tag topics and the full guidance for startup tags, and confirm the selected
repository in local context. Record concrete friction with `papercut` and useful
successes with `paperclip` while continuing the task. Publish a sanitized public
copy of a private observation before promoting it into tracked guidance.

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
  `cargo test --workspace --all-features`.

## Agent documentation

Maintain this file according to the [AGENTS.md standard](https://agents.md/).
Keep guidance portable across agent clients and preserve the format of existing
agent instructions when implementing opt-in integration in other repositories.
Maintain project-local skills under `.agents/skills/` according to the
[Agent Skills specification](https://agentskills.io/specification). For live MCP
testing, use [self-drive-testing](.agents/skills/self-drive-testing/SKILL.md).
For changes under `src/`, follow [src/AGENTS.md](src/AGENTS.md).
