# Task and sheet workflows

Use these cases when changing task graphs, saved sheets, or the v1alpha5 boundary.
Read the [usage contract](../../../../docs/usage/tasks-and-sheets.md) and discover
the installed schemas first. Select the affected cases to supplement the ordinary
feature matrix. Keep IDs, revisions, raw responses, expectations, and cleanup
ownership in the private ledger. Use separate owned current and legacy stores; do not upgrade
the developer's working store to make a feature test possible.

## Installed workflows

| Area | Cases and observable results |
| --- | --- |
| Classification and patches | Create with an empty task specification and confirm `todo`; ordinary note responses retain typed data in metadata. Convert an ordinary note while retaining UUID/body. Omitted patch fields retain values, empty relationship arrays clear them, and null/unknown fields reject without writes. Set/clear conflicts and kind changes require the documented separate edits. Use the latest returned revision for every edit and preview; stale requests leave sources unchanged |
| Prerequisite closure | Build a branching graph and a strict chain. Only done satisfies an ancestor, including archived done; cancelled and unfinished archived tasks block. Complete an intermediate task before its ancestor and confirm descendants still wait. Starting/completing blocked work succeeds without removing edges. Check one deterministic path per blocker, including ancestors outside query filters or sheet membership |
| Deferral and state | Exercise manual wait/resume, UTC dates, and RFC3339 offsets. Blocking and deferral can both be true. Terminal transitions clear deferral in the same save; reopening does not restore it, and terminal deferral rejects. Reads after expiry change readiness without changing source bytes or revisions |
| Task queries | Verify default active unfinished states, explicit terminal states, all-state conflicts, and true/false ready/blocked/deferred filters combined with note filters. Follow cursors with unchanged filters; compare exact UUID sets and ensure prerequisite evidence survives candidate filtering |
| Sheets | Render the same tasks in different authored orders. Display ordering never recommends; complete preferred ordering recommends its first ready member even when an earlier member is blocked. Positions create no prerequisites. Include external prerequisite context, full multiline bodies, source links, UUIDs/revisions, cancellation labels, and archived state. Use at least ten rows to check continuation indentation at wider numbered markers |
| Visibility | Default rendering of a private sheet returns `not_found`. Explicit private row scope can show placeholders for public members while public prerequisite evidence remains usable. All scope renders mixed local sheets. Missing, malformed, ambiguous, or excluded members make a visible sheet incomplete and suppress recommendations. Expect one member-resolution diagnostic per UUID, retaining separate task-source errors. Compare public responses before/after adding private duplicates or malformed private sources; include counts, paths, and diagnostics |
| Relationship edits | Reject self-links, duplicates, missing/non-task targets, cycles, and public references to private tasks. Reconcile incoming prerequisites and sheet membership before clearing task classification or moving a public target to private storage. No operation rewrites another source. Unrelated broken components permit valid work elsewhere; state/body edits can record reality while relationship defects remain visible |
| Publication | Default copies strip task/sheet metadata and retain the private original. Explicit reviewed specifications allow only public task targets. Preview and stale-source rejection create no copy; both require a current revision. Record and delete actual publication copies even if they are untagged |
| Source preservation | Exercise LF/CRLF, comments, flow and block collections, unrelated nested metadata, and no final newline. Typed patches change only supplied fields; unchanged dated-deferral spellings remain intact. Inspect edited frontmatter with the product and an independent YAML parser when available |
| Cache and direct edits | Compare indexed results with a rebuild after source edits/deletions and deferral expiry. Exercise missing/corrupt caches and source-only graph validation. Verify the failure precondition before crediting a fallback. Successful saves with cache warnings keep their IDs; repair and query rather than repeating creation |
| Concurrency and adapters | Use two independent MCP servers and a CLI writer for same-revision edits and reciprocal edge creation. One save wins; the other edit must not introduce a cycle or overwrite the winner. Bounded retries apply only to documented pre-save `busy` responses. Match CLI/MCP defaults, revision guards, typed validation, scope behavior, and domain codes; include manual deferral with an unsupported `until` field |

## Legacy boundary

In a pre-v1alpha5 store, seed custom `task` and `sheet` keys with scalar, null,
and valid-shaped mapping values. They remain opaque: ordinary reads/edits preserve
them, task queries do not classify them, and sheet rendering does not decode them.
An ordinary legacy note is an invalid rendering target; an out-of-scope private
note is still `not_found`. Do not treat an empty task query alone as proof that
all typed readers respect the boundary.

Preview the explicit upgrade and confirm collisions reject without changing the
version marker or sources. Reconcile fixture keys deliberately, then test preview,
apply, and idempotence. Compare both namespaces byte for byte except registered
migration changes. Exercise older-tool refusal separately from the current tool's
healthy legacy operation and optional upgrade advice.

## Time and comparison discipline

Use the operation's returned `evaluated_at` to decide which side of a deferral
boundary was observed. For an active unfinished fixture with no prerequisites,
first compare that time with the deadline, then check active deferral before the
deadline or readiness at/after it. Record a pre-deadline response as a valid
pre-expiry observation and continue polling for the post-expiry case.
For a live expiry case, use bounded read-only polling and retain the responses;
an arbitrary sleep does not prove the deadline passed.
Keep exact equality-at-deadline assertions in controlled-clock regression tests.

When comparing equivalent task pages or rendered sheets from separate calls,
normalize only the captured evaluation timestamp, including its exact occurrence
in rendered Markdown. Retain and compare all other fields and source bytes;
never normalize revisions, blockers, item order, completeness, or recommendations.
If an assertion fails, preserve its input/response before correcting the expectation
or fixture. Correct malformed fixtures and premature deadline assertions before
using them to support a product finding.
