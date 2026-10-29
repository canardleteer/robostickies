# Integration tests

Follow the [repository instructions](../AGENTS.md) as well as this file.

Use fresh stdio MCP processes from Cargo's built binary and isolated stores so
ordinary `cargo test` needs neither an installed executable nor pre-existing
notes.

Use the [shared process helper](support/mod.rs) for test-owned CLI and Git
subprocesses: it provides asynchronous I/O with a ten-second output deadline and
bounded kill/reap cleanup. Keep whole-test timeouts too.

Use the Rust subprocess fixture for timeout and lock-release checks.

Exercise shared-edit races through two independent MCP server processes and CLI
writers against the same isolated store, checking that the winning note retains
its fields and source paths while stale writers report conflicts.
