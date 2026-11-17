# hone

Sharpen the design before the code.

hone is an open protocol and a reference implementation for recording design artifacts in a project: specs, decisions, modules, test plans, their links, their lifecycle, and the steps that lead from a ticket to code. Every rule can be checked deterministically, without AI, so the same check can block an agent, a pre-push hook, or a CI run.

> **Status:** design phase. The protocol concept ([docs/protocol-concept.md](docs/protocol-concept.md)) and the schemas are in place. The CLI resolves a configuration (`hone resolve`), runs the standard gates (`hone check`), and blocks agent edits of code without a binding decision (`hone hook claude-code`). Gates that need the process store, `exec` gates, and plugins come later.
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

| Path                | Content                                                    |
| ------------------- | ---------------------------------------------------------- |
| `docs/`             | The protocol concept, the source for the specification     |
| `schemas/`          | JSON Schemas and conformance test cases (`schemas/tests/`) |
| `schemas/standard/` | The presets of the package `hone/standard`                 |
| `spec/`             | Planned: the normative protocol specification              |
| `crates/hone-core/` | The Rust reference implementation as a library             |
| `crates/hone-cli/`  | The CLI (binary `hone`), the base for a later GUI          |

Run the tests:

```sh
cargo test
```

`cargo test` covers the resolved configuration, the resolution test cases, the artifact fixtures, the content hash, and the gate test cases (`schemas/tests/check/`).

## Checking

`hone check` runs the gates over the checked directory. Without `--ticket`, it runs the directory gates (`schema-valid`, `ids-unique`, `links-resolve`) that a step of `.hone/resolved.json` contains. With `--ticket`, it also runs the gates of the ticket's `required` steps against the ticket's snapshot, or of one step with `--step`. It exits with 0 when every gate passed, 1 when a gate failed, and 2 when it could not run.

```sh
hone check --ticket tk-9f2x4a8m --changed src/payments/fee.rs
git diff --name-only main | hone check --ticket tk-9f2x4a8m --changed-from -
```

`--changed <path>` (repeatable) and `--changed-from <file|->` (one path per line, an empty file means no changes) supply the changed paths for `change-needs-binding`. `--class pure|exec` runs only one class of gates, and `--json` prints the result as JSON.

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
