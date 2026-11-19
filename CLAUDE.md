# hone

hone is an open protocol plus a Rust reference implementation for design artifacts (specs, decisions, modules, test plans), their links and lifecycle, and the steps from ticket to code. See `README.md`.

## Status

Design phase. The protocol concept is `docs/protocol-concept.md`, the artifact and configuration schemas and the standard presets are in `schemas/`. Do not invent protocol rules: if the concept does not answer a question, ask.

Until v1 nothing is stable: every layer (protocol, schemas, implementation) may be changed or rebuilt from scratch. Do not keep a design because it already exists or to preserve compatibility; propose the better design. Decisions already taken are still not reopened silently: name the change and ask.

Run the tests with `cargo test`: resolved configuration, resolution test cases (`schemas/tests/resolve/`), artifact fixtures and content hash (`schemas/tests/{valid,invalid,profile,hash}/`), gate test cases (`schemas/tests/check/`), event fixtures and the process store (`schemas/tests/events/`), replication test cases (`schemas/tests/{read,sync,append}/`), the replication server over HTTP, and the MCP tools. Build tooling in Rust; throwaway generator scripts (bash, jq) live outside the repository.

A step of concept Section 16 follows the skill `hone-step` (`.claude/skills/hone-step/`).

## Protocol versus tool

Keep a strict line between the two:

- **Protocol** (normative): data formats, the resolved configuration, standard gates, the plugin contract, the event log, and the replication contract. Everything must be checkable deterministically, without AI.
- **Tool** (not normative): versioning, hooks, CI, delivery, trackers, agents, the local index, contexts and credentials.

A rule that needs Git, a specific tracker, a server, or an agent belongs to the tool, never to the protocol.

## Conventions

- English everywhere: code, comments, documentation, commit messages.
- Every commit carries a DCO sign-off (`git commit -s`).
- Code is `MIT OR Apache-2.0`. Specification and concept text under `spec/` and `docs/` is CC0 1.0.
- Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` must pass. After a change under `plugins/` (a workspace of its own for `wasm32-wasip1`), its checks in `plugins/README.md` must pass too, and the rebuilt `.wasm` goes into every test case that uses it. After a change under `gui/` (a workspace of its own for the desktop GUI), or to the JSON of a command it calls, its checks in `gui/README.md` must pass too.
- Commits are split into concept, feat, test, and docs. Each commit builds and passes on its own: verify with `git stash push --keep-index --include-untracked`, and keep the fixtures a feat change needs in the feat commit.
- Markdown, JSON, and YAML are formatted with Prettier (pinned in `package.json`): `npm run format`, checked with `npm run format:check`. Artifact fixtures under `schemas/tests/` are excluded, because their exact bytes are under test.
- Deep modules with narrow interfaces. Few public symbols that hide a lot of complexity.
- YAML in artifacts follows the strict YAML profile of the specification (no anchors, aliases, tags, floats, or ambiguous plain scalars).

## Implementation invariants

- Paths from an untrusted state (a checked directory, a base state, a shared folder) are canonicalized and must stay under their root before hone mounts, copies, or reads them. A symbolic link can point anywhere.
- For an untrusted path, check the opened file, not only the path: it is a regular file with one link, and its device and inode match the canonical path below the root. Between a check and the open, a writer can swap in a link, a hard link, or a FIFO. Use `log::open_below`.
- `hone-core` works inside a tokio runtime, because the MCP server calls it from one. Work that blocks on a runtime (WASI in wasmtime) runs on a thread of its own.
- A request is validated before anything is created on disk, so a rejected call leaves no trace.

## Tests and fixtures

- Expected values are computed independently of the code under test: canonical JSON with `jq -cS`, hashes with `sha256sum`, expected findings written by hand.
- A negative case asserts its exact code or message. A positive case is built so that a plausible wrong implementation turns it red; for security properties, confirm it with a quick mutation of the code.
- Each test uses a temporary directory of its own name and removes it.
- Fixture directories hold at least one file, because Git keeps no empty directories. Hashed and deliberately malformed fixtures are listed in `.prettierignore`.
