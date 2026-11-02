# Maintainers

See the [development guide](docs/usage/development.md) for workspace checks,
coverage, Rust 1.90 verification, and platform policy. Rebase-merge checked PRs
to preserve individual commits; never squash them.

## Releases

Classify compatibility for each interface when reviewing a release:

| Contract | Examples requiring compatibility review |
| --- | --- |
| Rust library | Public types, struct literals, signatures, errors, and side effects |
| CLI | Commands, flags, required inputs, output envelopes, and exit codes |
| MCP | Tool/resource names, schemas, required arguments, results, and annotations |

While the package is `0.x`, use a minor release for breaking changes to any of
these contracts; use patch releases for compatible fixes. Review snapshot changes
explicitly and record which contracts changed. A new baseline does not establish
compatibility by itself.

Classify configuration interpretation and stored-data compatibility separately
using the [format and data policies](docs/usage/upgrades.md#format-version-policy).
Package releases do not automatically require migrations. Advisory metadata that
older writers preserve needs no new data version.

Release automation is enabled with
[release-plz](https://github.com/release-plz/release-plz/tree/release-plz-v0.4.2/website/docs)
and [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing).
Publication runs on Linux. Consult the matching upstream documentation before
upgrading the pinned release-plz executable.

Before editing a release PR manually, apply `release-plz-manual` so automation
preserves those edits. Keep the `release-plz-` branch prefix.

### Validate an RC before stable

1. Prepare and validate the RC in a release PR. Reconcile its manifest and
   lockfile with the changelog and installation examples. Run the development
   checks, fresh per-file coverage, and package inspection before merging the
   tested head.
2. Verify publication through OIDC, including temporary-token revocation.
   Confirm registry availability and matching tags/releases with the prerelease
   flag set. Test exact-version installation, CLI build identification, and the
   callable `version` result from a fresh MCP server. Keep general installation
   instructions unversioned; use exact-version commands only for release checks.
3. Prepare stable in a subsequent PR after the RC passes verification. Apply the
   hold label before manual edits, reconcile the version text, then repeat the
   checks and publication verification with the prerelease flag cleared.

Inspect registry and tag state before retrying a failed publication. Published
versions are immutable; repair through another PR and use another RC number
when needed.
