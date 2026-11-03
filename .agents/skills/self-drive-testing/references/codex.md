# Codex: verify through a separate agent without restarting the parent

Codex supports a native subagent route and an explicit CLI process route. Both
leave the parent conversation running. A native child exposed the newly installed
build during verification while the parent retained its older connection. Compare
the child's actual runtime identity each time: spawning a task alone is not proof.
Other clients can document equivalent launch methods alongside this reference.

## Prepare the checkpoint

Run the required checks, remove owned synthetic sources, and install the batch
with `cargo install --path . --locked --force`. Record the source revision/state,
absolute installed executable, CLI build fields, and its SHA-256 under the ignored
run directory. Resolve the executable before launching the child so PATH changes
cannot select another installation.

Prepare a task file for the full self-drive. It must require actual MCP calls,
fixture ownership and cleanup, private evidence, and reporting new papercuts.
Pause parent edits to note/tag/configuration/guidance sources until the entire
child process exits. Independent read-only work and run artifacts may continue.

For an unprompted discovery case, use a separate owned repository and an ordinary
orientation question. Keep the checkpoint, expected response fields, and this
testing skill out of that agent's context until its first answer is recorded.
The evaluator still checks its actual tool trace against the checkpoint.

## Native Codex subagent

The verified native route uses `collaboration.spawn_agent` with a fresh task
context, `fork_turns: "none"`, and no model or MCP configuration override:

```json
{
  "task_name": "self_drive",
  "fork_turns": "none",
  "message": "Read the self-drive-testing skill and private installation checkpoint. Call your actual MCP get_context and version tools; compare repository and all compiled fields before testing. Run the affected flow, look for papercuts, retain private evidence, clean up owned fixtures, and report."
}
```

Pass the exact checkout, skill, and checkpoint paths in the task message. No
special server arguments were needed for the verified connection. Keep the parent
source edits paused until the subagent's entire turn finishes. Review its actual
MCP responses and final preservation/cleanup evidence before applying outcomes.

If that child has an older catalog or identity, leave changed-behavior checks
pending and use the explicit CLI route below. Use that route for isolated
repositories too: the native task API does not configure a different server root.
A follow-up task on an older testing agent also needs the identity gate; start a
new agent when its server is stale.

## Launch with child-only server settings

The following recipe uses Codex CLI flags verified with `codex exec --help` on
0.166.0. Replace the configuration literals and paths together; create the run
directory and task file before starting. No global MCP configuration is rewritten.
No model override is supplied; the child uses its configured model.

```sh
qa_repo=/path/to/checkout
qa_run="$qa_repo/.robostickies/private/runs/<run-id>"
codex exec --ephemeral --approve-for-me --cd "$qa_repo" --json \
  --config 'mcp_servers.robostickies.command="/absolute/path/to/robostickies"' \
  --config 'mcp_servers.robostickies.args=["--repo","/path/to/checkout","--no-user-config","mcp"]' \
  --config 'mcp_servers.robostickies.default_tools_approval_mode="approve"' \
  --output-last-message "$qa_run/agent-last-message.md" \
  - < "$qa_run/task.md" > "$qa_run/events.jsonl" 2> "$qa_run/stderr.log"
```

`--approve-for-me` routes approval requests through automatic review with the
workspace-write sandbox. The MCP approval setting applies only to this child and
the authorized test scope. Preserve review failures and follow the stated policy.
`--no-user-config` belongs to robostickies and makes test configuration reproducible;
it does not disable the agent's own user configuration.

For an additional isolated-store connection, add another child-only server entry
with the same executable and an explicit owned temporary `--repo` directory.
Record that directory for cleanup. Never retarget a running server by changing CWD.

Automated launchers should construct an argument array, serializing configuration
values as TOML-compatible strings/arrays. Do not interpolate JSON into shell command
text: paths or fixture text can contain shell syntax. Redirect each attempt to new
evidence files so a retry cannot overwrite the earlier transcript.

## Prove which process was tested

On each actual connection, call `get_context` first and confirm the selected local
repository. Then call `version` with empty arguments and compare all seven fields
against the checkpoint. Recheck the executable hash separately as filesystem/CLI
evidence. A CLI subprocess or a child-created substitute MCP client does not verify
the agent's actual connection. Inspect the JSONL tool events as well as its report.

Only after this gate should the agent exercise changed behavior and explore for
new papercuts. A missing version tool or mismatched identity leaves affected checks
pending; do not infer the server version from the executable currently on disk.

If the agent is interrupted, retain its actual responses and latest ownership
checkpoint. Start another fresh process with the same owned roots and recovered
revisions, repeat its orientation/identity gate, and finish the remaining cases.
Do not repeat successful creates or silently relabel failed assertions. The failed
attempt remains infrastructure or product evidence according to its actual cause.

After process exit, verify all owned notes, public copies, fixture definitions,
and synthetic snapshots are absent. Recheck original source hashes, then remove
owned temporary roots/installations. Record findings and outcome updates afterward.
This route verifies the new build without upgrading the parent's older connection;
restart that older connection before relying on it for changed behavior.
