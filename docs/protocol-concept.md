# hone protocol: concept

Status: draft, as of 2026-11-17. This concept is the source for the normative specification, which will live in `spec/`.

## 1. Goal

The protocol describes **how design artifacts, their relationships, their lifecycle, and the flow of steps** are recorded in a project. It is a pure artifact producer: it creates and checks files and process data, and it knows nothing about version control, ticket systems, review platforms, or agents. Anyone may implement it. hone is the reference implementation. All of it is open source: specification, CLI, sync server, and user interface. There is no commercial layer and therefore no open-core boundary.

The protocol is built like Git: a small fixed core, everything else is conventions and extensions.

**The protocol is not:**

- an agent protocol (ACP and MCP exist for that),
- a ticket system, a review tool, or a version control system. How artifacts are versioned and delivered is up to the tool layer (Section 12),
- a metric definition. How "depth" is weighted is up to the implementation. Only the result format (reports) will be standardized later.

## 2. Principles

1. **Small core.** Better to define a few things cleanly than many things halfway.
2. **Artifacts are text.** Specs, decisions, and modules are readable files, readable without any tool. Process data (comments, approvals, intermediate states) may be stored compactly, as long as the format is open and the tool makes it accessible.
3. **Storage-agnostic.** The protocol works on a directory. Whether and how it is versioned (Git, another system, not at all) and whether it lives in the repository or is ignored locally does not matter to it. A single person can therefore use it without the team agreeing first.
4. **No server required.** Everything essential works locally and offline. A server (multiplayer) is an addition that mainly replicates.
5. **Scope-separated.** The project is the unit of log and sync. Nothing is mixed, shared, or queried across projects or tenants.
6. **No counters, no shared mutation.** IDs are random, every artifact is its own file, approvals are appended instead of overwritten, and every device writes only to its own log. Parallel changes therefore rarely collide.
7. **Checkable without AI.** Every rule of the protocol can be checked deterministically. A check is a function over a directory. The caller decides what triggers it.
8. **Skipping always works.** Every step can be skipped explicitly. A reason is optional. This is part of the protocol and cannot be turned off.
9. **Data in the protocol, behaviour in plugins.** The protocol defines data formats and interfaces. Whoever needs custom behaviour (checks, generating configuration) writes a plugin without changing the protocol.
10. **Frozen per ticket.** A ticket follows the configuration it was started with. Rule changes do not retroactively invalidate running tickets.
11. **Extensible without breaking the core.** Unknown fields are preserved. Custom extensions live in the namespace `x-<name>`.
12. **Versioned.** Every project states its protocol version.

## 3. Overview: layers

```
Scope             Workspace (tenant) → Project → Ticket
Configuration     Level 1: resolved data · Level 2: generation · Level 3: plugins (behaviour)
Steps & profiles  Building blocks of steps, selection by ticket attributes
Artifacts         Ticket record, kinds (presets: spec, decision, module, test plan), anchors, links, status
Process data      Event log per device and project: threads, approvals, step state; replication
-----------------------------------------------------------------------
Tool layer        Contexts and servers, versioning, triggers (hooks, CI), ticket↔code, delivery, trackers, agents
```

Everything above the line is the protocol. Everything below is up to the tool and replaceable (Section 12).

## 4. Scope, storage, and process store

### Scope

Data belongs to a scope. The protocol knows three levels:

| Level     | Meaning                                                                                   | ID      |
| --------- | ----------------------------------------------------------------------------------------- | ------- |
| Workspace | Tenant or organization (a client, a company). Without a server: the default value `local` | `ws-…`  |
| Project   | A unit of configuration, artifacts, and process data. May span several repositories       | `prj-…` |
| Ticket    | A unit of work in the project                                                             | `tk-…`  |

Rules:

- **The project is the unit of replication.** Log and sync belong to exactly one project. Whoever works with many tenants never receives foreign logs, and no client receives another client's data.
- Every event carries `project`. A server rejects events that do not match its scope.
- The IDs `workspace` and `project` live in `.hone/config.yaml`. They are not secret and are created by `init`. An ID stays the same across clones, worktrees, and devices. A hash of the path or remote URL would not.
- Server addresses, identities, and credentials never live in the project (Section 12).

### Data and its storage

|          | Artifacts                                                                       | Process data                                                  |
| -------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Content  | Specs, decisions, module definitions, test plans, ticket records, configuration | Threads, approvals, step state, further events                |
| Property | Readable without a tool, durable, part of the project                           | Large volume, value drops after completion, must be queryable |
| Storage  | Files in a directory (Section 11)                                               | Event log                                                     |

From artifact storage, the protocol requires only a directory that can be read and written. Everything else (versioning, sharing, ignoring) is outside the protocol.

### Process store

- **The source of truth is an append-only event log, one log per device and project**, to which only that device appends. There are no competing writers. Merging means reading all logs and taking the union.
- **Segments.** A device's log consists of segments: `log/<device>/<number>.ndjson`. A segment is closed after a fixed number of events or a fixed size. Closed segments are immutable and may be compressed (`.ndjson.zst`). Events are never rewritten, there is no compaction. Archiving and dropping (Section 10) operate on whole segments of completed tickets.
- **Every event has a sequence number** (`seq`) in its device's log. The **cursor** of a state is a map `device → last seq` and says exactly what a peer is missing.
- **Order without a shared clock.** The order is deterministic: topological by `after`, ties broken by `date`, then by `id`. States are causally chained through `after`. Diverging clocks therefore change at most the display, never the computed state.
- **Why no database format as the source of truth.** Database files from two devices cannot be merged without conflicts, neither through a shared folder nor through Git. Appended text files can. For fast queries, an implementation builds an index (for example an embedded database). It is a cache, can be rebuilt from the logs at any time, and is not part of the protocol (Section 12).
- **Local storage:** `~/.hone/<workspace>/<project>/` (on Linux preferably in the XDG data directory) with `log/` and the sync state. Without a server, the workspace is `local`. All clones and worktrees of a project share this storage, because a ticket lives across branches.
- **Lost devices.** Whatever a device never synchronized is lost with it. If a device's `seq` ends, that is not an error.

```yaml
# an event
id: ev-0193a8f2-6c1e-7b3d-9f42-8e1a5c7d2b90 # UUIDv7 (RFC 9562)
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
device: dev-3k9a5h2t # writing device
seq: 118 # sequence number in this device's log
type: thread.message # thread.* | approval.given | step.changed | step.skipped | ticket.started | ...
author: { name: alice, role: human }
date: 2026-11-16T10:42:00Z
after: [ev-0193a8f1-...] # optional: events this one refers to (causality)
payload: { ... }
```

Event IDs are UUIDv7: 128 bits, most of them random, available as a library in every language, and sortable by time. The full length is necessary because duplicate IDs are ignored during sync, and a collision would silently swallow an event. The field `sig` is reserved for a later protocol version (signature by the device). Version 1 has no signatures.

### Replication

**Contract (per project):** `append(events)`, `read(cursor) → events`, `sync(peer)`. During sync, both sides exchange their cursors and send each other what is missing. Events are never changed, duplicate IDs are ignored. The tool computes status and order deterministically from the set (order: see above), so there are no merge conflicts.

