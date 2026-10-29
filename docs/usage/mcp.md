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
[Codex MCP documentation](https://developers.openai.com/codex/mcp/).

## Claude Code

From the target repository, register the server with local scope:

```sh
claude mcp add --transport stdio --scope local robostickies -- robostickies mcp
claude mcp list
```

Start Claude Code there and use `/mcp` to check the connection. Local scope keeps
the registration specific to that project and user. See the
[Claude Code MCP documentation](https://code.claude.com/docs/en/mcp).

## Using the server

Discovery includes brief startup instructions even before initialization.
`get_context` returns tag topics with full startup guidance and an optional
AGENTS.md paragraph. `init_store` scaffolds a store through MCP.
Local all/private context includes the canonical `repository_root` before and
after initialization. The CLI displays it too. Public context omits the field
entirely, along with personal configuration.

| MCP tools | Purpose |
| --- | --- |
| `get_context`, `get_config` | Guidance and effective defaults |
| `create_note`, `get_note`, `update_note` | Record and maintain notes; archive through `status` |
| `list_notes` | Browse and search using shared filters |
| `review_notes` | Complete review, defaulting to public visibility |
| `create_tag`, `list_tags`, `get_tag` | Define tags and explore short/full guidance |
| `publish_note` | Immediately copy or preview a private note as public |
| `init_store`, `validate_store`, `rebuild_index` | Initialize and maintain the store |

Creation tools accept `private: true` or `false`; omission uses the relevant
defaults. Private tag references still force private note storage. `update_note`
and `publish_note` require `expected_revision`; get/create/update responses supply
the current revision. `publish_note` defaults to immediate copying, with
`dry_run: true` for previews. Tools return structured results and tool errors.

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
