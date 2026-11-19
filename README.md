# hone

Sharpen the design before the code.

hone is an open protocol and a reference implementation for recording design artifacts in a project: specs, decisions, modules, test plans, their links, their lifecycle, and the steps that lead from a ticket to code. Every rule can be checked deterministically, without AI, so the same check can block an agent, a pre-push hook, or a CI run.

> **Status:** design phase. The protocol concept ([docs/protocol-concept.md](docs/protocol-concept.md)) and the schemas are in place. The CLI resolves a configuration (`hone resolve`), runs the standard gates (`hone check`), blocks agent edits of code without a binding decision (`hone hook claude-code`), writes comment threads and step state to a local process store (`hone thread`, `hone step`, `hone events`), runs plugin gates in a WebAssembly sandbox and approved `exec` commands, syncs the process store through a shared folder or a replication server (`hone sync`, `hone serve`), and offers the same functions to agents as an MCP server (`hone mcp`).
>
> **Until v1 nothing is stable.** Every part, from the protocol and the schemas to the implementation, may still be changed, replaced, or rebuilt from scratch. There are no compatibility guarantees before v1.

## Ideas

- **Small core, like Git.** The protocol fixes a few things precisely. Everything else is configuration, packages, and plugins.
- **Artifacts are text.** Design artifacts are Markdown or YAML files, readable without any tool.
- **Storage-agnostic.** The protocol works on a directory. Whether it is versioned, shared, or ignored is up to you.
- **No server required.** Everything works locally and offline. A server only replicates process data such as review threads and approvals.
- **Checkable without AI.** Gates are deterministic functions over a directory.
- **Skipping always works.** Every step can be skipped explicitly, and the skip stays visible.
- **Presets, not dogma.** Spec, decision, test plan, and module are presets of the standard package. Teams can change, remove, or replace them.
- **Data in the protocol, behaviour in plugins.** Custom gates and generators are sandboxed WebAssembly plugins.

## Repository layout

| Path                  | Content                                                    |
| --------------------- | ---------------------------------------------------------- |
| `docs/`               | The protocol concept, the source for the specification     |
| `schemas/`            | JSON Schemas and conformance test cases (`schemas/tests/`) |
| `schemas/standard/`   | The presets of the package `hone/standard`                 |
| `spec/`               | Planned: the normative protocol specification              |
| `crates/hone-core/`   | The Rust reference implementation as a library             |
| `crates/hone-cli/`    | The CLI (binary `hone`), the base for a later GUI          |
| `crates/hone-mcp/`    | The MCP server over stdio, started by `hone mcp`           |
| `crates/hone-server/` | The reference replication server, started by `hone serve`  |

Run the tests:

```sh
cargo test
```

`cargo test` covers the resolved configuration, the resolution test cases, the artifact fixtures, the content hash, the gate test cases (`schemas/tests/check/`, including plugin and `exec` gates), the plugin input, the event fixtures and the process store (`schemas/tests/events/`), the replication test cases (`schemas/tests/read/`, `schemas/tests/sync/`, `schemas/tests/append/`), the replication server over HTTP, and every MCP tool.

## Checking

`hone check` runs the gates over the checked directory. Without `--ticket`, it runs the directory gates (`schema-valid`, `ids-unique`, `links-resolve`) that a step of `.hone/resolved.json` contains. With `--ticket`, it also runs the gates of the ticket's `required` steps against the ticket's snapshot, or of one step with `--step`. It exits with 0 when every gate passed, 1 when a gate failed, and 2 when it could not run.

```sh
hone check --ticket tk-9f2x4a8m --changed src/payments/fee.rs
git diff --name-only main | hone check --ticket tk-9f2x4a8m --changed-from -
```

`--changed <path>` (repeatable) and `--changed-from <file|->` (one path per line, an empty file means no changes) supply the changed paths for `change-needs-binding`. `--class pure|exec` runs only one class of gates, and `--json` prints the result as JSON.

### Plugins and `exec` gates

A plugin gate runs a WebAssembly module (WASI preview 1) from `.hone/plugins/<name>.wasm` in a sandbox: it reads the project read-only under `/project` and, with `--base <dir>`, the base state under `/base`, and has no network. An `exec` gate runs a command of the toolchain, without a shell and with only the environment variables it names. Both run only when the lockfile approves them: the plugin with its hash and capabilities, the command under `x-hone-exec`.

```yaml
# .hone/lock.yaml
plugins:
  public-api-has-decision:
    hash: "sha256:…"
    capabilities:
      { read: [project], net: [], timeout_ms: 5000, memory_pages: 256 }
x-hone-exec:
  - { command: [cargo, test], env: [CARGO_HOME, HOME, PATH, RUSTUP_HOME] }
```

`--lock <file>` replaces `.hone/lock.yaml`. CI passes the lockfile of the protected base state, so that a pull request cannot approve its own plugins or commands:

```sh
git show origin/main:.hone/lock.yaml > /tmp/lock.yaml
git worktree add /tmp/base origin/main
hone check --ticket tk-9f2x4a8m --changed-from changed.txt --base /tmp/base --lock /tmp/lock.yaml
```