| Peer                    | Purpose                                                                                                                                                        |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Server**              | Default for teams: real time, permissions, notifications                                                                                                       |
| Shared folder or branch | Fallback for small teams without a server. Since every device writes only its own file, nothing collides. No real time, no permissions below repository access |
| none                    | Solo use, everything local. Backup is then the user's responsibility. The tool offers a folder as a backup target (Section 12)                                 |

**Role of the server.** It is the rendezvous point and only replicates. In addition, it

- authenticates authors and sets the identity in the event. A client cannot choose it freely,
- checks the scope and, for approvals, the role according to `actors`,
- manages permissions per project and provides real time and notifications.

The replication contract is part of the protocol (Question 19). The server that speaks it and the user interface belong to the reference implementation and are open source. Anyone can build their own server or client or run the reference server themselves.

**Normative scope of the server contract** (Question 26), so that third-party clients and servers fit together:

- Replication: `append`, `read(cursor)`, `sync`.
- Real-time subscription: new events from a cursor onwards as a stream.
- Authentication only as a transport requirement: bearer token. How the client obtains it is governed by OAuth 2.0 or OpenID Connect.
- Rejection of events outside the scope and of approvals with a mismatching role, with defined error codes.

**Not normative** are permission management (who sees which project), administration, and notifications. Every server solves these on its own.

**Identity.** There is no identity across servers. Every context (Section 12) has its own, which matches tenant separation. With a server, the server sets the identity in the event. Without a server, authorship is trust-based until a later version introduces signatures (`sig`).

**CI does not have the process store.** For gates to work there, the `review` block in the artifact is the interface (Section 10). CI may additionally query the server, but the baseline check must not depend on it.

## 5. Artifacts

Every artifact is a file. The core knows exactly one fixed type, the **ticket record**. All other types are **kinds** (`kinds`), defined by the configuration (Section 8).

**`spec`, `decision`, `test-plan`, and `module` are presets, not protocol types.** They come with the package `hone/standard` because almost every team needs them. A project can delete, rename, or change them or replace them with its own kinds, exactly as it adds its own kinds such as `threat-model`. The protocol itself knows no design concept, only the mechanisms the presets are built on:

- **Lifecycle** (`kinds.<type>.lifecycle`):
  - `decided`: The artifact belongs to a ticket, has a status (Section 6) and a content hash, and can be approved. Required fields are `id`, `type`, `ticket`, `status`, `created`, and `authors`. Examples: `spec`, `decision`, `test-plan`.
  - `living`: A living definition without ticket, status, or content hash that evolves with the code. Example: `module`.
- **Anchors** (`anchors`): Every kind may have anchors that locate code (`kind: path | glob | symbol`). A language plugin can resolve them (Section 9).
- **Links** (see below).

Kinds in Markdown (`.md`) have front matter and a body, kinds in YAML (`.yaml`) only data. The format is described in "Format and content hash" below. An artifact of kind `decision` from the standard package:

```yaml
id: dec-7k3m2q8p # kind prefix + 8 random characters, never changes
type: decision # a kind from `kinds` in the configuration
ticket: tk-9f2x4a8m # reference to the ticket record
status: accepted # declared status, see Section 6
created: 2026-11-16
authors: [alice]
reach: ticket # ticket | cross-cutting
links:
  supersedes: [dec-3b8n1c5w]
  affects: [mod-5w7p1r2k] # optional: only if the decision deliberately binds anchored artifacts
  code: ["src/payments/gateway.rs#PaymentGateway"]
```

**IDs.** An artifact ID is the kind's prefix and 8 characters of Crockford Base32 (40 random bits). It never changes. The descriptive slug lives only in the file name (`design/decisions/dec-7k3m2q8p-payment-async.md`) and may change. Links use only the ID. IDs are short because people read and type them. That is enough because a collision between artifacts is noticed: the standard gate `ids-unique` reports two files with the same ID, and the tool rolls again on creation if the ID already exists. With 10,000 artifacts, the probability of a collision is about 0.005 %. Event IDs, in contrast, have full length (Section 4), because a collision there would go unnoticed.

On completion, the tool adds a `review` block to artifacts with lifecycle `decided` (approvals and a summary of the review history, see Section 10). It is not part of the artifact's content hash.

### Format and content hash

**The data model is JSON. The notation in files is a strict YAML profile.** Events (NDJSON), the resolved configuration (`resolved.json`), and plugin inputs and outputs are JSON anyway. That gives one data model for everything, and JSON Schema checks everything. Hash and gates operate on the data model, never on the bytes of the file.

**Why not TOML:** Merging (Section 8) needs `null` as a removal marker. TOML has no null value, keys are simply omitted there. TOML has supported multi-line inline tables since 1.1, so those no longer speak against TOML, but `null` still does.

**Why a profile:** Different YAML parsers read the same file differently. A YAML 1.1 parser turns `NO` into `false`, `010` into the number 8, `1:30` into the number 90, and `2026-11-16` into a date. YAML 1.2 cleaned this up with the core schema, but libraries differ, and two tools must see the same data and the same hash. The profile allows only the subset in which everyone reads the same:

1. YAML 1.2, UTF-8 without BOM, one document per file.
2. Values are strings, integers (±(2^53−1)), `true`, `false`, and `null`, plus lists and maps with string keys. No floating-point numbers.
3. Forbidden are anchors, aliases, tags, directives other than `%YAML 1.2`, merge keys (`<<`), duplicate keys, keys that are not strings, and empty values. `null` is written as `null`, so that a forgotten value does not silently remove something.
4. Ambiguous strings are quoted: `yes`, `no`, `on`, `off`, `y`, `n`, `True`, `~`, `Null`, and anything a parser could read as a number (`1.5`, `010`, `0x1F`, `1:30`, `9e10`). Everything else may stand without quotes.
5. A date is a string (`2026-11-16`), never a date type.

The gate `schema-valid` checks the profile first, then the schema. A YAML parser alone is not enough for this, because it does not always report anchors, tags, and duplicate keys. The reference implementation therefore checks the parser's event stream. Unknown fields are preserved (Principle 11). `schema-valid` reports unknown fields without `x-` as a warning, not as an error.

**File shapes.** A `.md` file starts with the line `---`, followed by the front matter and a line `---`. Everything after that is the body (Markdown). A `.yaml` file is the data model itself and has no body.

**Schema.** `schemas/artifact.schema.json` (JSON Schema 2020-12) distinguishes the ticket record and the two lifecycles `decided` and `living`. Which lifecycle applies is stated in `kinds.<type>.lifecycle`. The declared status knows only `proposed`, `accepted`, `rejected`, and `deprecated`. `superseded` is computed and must never appear in a file (Question 2). `kinds.<type>.statuses` may only restrict this set. Every kind can attach an additional schema through `kinds.<type>.schema`. The presets live with their definitions (`kinds/`) and additional schemas (`schemas/`) in the standard package, in this repository under `schemas/standard/`. `schema-valid` checks an artifact against the schema of its lifecycle, its additional schema, and the prefix of its kind (`id_prefix`). A kind that the configuration does not know is an error.

**Content hash.** Applies only to artifacts with lifecycle `decided`. The ticket record and artifacts with lifecycle `living` are mutable and have none. `sha256:` plus SHA-256 over the canonical form (RFC 8785) of the object `{ "meta": <front matter without review>, "body": <body> }`. The body is normalized first:

1. Unicode normalization form NFC.
2. Line endings become LF.
3. Spaces and tabs at the end of a line are removed.
4. Leading and trailing line breaks are removed.

