# Development tasks

Follow the [root instructions](../AGENTS.md). This crate is a root workspace
member; `.cargo/config.toml` maps `cargo xtask` to
`cargo run --package xtask --target-dir target/xtask --`. The separate launcher
directory lets workspace checks rebuild xtask on Windows without replacing a
running executable. Task subprocesses still honor `CARGO_TARGET_DIR`. The root
package remains the default member.

## Command policy

- `check` runs `fmt`, `check`, `clippy`, `machete`, and `test` in that order, failing at the
  first error. `ci` is a visible alias of the same parser and implementation.
  `--only` and `--exclude` accept comma-separated step names, reject empty or
  unknown selections, and retain registry order after deduplication.
- Feature-aware checks and coverage default to `--all-features` only when no
  feature option is supplied. Forward `--features` and `--no-default-features`
  with Cargo semantics; they may be combined. Formatting and machete receive no
  features. Machete runs `cargo machete --skip-target-dir` without Cargo's
  workspace, lockfile, or target flags; findings and execution failures stop checks.
- Check commands use the lockfile and whole workspace. Compilation and Clippy
  cover all targets; tests use `cargo test --workspace` to retain doctests.
  Formatting checks without rewriting; Clippy denies warnings.
- Keep CI/workflows and script callers aligned when these commands change.
  Keep the xtask independent of providers; it must not parse CI declarations.
- `coverage` supports `llvm-cov` (default) and `tarpaulin`. Reports are
  `target/coverage/llvm-cov/html/index.html` and
  `target/coverage/tarpaulin/tarpaulin-report.html`. Both `coverage --open` and
  `coverage-open` generate a fresh report before opening it.
- LLVM reports include build scripts, application, xtask, and profiling example. Override
  the engine's default filename exclusions so examples remain visible; omit
  test directories, dependencies, and build artifacts. The profiling example
  participates in tests through its Cargo target configuration.
- `profile` builds the `robostickies` example `profile_notes` with the
  release-derived `profiling` profile and selects its executable from Cargo
  JSON messages using the exact package, name, and target kind. It seeds 256
  synthetic private Markdown notes in a temporary store with user configuration
  disabled, then repeats index rebuilds, filtered queries, and reviews eight
  times. It accepts no workload arguments, never reads this checkout's notes,
  and removes the temporary store on success or error. Samply saves
  `target/profile/profile.json`. Both `profile --open` and `profile-open` record
  before loading the profile.
- `image` is intentionally omitted: this repository has no owned container
  image and does not need an image workflow.
- Optional `mcp-test` and its companion skill are not adopted. The existing
  self-drive skill governs installed CLI/MCP verification. Add the separate
  fresh-agent evaluator only when the user requests it.

Repository-specific tasks may extend this interface. Keep paths and workloads
in `src/policy.rs` and reconcile these instructions and the usage guide.

## Tools and implementation

Probe only tools needed by the selected operation. Distinguish missing programs
from programs that launch unsuccessfully and retain concise diagnostics. Use
these installation instructions when applicable:

```sh
rustup component add rustfmt
rustup component add clippy
rustup component add llvm-tools-preview
cargo install --locked cargo-llvm-cov
cargo install --locked cargo-machete --version 0.9.2
cargo install --locked cargo-tarpaulin
cargo install --locked samply
```

Coverage must not install Rust components automatically. For browser failures,
show the report path for manual opening. Read Linux's
`/proc/sys/kernel/perf_event_paranoid` before recording and refer to
[Samply's platform guidance](https://github.com/mstange/samply#readme) for
permission errors; never change privileged settings automatically.

Keep subprocess programs and OS argument vectors separate. Keep `main.rs` thin,
use Clap derive, and retain parser construction, selection, feature forwarding,
alias, report freshness, artifact identity, and tool failure tests. These tasks
are synchronous; add Tokio and tracing only if actual async work is introduced,
with subscriber setup at the binary boundary.

Prefer this crate for typed, portable Cargo orchestration over new Python
wrappers. Inspect overlapping Python scripts and propose a migration before
replacing mature scripts or their callers; obtain the user's approval first.
Retain Python when its libraries or data-processing strengths matter. There
are currently no Python orchestration exceptions.

## Verification

Run from the repository root:

```sh
cargo fmt --all
cargo xtask check
cargo xtask ci --only=fmt
cargo xtask check --only=check --no-default-features
```

Exercise coverage and profiling when changing their implementations and the
selected external tools are available. Test command construction and failure
paths without requiring every coverage engine, Samply, or a browser.
