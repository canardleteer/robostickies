# Install and start

Requires Rust 1.90 or newer.

## Install

Install the current Git version:

```sh
cargo install --git https://github.com/canardleteer/robostickies.git --locked
```

To build a checkout:

```sh
git clone https://github.com/canardleteer/robostickies.git
cd robostickies
cargo install --path . --locked
```

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

[All usage guides](README.md).