Files without a body have `body: ""`. Reformatting the YAML (order, quoting, style), CRLF, trailing whitespace, and changes to the `review` block leave the hash unchanged. Every change to fields or text changes it, including a new line break in the body. Markdown formatters should therefore not rewrap paragraphs (with Prettier, for example, `proseWrap: preserve`). Hashing a Markdown AST is deliberately excluded, because two parsers would hash differently. `schemas/tests/run.py` checks profile, schema, and hash as a reference. It is not an implementation of the protocol.

**Core and presets.**

| Type        | Origin                   | Lifecycle | Purpose                                                                                                           |
| ----------- | ------------------------ | --------- | ----------------------------------------------------------------------------------------------------------------- |
| `ticket`    | Core                     | own       | The record of a ticket: references to external systems, attributes, chosen profile, frozen configuration, context |
| `spec`      | Preset (`hone/standard`) | `decided` | Sharpened ticket: problem, acceptance criteria, non-goals (snapshot if the ticket lives elsewhere)                |
| `decision`  | Preset (`hone/standard`) | `decided` | A decision with options and consequences (close to MADR)                                                          |
| `test-plan` | Preset (`hone/standard`) | `decided` | What must be tested, derived from criteria and interfaces                                                         |
| `module`    | Preset (`hone/standard`) | `living`  | A module boundary in Ousterhout's sense (see below): name, interface, secrets, anchors                            |

**Domain types and interfaces are not a protocol type.** They live as code in the target language. The protocol only knows the reference `code: <path>#<symbol>`. Whether it resolves is checked by a language plugin (Section 9).

### Modules (preset)

A **module** is a named boundary in Ousterhout's sense: an interface behind which something is hidden (its _secrets_). Specs and decisions describe _what_ is built. Modules describe _how it is cut_. A module is not bound to any form: it can be a function, a class, a file, a package, a subsystem, or a service. Its **anchors** say where it lives in the code. The module artifact is not the code but the description of the boundary: name, interface, secrets, anchors, and parent module. The protocol prescribes **no granularity** and no package structure.

```yaml
id: mod-4t8w2n6c
type: module
name: FeeCalculator
parent: mod-2m6x8k3f # optional: modules may be nested
interface: # reference to the interface in code, or a short description
  code: "src/payments/fee.rs#FeeCalculator"
anchors: # where the module lives in code (at least one anchor)
  - { kind: symbol, ref: "src/payments/fee.rs#FeeCalculator" }
secrets:
  - Fee tiers and rounding rules
```

The modules of a project form a tree (or forest), not a flat list. A module has the lifecycle `living`: anchors move during refactoring, so it has neither `ticket` nor status nor content hash.

**Granularity.** Only **explicitly defined** modules are tracked, never every function automatically (Question 16).

**Changes.** A module is a normal file and may be edited freely. Changes are visible through versioning like any other file. A **deliberate cut** or a shift of a boundary is a decision with `affects: [mod-…]`, and the decision is approved (Question 23). Broken anchors are reported by the optional gate `module-anchors-resolve`, a language plugin.

**Which modules a ticket affects is context, not input.** It follows from research and analysis (references, import graph, change coupling) and is confirmed by a human. The ticket record keeps it as an observation (`context.observed`, see below). If the analysis finds code for which no module exists yet, an anchor stands there instead of an ID. New or changed modules only come into being during design.

### Links

**Link types in the core:**

| Link         | Target                                                                                           |
| ------------ | ------------------------------------------------------------------------------------------------ |
| `supersedes` | IDs of artifacts this one replaces                                                               |
| `affects`    | IDs of anchored artifacts (for example modules) that this artifact deliberately binds            |
| `code`       | References `path#symbol`, with several repositories `repo:path#symbol`. The path contains no `:` |
| `verifies`   | IDs of the artifacts whose criteria are checked. Test code is linked through `code`              |
| `depends-on` | IDs                                                                                              |
| `relates`    | IDs                                                                                              |
| `transcript` | URLs of agent transcripts. Transcripts are only linked, never stored (Question 18)               |

The ticket is in the field `ticket`, not in `links`. Custom link types live in the namespace `x-<name>`.
Only **forward links** are stored. The tool computes backward links such as "superseded by".

**Several repositories** (Question 4). The artifacts of a project live in one directory, typically in one repository. Code may live in any number of repositories and is linked with `repo:path#symbol`. The tool resolves locally which location belongs to a repository name (Section 12). This keeps `links-resolve` a check over one directory.

### Ticket record

```yaml
id: tk-9f2x4a8m
type: ticket
project: prj-8m2k4d7q
external:
  - { system: jira, key: PAY-123, url: https://example.invalid/browse/PAY-123 }
attributes: # attributes for profile selection (Section 8)
  type: feature
  size: M
  risk: high
profile: full # result of the selection, changeable by a human
config: sha256:5be2... # frozen configuration (Section 8)
overrides: {} # optional: ticket-specific changes to the configuration
context: # result of research and analysis, not input
  observed: # each entry has either `module` (ID) or `anchor` (no module yet)
    - { module: mod-4t8w2n6c, source: analysis, confirmed: true }
    - { module: mod-9c3d6h4v, source: research, confirmed: false }
    - {
        anchor: { kind: path, ref: "src/ledger/export.rs" },
        source: analysis,
        confirmed: true,
      }
```

The **state of the steps** (open, done, skipped, with an optional reason) changes constantly and therefore lives in the process store. On completion, the tool records the result in the ticket record, so that skipping stays visible to the team:

```yaml
completed:
  steps_done: [ticket, research, decision, interfaces, tests, implementation]
  steps_skipped:
    - { step: domain, reason: "no new terms" } # reason optional
```

The ticket record is the **key** that connects everything: artifacts, configuration, and external tickets. The mapping to commits, branches, or pull requests does not belong in the protocol (Section 12).

## 6. Lifecycle and status

There is a **declared** and an **effective** status.

- _Declared:_ what the author writes into the file (`proposed`, `accepted`, `rejected`, `deprecated`).
- _Effective:_ what the tool computes from the declared status and the links **in the checked directory**.

| Declared              | Superseded by an accepted artifact in the directory | Effective              |
| --------------------- | --------------------------------------------------- | ---------------------- |
| proposed              | –                                                   | proposed               |
| accepted              | no                                                  | **accepted (binding)** |
| accepted              | yes                                                 | superseded             |
| rejected / deprecated | –                                                   | rejected / deprecated  |

**The caller determines the checked directory.** An agent hook checks the working state, CI checks the state of the change, a review checks the result. The protocol knows no lines, branches, or "integration". What counts is what is in the directory.

**Only binding artifacts bind agents** and are enforced by the gates. Design-first therefore needs no special rule: a check such as "changed code needs a binding decision" (`change-needs-binding`, Section 7) is a pure state check. If the design is already on the base of the change, it passes. If it is part of the same state, it passes as well.

Accepted artifacts are not changed in content. A change is a new artifact that replaces the old one through `supersedes`. The old one is **not touched** for this. The status "superseded" is only computed and never written into the file, so that hash and approvals stay valid. The tool makes it readable (`explain`, user interface, editor hints).

Status exists only for kinds with lifecycle `decided`. Artifacts with lifecycle `living` (such as modules) have no status.

## 7. Steps, gates, profiles (the building blocks)

### Step definition

