# Development tasks

From this checkout, run `cargo xtask --help` to discover development commands.
The `xtask` crate shares the root workspace and lockfile. Ordinary `cargo run`,
`cargo build`, and `cargo install --path . --locked` still select robostickies.

The alias builds its launcher under `target/xtask`, letting workspace checks
rebuild xtask on Windows without replacing a running executable. Task
subprocesses still honor `CARGO_TARGET_DIR`.

If a locked local install cannot reach the registry and the dependencies are
cached, retry `cargo install --path . --locked --offline`.

## Checks

```sh
cargo fmt --all
cargo xtask check
# The same parser and checks:
cargo xtask ci
```

Checks stop at the first failure and run in this order:

| Step | Command |
| --- | --- |
| `fmt` | `cargo fmt --all -- --check` |
| `check` | `cargo check --locked --workspace --all-targets --all-features` |
| `clippy` | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` |
| `test` | `cargo test --locked --workspace --all-features` (including doctests) |

GitHub Actions runs `cargo xtask check` on pushes to `main`, pull requests, and
manual runs using stable Rust with rustfmt and Clippy. The Linux job is blocking.
macOS and Windows jobs are informative and may fail without blocking the
workflow; the matrix continues after a failure. Supporting either additional
platform requires explicit adoption. Check names remain
`cargo xtask check (Linux)`, `cargo xtask check (macOS)`, and
`cargo xtask check (Windows)`.

Ordinary merges require the Linux check after its first successful run.
Administrators may push directly during pipeline bring-up. No pull request or
reviewer requirement is imposed.

Select checks with either `--only` or `--exclude`:

```sh
cargo xtask check --only=fmt,clippy,test
cargo xtask ci --exclude=clippy
```

Selections run in the table's order, with duplicates removed. Empty selections,
unknown names, and excluding every check are errors. Formatting checks do not
rewrite files; use `cargo fmt --all` to apply formatting.

Checks and coverage enable all features by default. Supplying a feature option
replaces that default. Cargo's `--all-features`, `--features <FEATURE,...>`, and
`--no-default-features` are supported; `--features` may combine with
`--no-default-features`. `--all-features` conflicts with both. The repository
currently declares no optional features, so a useful alternative is:

```sh
cargo xtask check --only=check --no-default-features
```

Feature options never go to rustfmt. Only tools needed by the selected checks
are probed. Missing Rust components can be installed with:

```sh
rustup component add rustfmt
rustup component add clippy
```

## Coverage

```sh
cargo install --locked cargo-llvm-cov
rustup component add llvm-tools-preview
cargo xtask coverage
cargo xtask coverage --open
# Also regenerates the report before opening:
cargo xtask coverage-open
```

[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov#readme) is the default
engine. Coverage runs the workspace's tests across all targets and writes
`target/coverage/llvm-cov/html/index.html`. The xtask disables automatic Rust
component installation, so install the LLVM tools explicitly when needed.
The report includes application, xtask, and profiling-example sources. Test
directories, dependencies, and build artifacts are excluded.

Run coverage locally as a merge request nears completion and inspect each
production source file. Reports stay local; CI neither uploads coverage nor
enforces a percentage. Linux coverage does not measure macOS or Windows code.

To use [Tarpaulin](https://github.com/xd009642/tarpaulin#readme):

```sh
cargo install --locked cargo-tarpaulin
cargo xtask coverage --engine=tarpaulin
cargo xtask coverage-open --engine=tarpaulin --no-default-features
```

Its report is `target/coverage/tarpaulin/tarpaulin-report.html`. Both engines
accept the same feature options as `check`. Each run removes its old HTML entry
point before generation and verifies that a new report exists. If opening fails,
the report remains available at the printed path for manual viewing.

## Profiling

```sh
cargo install --locked samply
cargo xtask profile
cargo xtask profile --open
# Also records a fresh profile before opening:
cargo xtask profile-open
```

The workload builds [profile_notes](../../examples/profile_notes.rs) with the
release-derived `profiling` Cargo profile, debug symbols enabled, and stripping
disabled. The xtask selects the exact example executable from Cargo JSON output,
including when Cargo uses a custom target directory.

The example creates 256 synthetic private Markdown notes in a temporary store,
then repeats index rebuilding, filtered listing, and complete review eight
times. User configuration is disabled. The temporary store is removed when
the workload finishes or returns an error; the checkout's notes are never read.
You can run the workload without a profiler:

```sh
cargo run --locked --profile profiling --example profile_notes
```

[Samply](https://github.com/mstange/samply#readme) records to
`target/profile/profile.json`. Opening starts Samply's local viewer and browser;
stop the viewer with Ctrl-C. On Linux the command prints the current
`kernel.perf_event_paranoid` value. Consult Samply's platform guidance if
recording fails with a permission error; the xtask does not change that setting.

Reports stay under the root `target/` directory even when build artifacts use
`CARGO_TARGET_DIR`. No container image workflow is provided. The existing
[self-drive testing skill](../../.agents/skills/self-drive-testing/SKILL.md)
covers installed CLI/MCP verification. A separate `mcp-test` command with a
fresh-agent feedback loop can be added by request; it is not part of these
checks.
