# Working on robostickies

Robostickies stores repository notes in Markdown with YAML frontmatter. The
Markdown files are authoritative; the redb index is disposable and ignored by
Git. Keep private definitions and notes in their separate `private/` namespace.
Public review must never include private notes, tags, counts, or diagnostics.

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
