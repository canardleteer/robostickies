# Changelog

## Unreleased — 0.3.0-rc.1 development baseline

- Replace required tag topics with optional descriptions. CLI `--topic`, MCP
  `topic`, and Rust `topic: String` fields are removed; use `--description` or
  `description: Option<String>`. Name-only tags are supported, blank descriptions
  are rejected, and absent descriptions appear as null in JSON.
- Add `v1alpha4` and an explicit, lossless tag-key migration in both namespaces.
  Legacy stores remain usable with supplied descriptions; name-only creation
  requires an explicit upgrade. Discard older cached source entities on refresh.
- Use noyalib typed CST edits, retain displaced YAML comments, and preserve
  body bytes during frontmatter-only edits.
- Add the explicit `v1alpha2` → `v1alpha3` migration for malformed tag-list
  indentation, with previews, recovery snapshots, and source-preservation checks.
- Clarify when stored-data repairs require a versioned migration.
- Update YAML and TOML dependencies, adding TOML 1.1 configuration support.
- Refresh MCP output schemas to match fields already serialized in responses.

This is a development baseline; no release is published by this change.

## [0.2.0](https://github.com/canardleteer/robostickies/compare/v0.2.0-rc.2...v0.2.0) - 2026-11-04

- Promote the validated 0.2.0-rc.2 to stable without application code changes.
- Include advisory body-format hints, the MCP `version` tool, and explicit
  store-upgrade advice with optional deferrals and recovery snapshots.
- Include safer initialization paths, configuration handling, and startup guidance.

Public Rust input/result/error structs have additional fields since 0.1.1;
struct-literal callers may need updates. CLI and MCP additions are optional.
Configuration and stored-data versions are unchanged; current stores need no
migration.

## [0.2.0-rc.2](https://github.com/canardleteer/robostickies/compare/v0.2.0-rc.1...v0.2.0-rc.2) - 2026-11-03

- Direct agents to inspect startup upgrade advice independently of validation
  health, respect optional deferrals, and offer explicit migration choices.
- Explain how to identify a running MCP build after installing a new executable.
- Distinguish store migration from server replacement and document verified native
  Codex subagent testing alongside the separate-process fallback.
- Verify startup discovery through ordinary agent questions and snapshot MCP
  initialization instructions alongside tool/resource contracts.
- Allow bounded pre-save contention in concurrency tests and improve source,
  fixture-ownership, and prose/tool review procedures.
- Refuse symlinked local Git exclusion paths before canonicalization can hide
  them, and reject nonregular managed text files without blocking initialization.

CLI/MCP descriptions and startup prose change compatibly. Existing inputs and
response envelopes retain their shapes. Configuration and stored-data versions
are unchanged; current stores need no migration for this RC.

## [0.2.0-rc.1](https://github.com/canardleteer/robostickies/compare/v0.1.1...v0.2.0-rc.1) - 2026-11-02

- Add advisory body-format hints to note creation, editing, and publication.
  Preserve existing metadata and retain both supported store versions.
- Add a read-only MCP `version` tool sharing the CLI's compiled build identity.
- Add structured upgrade advice, private target-specific deferrals, verified
  recovery snapshots, and ordered migration validation independent of the cache.
- Clarify tag identifiers, topics, guidance, and nonempty UTF-8 note bodies.
- Reject nonregular configuration and advisory-preference files before reading,
  and describe both supported fresh-MCP verification routes in new-store guidance.
- Review CLI and MCP contracts through synthetic interface snapshots. Document
  per-file coverage targets and separate interface compatibility from migrations.

Public Rust input/result/error structs gain fields; struct-literal callers may need updates. CLI and MCP additions
are optional; upgrade/status responses gain advisory fields. There is no new
configuration or stored-data version, and no automatic migration.

## [0.1.1](https://github.com/canardleteer/robostickies/compare/v0.1.1-rc.2...v0.1.1) - 2026-11-02

### Other

- Publish the validated release candidate as 0.1.1.
- Update installation examples for 0.1.1.

## [0.1.1-rc.2](https://github.com/canardleteer/robostickies/compare/1336fbf750647689c7233d8cf3a130aff0efc2e9...v0.1.1-rc.2) - 2026-11-02

### Other

- add crates.io and CI badges and update installation examples for 0.1.1-rc.2
- attach checked main branch for release PRs
- dispatch checked releases for Trusted Publishing
- automate releases with release-plz and Trusted Publishing
- cache Rust checks and package release history

## [0.1.1-rc.1]

- Initial crates.io release candidate.
