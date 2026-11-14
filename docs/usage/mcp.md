# MCP server

Install the binary, then run `robostickies init` in the target repository.
Start the server from the directory the agent is working in:

```sh
robostickies mcp
```

Directory selection happens once. The process uses the Git root containing its
startup working directory, or that directory outside Git. The client controls
the launch directory, which should be the agent's repository or CWD.
Use `--repo DIR` to override selection; relative paths resolve from the startup
working directory. Start a separate process for each repository.

The binary must be on the client's PATH.

## Session startup

Call `get_context` first. Read its `upgrade.requirement` and `upgrade.message`
alongside warnings, then read tag names and optional descriptions and full startup guidance. A healthy
`validate_store` report can still recommend a data upgrade. Surface `should`
recommendations unless `upgrade.deferred` is true; explain `must` requirements
before ordinary work. Offer explicit preview, upgrade, or deferral choices instead
of migrating automatically. See [store upgrades](upgrades.md).

Call the MCP `version` tool to identify the running server. Replacing a CLI binary
does not update an existing server process. A missing tool or unexpected build is
a verification gap: restart/reconnect that server or use a new connection before
relying on changed behavior. CLI `--version` describes a separate CLI invocation.
Keep each process bound to its intended repository.

## Codex

Register the server with the Codex CLI:

```sh
codex mcp add robostickies -- robostickies mcp
codex mcp list
```

This reusable user-level configuration lets each launched server process
discover its repository from its working directory. Start Codex in the repository
and use `/mcp` to check the connection.

For project-scoped setup, put the equivalent entry in `.codex/config.toml`:

```toml
[mcp_servers.robostickies]
command = "robostickies"
args = ["mcp"]
```

Project configuration loads for trusted projects. See the
[Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

### Pre-approve Codex tools

To pre-approve every robostickies tool, extend the existing server entry in
`~/.codex/config.toml`:

```toml
[mcp_servers.robostickies]
command = "robostickies"
args = ["mcp"]
default_tools_approval_mode = "approve"
```

This includes note edits and creation of public copies. For selective approval,
set that default to `"prompt"` and add entries for the tools you want to approve:

```toml
[mcp_servers.robostickies.tools.get_context]
approval_mode = "approve"

[mcp_servers.robostickies.tools.create_note]
approval_mode = "approve"
```

`approval_mode` belongs under an individual tool; the server-wide key is
`default_tools_approval_mode`. Per-tool settings override the default. See the
[Codex configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
for supported policies.

## Claude Code

From the target repository, register the server with local scope:

```sh
claude mcp add --transport stdio --scope local robostickies -- robostickies mcp
claude mcp list
```

Start Claude Code there and use `/mcp` to check the connection. Local scope keeps
the registration specific to that project and user in `~/.claude.json`, outside
the repository. See the
[Claude Code MCP documentation](https://code.claude.com/docs/en/mcp#mcp-installation-scopes).

### Pre-approve Claude Code tools

Merge this rule into `~/.claude/settings.json` to pre-approve all tools from the
registered `robostickies` server:

```json
{
  "permissions": {
    "allow": ["mcp__robostickies__*"]
  }
}
```

This also permits note edits and publication. For selective approval, replace
the wildcard entry with full tool names such as
`mcp__robostickies__get_context` and `mcp__robostickies__create_note`.
Existing ask or deny rules can still require confirmation or block a call.
Use `/permissions` to review the rules.

For settings limited to this project, use `.claude/settings.local.json` instead.
If you create that file by hand, add `/.claude/settings.local.json` to Git's local
exclude file to keep it untracked. The
[Claude Code permissions guide](https://code.claude.com/docs/en/permissions#mcp)
describes MCP matching; the
[settings guide](https://code.claude.com/docs/en/settings#settings-files-and-who-they-affect)
explains file scopes.

## Using the server

Discovery includes brief startup instructions even before initialization.
`get_context` returns tag names and optional descriptions with full startup guidance and an optional
AGENTS.md paragraph. `init_store` scaffolds a store through MCP.
Local all/private context includes the canonical `repository_root` before and
after initialization. The CLI displays it too. Public context omits the field
entirely, along with personal configuration.

| MCP tools | Purpose |
| --- | --- |
| `version` | Compiled identity of the running server; empty arguments and no store reads |
| `get_context`, `get_config` | Guidance and effective defaults |
| `create_note`, `get_note`, `update_note` | Record and maintain notes; archive through `status` |
| `list_notes` | Browse and search using shared filters |
| `review_notes` | Complete review, defaulting to public visibility |
| `create_tag`, `list_tags`, `get_tag` | Define tags and explore short/full guidance |
| `publish_note` | Immediately copy or preview a private note as public |
| `init_store`, `validate_store`, `rebuild_index` | Initialize and maintain the store |
| `upgrade_store` | Preview, apply, or defer an explicit store upgrade with recovery snapshots |

Creation tools accept `private: true` or `false`; omission uses the relevant
defaults. Private tag references still force private note storage. `update_note`
and `publish_note` require `expected_revision`; get/create/update responses supply
the current revision. `publish_note` defaults to immediate copying, with
`dry_run: true` for previews. Tools return structured results and tool errors.

Create a tag using just its name:

```json
{"name": "fresh-install"}
```

Self-explanatory tags usually do not need a description. When the name needs
explanation, supply `description`; use `body` for separate Markdown instructions:

```json
{
  "name": "onboarding",
  "description": "Setup and first-run checks",
  "body": "Record setup steps, missing tools, and first-run errors."
}
```

`create_tag` accepts a missing or null description, rejects empty or whitespace-only
text, and preserves supplied nonblank text without trimming. `get_tag`, `list_tags`,
and context return `description: null` when absent; new Markdown omits the field.
Tag search matches names and any descriptions. The old `topic` input is rejected.
Legacy stores expose existing topics as descriptions and still require a supplied
description for creation. Name-only creation returns `upgrade_required` with
instructions to preview and explicitly [upgrade](upgrades.md) first.

Note creation, updates, and publication accept optional `body_format` strings.
Updates also accept `clear_body_format: true`; set and clear conflict. Hints
appear in `metadata.body_format` and never interpret the body. Publication drops
the source hint and accepts a public hint only when explicitly supplied.

Context, configuration, validation, and upgrade results include structured
`upgrade` advice. `upgrade_store` accepts `dry_run`, `defer`, and `keep_snapshot`;
see [upgrades and recovery](upgrades.md) for safety and retention rules.

Resources `robostickies://context`, `robostickies://guidance`,
`robostickies://tags`, and `robostickies://config` offer read access as well.
The server binds to one repository and logs exclusively to stderr. Set `RUST_LOG`
to adjust diagnostics. No HTTP transport is exposed.
The context resource includes the local repository root and matches unscoped
`get_context`; use the scoped tool for public output.

`init --agents` optionally appends a small managed pointer to `AGENTS.md`; it
preserves other content and is idempotent. You can instead obtain the paragraph
from context and place it in your preferred format. Guidance follows the
[AGENTS.md standard](https://agents.md/).

[All usage guides](README.md).