hone's own plugins live in [`plugins/`](plugins/README.md).

### Claude Code hook

`hone hook claude-code` blocks an edit of code that the ticket's `change-needs-binding` gates reject, for example code anchored by a module without a binding decision. Edits of artifact files are never blocked. In `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit|NotebookEdit",
        "hooks": [{ "type": "command", "command": "hone hook claude-code" }]
      }
    ]
  }
}
```

The ticket comes from `--ticket` or the environment variable `HONE_TICKET` (`HONE_TICKET=tk-9f2x4a8m claude`). Without one, the hook lets every edit through and says so on stderr.

## Process store

Threads and step state are events in a local process store, one append-only log per device and project (concept Section 4). hone keeps it in `$XDG_DATA_HOME/hone/<workspace>/<project>/` (default `~/.local/share/hone/…`, `~/Library/Application Support/hone/…` on macOS, `%APPDATA%\hone\…` on Windows), so all clones and worktrees of a project share it. `HONE_DATA_DIR` replaces the base directory. The IDs come from `workspace` and `project` in `.hone/config.yaml`.

```sh
export HONE_AUTHOR=bob HONE_ROLE=human
hone thread open --ticket tk-9f2x4a8m --kind concern --blocking \
  --anchor-artifact dec-7k3m2q8p --quote "asynchronously" --body "What happens when the queue fails?"
hone thread reply th-… --body "Fall back to synchronous processing." --author alice
hone thread resolve th-…
hone step set --ticket tk-9f2x4a8m domain skipped --reason "no new terms"
hone events --ticket tk-9f2x4a8m
```

A thread is anchored with `--anchor-artifact <id>` (hone takes the content hash from the file), `--anchor-code <path#symbol>`, `--anchor-change <reference>`, or `--anchor-transcript <url>`. The role must exist in the ticket's configuration, and `hone step set` accepts only steps of the ticket's profile. The gates `no-open-blocking-threads`, `threads-answered`, and `steps-complete` read this store. Where it does not exist, for example in CI, they fail with `process-store-unavailable`.

Without a server, a team shares the store through a folder that every member can write, for example a Nextcloud or Syncthing folder or a network drive. The same folder works as a backup:

```sh
hone sync --folder ~/Nextcloud/hone   # or HONE_SYNC_FOLDER
```

hone keeps the store in the folder under `<workspace>/<project>/log/`, imports the events of every other device, and writes only the log of this device, so two members never write the same file. A fork (two machines writing as one device), a conflicting event, a broken line, or a symbolic link that leaves the folder is reported and not imported. The exit code is 0 for a clean sync, 1 when problems were reported, and 2 when the sync could not run, for example because the folder does not exist.

A team with a server runs the reference server and syncs with it. The server keeps one store per project in a data directory with the layout of a shared folder, so `hone sync --folder <data>` backs it up:

```sh
hone serve --config server.yaml          # listen, data, identities with hashed tokens, projects
HONE_TOKEN=… hone sync --server https://hone.example
HONE_TOKEN=… hone sync --server https://hone.example --watch   # stay connected until Ctrl-C
```

The server checks every event against the authenticated identity, the project, and for approvals the actors in its configuration, and rejects it with a fixed error code (concept Section 4). A token is configured only as its SHA-256 (`printf %s "$TOKEN" | sha256sum`). `--watch` imports events as they arrive and sends new own events every 2 seconds.

## MCP server

`hone mcp` serves the tools `resolve`, `check`, `check_edit`, `check_artifact`, `content_hash`, `thread_open`, `thread_reply`, `thread_resolve`, `thread_reopen`, `step_set`, `events`, and `sync` over stdio for one project (`--root`, default `.`, and `--packages`). The author of every event it writes is fixed at the start with `--author` and `--role` (or `HONE_AUTHOR` and `HONE_ROLE`), so that an agent cannot write as someone else; without one, the writing tools return an error. `sync` uses the folder fixed with `--sync-folder` (or `HONE_SYNC_FOLDER`) or the server fixed with `--sync-server` (or `HONE_SYNC_SERVER`, the token from `HONE_TOKEN`). The tools return the same JSON as `--json`. A failed gate is a normal result with `status: failed`; a tool that could not run, or a call with invalid arguments, returns a result marked `isError`. `check_edit` asks beforehand what the hook would decide, and `check_artifact` checks a draft before it is written. For Claude Code:

```sh
claude mcp add hone -- hone mcp --author critic-bot --role critic
```

## License

Everything in hone is open source. There is no commercial layer and no open-core boundary. Anyone may do anything with it, including closed forks and hosted services.

- **Code** (CLI, server, user interface, plugins, packages, schemas, tests) is licensed under either of

  - Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
  - MIT license ([LICENSE-MIT](LICENSE-MIT)),

  at your option.

- **Specification and concept text** (`spec/`, `docs/`) is dedicated to the public domain under CC0 1.0 Universal ([LICENSE-CC0](LICENSE-CC0)).

## Contributing

Contributions are welcome. Every commit needs a Developer Certificate of Origin sign-off (`git commit -s`). See [CONTRIBUTING.md](CONTRIBUTING.md).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