```yaml
id: decision
title: Decision
acts: [human, critic] # roles that work (roles: Section 8)
decides: architect # role that decides
inputs: [spec, domain] # artifact kinds
outputs: [decision]
gates: # deterministic checks
  - { gate: artifact-exists, params: { type: decision } }
  - { gate: links-resolve }
  - { gate: approvals-valid, params: { role: architect, min: 1 } }
```

A step describes inputs, outputs, participants (as roles, never as people), and checks. It contains **no** instructions for a specific agent.

**The order follows from inputs and outputs.** A step is ready when its inputs are done or skipped. Whoever wants a fixed order adds `requires: [<step>]`. Cycles are an error. Adding a step means inserting a node, not rewriting a list.

### Gates

A gate is a **deterministic function** over a directory: the input is the checked state (optionally a second state `base` and the list `changed_paths`, both supplied by the caller), the output is pass or fail, plus findings. The call is the same everywhere (`hone check`), whether hook, CI, or by hand.

**Standard gates** belong to the protocol. Every implementation ships them natively:

| Gate                       | Checks                                                                                                                                          |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `artifact-exists`          | There is an artifact of the kind (optionally with a status) in the ticket                                                                       |
| `status-is`                | An artifact has the effective status X                                                                                                          |
| `schema-valid`             | Front matter matches the schema of the kind                                                                                                     |
| `ids-unique`               | No two files carry the same ID                                                                                                                  |
| `links-resolve`            | All links point to existing artifacts                                                                                                           |
| `no-open-blocking-threads` | No blocking threads are open                                                                                                                    |
| `threads-answered`         | All threads of a role (for example `critic`) are answered                                                                                       |
| `approvals-valid`          | Approvals of role R, at least N, hash matches the content. With `require_proof: true`, every approval needs a proof (Section 10)                |
| `steps-complete`           | Every required step is done or explicitly skipped                                                                                               |
| `change-needs-binding`     | If `changed_paths` hits the anchors of an artifact of kind `anchored`, there is a binding artifact of kind `binding` for it (through `affects`) |

The standard package sets the parameters `{ anchored: module, binding: decision }` for `change-needs-binding`.

**Everything else is a plugin** (Section 9): a gate is then a WASM module, included through the same entry.

### Profile

A profile selects steps and their strictness. The steps are a **map**, so that layers can merge them key by key (Section 8).

```yaml
id: medium
steps:
  ticket: required
  research: required
  analyze: required
  domain: optional
  decision: optional
  interfaces: required
  tests: required
  implementation: required
  drift-check: required
  completion: required
```

Rules:

- `required` means: do it **or** skip it explicitly. If neither happened, the gate `steps-complete` fails.
- The selection rules (Section 8) choose the profile for a ticket. The user can change the result per ticket.

## 8. Configuration: Level 1 and Level 2

The configuration must be dynamic: actors come and go, kinds of tickets need different flows, teams invent their own artifacts. The protocol separates three levels for this.

| Level                         | What                                                                        | Who reads it                    | Flexibility      |
| ----------------------------- | --------------------------------------------------------------------------- | ------------------------------- | ---------------- |
| **1. Resolved configuration** | Pure data, one document                                                     | Every implementation, gates, CI | none, on purpose |
| **2. Generation**             | Layers of YAML files, packages, optional generator plugins that output data | Only `resolve`                  | arbitrary        |
| **3. Behaviour**              | Gates, generators, language plugins as WASM (Section 9)                     | The tool, in a sandbox          | arbitrary        |

Rule of thumb: **Whoever only reads the configuration needs only data (Level 1). Whoever changes it works in Level 2. Whoever adds behaviour writes a plugin (Level 3).**

### Level 1: the resolved configuration

The protocol reads only this document (`.hone/resolved.json`, generated, not edited by hand; shown here as YAML for readability). It contains everything needed to evaluate a ticket and nothing that executes code.

```yaml
protocol: 1

kinds: # kinds of artifacts (spec, decision, test-plan, module come from hone/standard)
  decision:
    path: "design/decisions/{id}-{slug}.md" # {slug} may change, {id} never
    id_prefix: dec
    lifecycle: decided # decided | living
    schema: schemas/decision.json # JSON Schema
    statuses: [proposed, accepted, rejected, deprecated]
  module:
    path: "design/modules/{id}-{slug}.yaml"
    id_prefix: mod
    lifecycle: living
    schema: schemas/module.json
  threat-model: # custom kind, no protocol change needed
    path: "design/threat-models/{id}-{slug}.md"
    id_prefix: thr
    lifecycle: decided
    schema: schemas/threat-model.json

roles: # what someone may do in the process
  human: { can: [act, comment, approve] }
  architect: { can: [act, comment, approve] }
  critic: { kind: ai, can: [act, comment] }

actors: # who has which roles (handles, not real names)
  alice: [human, architect]
  bob: [human]
  "*": [human] # fallback for unknown people

steps: # Section 7
  decision:
    {
      acts: [human, critic],
      decides: architect,
      inputs: [spec],
      outputs: [decision],
      gates: [...],
    }

profiles:
  quick-fix:
    {
      steps:
        { ticket: required, implementation: required, completion: required },
    }
  standard: { steps: { ... } }
  full: { steps: { ... }, retention: archive, review_summary: full } # Section 10

select: # profile selection, first matching rule wins
  - { when: { type: bugfix, size: S }, profile: quick-fix }
  - { when: { type: feature, risk: high }, profile: full }
  - { default: standard }

plugins: # Section 9
  public-api-has-decision:
    wasm: plugins/public-api-has-decision.wasm
    sha256: 9a1f...
    capabilities:
      { read: [project], net: [], timeout_ms: 5000, memory_pages: 256 }
```

**Roles and actors are separate.** Steps and gates name only roles. Actors are mappings, and where they come from is replaceable (file, generator from CODEOWNERS or an identity provider, tool adapter). Splitting people up means adding or removing entries. The fallback `"*"` makes sure unknown people break nothing. Actors are **handles** (the same as in version control), not real names. Whoever wants more distance generates them from an external source (Question 10).

**Selection rules are deliberately simple:** `when` compares ticket attributes (`attributes` in the ticket record) for equality or set membership. There is no expression language. Whoever needs more sets attributes beforehand with a generator or in the user interface.

**Origin of attributes** (Question 9). Trackers or analysis may suggest attributes, a human sets them. If someone changes them after the start, that is an event (`ticket.attributes_changed`) and appears in the record on completion. The protocol does not prevent someone from entering `risk: low` to avoid gates. It makes it visible, as with skipping (Principle 8). Whoever wants more writes a plugin gate, for example "the risk must match the analysis".

**Properties of the resolved configuration**

- **Canonical:** fixed key order, so the hash (`sha256:…`) is a stable identity of the content.
- **Self-contained:** no includes, no references to other files except schemas and plugins with a hash.
- **Origin lives next to it, not inside it:** `resolved.origin.json` records for every definition which layer and file it comes from. This feeds `hone explain`. The hash depends only on the content.
- **Validated:** every reference must resolve (roles in steps, gates, plugins, schemas), otherwise `resolve` fails.

### Frozen per ticket

When a ticket starts, the resolved configuration (with its `overrides`) is stored as a snapshot and recorded in the ticket record by hash (`config: sha256:…`). The snapshot is stored content-addressed under `.hone/snapshots/<hash>.json`. If the configuration does not change, no new snapshot is created, and several tickets share one file.

