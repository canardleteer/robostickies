# Install and start

Requires Rust 1.90 or newer.

## Install

Install robostickies from crates.io:

```sh
cargo install robostickies --locked
```

To install the current Git version instead:

```sh
cargo install --git https://github.com/canardleteer/robostickies.git --locked
```

To build a checkout:

```sh
git clone https://github.com/canardleteer/robostickies.git
cd robostickies
cargo install --path . --locked
```

## Version and build identification

`robostickies -V` prints the package name and SemVer on one line.
`robostickies --version` adds labeled build details:

```text
robostickies <version>
Commit: <full commit hash or unknown>
Source state: <clean, dirty, or unknown>
Target: <target triple>
Compiler: <rustc version>
Build profile: <Cargo profile>
```

These values are captured at build time. Git checkouts use their own commit and
tracked-file state; untracked and ignored files do not make a build dirty.
Packaged sources use Cargo's `.cargo_vcs_info.json`, including its dirty flag
when present. Missing or malformed provenance produces `unknown` values. An
enclosing consumer checkout never supplies the package's source identity.

Both flags work without Git, outside a repository, and without opening a note
store. Output omits builder paths and timestamps. It includes no usernames or hostnames.
MCP advertises the plain package SemVer during initialization. Its read-only
`version` tool accepts `{}` and returns `name`, `version`, `commit`, `source_state`,
`target`, `compiler`, and `profile` for the running server. It uses the same
compiled identity as the CLI and reads no repository files. This identifies a
long-running server even after the executable on disk has been replaced.
Package versions are separate from
[configuration and stored-data versions](upgrades.md#format-version-policy).

## Initialize a repository

For [local-only use](privacy-and-configuration.md#keep-the-entire-store-local),
add the exclusion before initializing. To share the store in Git, initialize
normally:

```sh
cd /path/to/repository
robostickies init
robostickies context
robostickies add "The workspace test helper saved several setup steps" --tag paperclip
robostickies add "Tests need the package directory as their working directory" --tag papercut
robostickies list
robostickies search "working directory"
robostickies review
```

Run commands from any directory inside a Git repository, or select an explicit
root with `--repo /path/to/repository`. Outside Git, initialization uses the
current directory. Every CLI operation supports `--json`; errors go to stderr
with stable codes. Pass `-` or `--file <path>` to read a note body from stdin or
a UTF-8 file. Text after `update` replaces the body; omitted fields remain intact.

Connect your agent using the [MCP setup guide](mcp.md).

## Notes and tags

```sh
robostickies tags create tooling --topic "Tooling and development workflow" \
  "Read the relevant tool's help and record concrete failures or useful helpers."
robostickies tags list
robostickies tags get tooling
robostickies add "The build helper avoids repeated environment setup" --tag paperclip --tag tooling
robostickies get <uuid>
robostickies update <uuid> "Corrected observation" --if-match <revision>
robostickies archive <uuid> --if-match <revision-returned-by-update>
robostickies unarchive <uuid> --if-match <revision-returned-by-archive>
```

Predefined tags are optional. Notes can use multiple defined tags or no
tags. Edit tag guidance directly in its Markdown file. Definitions require a
short YAML `topic`; `read_on_start: true` includes full guidance in session
context. Additional frontmatter is retained during note edits, including comments.

In the example, `tooling` is the tag identifier used by `--tag` and `tags get`.
The topic, `Tooling and development workflow`, is a short catalog description.
The body is longer Markdown guidance explaining how to use that tag. Private
definitions use qualified identifiers such as `private:tooling`.

## Bodies and optional format hints

Note bodies are nonempty UTF-8 text. Markdown and plain text are preferred;
other text documents are accepted without interpreting or executing them.
Optionally describe the body with an advisory hint:

```sh
robostickies add --private --file observation.yaml --body-format yaml
robostickies update <uuid> --body-format text --if-match <revision>
robostickies update <uuid> --clear-body-format --if-match <revision>
```

`markdown`, `text`, and `yaml` are recommended hints. Other nonblank strings are
accepted. The response exposes the hint as `metadata.body_format`; it is stored
as additional frontmatter rather than a required field. Omitted edit hints retain
existing metadata, and setting and clearing together is an error. Legacy
non-string values remain readable and survive unrelated edits; reconcile those
values in the source before using the hint interface to set or clear them.

Publishing strips the source hint with other private metadata. Supply
`publish --body-format <hint>` explicitly to describe a new public copy.

[All usage guides](README.md).
