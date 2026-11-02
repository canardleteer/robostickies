# Releases and Trusted Publishing

`release-plz.yml` publishes `robostickies` and prepares release pull requests.
The executable is also named `robostickies`; xtask remains unpublished.
The crate includes its changelog, MIT license, README, build support, Rust
sources, tests, examples, usage guides, and assets. The package allowlist excludes
the entire `.robostickies` store, agent instructions and skills, workflows,
caches, and xtask.

The workflow pins release-plz to **0.4.2**. Review its
[versioned documentation](https://github.com/release-plz/release-plz/tree/release-plz-v0.4.2/website/docs)
before changing that version. Weekly Dependabot updates cover GitHub Actions;
the executable version remains an explicit workflow input.

## How automation runs

Keep repository variable `RELEASE_PLZ_ENABLED` unset or `false` until setup is
complete. Only the exact value `true` enables automation.

Automatic runs follow successful `Workspace checks` runs caused by pushes to
this repository's `main`. Pull request and fork runs are rejected. The workflow
validates the triggering SHA, then uses the App to dispatch `release-plz.yml` on
`main` with that SHA. Only the dispatched run publishes and prepares a PR; it
checks out the validated commit with full history and rechecks its main-push
results. This uses a `workflow_dispatch` identity because crates.io
[rejects Trusted Publishing from `workflow_run` events](https://blog.rust-lang.org/2026/01/21/crates-io-development-update/#blocked-triggers).

Manual dispatch is also limited to `main` and requires successful main-push
checks for the selected SHA. Its optional `checked_sha` input selects a previous
checked main commit for recovery; leaving it empty uses the dispatch's main SHA.

Publication finishes before PR generation starts. `release_always = false`
restricts publication to commits associated with merged PRs whose branch starts
with `release-plz-`. Preserve that prefix, including for manually managed release
PRs. Rebase-merge the checked head and preserve every implementation and release
edit commit; do not squash. Verify the resulting commit sequence on `main`.

PR updates are serialized without canceling active runs. Outdated runs skip PR
generation once their checked SHA is no longer the main tip. Publication has no
concurrency group because GitHub can discard pending jobs even with
`cancel-in-progress: false`; see the pinned
[concurrency guidance](https://github.com/release-plz/release-plz/blob/release-plz-v0.4.2/website/docs/github/quickstart.md#concurrency).

Untouched bot PRs receive normal updates. Before editing a release PR manually,
create the hold label if needed and apply it:

```sh
gh label create release-plz-manual --repo canardleteer/robostickies \
  --description 'Preserve manual release PR edits' --color B60205
gh pr edit "$release_pr" --repo canardleteer/robostickies --add-label release-plz-manual
```

Any open `release-plz-` PR targeting `main` with that label stops PR generation.
Keep the label until the PR merges, or remove it to resume automatic updates.
It does not block publication of a merged release PR.

`release-plz.toml` processes only `robostickies`, generates `CHANGELOG.md` and
`v<version>` tags, and selects the GitHub prerelease flag from SemVer. Package
verification and SemVer checks remain enabled. Unrelated dependency upgrades
are disabled; only workspace version changes update the lockfile.

## Register the release App

Finish and review the setup PR, including local validation, before this human
checkpoint. Leave automation disabled and retain the administrator bypass
through the RC, stable, and cleanup validation sequence.

Following GitHub's current
[registration instructions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app),
register a dedicated App under `canardleteer`:

- Use the repository URL as its homepage and disable webhooks by clearing Active.
- Grant repository **Contents**, **Pull requests**, and **Actions** read/write.
  Leave administration and ruleset bypass access ungranted.
- Select Only on this account, then
  [install the App](https://docs.github.com/en/apps/using-github-apps/installing-your-own-github-app)
  with Only select repositories: `robostickies`.
- Generate and download a private key locally. Supply the client ID and local
  key-file path to the person configuring the repository; never paste key
  contents into chat, logs, or a tracked file.

With those values in local shell variables, configure the repository:

```sh
gh variable set RELEASE_PLZ_APP_CLIENT_ID --repo canardleteer/robostickies \
  --body "$release_plz_client_id"
gh secret set RELEASE_PLZ_APP_PRIVATE_KEY --repo canardleteer/robostickies \
  < "$release_plz_key_path"
```

See the current CLI documentation for
[variables](https://cli.github.com/manual/gh_variable_set) and
[secret input](https://cli.github.com/manual/gh_secret_set).
Verify the App's installation scope and permissions before continuing. Each
release job requests a repository-scoped installation token and revokes it when
the job ends. GitHub documents that App-created PRs
[start PR checks automatically](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow),
without the approval prompt associated with PRs created by the built-in token.

## Configure the publishing environment

Create environment `crates-io` with selected deployment branches limited to
`main`, no required reviewers, and no wait timer. These commands follow GitHub's
[environment API](https://docs.github.com/en/rest/deployments/environments) and
[deployment branch policy API](https://docs.github.com/en/rest/deployments/branch-policies):

```sh
gh api --method PUT repos/canardleteer/robostickies/environments/crates-io \
  --input - <<'JSON'
{
  "wait_timer": 0,
  "reviewers": [],
  "deployment_branch_policy": {
    "protected_branches": false,
    "custom_branch_policies": true
  }
}
JSON
gh api --method POST \
  repos/canardleteer/robostickies/environments/crates-io/deployment-branch-policies \
  -f name=main -f type=branch
```

For an existing environment, inspect its protection rules and branch policies
first. Retain only the intended `main` branch policy; avoid creating duplicates.
The publishing job alone uses this environment and `id-token: write`.

Wait for Linux, macOS, and Windows checks to pass on the setup PR. Rebase-merge
that checked head, verify its individual commits on `main`, and wait for the
successful main-push check run. Automation remains disabled during this merge.

## Configure crates.io and enable automation

This is a separate human checkpoint after the workflow exists on `main`.
In the existing crate's [settings](https://crates.io/crates/robostickies/settings),
add a [Trusted Publisher](https://crates.io/docs/trusted-publishing) with:

| Field | Value |
| --- | --- |
| Owner | `canardleteer` |
| Repository | `robostickies` |
| Workflow filename | `release-plz.yml` |
| Environment | `crates-io` |

Wait for confirmation that this configuration is saved. Release-plz performs
the OIDC exchange itself, following its
[0.4.2 Trusted Publishing instructions](https://github.com/release-plz/release-plz/blob/release-plz-v0.4.2/website/docs/github/quickstart.md).
Configure no Cargo registry token, release-plz action `token` input, or separate
crates.io authentication action.

Once setup is confirmed and the main commit has passed its push checks:

```sh
gh variable set RELEASE_PLZ_ENABLED --repo canardleteer/robostickies --body true
gh workflow run release-plz.yml --repo canardleteer/robostickies --ref main
```

Inspect the run's checked SHA and both jobs. Review cache restoration in
Workspace checks and release jobs, including `target/xtask` and installed tools.

## Validate the RC, then stable

1. Require an App-created release PR with automatically started `pull_request`
   checks. Manual dispatch or reopening a PR does not establish this criterion.
2. Confirm that its manifest, lockfile, changelog, title, and description target
   exactly `0.1.1-rc.2`. If correction is needed, apply `release-plz-manual` first,
   then run `release-plz set-version robostickies@0.1.1-rc.2` and reconcile all
   release text and installation examples.
3. Run `cargo fmt --all`, `cargo xtask check`, fresh `cargo xtask coverage`, and
   Rust 1.90 verification. Review per-file line, function, and region coverage
   and meaningful failure paths, including build scripts. Inspect
   `cargo package --locked --package robostickies --list` and verify the package.
   Review prose, commit text, and the final PR title and description with
   slop-guard. Keep evidence private and remove owned fixtures before committing.
4. Wait for all three platform jobs during this rollout. Rebase-merge the tested
   head, verify commit preservation, and verify the RC publication below.
5. After another release PR exists, apply the hold label and run
   `release-plz set-version robostickies@0.1.1`. If automation creates none, create
   a manually managed `release-plz-` branch and PR first. Reconcile the lockfile,
   changelog, installation examples, title, and description, then repeat checks,
   slop-guard review, rebase merge, and publication verification for stable.

Follow the [format version](upgrades.md#format-version-policy) and
[data version](upgrades.md#data-version-policy) policies independently of the
crate version. macOS and Windows remain informative for ordinary development;
waiting for them during release validation does not change platform support.

For each publication, inspect the release logs for the OIDC exchange and
temporary-token handling, including revocation. Check registry availability,
matching Git tags and GitHub Releases, and correct prerelease flags. Install the
exact published version into a temporary prefix:

```sh
release_version=0.1.1-rc.2
cargo install robostickies --version "=$release_version" --locked \
  --root /tmp/robostickies-release-check
/tmp/robostickies-release-check/bin/robostickies -V
/tmp/robostickies-release-check/bin/robostickies --version
```

Check packaged build identification with Git absent from `PATH`, and confirm
the server version through a fresh MCP connection to that executable. Repeat
with `release_version=0.1.1` for stable. Remove the temporary installation and
owned fixtures afterward; retain run evidence privately.

## Recovery and final cleanup

Before retrying a failed run, inspect registry, tag, and release state. Published
versions are immutable. Repair through additional commits and PRs; use another
RC number if necessary, then prepare stable through a subsequent PR. Recovery
dispatches use `main` and still require successful main-push checks for that SHA.
Set `RELEASE_PLZ_ENABLED=false` to stop new release jobs while investigating.

Keep `publish-bootstrap.yml` available throughout validation. Its
`FIRST_CRATES_IO_RELEASE` secret has been deleted, so authenticated uploads fail
at its credential check. Its default dry run still checks the package and registry
state. Do not recreate that secret for the release-plz rollout.

After stable publication succeeds, remove the bootstrap workflow and these
obsolete bootstrap instructions in a checked cleanup PR, preserving its commits.
After cleanup and final validation, remove only the administrator bypass from
the required-check ruleset. Preserve the Linux check, deletion and history
protection, and platform policy. Leave any later automatically generated release
PR open; publication beyond `0.1.1` is outside this validation sequence.

[All usage guides](README.md).