- Gates check a ticket **always against its snapshot**. CI needs neither generators nor packages for this.
- If a ticket deliberately switches to new rules, that is an event (`ticket.config_changed`) and visible in the record.
- The tool may delete a snapshot only when **no ticket record** refers to it any more, not even a completed one. Snapshots are small and shared. This keeps every ticket auditable (Question 8).

### Level 2: generation

The resolved configuration is built from **layers**. From bottom to top:

```
Standard (package hone/standard)  →  further packages (extends)  →  project  →  ticket (overrides)
```

There is deliberately **no global or personal layer.** It would mix context between projects and tenants and does not fit into a snapshot. Personal settings of the tool (agent, model, notifications) are not protocol configuration. A custom standard process is a package (see below).

**Merging** (the specification must define this exactly, so that two tools resolve identically):

1. Objects are merged key by key, recursively.
2. Lists are replaced. Where merging is needed, the schema uses maps instead of lists (hence `steps` in the profile as a map).
3. `null` removes a key or a whole definition from a lower layer. The profile (Section 5) requires an explicit `null` for this. An empty value is an error and removes nothing.
4. The higher layer wins.

There are deliberately no explicit list operations (`add`, `remove`) (Question 7). Where a list would have to be merged, it becomes a map in the schema.

```yaml
# project: remove the step `domain` from the profile `medium` and make `decision` required
profiles:
  medium:
    steps:
      domain: null
      decision: required
```

**Layout in the project**

```
.hone/
  config.yaml            # workspace, project (IDs), extends, generators
  kinds/  roles/  steps/  profiles/     # one definition per file
  select.yaml  actors.yaml
  plugins/               # .wasm files
  lock.yaml              # packages and plugins: name, version, hash, approved capabilities
  resolved.json          # generated Level 1 (like a lockfile, committed if the team wants)
  snapshots/             # frozen configurations by hash
```

The minimal configuration consists of the IDs that `init` creates and one `extends`:

```yaml
# .hone/config.yaml
workspace: local # or ws-…, once a server is bound
project: prj-8m2k4d7q
extends: [hone/standard@1]
```

**Packages** have the same layout as the `.hone/` directory. They are pinned by name, version, and hash in `lock.yaml`. Where a package comes from (directory, archive, registry) is up to the tool. A hash is mandatory. A workspace can publish a package (the organization level of a tenant, for example its standard profiles for all of its repositories). It is pinned by hash like any other. A single person's own standard process is also just a package.

**Generators** are plugins (Section 9) that output parts of the configuration as data:

```yaml
# .hone/config.yaml
generate:
  - plugin: plugins/actors-from-codeowners.wasm
    sha256: 7c20...
    capabilities: { read: [".github/CODEOWNERS"], net: [] }
    into: actors # only this part of the tree may be set
```

A generator receives a layer and parameters and returns a patch in the same shape as any other layer. It runs only on `hone resolve`, started by a human or the tool. **Not on every check and not in CI.** It may therefore do things that vary over time (for example an identity provider as a source with `net: [idp.example.com]`), because the result is frozen.

**Growing and shrinking**

| Change                                         | Effort                                                         |
| ---------------------------------------------- | -------------------------------------------------------------- |
| New person                                     | One line in `actors.yaml` (or a generator)                     |
| Split a role (`human` → `human` + `architect`) | Add the role, change the mapping, adjust `decides` in the step |
| New kind of artifact                           | One `kind` file, one step, one schema                          |
| Remove a preset (for example `test-plan`)      | `test-plan: null` under `kinds` and remove the step            |
| Remove a step                                  | `step: null` in the higher layer                               |
| Stricter rules for risky tickets               | Selection rule and profile                                     |
| Custom check                                   | Write a plugin and add it to the step                          |

**Tools:** `hone explain <ticket>` shows the resolved configuration with the origin of every line. `resolve --check` checks whether `resolved.json` matches its sources (with generators only in a trusted environment).

## 9. Plugins: Level 3

