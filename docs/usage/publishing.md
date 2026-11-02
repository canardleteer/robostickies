# Publishing release candidates

The temporary `publish-bootstrap.yml` workflow publishes package `robostickies`
to `crates-io`. The executable is also named `robostickies`. The xtask crate
remains unpublished. MIT licensing and the README ship in the crate, along with
build support and Rust sources, plus tests and examples. Usage guides and assets
also ship in the crate.
The entire `.robostickies` store, agent instructions and skills, workflows,
caches, and xtask are excluded by the package allowlist.

## Prepare a candidate

1. Change the application version in `Cargo.toml` and update `Cargo.lock`.
   Later candidates require an explicit new SemVer, such as `0.1.1-rc.2`.
   Follow the [format version policy](upgrades.md#format-version-policy) and
   [data version policy](upgrades.md#data-version-policy) independently.
2. Run `cargo fmt --all`, `cargo xtask check`, and fresh `cargo xtask coverage`.
   Review per-file line, function and region coverage with meaningful failure paths,
   including build scripts; generated and dependency code remain separate.
3. Verify Rust 1.90 compatibility and exercise the installed CLI and a newly
   started MCP connection through the repository's self-drive workflow. Keep
   evidence private and remove owned synthetic fixtures before committing.
4. Review help and comments, then prose and commit text with slop-guard. Check links
   and inspect `cargo package --locked --package robostickies --list`, including
   the README image at `docs/assets/robostickies-desk.png`.
5. Commit and push without rewriting history. For this first release, require
   successful workspace checks, version flags, and package verification on
   Linux plus macOS and Windows. macOS and Windows remain informative for ordinary
   development; this release requirement does not alter the platform policy or
   active rulesets.

## Dispatch and verify

Run the workflow on `main`, initially with its default `dry_run=true`:

```sh
gh workflow run publish-bootstrap.yml --ref main -f version=0.1.1-rc.1 -F dry_run=true
```

Inspect the resulting Actions run. The workflow checks that the requested
version matches the manifest and is absent from the registry, runs
`cargo xtask check`, inspects the package allowlist, and performs a locked publish
dry run. Dispatches on other branches fail. Publication runs share a concurrency
group; active runs are never canceled.

After the dry run succeeds, dispatch the real publication:

```sh
gh workflow run publish-bootstrap.yml --ref main -f version=0.1.1-rc.1 -F dry_run=false
```

The repository secret `FIRST_CRATES_IO_RELEASE` must contain a crates.io token
authorized to publish `robostickies`. Only the upload step maps it to
`CARGO_REGISTRY_TOKEN`; GitHub permissions remain read-only. An empty credential
or an already-published version fails clearly. The workflow issues one publish
command, creates no Git tags or GitHub Releases, and leaves the secret in place.

If an upload fails after it might have reached crates.io, inspect the registry
before retrying. The workflow reports the sparse index state on failure. If the
version exists, verify that publication instead of dispatching another upload.
If registry state remains uncertain, wait and inspect again; never retry blindly.

Confirm availability at [crates.io](https://crates.io/crates/robostickies/0.1.1-rc.1).
Install the exact version into an isolated prefix, run both version flags with
Git absent from `PATH`, and confirm the executable name and packaged provenance:

```sh
cargo install robostickies --version '=0.1.1-rc.1' --locked --root /tmp/robostickies-release-check
```

Remove that temporary prefix after verification. Repeat the preparation and both
dispatches for later RCs. Replace this bootstrap workflow with release-plz once
the release process is adopted; release-plz is not installed by this procedure.

[All usage guides](README.md).
