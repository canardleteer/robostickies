# Development tasks

From this checkout, run `cargo xtask --help` to discover development commands.
The `xtask` crate shares the root workspace and lockfile. Ordinary `cargo run`,
`cargo build`, and `cargo install --path . --locked` still select robostickies.

The alias builds its launcher under `target/xtask`, letting workspace checks
rebuild xtask on Windows without replacing a running executable. Task
subprocesses still honor `CARGO_TARGET_DIR`.

`rust-toolchain.toml` selects stable Rust with the minimal profile, Clippy, and
rustfmt. The declared minimum remains Rust 1.90; LLVM tools are optional and
installed separately for coverage. Verify the minimum with:

```sh
cargo +1.90.0 check --locked --workspace --all-targets --all-features
cargo +1.90.0 test --locked --workspace --all-features
```

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
| `machete` | `cargo machete --skip-target-dir` |
| `test` | `cargo test --locked --workspace --all-features` (including doctests) |

GitHub Actions runs `cargo xtask check` on pushes to `main`, pull requests, and
manual runs using stable Rust with rustfmt, Clippy, and cargo-machete 0.9.2. The Linux job is blocking.
macOS and Windows jobs are informative and may fail without blocking the
workflow; the matrix continues after a failure. Supporting either additional
platform requires explicit adoption. Check names remain
`cargo xtask check (Linux)`, `cargo xtask check (macOS)`, and
`cargo xtask check (Windows)`.

The toolchain action runs before rust-cache, which restores dependencies and
Cargo-installed binaries before cargo-machete installation or builds. Both
`target/` and the separate `target/xtask` launcher directory are cached. Keys
include the compiler and platform alongside manifest and lockfile hashes.
Only pushes to this repository's `main` save caches; other jobs restore them.
Packaged executable checks also use the root target directory.

[Release automation](https://github.com/canardleteer/robostickies/blob/main/MAINTAINERS.md)
follows successful main-push checks through
an App-authenticated workflow dispatch carrying the checked SHA. Its repository
variable keeps it disabled until App and Trusted Publishing setup is complete.
The release-plz executable is pinned independently of weekly Action updates;
review its matching upstream documentation before upgrading.

The Linux check is required for updates to `main`, including administrator
updates. Deletion and non-fast-forward updates to `main` are blocked. No pull
request or reviewer requirement is imposed.

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

Feature options never go to rustfmt or machete. Machete receives no workspace,
lockfile, target, or feature flags and never rewrites manifests. Unused dependency
findings and execution errors fail the step. Only tools needed by the selected checks
are probed. Subprocesses search the caller's `PATH` before platform fallback
locations. Missing Rust components can be installed with:

```sh
rustup component add rustfmt
rustup component add clippy
cargo install --locked cargo-machete --version 0.9.2
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
The report includes build scripts, application, xtask, and profiling-example
sources. Test directories, dependencies, and generated build artifacts are excluded.

Run coverage locally as a merge request nears completion and inspect each
production source file. Aim for at least 95% line coverage per production file,
including useful failure paths. A lower result is acceptable when additional
tests would add little assurance: identify the uncovered behavior and explain the
exception in the review. Reports stay local; CI neither uploads coverage nor
enforces a percentage. Linux coverage does not measure macOS or Windows code.

## Interface snapshots

Canonical `cargo test` and `cargo xtask check` run the `insta` baselines in
`tests/snapshots/`. They cover CLI help, selected success/error envelopes, and
actual MCP tools/resources with schemas and annotations. Fixtures are synthetic;
only the named contract snapshot files enter the published package.
Help comparisons trim trailing terminal padding; dynamic IDs, revisions, and
timestamps in selected envelopes use explicit placeholders.

When a comparison fails, inspect the proposed `.snap.new` file, classify the
Rust, CLI, and MCP compatibility impact in the review, then deliberately accept
the intended baseline with `cargo insta review` (install `cargo-insta` if needed).
Never auto-accept baselines locally or in CI. Keep behavioral regression tests:
snapshots reveal changes but do not prove they are correct.

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
[self-drive testing skill](https://github.com/canardleteer/robostickies/blob/main/.agents/skills/self-drive-testing/SKILL.md)
covers installed CLI/MCP verification. A separate `mcp-test` command with a
fresh-agent feedback loop can be added by request; it is not part of these
checks.