Behaviour beyond the standard gates comes from **WebAssembly plugins**. Only the contract below is normative (entry point, JSON in, JSON out, capabilities). The reference runtime is [Extism](https://extism.org/docs/overview), but it is not part of the specification. The WASM component model may be added later as a second binding (Question 5).

**Why WASM**

- **Sandbox:** A plugin runs isolated from host memory. What it may do is defined by the manifest (network hosts, file paths, memory limit, time limit).
- **Language-independent:** Plugins are written in the language the team knows. Extism offers plugin development kits (PDKs) for several languages, including Rust, Go, and C/C++, and host SDKs for many languages, including Rust and PHP.
- **Simple contract:** A call is `function_name(bytes) → bytes`. The protocol uses JSON.

### Contract

A plugin exports named entry points. The protocol defines these:

| Entry point        | Kind            | Input                                                                                           | Output                                                               |
| ------------------ | --------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `describe()`       | all             | –                                                                                               | Name, version, protocol version, entry points, required capabilities |
| `check(input)`     | gate            | checked state, ticket, step, parameters, optionally `base`, `changed_paths`, and `tool_outputs` | `{ ok, findings[] }`                                                 |
| `generate(input)`  | generator       | layer, parameters, resolved configuration so far                                                | patch                                                                |
| `resolve(anchor)`  | language plugin | anchor (`path#symbol`)                                                                          | location or error                                                    |
| `public_api(path)` | language plugin | path                                                                                            | list of public symbols (for the drift check)                         |

Custom entry points are allowed in the namespace `x-<name>`.

```json
// check: input (shortened)
{ "protocol": 1,
  "ticket": { "id": "tk-9f2x4a8m", "profile": "full", "attributes": { "risk": "high" } },
  "step": "decision",
  "params": {},
  "artifacts": [ { "id": "dec-7k3m2q8p", "type": "decision", "status": "accepted", "hash": "sha256:3f1c...", "path": "design/decisions/..." } ],
  "changed_paths": ["src/payments/gateway.rs"] }

// check: output
{ "ok": false,
  "findings": [ { "level": "error", "message": "Public API of PaymentGateway changed without a binding decision", "path": "src/payments/gateway.rs" } ] }
```

### Capabilities and trust

Capabilities are part of the configuration next to the plugin and are granted by the tool. A plugin cannot grant itself any.

```yaml
capabilities:
  read: [project] # files of the checked directory, read-only
  net: [] # allowed hosts, empty = none
  timeout_ms: 5000
  memory_pages: 256
```

Rules:

- **The default is nothing.** `net: []` means no network. The tool must always set the list explicitly. In Extism an empty list means "no host allowed", but a missing value (`null`) means "all hosts allowed", and this rule closes exactly that trap.
- **Gates are pure.** No network, no writing. The result is then a function of input and files and therefore repeatable.
- **Hash pinning.** Plugin hash and approved capabilities are recorded in `lock.yaml`. If either changes, a new approval is required. CI runs only plugins that are recorded there with a hash.
- **Generators are the exception:** they may get network access, but run only on `resolve`, never in CI.

### What WASM cannot do

A plugin in the sandbox cannot start a compiler or a test runner. "Interfaces compile" and "tests exist and fail" need the toolchain of the target language. The host can do this, because `hone check` runs as a normal program with the permissions of the calling person. For this there is a second gate type that is data, not code:

```yaml
- { gate: exec, params: { command: ["cargo", "check", "--workspace"] } }
```

Rules:

- **Two classes.** `pure` gates (standard gates and WASM) are fast and deterministic and may run in every agent hook. `exec` gates run on completion of a step, before a push, and in CI, not on every edit. The caller chooses the class (`check --class pure`). The trigger remains the caller's business.
- **An argv list, no shell.** Command, working directory, and allowed environment variables are recorded with a hash in `lock.yaml` and need approval. CI takes the lockfile from the protected base state, never from the change itself. Otherwise a pull request could inject arbitrary code into CI. The protocol regulates nothing beyond that. What "protected" means (branch protection, mandatory review for `lock.yaml`) is defined by the platform (Section 12). Hash pinning is enough (Question 6). `exec` gates are allowed only where the team trusts them (locally, and in CI on protected states).
- **Plugins evaluate tool output.** Optionally, the host passes the command's result (`exit`, `stdout`, `stderr`, truncated) as `tool_outputs` to a plugin's `check`, which turns it into findings. The plugin stays a pure function of its input. Host functions with which a plugin starts commands itself are deliberately not provided, because they would break "gates are pure".
- **Language packages supply the commands** (`hone/lang-rust`, `hone/lang-php` through `extends`). Nobody writes them by hand.
- **CI does not trust local `exec` results** and runs them itself. The toolchain version is part of the result, because it influences it.

Parsing-based language functions (resolving symbols, reading the public API), in contrast, can be built as WASM plugins, for example with tree-sitter.

## 10. Approvals, comment threads, and condensation

Approvals and threads are **events in the process store**. They are not individual files in the working tree. On completion, whatever should remain is condensed.

### Approvals

An approval is the event `approval.given`:

```yaml
id: ev-0193a8f4-1d2e-7a5b-8c3d-4e5f6a7b8c9d
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
type: approval.given
author: { name: alice, role: architect }
date: 2026-11-16T14:05:00Z
payload:
  artifact: dec-7k3m2q8p
  hash: sha256:3f1c... # content that was approved (without the `review` block)
  decision: approved # approved | changes-requested
```

- The approval binds to the **content hash**. If the artifact changes afterwards, it becomes invalid.
- Who must give how many approvals is defined in the step (`approvals-valid` with role and count). The protocol checks that the author's role may approve according to `actors`.
- The protocol does not define a proof that this person really approved. A tool can store one as a free field (`proof`) in the event and in the `review` block (for example a signed approval or a confirmation from the server). Without a proof, an approval is trust-based. With `approvals-valid: { require_proof: true }`, a step requires a proof for every approval, checked by a plugin.

### Comment threads

Review consists mainly of questions, objections, and answers, and that is often where the **why** behind a decision lives. Threads are therefore part of the protocol, as a data model and not as a user interface.

**Anchors.** A thread is attached to something specific:

| Anchor       | Refers to                                                                                                      |
| ------------ | -------------------------------------------------------------------------------------------------------------- |
| `artifact`   | Artifact ID, content hash at the time of the comment, optionally a quote to find the spot again                |
| `code`       | Reference `path#symbol` (the language plugin follows the symbol when it moves)                                 |
| `change`     | Description of an external change (for example pull request and line range), stored by the tool as a reference |
| `transcript` | URL of an agent transcript and a position in it. The transcript itself is not stored                           |

If the content changes, the thread becomes **outdated** (the hash no longer matches), but it stays visible and must still be closed explicitly.

**Events.** A thread consists of the events `thread.opened`, `thread.message`, `thread.resolved`, and `thread.reopened`. The status (`open` or `resolved`) is computed.

```yaml
id: ev-0193a8f0-9b2c-7e4d-a1f3-5c6d7e8f9a0b
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
type: thread.opened
author: { name: bob, role: human }
date: 2026-11-16T10:12:00Z
payload:
  thread: th-0193a8f0-9b2c-7e4d-a1f3-5c6d7e8f9a0b # th- + UUID of the opening event
  kind: concern # question | concern | suggestion | note
  blocking: true
  anchor:
    kind: artifact
    artifact: dec-7k3m2q8p
    hash: sha256:3f1c...
    quote: "We process payments asynchronously"
  body: "What happens when the queue fails?"
```

**Rules**

- **Blocking threads** (`blocking: true`) make the gate `no-open-blocking-threads` fail while they are open.
- **Approvals and threads are independent:** an approval stays valid as long as the hash matches. An open blocking thread blocks anyway.
- **Roles are labelled.** Authors carry a role (`human`, `critic`, ...). Whether it is an AI is stated in the role definition (`kind: ai`). This shows what comes from an AI.
- **The critic's questions are threads.** The questions and red flags of a step are created as threads on the artifact, answers are messages. The why documents itself this way, and `threads-answered` can be a gate. (The examiner's comprehension questions are not review and stay out of this.)

### Condensation on completion

As soon as all required approvals are present and before the artifact is completed, the tool writes a `review` block into the artifact. The AI drafts the summary. The role that decides in the step (`decides`) must confirm it, and the confirmation is an event (`review.confirmed`).

```yaml
review:
  approved_by:
    - { name: alice, role: architect, hash: sha256:3f1c..., date: 2026-11-16 }
  summary:
    - q: "What happens when the queue fails?"
      a: "Fall back to synchronous processing, see dec-4h7r2x9b (idempotency)."
    - rejected: "Outbox pattern: too much operational effort for this ticket."
```

**The block is the interface between process and check.** Gates that only see the directory (for example in CI, where there is no local process store) check `approvals-valid` against this block. Locally, the tool can additionally consult the process store. The block can be edited by hand. Trust therefore comes from the team's review of the diff. Whoever needs more requires `require_proof` (see approvals).

Whoever later sees only the directory finds the why through this block. The quality of the condensation therefore decides whether something is missing later.

**Visibility** (Question 14). The summary can contain internal considerations. The profile sets what goes into the file: `review_summary: full` (approvals and summary, the default) or `approvals-only` (only approvals, the why stays in the process store). `approvals-only` is intended for public repositories.

**Retention of raw data** is set by the profile (`retention`): `keep` (stays active), `archive` (the segments are kept, but the tool no longer loads them into its index), `drop` (discarded). The default is `archive`.

## 11. File layout (proposal)

```
.hone/                   # configuration, see Section 8
design/
  tickets/<ticket-id>/
    ticket.yaml          # ticket record
    spec.md              # snapshot, if needed
  decisions/<id>-<slug>.md   # presets from hone/standard
  modules/<id>-<slug>.yaml
  test-plans/<id>-<slug>.md
src/ ...                 # domain types and interfaces as code
```

The paths come from the `kinds` of the configuration and can be changed. **Not in the working tree:** threads, approvals, and step state. They live in the process store (locally under `~/.hone/<workspace>/<project>/`, Section 4).

Decisions do **not** live in the ticket folder, because they live longer than the ticket and are superseded by later tickets.

Where this directory lives (in the repository, outside, ignored locally) is not a protocol question. If a project spans several repositories, the directory lives in one of them, and code links point into the others with `repo:path#symbol` (Section 5).

## 12. Tool layer (not part of the protocol)

This section is **not normative**. It collects what a tool can build on the protocol and how hone will probably do it.

