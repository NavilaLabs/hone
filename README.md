# hone

Sharpen the design before the code.

hone is an open protocol and a reference implementation for recording design artifacts in a project: specs, decisions, modules, test plans, their links, their lifecycle, and the steps that lead from a ticket to code. Every rule can be checked deterministically, without AI, so the same check can block an agent, a pre-push hook, or a CI run.

> **Status:** design phase. The protocol concept and the first artifact schemas are being moved into this repository. There is no usable code yet.

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

Planned:

| Path       | Content                                              |
| ---------- | ---------------------------------------------------- |
| `spec/`    | The protocol specification (normative text)          |
| `schemas/` | JSON Schemas for all formats, with conformance tests |
| `crates/`  | The Rust reference implementation (`hone` CLI)       |

## License

Everything in hone is open source. There is no commercial layer and no open-core boundary. Anyone may do anything with it, including closed forks and hosted services.

- **Code** (CLI, server, user interface, plugins, packages, schemas, tests) is licensed under either of
  - Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
  - MIT license ([LICENSE-MIT](LICENSE-MIT)),

  at your option.

- **Specification text** (`spec/`) is dedicated to the public domain under CC0 1.0 Universal ([LICENSE-CC0](LICENSE-CC0)).

## Contributing

Contributions are welcome. Every commit needs a Developer Certificate of Origin sign-off (`git commit -s`). See [CONTRIBUTING.md](CONTRIBUTING.md).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
