# hone

hone is an open protocol plus a Rust reference implementation for design artifacts (specs, decisions, modules, test plans), their links and lifecycle, and the steps from ticket to code. See `README.md`.

## Status

Design phase. The protocol concept is `docs/protocol-concept.md`, the artifact schemas and the standard presets are in `schemas/`. Do not invent protocol rules: if the concept does not answer a question, ask.

Run the schema tests with `uv run --no-project --with pyyaml --with jsonschema --with referencing python schemas/tests/run.py`.

## Protocol versus tool

Keep a strict line between the two:

- **Protocol** (normative): data formats, the resolved configuration, standard gates, the plugin contract, the event log, and the replication contract. Everything must be checkable deterministically, without AI.
- **Tool** (not normative): versioning, hooks, CI, delivery, trackers, agents, the local index, contexts and credentials.

A rule that needs Git, a specific tracker, a server, or an agent belongs to the tool, never to the protocol.

## Conventions

- English everywhere: code, comments, documentation, commit messages.
- Every commit carries a DCO sign-off (`git commit -s`).
- Code is `MIT OR Apache-2.0`. Specification and concept text under `spec/` and `docs/` is CC0 1.0.
- Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` must pass.
- Deep modules with narrow interfaces. Few public symbols that hide a lot of complexity.
- YAML in artifacts follows the strict YAML profile of the specification (no anchors, aliases, tags, floats, or ambiguous plain scalars).
