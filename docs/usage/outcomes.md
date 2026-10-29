# Resolution and adoption

Initialization adds missing `resolved` and `adopted` definitions, including in
existing stores. It preserves customized definitions and honors tag privacy
defaults. Both tags provide startup guidance and use the existing tag/update
commands and APIs.

| Observation | Condition | Action |
| --- | --- | --- |
| Papercut | Fix verified in the affected environment | Retain `papercut`, add `resolved`, record evidence, archive |
| Paperclip for permanent guidance | Guidance exists in the appropriately scoped AGENTS.md and has been reviewed | Retain `paperclip`, add `adopted`, record the destination, archive |
| Other useful paperclip | Permanent instructions are unwarranted | Keep active without `adopted` |

MCP fixes require installation, an agent restart, and verification through the
installed connection before resolution. Archived status alone proves no outcome.
Publish a sanitized public copy of a private observation before promoting it
into tracked guidance.

Updates replace the entire tag set, so retain the original tags when adding an
outcome. Preserve the note's visibility: use a public outcome definition for a
public note; a private note can use public or private definitions. If the needed
definition is missing, create it with an explicit `--public`/`--private` or MCP
`private` value to override current defaults. Adding a private tag moves a public
note into private storage.

```sh
robostickies update <uuid> "Observation and verification evidence" --tag papercut --tag resolved --if-match <revision>
robostickies archive <uuid> --if-match <revision-returned-by-update>
robostickies review --status archived --tag resolved
robostickies review --status archived --tag adopted
```

In MCP, use `update_note` with the retained tags plus the outcome and
`status: "archived"`. Query history with `review_notes` using
`{"status":"archived","tags":["resolved"]}` or
`{"status":"archived","tags":["adopted"]}`. Public/active defaults still apply;
select `scope` and qualified tag names explicitly for private history.

[All usage guides](README.md).