**Versioning.** The tool usually puts the directory into the repository. Whoever does not want that excludes it (with Git, for example, through `.git/info/exclude`, the uncommitted, repository-specific exclude file) or places it outside. The protocol notices no difference. It assumes nothing beyond "a directory".

**Triggers for gates.** The call `hone check` is the same everywhere. The team decides when it runs:

| Trigger                                              | Purpose                                                        |
| ---------------------------------------------------- | -------------------------------------------------------------- |
| Agent hook (for example `PreToolUse` in Claude Code) | The agent may not write code while the decision is not binding |
| pre-commit / pre-push                                | Fast local feedback                                            |
| CI                                                   | Binding for the team, because local hooks can be bypassed      |

**Ticket ↔ code.** "Which commits belonged to ticket X" is answered by the tool, for example through the ticket ID in branch names or commit trailers. The protocol only contains what is in the artifacts (`context`, `links.code`).

**Index.** For queries, the tool keeps a local index (hone: an embedded database) next to `log/`. It is a cache, built from the logs, and simply rebuilt when its schema changes.

**Repository names.** Which location belongs to `repo:` in `repo:path#symbol` is bound locally by the device, as with contexts.

**Protected state in CI.** For CI to take `lock.yaml` from the base state, the platform needs a protected base branch and a mandatory review for `lock.yaml` (for example CODEOWNERS).

**Backup without a server.** The tool offers a folder (including a synchronized one such as Nextcloud) as a backup target. Technically this is a "shared folder" peer. Whatever a lost device never backed up is gone. The tool says so clearly during setup.

**Reports.** `check` and `report` output the design diff and drift report as JSON. The protocol will standardize the format later. Whether that becomes a pull request comment or a file is up to the tool. Reports are not stored in the process store, because they can be recomputed at any time (Question 13).

**Delivery.** How artifacts and code arrive as pull requests or stacks is a recommendation of the tool, not a protocol rule. Strategies hone can offer:

| Strategy            | Plan                                                                                |
| ------------------- | ----------------------------------------------------------------------------------- |
| `single`            | One unit with everything                                                            |
| `design-first`      | Unit 1: design (spec, decisions, domain, interfaces, tests), unit 2: implementation |
| `stack-per-group`   | Design unit, with one unit per group of related changes stacked on top              |
| `decision-separate` | Like `design-first`, but cross-cutting decisions in their own, independent unit     |

The implementation depends on the platform: one branch per unit, pull requests, stacked pull requests for chained units (where available), dependent pull requests or merge requests on Bitbucket and GitLab.

**Contexts.** Like Git remotes or the SSH configuration, the tool keeps a _context_ per tenant: server address, identity, and credentials. It lives locally and never in the project. The device binds locally which project belongs to which context. A project belongs to exactly one context, a device can have many. A freelancer typically has one per client, each with its own identity (the client's SSO or their own).

**Tenant separation.** The local store is separated per project. A confidential ticket gets its own project. There is no scope below the project (Question 20). An agent run receives context from exactly one project, never from two, otherwise one client's data flows into another client's prompts. At the end of an engagement there are `export` and `forget`, so that nothing is left behind (NDA, GDPR). Optionally, the tool encrypts the local store per project.

**Creation.** `hone init --from <package>` writes `extends` into the new configuration. This brings a custom standard process into every project without needing a global layer.

**Process store in Git (fallback).** For small teams without a server, a dedicated branch (for example `hone/process`) works, which is never merged into code branches. Since every device writes only its own log file, there are no merge conflicts. It remains a fallback: no real time, write access to the repository required, the history grows, and whoever can read the repository sees everything. Git notes and branch descriptions are unsuitable: branch descriptions are local and not pushed, notes can only be shared with an explicit push and can overwrite each other with several writers.

**Further integrations:** trackers (GitHub Issues, Jira, Linear: read tickets, write status back), review platforms (synchronize threads and approvals), agents (AGENTS.md, MCP, ACP: provide context, report drift). Language-specific tooling will be described in its own document.

## 13. Example: asynchronous payments

A team uses the standard configuration and has added a role:

```yaml
# .hone/config.yaml
extends: [hone/standard@1]
```

```yaml
# .hone/actors.yaml
alice: [human, architect]
bob: [human]
"*": [human]
```

**Start.** Ticket `tk-9f2x4a8m` (payments should run asynchronously) has the attributes `type: feature`, `risk: high`. The rule `feature + high` selects the profile `full`. The tool freezes the resolved configuration and records `config: sha256:5be2...`.

**Context (research and analysis, confirmed by a human).** The analysis finds the participants, at different granularity and nested:

- `PaymentGateway` (trait, its interface changes)
- `FeeCalculator` (class in the subsystem `payments`, only called, interface unchanged)
- `ledger` (subsystem, newly fed with events)

The ticket record keeps them under `context.observed`. Modules are only newly defined or changed during design.

**Decisions.** Two: `dec-…-payment-async.md` (`reach: cross-cutting`) and `dec-…-idempotency.md` (`reach: ticket`). The step `decision` requires an approval from the role `architect`. Bob only has `human` and can comment but not approve. Alice approves, the hash is recorded, and the condensation writes the `review` block.

**Check.** The team has added a plugin `public-api-has-decision` (WASM, read-only access to the project, no network) to the step `interfaces`. It reports an error if the public API of a module changes without a binding decision for it. The call `hone check` runs

- as an agent hook, before the agent writes code,
- as a pre-push hook,
- in CI, against the state of the change and its snapshot, without generators.

**Delivery.** Whether design and implementation arrive in one or several pull requests is decided by the tool layer (Section 12). The protocol sees the same files in both cases and checks the same rules.

## 14. Conformance and versioning

Three levels, so that implementations can start small:

1. **Core:** artifact format, IDs, kinds with lifecycle and anchors, links, effective status, reading the resolved configuration, standard gates.
2. **Flow:** + scope IDs, steps, profiles, selection rules, skipping, roles and actors, event log per device, approvals, threads, condensation.
3. **Extended:** + resolution from layers and packages, frozen snapshots, WASM plugins (gates, generators, language plugins), replication contract.

The specification includes JSON Schemas for all formats (artifacts, resolved configuration, events, plugin inputs and outputs) and a **conformance test suite** with which every implementation proves its level. Resolution comes with a collection of test cases (layers in, resolved document out), because two tools must resolve identically. The YAML profile and the content hash (Section 5) also have valid and invalid test cases, because two parsers must produce the same data and the same hash.

**Licenses** (Question 25). Anyone may do anything with it, including closed forks and hosted services.

- Code (CLI, sync server, user interface, plugins, packages, schemas, test suite): `MIT OR Apache-2.0`. Whoever wants chooses MIT. Whoever needs patent protection chooses Apache-2.0.
- Specification and concept text: CC0 1.0. The text becomes effectively public domain, with no attribution requirement.
- Contributions: DCO (Developer Certificate of Origin). Every commit carries `Signed-off-by:` (`git commit -s`) and thereby confirms that the contribution may be published under the project license. A contract (CLA) is not needed, because nobody has to relicense.

**Governance** (Question 15). For now, the project founder decides. Changes to the specification go through a public RFC in the repository. A foundation or committee comes only once there is a second implementation. The name is not protected as a trademark. Conformance is proven through the test suite, not through the name.

## 15. Questions

All questions of this draft are decided. The numbers stay stable because the text refers to them.

1. **Format:** Decided. JSON data model, written as a strict YAML profile (front matter in Markdown, plain YAML files for data kinds). TOML is rejected because it has no `null` (Section 5, "Format and content hash").
2. **Status "superseded":** Decided. Only computed, never written into the file. Hash and approvals stay valid, readability comes from the tool (Section 6).
3. **ID format:** Decided. Artifact IDs are a prefix plus 8 characters of Crockford Base32 and never change. The slug lives only in the file name (`{id}-{slug}`) and may change. The gate `ids-unique` catches collisions. Event IDs are UUIDv7, because a collision there would go unnoticed (Sections 4 and 5).
4. **Several repositories:** Decided. The artifacts live in one directory (one repository). Code may live in many repositories and is linked with `repo:path#symbol` (Section 5).
5. **Plugin runtime:** Decided. Only the contract is normative. Extism is the reference runtime, the component model may follow later as a second binding (Section 9).
6. **Trust in `exec` gates and plugins:** Decided. CI takes `lock.yaml` from the base state, hash pinning is enough. What "protected" means is up to the platform (Sections 9 and 12).
7. **Merging:** Decided. Maps instead of lists and `null` for removal are enough. Explicit list operations come only when a real case appears (Section 8).
8. **Snapshots:** Decided. Only snapshots no ticket record refers to are deleted, not even a completed one (Section 8).
9. **Attributes for selection:** Decided. Trackers or analysis suggest, a human sets. Changes are events and visible in the record. Abuse is made visible, not prevented (Section 8).
10. **Actors in the repository:** Decided. Handles instead of real names, optionally a generator from an external source (Section 8).
11. **Log format:** Decided. Segments per device, deterministic order (`after`, then `date`, then `id`), no compaction, archiving at segment level. Lost devices are not an error. Only the fully written event schema remains open (Section 16).
12. **Condensation:** Decided. The AI drafts, the role from `decides` confirms through the event `review.confirmed`. Trust comes from the diff review, optionally `require_proof` enforces a proof. The hash additionally normalizes NFC and trailing whitespace, but not line breaks (Sections 5 and 10).
13. **Role of reports:** Decided. JSON output of `check` and `report`, the format will be standardized later. Not an event in the process store (Section 12).
14. **Visibility:** Decided. Profile setting `review_summary: full | approvals-only` (Section 10).
15. **Governance:** Decided. For now the project founder decides, changes go through a public RFC. No trademark protection, conformance through the test suite (Section 14).
16. **Granularity of modules:** Decided. Only explicitly defined modules. The optional gate `module-anchors-resolve` reports moved anchors (Section 5).
17. **Open-core boundary:** Decided: there is none. Everything is open source.
18. **Agent transcripts:** Decided. Only linked: link type `transcript` and thread anchor `transcript` (Sections 5 and 10).
19. **Standardize the sync interface?** Decided: yes. The replication contract is part of the protocol (conformance level Extended).
20. **Confidential tickets:** Decided. The scope stays at project level, a confidential ticket gets its own project (Section 12).
21. **Author identity:** Decided. No identity across servers, every context has its own. With a server, the server sets the identity. Without one, it is trust-based. The field `sig` is reserved for signatures in a later version (Section 4).
22. **Backup and device loss without a server:** Decided. The tool offers a folder as a backup target. Whatever was never backed up is lost (Sections 4 and 12).
23. **Modules as living definitions:** Decided. Modules are a preset with lifecycle `living`, freely editable. A deliberate cut is an approved decision with `affects`. `context.observed` takes a module ID or an anchor (Section 5).
24. **Terms in the schema:** Decided. The artifact field `scope` is now called `reach`. `verifies` takes only artifact IDs, test code is linked through `code` (Section 5).
25. **Licenses and contributions:** Decided. Code under `MIT OR Apache-2.0`, specification text under CC0 1.0, contributions through DCO (Section 14).
26. **Scope of the server contract:** Decided. Normative are replication, real-time subscription, bearer authentication, and defined error codes. Permission management, administration, and notifications are left to the server (Section 4).

## 16. Next steps

1. Done: format (Question 1) and the artifact schema with tests (`schemas/`), adapted to all decisions (ID format, `reach`, lifecycles instead of fixed types, presets under `schemas/standard/`, module with `interface`, `context.observed`, hash normalization).
2. Done: README, license files (`LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE-CC0`), and `CONTRIBUTING.md` with DCO.
3. Write the schema of the resolved configuration (including `kinds.<type>.lifecycle` and the placeholders `{id}`, `{slug}`, and `{ticket}` in `kinds.<type>.path`).
4. Build `resolve`: layers, merging, test cases. This is the core of Level 2 and pins down Level 1.
5. Build `check` with the standard gates (including `ids-unique` and `change-needs-binding`) and connect it as an agent hook. With that, the tool can be used alone right away, without a team and without Git integration.
6. Define scope IDs and the event schema (UUIDv7, `seq`, `after`, reserved `sig`), build the local backend (segments per device, index as cache).
7. Implement the plugin contract with Extism: a first gate (`public-api-has-decision`) in Rust, capabilities, and hash pinning.
8. Write down the replication contract in the decided scope (Question 26) and build a reference backend: shared folder, then server.
9. After that: condensation, language plugins (including `module-anchors-resolve`), the user interface, and the tool layer (contexts, Git integration, delivery).
10. Turn this concept into the normative specification in `spec/`.

## Sources

- MADR (decision format): https://adr.github.io/madr/
- Extism, overview: https://extism.org/docs/overview
- Extism, manifest (allowed_hosts, allowed_paths, timeout, memory limit, config): https://extism.org/docs/concepts/manifest
- Extism, memory and data exchange (bytes in, bytes out): https://extism.org/docs/concepts/memory
- GitHub stacked pull requests (public preview since 2026-07-30): https://privatedevops.com/news/github-stacked-pull-requests-public-preview
- Git: branch descriptions are local, notes need an explicit push: https://lore.kernel.org/all/CAB9Jk9ABenaj=R0a6OW2GCsin8PdDCW3ZbuQbu6G0jnGG3s+sA@mail.gmail.com/T/ and https://public-inbox.org/git/CALKQrgfQ4-A=NgR_gSsaSXMjRzQZtXBdvrWbzTBGH3jyfC4bJg@mail.gmail.com/T/
- Quarkus ADR process (status through pull requests): https://git.struchkov.dev/OpenSourceArk/quarkus/src/branch/main/adr
- TOML 1.1.0 (no null type, multi-line inline tables allowed): https://toml.io/en/v1.1.0
- YAML 1.2.2, changes from 1.1 (core schema, merge keys removed): https://yaml.org/spec/1.2.2/ext/changes/
- YAML 1.2.2, specification (core schema without timestamp, keys must be unique): https://yaml.org/spec/1.2.2/
- JSON Canonicalization Scheme (RFC 8785): https://www.rfc-editor.org/rfc/rfc8785
- serde_yaml is deprecated, alternatives: https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868
- serde-saphyr (rejects duplicate keys by default): https://docs.rs/serde-saphyr/latest/
- jsonschema for Rust (supports draft 2020-12): https://docs.rs/crate/jsonschema/0.49.6
- Extism, host functions (a plugin can call host code, deliberately not used for commands): https://extism.org/docs/concepts/host-functions
- Claude Code hooks (exit code 2 blocks PreToolUse, exit 1 does not): https://docs.anthropic.com/en/docs/claude-code/hooks
