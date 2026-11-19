# hone protocol: concept

Status: draft, as of 2026-11-19. This concept is the source for the normative specification in `spec/`. Where a chapter of `spec/` exists, it is normative and wins over this text.

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

Scope IDs have the form of artifact IDs (Question 3): the prefix, a hyphen, and 8 characters of lowercase Crockford Base32, for example `prj-8m2k4d7q`. `workspace` is `local` or a `ws-…` ID. A device writing to a project carries a device ID of the same form (`dev-3k9a5h2t`). It identifies the device within one project only: the same machine has a different device ID in every project, so logs of two tenants cannot be linked to one device.

Rules:

- **The project is the unit of replication.** Log and sync belong to exactly one project. Whoever works with many tenants never receives foreign logs, and no client receives another client's data.
- Every event carries `project`. A server rejects events that do not match its scope.
- The IDs `workspace` and `project` live in `.hone/config.yaml`. They are not secret and are created by `init`. A value that does not have the form above is an error (`manifest`). An ID stays the same across clones, worktrees, and devices. A hash of the path or remote URL would not.
- Server addresses, identities, and credentials never live in the project (Section 12).

### Data and its storage

|          | Artifacts                                                                       | Process data                                                  |
| -------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Content  | Specs, decisions, module definitions, test plans, ticket records, configuration | Threads, approvals, step state, further events                |
| Property | Readable without a tool, durable, part of the project                           | Large volume, value drops after completion, must be queryable |
| Storage  | Files in a directory (Section 11)                                               | Event log                                                     |

From artifact storage, the protocol requires only a directory that can be read and written. Everything else (versioning, sharing, ignoring) is outside the protocol.

### Process store

The rules of the process store (logs, segments, lines, the envelope, the event types, the reading rules, the order, and the cursor) are normative text in [`spec/process-store.md`](../spec/process-store.md) (Question 41). The reasons behind them:

- **One log per device.** Only one device appends to a log, so there are no competing writers, and merging is a union. Diverging clocks change at most the order of concurrent events, never the order of events that knew each other, because `after` carries causality.
- **Canonical lines.** RFC 8785 makes the bytes of an event unambiguous, which a later `sig` will sign. A torn last line is an interrupted write, not corruption.
- **Full UUIDv7 IDs.** Duplicate IDs are ignored during sync, so a collision would silently swallow an event. The timestamp of the ID is the only time of an event, so two fields cannot contradict each other.
- **Why no database format as the source of truth.** Database files from two devices cannot be merged without conflicts, neither through a shared folder nor through Git. Appended text files can. For fast queries, an implementation builds an index (for example an embedded database). It is a cache, can be rebuilt from the logs at any time, and is not part of the protocol (Section 12).
- **Lost devices.** Whatever a device never synchronized is lost with it. If a device's `seq` ends, that is not an error.

```yaml
# an event (shown as YAML, stored as one line of JSON)
id: ev-0193a8f2-6c1e-7b3d-9f42-8e1a5c7d2b90 # UUIDv7 (RFC 9562), lowercase
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
device: dev-3k9a5h2t # writing device
seq: 118 # sequence number in this device's log
type: thread.message
author: { name: alice, role: human }
after: [ev-0193a8f1-...] # optional: the newest known event of every other device
payload: { thread: th-0193a8f0-..., body: "..." }
```

### Replication

The replication contract (`append`, `read`, `sync`, duplicates and conflicts), the shared folder, and the server transport with its error codes are normative text in [`spec/replication.md`](../spec/replication.md) (Question 41). Events are never changed and status and order are computed from the set, so there are no merge conflicts; a `fork` (two machines writing as one device, for example after a copied data directory) is the one conflict a person resolves. A shared folder needs no program on the other side; its layout is normative so that tools of different vendors can share one folder, and one folder can serve many projects, for example as a backup target. Push never touches another device's files, so two writers never write the same file, and a planted entry in the folder cannot stop the sync of everyone else.

A shared folder has no real time and no permissions below the access to the folder. A Git branch is a shared folder whose root is a worktree of that branch (Section 12).

| Peer                    | Purpose                                                                                                                                                        |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Server**              | Default for teams: real time, permissions, notifications                                                                                                       |
| Shared folder or branch | Fallback for small teams without a server. Since every device writes only its own file, nothing collides. No real time, no permissions below repository access |
| none                    | Solo use, everything local. Backup is then the user's responsibility. The tool offers a folder as a backup target (Section 12)                                 |

**Role of the server.** It is the rendezvous point and only replicates. In addition, it

- authenticates authors and enforces the identity in the event: it rejects an event whose `author.name` is not the authenticated identity. A client cannot choose it freely. The server never rewrites an event, because the client already holds it and a changed copy with the same `id` would be an `id-conflict`,
- checks the scope and, for approvals, the role according to `actors`. Where the server learns the actors (its own administration, a linked repository) is its own business, because it does not hold the project's files,
- manages permissions per project and provides real time and notifications.

The replication contract is part of the protocol (Question 19). The server that speaks it and the user interface belong to the reference implementation and are open source. Anyone can build their own server or client or run the reference server themselves.

**Normative scope of the server contract** (Question 26): replication, the real-time subscription, the bearer token as a transport requirement (how a client obtains it is governed by OAuth 2.0 or OpenID Connect), and the rejection of events outside the scope and of approvals with a mismatching role, with defined error codes. The transport is specified in [`spec/replication.md`](../spec/replication.md) (Questions 33 and 34). The identity is checked before duplicates, so that a client cannot append another author's event even when the server already holds it; a device belongs to the author of its first event on the server, because device IDs are not secret and another member could otherwise block a device with a `fork`.

A client writes the author of its context into its events. It appends only events it wrote itself, because the identity check would reject those of others. Sync with a server is `cursor`, `append`, and `read` composed by the client.

**Not normative** are permission management (who sees which project), administration, and notifications. Every server solves these on its own.

**Identity.** There is no identity across servers. Every context (Section 12) has its own, which matches tenant separation. With a server, the server enforces the identity in the event: the client writes it, the server rejects a wrong one (`identity-mismatch`). Without a server, authorship is trust-based until a later version introduces signatures (`sig`).

**CI does not have the process store.** For gates to work there, the `review` block in the artifact is the interface (Section 10). CI may additionally query the server, but the baseline check must not depend on it.

## 5. Artifacts

The rules of this section are normative text in [`spec/artifacts.md`](../spec/artifacts.md) (Question 41); this section keeps the examples and the reasons.

Every artifact is a file. The core knows exactly one fixed type, the **ticket record**. All other types are **kinds** (`kinds`), defined by the configuration (Section 8).

**`spec`, `decision`, `test-plan`, and `module` are presets, not protocol types.** They come with the package `hone/standard` because almost every team needs them. A project can delete, rename, or change them or replace them with its own kinds, exactly as it adds its own kinds such as `threat-model`. The protocol itself knows no design concept, only the mechanisms the presets are built on:

- **Lifecycle** (`kinds.<type>.lifecycle`):
  - `decided`: The artifact belongs to a ticket, has a status from `kinds.<type>.statuses` (Section 6) and a content hash, and can be approved. Required fields are `id`, `type`, `ticket`, `status`, `created`, and `authors`. Examples: `spec`, `decision`, `test-plan`.
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

**Why a profile:** Different YAML parsers read the same file differently. A YAML 1.1 parser turns `NO` into `false`, `010` into the number 8, `1:30` into the number 90, and `2026-11-16` into a date. YAML 1.2 cleaned this up with the core schema, but libraries differ, and two tools must see the same data and the same hash. The profile allows only the subset in which everyone reads the same: no floating-point numbers, no anchors, aliases, tags, or duplicate keys, `null` written out so that a forgotten value does not silently remove something, ambiguous strings quoted, and dates as strings ([`spec/artifacts.md`](../spec/artifacts.md#data-model-and-yaml-profile)).

A YAML parser alone is not enough to check the profile, because it does not always report anchors, tags, and duplicate keys. The reference implementation therefore checks the parser's event stream. Unknown fields are preserved (Principle 11) and reported as a warning, not as an error, so that a newer tool's fields do not break an older check; a kind schema that wants deeper levels strict sets `additionalProperties: false` itself.

**File shapes, paths, and schema** ([`spec/artifacts.md`](../spec/artifacts.md#file-shapes)). A `.md` file has front matter between two `---` lines and a Markdown body; a `.yaml` file is the data model itself. A file is an artifact when its path matches the pattern of `ticket.path` or of a kind, and path and content must agree, because a file name and an ID that tell different stories would make links ambiguous. When `type` disagrees with the path, the content is not checked further, because it is unclear which kind applies. A file that looks like an artifact but matches no pattern, for example `dec-7k3m2q8-payment.md` with one character missing, is probably a typo and gets a warning; other files such as a `README.md` are ignored. `schemas/artifact.schema.json` distinguishes the ticket record and the two lifecycles; every kind can add a schema of its own, and the presets live with their definitions and schemas in the standard package (`schemas/standard/`). `superseded` must never appear in a file, because it is computed (Question 2).

**Content hash** ([`spec/artifacts.md`](../spec/artifacts.md#content-hash)). Only artifacts with lifecycle `decided` have one, because only they are approved; the ticket record and living artifacts are mutable. The hash is taken over the canonical JSON of front matter (without the `review` block) and body, after normalizing the body (NFC, LF, no trailing whitespace, no leading or trailing line breaks). Reformatting the YAML, CRLF, trailing whitespace, and changes to the `review` block therefore leave the hash unchanged, while every change to fields or text changes it, including a new line break in the body. Markdown formatters should therefore not rewrap paragraphs (with Prettier, for example, `proseWrap: preserve`). Hashing a Markdown AST is deliberately excluded, because two parsers would hash differently. The test cases for profile, schema, and hash are described in [`spec/conformance.md`](../spec/conformance.md#artifact-files).

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

**Changes.** A module is a normal file and may be edited freely. Changes are visible through versioning like any other file. A **deliberate cut** or a shift of a boundary is a decision with `affects: [mod-…]`, and the decision is approved (Question 23). Broken anchors are reported by the optional standard gate `module-anchors-resolve` (Questions 40 and 46, [`spec/gates.md`](../spec/gates.md#the-standard-gates)): hone checks paths and globs itself and asks the language plugin of each file whether a symbol exists, so one gate covers every language a project has a plugin for. A symbol in a file without a language plugin is a warning, not an error, because the gate cannot see it.

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

The **state of the steps** (open, done, skipped, with an optional reason) changes constantly and therefore lives in the process store. It is the event `step.changed` with `step`, `status` (`open`, `done`, or `skipped`), and an optional `reason`. A correction is simply a later event. On completion, the tool records the result in the ticket record, so that skipping stays visible to the team:

```yaml
completed:
  steps_done: [ticket, research, decision, interfaces, tests, implementation]
  steps_skipped:
    - { step: domain, reason: "no new terms" } # reason optional
```

The ticket record is the **key** that connects everything: artifacts, configuration, and external tickets. The mapping to commits, branches, or pull requests does not belong in the protocol (Section 12).

## 6. Lifecycle and status

The rules of this section are normative text in [`spec/artifacts.md`](../spec/artifacts.md#status) (Question 41); this section keeps the examples and the reasons.

There is a **declared** and an **effective** status.

- _Declared:_ what the author writes into the file, one of the statuses of the kind (`kinds.<type>.statuses`).
- _Effective:_ what the tool computes from the declared status and the links **in the checked directory**.

**Statuses are configurable per kind** (Question 27). The protocol knows only one property of a status: whether it is **binding** (`binding: true`). Every kind with lifecycle `decided` has at least one binding status. The name `superseded` is reserved, because that status is computed. The standard package sets `proposed`, `accepted` (binding), `rejected`, and `deprecated`. A project can add its own (`draft`) or remove one (`deprecated: null`).

| Declared           | Superseded by a binding artifact in the directory | Effective                |
| ------------------ | ------------------------------------------------- | ------------------------ |
| binding status     | no                                                | **the status (binding)** |
| binding status     | yes                                               | superseded               |
| non-binding status | –                                                 | the status               |

With the standard statuses: `accepted` stays `accepted` or becomes `superseded`, all others stay as declared.

A superseding artifact counts when its **declared** status is binding, whatever its kind. Its own effective status does not matter. In a chain where B supersedes A and C supersedes B, all three declared `accepted`, A and B are superseded and C stays `accepted`. Whether a status is binding comes from the configuration the gate uses (Section 7). A status that this configuration does not know is not binding.

**The caller determines the checked directory.** An agent hook checks the working state, CI checks the state of the change, a review checks the result. The protocol knows no lines, branches, or "integration". What counts is what is in the directory.

**Only binding artifacts bind agents** and are enforced by the gates. Design-first therefore needs no special rule: a check such as "changed code needs a binding decision" (`change-needs-binding`, Section 7) is a pure state check. If the design is already on the base of the change, it passes. If it is part of the same state, it passes as well.

Artifacts with a binding status are not changed in content. A change is a new artifact that replaces the old one through `supersedes`. The old one is **not touched** for this. The status "superseded" is only computed and never written into the file, so that hash and approvals stay valid. The tool makes it readable (`explain`, user interface, editor hints).

Status exists only for kinds with lifecycle `decided`. Artifacts with lifecycle `living` (such as modules) have no status.

## 7. Steps, gates, profiles (the building blocks)

The rules of this section are normative text in [`spec/gates.md`](../spec/gates.md) (Question 41); this section keeps the examples and the reasons.

### Step definition

```yaml
# steps.decision in the resolved configuration; the key is the name
title: Decision # optional
acts: [human, critic] # roles that work (roles: Section 8)
decides: architect # optional: role that decides and confirms the condensation
inputs: [spec] # artifact kinds, `ticket` stands for the ticket record
outputs: [decision]
requires: [] # further steps that must come first
gates: # deterministic checks, a map by label
  decision-exists: { gate: artifact-exists, params: { type: decision } }
  links: { gate: links-resolve }
  approved: { gate: approvals-valid, params: { role: architect, min: 1 } }
```

A step describes inputs, outputs, participants (as roles, never as people), and checks. It contains **no** instructions for a specific agent.

**Gates are a map by label**, so that a higher layer can add a single gate (for example a plugin gate on a standard step) or remove one (`links: null`) without repeating the others. The same gate may appear several times under different labels. A gate entry is `{ gate, params }`. `gate` names a standard gate, `exec` (Section 9), or a plugin from `plugins`. A plugin must not carry the name of a standard gate or `exec`.

**The order follows from inputs and outputs.** A step is ready when its inputs are done or skipped. Whoever wants a fixed order adds `requires: [<step>]`. Cycles are an error. Adding a step means inserting a node, not rewriting a list.

### Gates

A gate is a **deterministic function** over a directory: the input is the checked state (optionally a second state `base` and the list `changed_paths`, both supplied by the caller), the output is pass or fail, plus findings. The call is the same everywhere (`hone check`), whether hook, CI, or by hand.

**Standard gates** belong to the protocol, and every implementation ships them natively, so that the same state gives the same result everywhere. Their parameters and checks are listed in [`spec/gates.md`](../spec/gates.md#the-standard-gates). `schema-valid`, `ids-unique`, and `links-resolve` check the whole checked directory, because a collision or a broken link concerns everyone. All other gates check the ticket. The standard package sets the parameters `{ anchored: module, binding: decision }` for `change-needs-binding`.

**What `check` runs** ([`spec/gates.md`](../spec/gates.md#what-check-runs)). The directory gates run against the current resolved configuration, because a directory gate concerns the artifacts of all tickets and an older snapshot does not know the kinds of newer tickets. Ticket gates run against the ticket's snapshot, so that a ticket keeps the rules it started with. A missing or out-of-date configuration stops `check` instead of producing a finding, because no gate result would be meaningful.

**Findings** carry fixed, normative codes, so that a test case can name the expected reason ([`spec/gates.md`](../spec/gates.md#codes)). The gates that read the process store fail without one (`process-store-unavailable`), so that nothing passes by mistake: no threads would otherwise look like no open threads. An invalid event is only a warning, so that an immutable broken segment of one device cannot block a project forever.

**`change-needs-binding`** is a pure state check: if the binding decision is already on the base of the change, or part of the same state, it passes. A superseded decision binds nothing any more. A `symbol` anchor hits its whole file, because finding the symbol itself is the job of a language plugin.

**Everything else is a plugin** (Section 9): a gate is then a WASM module, included through the same entry.

### Profile

A profile selects steps and their strictness. The steps are a **map**, so that layers can merge them key by key (Section 8).

```yaml
# profiles.medium in the resolved configuration
retention: archive # keep | archive | drop (Section 10)
review_summary: full # full | approvals-only (Section 10)
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
- Steps that the profile does not list are not part of the flow.
- A profile selects only steps and their strictness. There is no strictness per gate: whoever needs other gates uses another step or a plugin.
- The selection rules (Section 8) choose the profile for a ticket. The user can change the result per ticket.

## 8. Configuration: Level 1 and Level 2

The rules of this section are normative text in [`spec/configuration.md`](../spec/configuration.md) (Question 41); this section keeps the examples and the reasons.

The configuration must be dynamic: actors come and go, kinds of tickets need different flows, teams invent their own artifacts. The protocol separates three levels for this.

| Level                         | What                                                                        | Who reads it                    | Flexibility      |
| ----------------------------- | --------------------------------------------------------------------------- | ------------------------------- | ---------------- |
| **1. Resolved configuration** | Pure data, one document                                                     | Every implementation, gates, CI | none, on purpose |
| **2. Generation**             | Layers of YAML files, packages, optional generator plugins that output data | Only `resolve`                  | arbitrary        |
| **3. Behaviour**              | Gates, generators, language plugins as WASM (Section 9)                     | The tool, in a sandbox          | arbitrary        |

Rule of thumb: **Whoever only reads the configuration needs only data (Level 1). Whoever changes it works in Level 2. Whoever adds behaviour writes a plugin (Level 3).**

### Level 1: the resolved configuration

The protocol reads only this document (`.hone/resolved.json`, generated, not edited by hand; shown here as YAML for readability). It contains everything needed to evaluate a ticket and nothing that executes code. Its schema is `schemas/config.schema.json`. The document contains no `null`, every default is written out, and unknown keys are an error except in the namespace `x-<name>`. Definitions are maps whose key is the name, so there is no inner `id`.

```yaml
protocol: 1

ticket: # the only fixed type of the core; only the placeholder {ticket}
  path: "design/tickets/{ticket}/ticket.yaml"

kinds: # kinds of artifacts (spec, decision, test-plan, module come from hone/standard)
  spec:
    path: "design/tickets/{ticket}/{id}-{slug}.md" # {ticket} only for lifecycle decided
    id_prefix: spec
    lifecycle: decided
    statuses:
      proposed: { binding: false }
      accepted: { binding: true }
      rejected: { binding: false }
      deprecated: { binding: false }
  decision:
    path: "design/decisions/{id}-{slug}.md" # {slug} may change, {id} never
    id_prefix: dec
    lifecycle: decided # decided | living
    statuses: # a map, so that a layer can add or remove a single status
      proposed: { binding: false }
      accepted: { binding: true } # binding: binds agents and gates, can supersede
      rejected: { binding: false }
      deprecated: { binding: false }
    schema: # JSON Schema, embedded by resolve
      type: object
      properties:
        reach: { enum: [ticket, cross-cutting], default: ticket }
  module:
    path: "design/modules/{id}-{slug}.yaml"
    id_prefix: mod
    lifecycle: living # no ticket, no statuses
    schema: { type: object, required: [name, anchors] } # shortened
  threat-model: # custom kind with its own statuses, no protocol change needed
    path: "design/threat-models/{id}-{slug}.md"
    id_prefix: thr
    lifecycle: decided
    statuses:
      draft: { binding: false }
      accepted: { binding: true }
      withdrawn: { binding: false }

roles: # what someone may do in the process
  human: { kind: human, can: [act, comment, approve] }
  architect: { kind: human, can: [act, comment, approve] }
  critic: { kind: ai, can: [act, comment] }

actors: # who has which roles (handles, not real names)
  alice: [human, architect]
  bob: [human]
  "*": [human] # fallback for unknown people

steps: # Section 7
  specify:
    title: Specification
    acts: [human]
    decides: human
    inputs: [ticket]
    outputs: [spec]
    requires: []
    gates:
      spec-exists: { gate: artifact-exists, params: { type: spec } }
      spec-approved: { gate: approvals-valid, params: { role: human } }
  decision:
    title: Decision
    acts: [human, critic]
    decides: architect
    inputs: [spec]
    outputs: [decision]
    requires: []
    gates: # a map by label, so that a layer can add or remove a single gate
      decision-exists: { gate: artifact-exists, params: { type: decision } }
      links: { gate: links-resolve }
      critic-answered: { gate: threads-answered, params: { role: critic } }
      approved: { gate: approvals-valid, params: { role: architect, min: 1 } }
  threat-model:
    title: Threat model
    acts: [human]
    decides: architect
    inputs: [spec]
    outputs: [threat-model]
    requires: []
    gates:
      threat-model-accepted:
        {
          gate: artifact-exists,
          params: { type: threat-model, status: accepted },
        }
  interfaces:
    title: Interfaces
    acts: [human]
    inputs: [decision]
    outputs: [module]
    requires: []
    gates:
      compiles:
        { gate: exec, params: { command: ["cargo", "check", "--workspace"] } }
      public-api: { gate: public-api-has-decision } # plugin gate
  implementation:
    title: Implementation
    acts: [human]
    inputs: [module]
    outputs: []
    requires: []
    gates:
      binding:
        {
          gate: change-needs-binding,
          params: { anchored: module, binding: decision },
        }
      tests:
        {
          gate: exec,
          params: { command: ["cargo", "test"], env: ["CARGO_HOME"] },
        }
  completion:
    title: Completion
    acts: [human]
    decides: architect
    inputs: []
    outputs: []
    requires: [implementation]
    gates:
      complete: { gate: steps-complete }
      no-blockers: { gate: no-open-blocking-threads }

profiles:
  quick-fix:
    steps: { implementation: required, completion: required }
    retention: drop
    review_summary: full
  standard:
    steps:
      specify: required
      decision: optional
      interfaces: required
      implementation: required
      completion: required
    retention: archive
    review_summary: full
  full:
    steps:
      specify: required
      decision: required
      threat-model: required
      interfaces: required
      implementation: required
      completion: required
    retention: keep # Section 10
    review_summary: approvals-only

select: # profile selection, the first matching rule wins, otherwise default
  rules:
    - { when: { type: bugfix, size: S }, profile: quick-fix }
    - { when: { type: feature, risk: [high, critical] }, profile: full }
  default: standard

plugins: # Section 9
  public-api-has-decision:
    wasm: .hone/plugins/public-api-has-decision.wasm # relative to the project root
    hash: sha256:d83e964ad830a888709a27907868418c04d8847563a35859e04a1681db24b733
    capabilities:
      { read: [project], net: [], timeout_ms: 5000, memory_pages: 256 }
```

**Roles and actors are separate.** Steps and gates name only roles. Actors are mappings, and where they come from is replaceable (file, generator from CODEOWNERS or an identity provider, tool adapter). Splitting people up means adding or removing entries. The fallback `"*"` makes sure unknown people break nothing. Actors are **handles** (the same as in version control), not real names. Whoever wants more distance generates them from an external source (Question 10).

**Selection rules are deliberately simple:** `select` holds a list `rules` and a `default`. `when` compares ticket attributes (`attributes` in the ticket record): a scalar for equality, a list for set membership, and every attribute of `when` must match. The first matching rule wins, otherwise `default`. There is no expression language. `default` is its own key, so that a layer can change it without replacing the rules. Whoever needs more sets attributes beforehand with a generator or in the user interface.

**Origin of attributes** (Question 9). Trackers or analysis may suggest attributes, a human sets them. If someone changes them after the start, that is an event (`ticket.attributes_changed`) and appears in the record on completion. The protocol does not prevent someone from entering `risk: low` to avoid gates. It makes it visible, as with skipping (Principle 8). Whoever wants more writes a plugin gate, for example "the risk must match the analysis".

**Properties of the resolved configuration** ([`spec/configuration.md`](../spec/configuration.md#the-resolved-configuration)). It is canonical (RFC 8785), so that its hash is a stable identity of the content and no formatter may touch it. It is self-contained, so that a snapshot needs no package: `resolve` embeds the schemas of the kinds. It is explicit, so that whoever reads it needs to know no defaults. The origin of every definition may live next to it (`resolved.origin.json`, for `hone explain`), never inside it, so that the hash depends only on the content. Every reference is validated; otherwise `resolve` fails.

### Frozen per ticket

When a ticket starts, its resolved configuration is stored as a snapshot and recorded in the ticket record by hash ([`spec/configuration.md`](../spec/configuration.md#snapshots)). Gates check a ticket against its snapshot, so CI needs neither generators nor packages, and a ticket keeps the rules it started with. A deliberate switch to new rules is an event and visible in the record. Snapshots are stored by content, so tickets with the same configuration share one file; the file name uses the hex part only, because a colon is not allowed in file names on every system. Plugins are not frozen: `lock.yaml` approves one hash per plugin, so after an update a ticket on the old configuration fails `plugin-hash` until it switches. A snapshot is deleted only when no ticket record refers to it any more, not even a completed one, so that every ticket stays auditable (Question 8).

### Level 2: generation

The resolved configuration is built from **layers**. From bottom to top:

```
packages (extends, dependencies first)  →  project (.hone/)  →  generators (generate)  →  ticket (overrides)
```

`hone/standard` is a package like any other. It is not implicit: a project that wants it lists it in `extends`.

There is deliberately **no global or personal layer.** It would mix context between projects and tenants and does not fit into a snapshot. Personal settings of the tool (agent, model, notifications) are not protocol configuration. A custom standard process is a package (see below).

**Merging** ([`spec/configuration.md`](../spec/configuration.md#merging)) must be defined exactly, so that two tools resolve identically. Objects merge key by key, lists are replaced, and `null` removes. Where merging is needed, the schema uses maps instead of lists (hence `steps` in the profile as a map), so there are deliberately no explicit list operations (Question 7). A `null` that removes nothing is an error, so that a typo does not silently do nothing, and the profile requires an explicit `null`, so that a forgotten value removes nothing either.

```yaml
# project, .hone/profiles/medium.yaml: remove the step `domain` and make `decision` required
steps:
  domain: null
  decision: required
```

**Layer layout** ([`spec/configuration.md`](../spec/configuration.md#layers)). Every package and the project's `.hone/` have the same layout, one definition per file with the file stem as the key, so that a layer adds or removes a single definition without repeating the others. Every unknown entry is an error, because a typo such as `profile/` would otherwise do nothing silently.

```
.hone/
  config.yaml            # workspace, project (IDs), extends, generators
  kinds/  roles/  steps/  profiles/     # one definition per file, the file stem is the key
  plugins/               # <name>.yaml (capabilities) and <name>.wasm
  schemas/               # JSON Schemas of the project's own kinds
  select.yaml  actors.yaml  ticket.yaml
  lock.yaml              # packages and plugins: version, hash, approved capabilities
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

**Packages** have the same layout as the `.hone/` directory. Where a package comes from (directory, archive, registry) is up to the tool. A workspace can publish a package (the organization level of a tenant, for example its standard profiles for all of its repositories), and a single person's own standard process is also just a package. Packages are ordered depth-first, dependencies first, and the first occurrence counts, which also ends cycles. Two majors of one package are an error.

**`lock.yaml`** pins every package, including the transitive ones, by an exact version and a hash, and approves every plugin with its hash and capabilities:

```yaml
# .hone/lock.yaml
packages:
  hone/standard: { version: "1.4.2", hash: "sha256:…" } # exact semver version
plugins:
  public-api-has-decision:
    hash: "sha256:d83e…" # SHA-256 of the .wasm file
    capabilities: # the approved capabilities
      { read: [project], net: [], timeout_ms: 5000, memory_pages: 256 }
```

How the tool pins `exec` commands (Section 9) is the tool's business.

**Kind schemas** are paths in a layer, relative to that layer's root, and are embedded by `resolve` after merging. An inline schema in YAML is not allowed, and the path is a leaf during merging: a higher layer replaces or removes it, never merges into the schema.

**Plugins** in a layer declare only their capabilities; `resolve` adds the file and its hash and checks them against `lock.yaml` and fails when they exceed the approved ones. Copying a `.wasm` file out of a package is the tool's job.

**Defaults** are written out by `resolve` ([`spec/configuration.md`](../spec/configuration.md#defaults)), so that the resolved document needs no knowledge of defaults.

**Ticket overrides** are a last patch with the same merge rules, part of the snapshot. They must not touch `protocol`, `ticket`, `plugins`, or a kind schema, because an override has no layer root that a schema path could be resolved against.

**Generators** are plugins (Section 9) that output parts of the configuration as data:

```yaml
# .hone/config.yaml
generate:
  - plugin: .hone/plugins/actors-from-codeowners.wasm
    hash: sha256:7c20...
    capabilities: { read: [".github/CODEOWNERS"], net: [] }
    into: actors # only this part of the tree may be set
```

A generator receives a layer and parameters and returns a patch in the same shape as any other layer. Only the project declares generators. Their patches are applied after the project files, in the order of `generate`, and before the ticket overrides. A generator runs only on `hone resolve`, started by a human or the tool. **Not on every check and not in CI.** It may therefore do things that vary over time (for example an identity provider as a source with `net: [idp.example.com]`), because the result is frozen.

**Resolution steps and errors** ([`spec/configuration.md`](../spec/configuration.md#resolution)). `resolve` works in a fixed order and reports fixed error categories, so that test cases can name the expected error.

**Growing and shrinking**

| Change                                         | Effort                                                         |
| ---------------------------------------------- | -------------------------------------------------------------- |
| New person                                     | One line in `actors.yaml` (or a generator)                     |
| Split a role (`human` → `human` + `architect`) | Add the role, change the mapping, adjust `decides` in the step |
| New kind of artifact                           | One `kind` file, one step, one schema                          |
| Remove a preset (for example `test-plan`)      | `kinds/test-plan.yaml` containing `null`, and remove the step  |
| Remove a step                                  | `steps/<step>.yaml` containing `null` in the higher layer      |
| Stricter rules for risky tickets               | Selection rule and profile                                     |
| Custom check                                   | Write a plugin and add it to the step                          |

**Tools:** `hone explain <ticket>` shows the resolved configuration with the origin of every line. `resolve --check` checks whether `resolved.json` matches its sources (with generators only in a trusted environment).

## 9. Plugins: Level 3

The rules of this section are normative text in [`spec/plugins.md`](../spec/plugins.md) (Question 41); this section keeps the examples and the reasons.

Behaviour beyond the standard gates comes from **WebAssembly plugins**. Only the contract below is normative (entry point, JSON in, JSON out, capabilities). The reference runtime is [Extism](https://extism.org/docs/overview), but it is not part of the specification. The WASM component model may be added later as a second binding (Question 5).

**Why WASM**

- **Sandbox:** A plugin runs isolated from host memory. What it may do is defined by the manifest (network hosts, file paths, memory limit, time limit).
- **Language-independent:** Plugins are written in the language the team knows. Extism offers plugin development kits (PDKs) for several languages, including Rust, Go, and C/C++, and host SDKs for many languages, including Rust and PHP.
- **Simple contract:** A call is `function_name(bytes) → bytes`. The protocol uses JSON.

### Contract

A plugin exports named entry points: `describe`, `check` for gates, `generate` for generators, and `resolve` and `public_api` for language plugins ([`spec/plugins.md`](../spec/plugins.md#contract)). Input and output are JSON, so the contract is the same in every language.

**Language plugins** (Question 46, [`spec/plugins.md`](../spec/plugins.md#language-plugins)). A language plugin knows the symbols and the public API of the files of its languages and nothing else; the standard gates `module-anchors-resolve` and `public-api-has-decision` call it, because WASM plugins cannot call each other. Its plugin file names the extensions it handles, so the mapping lives in one place per plugin:

```yaml
# .hone/plugins/lang-typescript.yaml
capabilities: { read: [project], timeout_ms: 5000, memory_pages: 256 }
language: { extensions: [ts, tsx, js, jsx, mjs, cjs] }
```

```json
// resolve: input and output
{ "protocol": 1, "state": "project", "symbols": [{ "path": "src/api.ts", "symbol": "createUser" }] }
{ "results": [{ "status": "found", "line": 12 }] }
```

One call per plugin carries every symbol of a gate run, because each call starts a fresh instance that parses its files again. `public_api` returns strings that the host compares only as sets, so each language decides what counts as a change (hone's Rust plugin uses signatures). The package `hone/standard` ships `lang-rust`, `lang-typescript`, `lang-python`, `lang-php`, and `lang-dart`; a team approves each in `lock.yaml` like any plugin.

```json
// check: input (shortened)
{ "protocol": 1,
  "config": { "protocol": 1, "kinds": { … }, "steps": { … } },
  "ticket": { "id": "tk-9f2x4a8m", "type": "ticket", "profile": "full", "attributes": { "risk": "high" }, … },
  "step": "interfaces",
  "params": { "anchored": "module", "binding": "decision" },
  "artifacts": [ { "path": "design/decisions/dec-7k3m2q8p-payment-async.md", "status": "accepted", "hash": "sha256:3f1c…", "data": { "id": "dec-7k3m2q8p", "type": "decision", … }, "body": "## Context\n…" } ],
  "changed_paths": ["src/payments/gateway.rs"] }

// check: output
{ "findings": [ { "level": "error", "code": "public-api-unbound", "message": "The public API of src/payments/gateway.rs changed without a binding decision", "path": "src/payments/gateway.rs" } ] }
```

**Input of `check`** ([`spec/plugins.md`](../spec/plugins.md#input-of-check)). `artifacts` lists every artifact of the checked directory, not only the ticket's, because a binding decision can come from an earlier ticket. `data` and `body` are the same split as for the content hash.

**Output of `check`.** Only `findings`. There is no `ok` field, because it would follow from the findings and could contradict them. `describe` is not called by `check`; it serves tools that install a plugin and propose its capabilities.

**Runtime.** A plugin is a WASI preview 1 module (Rust: target `wasm32-wasip1`) that sees the checked directory and the base state read-only and nothing else. The sandbox cannot hide the clock and random numbers from WASI, so the rule that a plugin must not use them is a contract, not an enforcement: a gate is a function of its input and the files. A fresh instance per call means no state passes from one call to the next. A plugin's failure is a finding (`plugin-failed`), never "check could not run", so that one broken plugin does not hide the results of the other gates.

### Capabilities and trust

Capabilities are part of the configuration next to the plugin and are granted by the tool. A plugin cannot grant itself any.

```yaml
# plugins.public-api-has-decision in the resolved configuration
wasm: .hone/plugins/public-api-has-decision.wasm # relative to the project root
hash: sha256:9a1f... # SHA-256 of the module, same format as every other hash
capabilities: # all four are required
  read: [project] # `project` (the checked directory) or paths relative to the project root, read-only
  net: [] # allowed hosts, empty = none
  timeout_ms: 5000
  memory_pages: 256 # WASM pages of 64 KiB
```

Reasons for the rules ([`spec/plugins.md`](../spec/plugins.md#capabilities-and-trust)):

- **The default is nothing.** The tool must always set `net` explicitly. In Extism an empty list means "no host allowed", but a missing value (`null`) means "all hosts allowed", and this rule closes exactly that trap.
- **Gates are pure.** No network, no writing, no clock, no random numbers. The result is then a function of input and files and therefore repeatable.
- **Hash pinning.** Plugin hash and approved capabilities are recorded in `lock.yaml`. If either changes, a new approval is required. CI runs only plugins recorded there, with the lockfile of the protected base state.
- **Checked on every run.** `resolve` checking the same thing is not enough, because a change can bring `.wasm`, `lock.yaml`, `resolved.json`, and the snapshots along together.
- **Generators are the exception:** they may get network access, but run only on `resolve`, never in CI.

### What WASM cannot do

A plugin in the sandbox cannot start a compiler or a test runner. "Interfaces compile" and "tests exist and fail" need the toolchain of the target language. The host can do this, because `hone check` runs as a normal program with the permissions of the calling person. For this there is a second gate type that is data, not code:

```yaml
compiles: # a gate entry in a step
  gate: exec
  params:
    command: ["cargo", "check", "--workspace"] # required: argv list, no shell
    cwd: crates/core # optional, relative to the project root, default: the project root
    env: ["CARGO_HOME"] # optional: environment variables passed through, default: none
    timeout_ms: 600000 # optional, default: 600000 (10 minutes)
    evaluate: cargo-findings # optional: plugin that turns the result into findings
```

Without `evaluate`, exit code 0 passes. The rules are in [`spec/plugins.md`](../spec/plugins.md#exec-gates); the reasons:

- **Two classes.** `pure` gates (standard gates and WASM) are fast and deterministic and may run in every agent hook. `exec` gates run on completion of a step, before a push, and in CI, not on every edit. The caller chooses the class (`check --class pure`); the trigger remains the caller's business. There is no separate field for the class, because it follows from the gate.
- **An argv list, no shell, and approval.** CI takes the lockfile from the protected base state, never from the change itself; otherwise a pull request could inject arbitrary code into CI. What "protected" means (branch protection, mandatory review for `lock.yaml`) is defined by the platform (Section 12). Hash pinning is enough (Question 6). `exec` gates are allowed only where the team trusts them (locally, and in CI on protected states).
- **Only the named environment.** Whatever the toolchain needs (for Rust, for example, `PATH`, `HOME`, `CARGO_HOME`, `RUSTUP_HOME`) the language package lists, visible and pinned. The host looks up the program through its own `PATH`, so that `cargo` starts without a full path.
- **Plugins evaluate tool output.** The plugin stays a pure function of its input. Host functions with which a plugin starts commands itself are deliberately not provided, because they would break "gates are pure".
- **Language packages supply the commands** (`hone/lang-rust`, `hone/lang-php` through `extends`). Nobody writes them by hand.
- **CI does not trust local `exec` results** and runs them itself. The toolchain version is part of the result, because it influences it.

Parsing-based language functions (resolving symbols, reading the public API), in contrast, can be built as WASM plugins, for example with tree-sitter.

## 10. Approvals, comment threads, and condensation

The rules of this section are normative text in [`spec/review.md`](../spec/review.md) (Question 41); this section keeps the examples and the reasons.

Approvals and threads are **events in the process store**. They are not individual files in the working tree. On completion, whatever should remain is condensed.

### Approvals

An approval is the event `approval.given`:

```yaml
id: ev-0193a8f4-1d2e-7a5b-8c3d-4e5f6a7b8c9d
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
type: approval.given
author: { name: alice, role: architect }
payload:
  artifact: dec-7k3m2q8p
  hash: sha256:3f1c... # content that was approved (without the `review` block)
  decision: approved # approved | changes-requested
```

- The approval binds to the **content hash**. If the artifact changes afterwards, it becomes invalid.
- Who must give how many approvals is defined in the step (`approvals-valid` with role and count). The protocol checks that the author's role may approve according to `actors`.
- The protocol does not define a proof that this person really approved. A tool can store one as a free field (`proof`) in the event and in the `review` block (for example a signed approval or a confirmation from the server). Without a proof, an approval is trust-based. With `approvals-valid: { require_proof: true }`, a step requires a proof for every approval, checked by a plugin.
- `approvals-valid` reads the approvals from `review.approved_by`, with or without a process store, so that a local check and CI give the same result (Question 31). The events `approval.given` are the raw data from which the tool condenses the block. Every person counts once, however many entries they have, so that one person cannot fill a quorum alone. If the step has no output artifact in the ticket, the gate passes, because existence is the job of `artifact-exists`.

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

**Events.** A thread consists of the events `thread.opened`, `thread.message`, `thread.resolved`, and `thread.reopened`. The status (`open` or `resolved`) is computed. The thread ID is `th-` plus the UUID of the opening event (here `th-0193a8f0-9b2c-7e4d-a1f3-5c6d7e8f9a0b`). `thread.opened` does not repeat it, the other three events name it in `payload.thread`.

```yaml
id: ev-0193a8f0-9b2c-7e4d-a1f3-5c6d7e8f9a0b
project: prj-8m2k4d7q
ticket: tk-9f2x4a8m
type: thread.opened
author: { name: bob, role: human }
payload:
  kind: concern # a free key; hone/standard suggests question | concern | suggestion | note
  blocking: true
  anchor:
    kind: artifact
    artifact: dec-7k3m2q8p
    hash: sha256:3f1c...
    quote: "We process payments asynchronously"
  body: "What happens when the queue fails?"
```

**Rules** (normative in [`spec/review.md`](../spec/review.md)). Anyone may resolve or reopen a thread. Resolving without a message does not answer it, because a silent "done" documents no why. Authors carry a role, and whether it is an AI is stated in the role definition (`kind: ai`), so it shows what comes from an AI.

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

`hone check` exits with 0 when every gate passed, 1 when a gate failed, and 2 when it could not run. **Agent hook.** `hone hook claude-code` is a `PreToolUse` hook for Claude Code. It reads the hook input from stdin and treats the file an edit tool wants to write as the only changed path. It runs only the `change-needs-binding` gates of the ticket's `required` steps, because that is the purpose of the hook: no code while the decision is not binding. Everything else stays with `hone check` (pre-push, CI). Edits to artifact files are never blocked, otherwise a broken artifact would block the edit that repairs it. The ticket comes from `--ticket` or the environment variable `HONE_TICKET`. Without one, the hook lets the edit through and says so on stderr. A failed gate blocks the edit with exit code 2 and the findings on stderr.

**Ticket ↔ code.** "Which commits belonged to ticket X" is answered by the tool, for example through the ticket ID in branch names or commit trailers. The protocol only contains what is in the artifacts (`context`, `links.code`).

**Local process store.** hone keeps it under `$XDG_DATA_HOME/hone/<workspace>/<project>/` on Linux (default `~/.local/share/hone/…`), `~/Library/Application Support/hone/…` on macOS, and `%APPDATA%\hone\…` on Windows. The environment variable `HONE_DATA_DIR` replaces the base directory (tests, CI). The directory is created on the first write. Next to `log/` it holds `device`, the device ID of this machine in this project, created at random on the first write, and a lock file, so that two hone processes on one device never hand out the same `seq`. A segment is closed after 1000 events or 1 MiB, whichever comes first. hone does not read compressed segments yet. That comes with archiving (Section 16, step 10).

**Index.** For queries, the tool can keep a local index next to `log/`. It is a cache, built from the logs, and simply rebuilt when its schema changes. hone has none yet: every call reads the logs and orders them in memory, which takes milliseconds for thousands of events. An index (for example an embedded database) follows when queries need it, such as the user interface or large projects.

**Repository names.** Which location belongs to `repo:` in `repo:path#symbol` is bound locally by the device, as with contexts.

**Protected state in CI.** For CI to take `lock.yaml` from the base state, the platform needs a protected base branch and a mandatory review for `lock.yaml` (for example CODEOWNERS). With Git, CI passes it as `git show origin/main:.hone/lock.yaml > /tmp/lock.yaml && hone check --lock /tmp/lock.yaml …`.

**Plugins and `exec` in hone** (Question 32). `hone check` takes `--base <dir>` (the base state, for example a `git worktree` of the target branch) and `--lock <file>` (the lockfile instead of `.hone/lock.yaml`). The runtime is Extism, always built in, with no feature switch. hone approves an `exec` command in `lock.yaml` under `x-hone-exec`, a list of readable entries:

```yaml
# .hone/lock.yaml
x-hone-exec:
  - {
      command: [cargo, test],
      cwd: ".",
      env: [CARGO_HOME, HOME, PATH, RUSTUP_HOME],
    }
```

An `exec` gate runs only when `command`, `cwd` (default `.`), and `env` (default `[]`, compared as a set) equal one entry. The entry itself is the approval, and a reviewer reads the command in plain text. A hash next to it would say the same thing twice. hone caches no result of a plugin or a command, every call runs fresh, and it compiles each plugin once per call. Extism prints the output of a plugin only when the environment variable `EXTISM_ENABLE_WASI_OUTPUT` is set. That is meant for debugging and breaks `--json` and the MCP server. The sources of hone's own plugins live in `plugins/`, a Cargo workspace of their own for the target `wasm32-wasip1`. The built `.wasm` files are committed to the test cases, so that `cargo test` needs neither network nor a WASM toolchain.

**Backup without a server.** The tool offers a folder (including a synchronized one such as Nextcloud) as a backup target. Technically this is a "shared folder" peer (Section 4): since pull imports the own device too, a new machine that keeps the old `device` file gets its log back from the folder. Whatever a lost device never backed up is gone. The tool says so clearly during setup.

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

hone keeps the contexts in `contexts.yaml` in the platform's configuration directory (`$XDG_CONFIG_HOME/hone/` on Linux, `%APPDATA%\hone\` on Windows, `~/Library/Application Support/hone/` on macOS; `HONE_CONFIG_DIR` replaces the directory), a file in the strict profile (Question 39):

```yaml
contexts:
  acme:
    peer: { server: "https://hone.acme.example" } # or { folder: "/mnt/share" } or { git: "hone/process", remote: "origin" }
    author: { name: "alice", role: "architect" } # optional
    token_env: "HONE_TOKEN_ACME" # optional, default HONE_TOKEN: the variable that holds the token
projects:
  prj-8m2k4d7q: { context: "acme", root: "/home/alice/acme/payments" } # bound in this checkout
```

The file holds no secret: a context names the environment variable of its token, and the token stays in the environment or in the OS keyring (Question 44). `hone context set <name> --server <url> | --folder <dir> | --git <branch> [--remote <name>] [--author <handle> --role <role>] [--token-env <variable>]` creates or replaces a context, `hone context use <name> [--root <dir>]` binds the project of the root together with the root's canonical path, because a repository chooses its project ID itself and could otherwise claim the context of another tenant's project; in another checkout the binding does not apply and hone asks to bind that checkout, and `hone context show [--root <dir>] [--json]` prints the bound context. hone writes the file anew on every change, so comments in it do not survive. A flag or its environment variable wins over the context: `hone sync` without `--folder`, `--server`, or `--git` takes the peer of the bound context, and the writing commands without `--author` and `--role` take its author. Without a binding, they fail as before. `hone context login <name>` reads a server's token as one line from standard input and stores it in the OS keyring (Keychain on macOS, the Credential Manager on Windows, the Secret Service elsewhere) under the service `hone` and the account `<name> <server>`, so a token is never sent to another server after the context's server changes; `hone context logout <name>` removes it. A context takes the variable of `token_env` (default `HONE_TOKEN`) when it is set and not empty, otherwise the keyring's token; without a keyring there is no stored token. `hone mcp` follows the same precedence once at its start (Question 42): without `--author` and `--role` it takes the context's author, without `--sync-folder`, `--sync-server`, or `--sync-git` the context's peer, with the token from the context's `token_env`, so both stay fixed for the session. A context it cannot use (a malformed file, a binding made in another checkout, a missing token) stops the start with exit code 1 and the message on standard error; a project whose scope cannot be read has no context, and its tools report the broken project.

**Tenant separation.** The local store is separated per project. A confidential ticket gets its own project. There is no scope below the project (Question 20). An agent run receives context from exactly one project, never from two, otherwise one client's data flows into another client's prompts. At the end of an engagement there are `export` and `forget`, so that nothing is left behind (NDA, GDPR) (Question 43). `hone export --to <dir> [--root <dir>] [--json]` writes the complete lines of every device's log, byte for byte, into a new or empty directory in the layout of a shared folder (`<dir>/<workspace>/<project>/log/<device>/`), so that `hone sync --folder <dir>` imports the archive again. `hone forget --confirm <project> [--root <dir>] [--json]` removes the project's local store (every device's log, the device ID, the worktrees of `hone sync --git`) and its binding in `contexts.yaml`, and leaves the context itself. It takes the project ID once more, because nothing it removes comes back, and it reads `contexts.yaml` before removing anything, so a malformed file stops it. Copies in a shared folder or on a server stay: forgetting there is a matter of their operators. The MCP server offers neither, because an export writes outside the project and forgetting is a decision of a person. Optionally, the tool encrypts the local store per project.

**Creation.** `hone init --from <package>` writes `extends` into the new configuration. This brings a custom standard process into every project without needing a global layer.

**Process store in Git (fallback).** For small teams without a server, a dedicated branch (for example `hone/process`) works, which is never merged into code branches. It is a shared folder (Section 4) whose root is a worktree of that branch: the tool pulls, runs the sync, commits the own `log/<device>/`, and pushes. Since every device writes only its own log file, there are no merge conflicts. `hone sync --git <branch> [--remote <name>]` does this (Question 38): in the Git repository of the project root, it keeps a worktree of the branch under the store directory (`<store>/git/<branch>/`, with `/` in the branch name replaced by `-`), fetches the branch from the remote (default `origin`) and resets the worktree to it, or creates the branch as an orphan if the remote does not have it yet, runs the sync with the worktree as the shared folder, commits the own `log/<device>/` with the message `hone: log of <device>`, and pushes. A rejected push is retried up to three times after a rebase onto the fetched branch, which cannot conflict, because no other device writes the own files. Resetting the worktree loses nothing, because the own log is written again from the local store. Commits use the Git identity of the repository. A failing Git command ends the sync as `unavailable` with Git's message. The MCP server takes `--sync-git <branch> [--sync-remote <name>]` like a folder. It remains a fallback: no real time, write access to the repository required, the history grows, and whoever can read the repository sees everything. Git notes and branch descriptions are unsuitable: branch descriptions are local and not pushed, notes can only be shared with an explicit push and can overwrite each other with several writers.

**Command line and MCP.** The reference implementation is a Cargo workspace: the library `hone-core` and the crate `hone-cli`, which holds only the command line (binary `hone`, for example `hone resolve [--check] [--json]`). The GUI builds on the command line (see "Desktop GUI" below). Before v1, the crate `hone-mcp` adds an MCP server over stdio, so that agents call the same functions as people. It depends on `hone-core` like `hone-cli` does, and `hone-cli` only starts it as the subcommand `hone mcp`, without MCP logic of its own. Every result and every error of `hone-core` is serializable, and every command has `--json`, so the command line and the MCP server stay thin adapters over the library. Where packages come from is the tool's business: `hone-core` asks a package source for a name and a version.

**MCP server.** `hone mcp [--root <dir>] [--packages <dir>]` serves exactly one project, the same as `hone resolve` (default root `.`). The tools take no root, because an agent run receives context from exactly one project (tenant separation). The server uses the official Rust SDK `rmcp`. It offers one tool per public function of `hone-core`, named like the function without a prefix (the client adds the server name, for example `mcp__hone__check`):

| Tool              | Input                                                                     | Result                                                                                   |
| ----------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `resolve`         | `check` (optional, only compare, write nothing)                           | Like `hone resolve --json`: `status` (`written`, `up-to-date`, `out-of-date`) and `hash` |
| `check`           | `ticket`, `step`, `changed_paths`, `class`, `base`, `lock` (all optional) | Like `hone check --json`: `status` (`passed`, `failed`) and `gates`                      |
| `check_edit`      | `ticket`, `path` (relative to the root)                                   | The same as the agent hook, asked beforehand: `status` and `gates`                       |
| `check_artifact`  | `path`, `content` (both required)                                         | `status` and `findings`, `failed` exactly when a finding is an `error`                   |
| `content_hash`    | `path`, `content` (both required)                                         | `hash`                                                                                   |
| `catalog`         | –                                                                         | Like `hone artifacts --json`: `project`, `artifacts`, `unreadable`                       |
| `ticket_overview` | `ticket` (required)                                                       | Like `hone ticket show --json`: profile, attributes, roles, and steps in order           |

`check_artifact` and `content_hash` take the content as text, so that an agent checks a draft before it writes the file. Results map to MCP like the exit codes of the command line: a failed gate is a normal result with `status: failed`, because it is an answer and not an error. A tool that could not run (exit code 2, a failed resolution, content outside the YAML profile for `content_hash`) returns a result marked `isError` with `status` (`error`, `failed`, or `io-error`) and `message`, `problems` (resolution), or `violations` (`content_hash`). Invalid arguments, such as a missing `path` or `step` without `ticket`, are also a result marked `isError` and not a protocol error, so that the agent reads the reason and corrects the call (the behaviour of the MCP specification and of `rmcp`). Writing and comparing `resolved.json` and reading it for `check_artifact` live in `hone-core`, so that the command line and the MCP server cannot drift apart.

**Process commands** (Question 31). The command line writes the events that the gates read and shows the process state of a ticket:

| Command                                                                      | Event or result                                                                          |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `hone thread open --ticket <tk> --kind <key> [--blocking] <anchor> --body …` | `thread.opened`, prints the thread ID                                                    |
| `hone thread reply <th> --body …`                                            | `thread.message`                                                                         |
| `hone thread resolve <th>` / `hone thread reopen <th>`                       | `thread.resolved` / `thread.reopened`                                                    |
| `hone step set --ticket <tk> <step> open\|done\|skipped [--reason …]`        | `step.changed`                                                                           |
| `hone events --ticket <tk>`                                                  | The ticket's events in order, the computed thread and step state, and the invalid events |
| `hone log`                                                                   | Every valid event of the project's store in the order of `read`, and the store's cursor  |

The anchor is one of `--anchor-artifact <id> [--quote …]` (hone computes the content hash of the artifact's current file), `--anchor-code <path#symbol>`, `--anchor-change <reference>`, or `--anchor-transcript <url> [--position …]`. A reply, resolution, or reopening takes the ticket from the opening event, which must be present. `hone step set` accepts only steps of the ticket's profile, so that a typo does not end up in an immutable log. The author comes from `--author <handle>` and `--role <role>`, by default from `HONE_AUTHOR` and `HONE_ROLE`. The role must exist in the ticket's snapshot, the handle is free, because without a server authorship is trust-based (Section 4). hone fills `after` itself. The MCP server offers the same functions as the tools `thread_open`, `thread_reply`, `thread_resolve`, `thread_reopen`, `step_set`, and `events`, with the same JSON as `--json`. Its author is fixed at the start (`hone mcp --author critic-bot --role critic`) and cannot be chosen per call, so that an agent does not write as a human. Without an author, the writing tools return a result marked `isError`. Approvals and `review.confirmed` have their commands below. Tickets have their own commands (Question 45):

| Command                                                                                                  | Writes                                                                  |
| -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `hone ticket start [--attribute <key>=<value>]… [--external <system>:<key>[=<url>]]… [--profile <name>]` | The ticket record, the snapshot, `ticket.started`; prints the ticket ID |
| `hone ticket set-attributes --ticket <tk> [--attribute <key>=<value>]… [--unset <key>]…`                 | The record's `attributes`, `ticket.attributes_changed`                  |
| `hone ticket switch-config --ticket <tk>`                                                                | A new snapshot, the record's `config`, `ticket.config_changed`          |
| `hone ticket show --ticket <tk>`                                                                         | Nothing: the profile, its steps, and the snapshot's roles               |

A value of `--attribute` is read like a YAML scalar: digits are an integer, `true` and `false` a boolean, everything else a string, and a value in double quotes stays a string. The profile is the result of `select` for the attributes; `--profile` chooses another one, which `steps-complete` keeps visible as the warning `profile-not-selected`. A start refuses a `.hone/resolved.json` that does not match its sources, so that no ticket freezes rules that nobody sees in the sources. Every command checks everything before it writes a file, and needs an author like every writing command, because the events record who started or changed the ticket. `switch-config` keeps the profile and refuses a configuration that no longer has it. The MCP server offers `ticket_start` with its fixed author: a start freezes rules the team set, it decides nothing. Changing attributes or switching rules stays with people for now, because an agent that sets `risk: low` could avoid the gates of a stricter profile.

**Approvals and condensation** (Question 37). The command line writes approvals and condenses them into the artifact:

| Command                                                                   | Event or result                                              |
| ------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `hone approve --ticket <tk> <artifact> [--changes-requested] [--proof …]` | `approval.given` with the artifact's current content hash    |
| `hone review condense --ticket <tk> <artifact> [--summary <file>]`        | Writes the `review` block into the artifact, writes no event |
| `hone review confirm --ticket <tk> <artifact>`                            | `review.confirmed` with the artifact's current content hash  |
| `hone review show --ticket <tk> <artifact>`                               | The review state, writes nothing                             |

`hone approve` accepts only a role whose `can` holds `approve` and that `actors` gives the author (a person not listed gets the roles of `"*"`), the same rule a server enforces with `role-mismatch`, so that no approval lands in the log that no gate would count. `--proof` stores free text as `proof`. `hone review condense` reads the ticket's `approval.given` events for the artifact. For every person and role, the last one in the order of Section 4 counts. It enters `approved_by` when its decision is `approved` and its hash is the artifact's current content hash, with `name`, `role`, `hash`, `date` (the UTC date of the event ID, a UUIDv7), and `proof` if present, sorted by name and role. `--summary` names a YAML file in the strict profile with a list of entries `{ q, a }` or `{ rejected }`, which becomes `summary`. The AI drafts that file. Under `review_summary: approvals-only` a summary is refused, so that internal considerations do not reach a public file. Condensing replaces an existing `review` block and leaves every other byte of the file unchanged. It writes the block with double-quoted strings and flow maps, so the result stays in the strict profile, and it checks that the content hash did not change. `hone review confirm` checks `actors` in the same way and accepts the role from `decides` of a step of the ticket's profile whose `outputs` hold the artifact's kind, or, if no such step names `decides`, any role that may approve. `hone review show` prints the artifact's `path` and current content `hash`, `decisions` (for every person and role the last `approval.given` for the artifact in the ticket, the rule of condensing, with `decision`, `hash`, `current` when the hash is the current one, `date`, and `proof`), the `review.confirmed` events as `confirmations` with `current`, and the file's `review` block; a store that does not exist yet has no decisions. The MCP server offers no tool for these commands yet, because an approval is a decision of a person.

**Sync** (Questions 33 and 34). `hone sync --folder <dir> [--json]` syncs the project's store with a shared folder (Section 4). The folder comes from `--folder`, by default from the environment variable `HONE_SYNC_FOLDER`. `hone sync --server <url> [--watch] [--json]` syncs with a server: the address comes from `--server`, by default from `HONE_SYNC_SERVER`, the bearer token only from the environment variable `HONE_TOKEN`, because a flag would show in the process list and the shell history. `--folder` and `--server` exclude each other. Until contexts exist (Section 16, step 10), hone stores no binding between a project and a peer. The result names, per device, how many events were received from the folder and how many were sent to it, and the problems with code, path, and line. The exit code is 0 when the sync finished without a problem, 1 when it finished and reported problems (an answer, as a failed gate is), and 2 when it could not run (a missing folder, an unreadable store). Import and own appends hold the same lock file, so that a sync never races a write of the same device. The MCP server offers the tool `sync` without arguments, with the folder fixed at the start (`hone mcp --sync-folder <dir>`), so that an agent cannot sync into an arbitrary path. Without a folder, the tool returns a result marked `isError`.

**Sync with a server.** hone first reads the server's cursor and appends the own events after it, in batches of at most 1000 events and 4 MiB, then reads with its own cursor and imports under the rules of a pull from a folder. Only the own device's events are appended (Section 4). An event larger than the server's limit stops the push with `too-large`. The client waits at most 10 seconds for a connection and 60 seconds for a request. A problem names the local file and line of the event concerned: the rejected event, or the event held at a forked place. Where there is no local event, for example an invalid line from the server, it names the request URL and the line of the answer. A call the server refuses as a whole (`invalid-request`, `unauthenticated`, `forbidden`, `not-found`, `too-large`, or an unknown status) is the error `refused` with the code and the message, and an unreachable server is `unavailable`. Both end with exit code 2. With `--watch`, hone stays connected after the first sync: it subscribes from its cursor and imports every event as it arrives, checks the own log every 2 seconds and appends new own events, and after a dropped connection subscribes again with its current cursor, waiting 1 second and doubling up to 60 seconds. It ends with exit code 0 on Ctrl-C and with exit code 2 on a refused call, and prints one result per batch received or sent (with `--json`, one JSON object per line). The MCP server takes the server instead of a folder at the start (`hone mcp --sync-server <url>`, the token from `HONE_TOKEN`). Folder and server exclude each other, and the tool `sync` offers no `--watch`.

**Reference server.** The crate `hone-server` (axum on tokio) holds the HTTP layer, the authentication, and the configuration, while the checks of an append live in `hone-core`, like every rule of the protocol. The client in `hone-core` uses reqwest on a thread of its own, because its API is blocking and is also called from inside the MCP server's runtime. `hone serve --config <file>` starts the server with a YAML file (strict profile), read once at the start:

```yaml
listen: 127.0.0.1:8470
data: /var/lib/hone # one store per project: <data>/<workspace>/<project>/log/...
limits: { request_bytes: 16777216, heartbeat_seconds: 15 } # optional, these are the defaults
identities:
  alice: { token: "sha256:9f86d0..." } # only the SHA-256 of the token, so a leaked file leaks no token
  bob: { token: "sha256:60303a..." }
projects:
  prj-8m2k4d7q:
    workspace: ws-4c7d9e2f
    members: [alice] # who may read and append
    actors: { alice: [human, architect], "*": [human] } # for role-mismatch, like actors in Section 8
```

The data directory has the layout of a shared folder (Section 4), so `hone sync --folder <data>` backs a server up or restores it. The server reads a project's log fresh for every request (there is no index yet, Section 16, step 10) and holds the store's lock file while it appends, like a local store. A project without `actors` rejects every approval. Tokens are static, OAuth 2.0 and OpenID Connect come later. On shutdown, the server ends open subscriptions, so that it does not wait for them.

**Lists for a user interface.** `hone artifacts [--root <dir>] [--json]` lists every artifact of the checked directory under the current resolved configuration, ticket records included, sorted by path: `id`, `type` (the kind, or `ticket`, taken from the path), `path`, `ticket`, `title` (the field `title`, otherwise the first `# ` heading of a Markdown body), the declared `status`, the `effective` status (Section 6), `links` (its ID links by type: `supersedes`, `affects`, `verifies`, `depends-on`, `relates`), and for a ticket record its `profile` and `external`. It also names the `project` and the files that match a pattern but cannot be read (`unreadable`). `hone ticket show --ticket <tk> [--root <dir>] [--json]` prints a ticket record with its snapshot: `profile`, `config`, `attributes`, `external`, the snapshot's `roles`, and the profile's `steps` with `requirement` (`required` or `optional`), `title`, and `after`, the steps of the profile that come before it (those it `requires` and those whose outputs are its inputs, Section 7). The steps are listed so that every step follows the steps in its `after`; steps that no order separates keep the order of their names, and a cycle of inputs and outputs puts its steps at the end. Both exit with 1 and `{ "status": "error", "message" }` when the configuration or the ticket cannot be read. They write nothing and read no process store. The MCP server offers neither yet, like `hone log`.

**Desktop GUI** (Question 36). The crate `hone-gui` is a desktop application for Linux and Windows (Dioxus with the system web view). It calls the command line `hone … --json` as a child process and never links `hone-core`, so the JSON of the command line is its only interface to hone, and a change to that JSON is a breaking change for the GUI. It finds `hone` through the environment variable `HONE_BIN`, otherwise next to its own executable, otherwise on the `PATH`. It works on one project root, given as its first argument (default: the working directory) and changeable in the window. It writes with the author and role chosen in the window (`--author`, `--role`), or, with both empty, as the author of the project's context, and it passes its environment on, so `HONE_DATA_DIR`, `HONE_CONFIG_DIR`, `HONE_TOKEN`, `HONE_SYNC_FOLDER`, and `HONE_SYNC_SERVER` work as on the command line. It shows what the command line offers: the tickets (the ticket records from `hone artifacts` and the tickets of the store from `hone log`), a ticket with the steps of its profile, its artifacts, and its threads (`hone ticket show`, `hone events`, `hone step set`, `hone thread …`), the artifacts with their effective status, an artifact with its links in both directions and its review (`hone review show`, `hone approve [--changes-requested] [--proof …]`, `hone review condense`, `hone review confirm`), the link graph (`hone artifacts`), the event log, a check run (`hone check`), a sync (`hone sync`), and a reference server it starts and stops (`hone serve`). Screens that need commands the command line does not have yet (profiles, plugins) follow with those commands. A search in the top bar (Ctrl+K or Cmd+K) finds tickets, artifacts, threads by their first line, and, with a tracker, the tracker's tickets. Where the set of values is known, a field is a searchable list instead of free text: tickets, the steps of the ticket's profile, thread kinds (the four that hone/standard suggests and the kinds in the log, or a new one), artifacts for an anchor, the author (the context's author and the authors of the log, or a new handle), and the role (the roles of the open ticket's snapshot). Its components are taken from the Dioxus registry rust-ui and keep its Tailwind classes, while hone's design tokens replace the theme values behind them; the accent is blue (`#6b8cff` dark, `#1d4ed8` light). It lives in a Cargo workspace of its own under `gui/`, so that the checks of the protocol crates do not need the system libraries of a web view.

**Progress of a ticket in the GUI.** The steps stand side by side in the order of `hone ticket show`, each done, skipped with its reason, open, or waiting for a step in its `after` that is neither done nor skipped (Section 7: a step is ready when its inputs are done or skipped; nothing stops the user from finishing it first). The first open step that waits for nothing is the current one. Per step it shows the gates of `hone check --ticket <tk> --class pure`, which the GUI runs when the ticket opens and after every write it makes, so that no `exec` gate runs a command unasked; a gate whose only error is `changed-paths-missing` is shown as needing changed paths, because the GUI knows no change. The full check runs on demand. An error, a failed gate, a refused write, and an invalid field are shown in `bad`, never in the accent, and the focus ring of an invalid field stays `bad`.

**Ticket tracker in the GUI.** Per project and checkout, the user names where the project's tickets live: a GitHub repository (suggested from the Git remote `origin`) or a Jira project (base URL, project key, and for Jira Cloud the account email). The GUI keeps this in `gui.json` in hone's configuration directory, next to `contexts.yaml`, keyed by project ID and canonical root like a context binding, and never in the project. It lists and searches the tracker's tickets, opens the hone ticket whose record names one under `external`, copies the `external` entry for one that no record names, and shows the tracker's title on a linked ticket. The token comes from an environment variable (default `GITHUB_TOKEN` or `JIRA_API_TOKEN`, or the setting's `token_env`) when it is set and not empty, otherwise from the OS keyring (service `hone`, account `tracker <API address>`), never from a file. Starting a ticket from a tracker ticket waits for the start of a ticket (Section 16, step 10).

**Rejected events.** A server rejects an event and every later one of its device in the same call (Section 4). The event stays in the local log and the device cannot sync past it. Typical cause: an approval written offline after the role was withdrawn. To keep that rare, a client checks the server's rules before it writes. Recovery is the tool's business, for example a new device ID under which the later events are written anew. The same holds for a computer handed to another person under the same account: the device stays with the first person, and the new person needs a new device ID.

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

The specification includes JSON Schemas for all formats (artifacts, resolved configuration, events, plugin inputs and outputs) and a **conformance test suite** with which every implementation proves its level. Resolution comes with a collection of test cases (layers in, resolved document out), because two tools must resolve identically.

The test suites and the format of every test case are normative text in [`spec/conformance.md`](../spec/conformance.md) (Question 41).

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
11. **Log format:** Decided. Segments per device, deterministic order (`after` and `seq`, then `id`, see Question 31), no compaction, archiving at segment level. Lost devices are not an error. The event schema is Question 31 (Section 4).
12. **Condensation:** Decided. The AI drafts, the role from `decides` confirms through the event `review.confirmed`. Trust comes from the diff review, optionally `require_proof` enforces a proof. The hash additionally normalizes NFC and trailing whitespace, but not line breaks (Sections 5 and 10).
13. **Role of reports:** Decided. JSON output of `check` and `report`, the format will be standardized later. Not an event in the process store (Section 12).
14. **Visibility:** Decided. Profile setting `review_summary: full | approvals-only` (Section 10).
15. **Governance:** Decided. For now the project founder decides, changes go through a public RFC. No trademark protection, conformance through the test suite (Section 14).
16. **Granularity of modules:** Decided. Only explicitly defined modules. The optional gate `module-anchors-resolve` reports moved anchors (Section 5).
17. **Open-core boundary:** Decided: there is none. Everything is open source.
18. **Agent transcripts:** Decided. Only linked: link type `transcript` and thread anchor `transcript` (Sections 5 and 10).
19. **Standardize the sync interface?** Decided: yes. The replication contract is part of the protocol (conformance level Extended).
20. **Confidential tickets:** Decided. The scope stays at project level, a confidential ticket gets its own project (Section 12).
21. **Author identity:** Decided. No identity across servers, every context has its own. With a server, the server enforces the identity: it rejects an event with a wrong author and never rewrites one (Question 33). Without one, it is trust-based. The field `sig` is reserved for signatures in a later version (Section 4).
22. **Backup and device loss without a server:** Decided. The tool offers a folder as a backup target. Whatever was never backed up is lost (Sections 4 and 12).
23. **Modules as living definitions:** Decided. Modules are a preset with lifecycle `living`, freely editable. A deliberate cut is an approved decision with `affects`. `context.observed` takes a module ID or an anchor (Section 5).
24. **Terms in the schema:** Decided. The artifact field `scope` is now called `reach`. `verifies` takes only artifact IDs, test code is linked through `code` (Section 5).
25. **Licenses and contributions:** Decided. Code under `MIT OR Apache-2.0`, specification text under CC0 1.0, contributions through DCO (Section 14).
26. **Scope of the server contract:** Decided. Normative are replication, real-time subscription, bearer authentication, and defined error codes. Permission management, administration, and notifications are left to the server (Section 4).
27. **Statuses:** Decided. Statuses are configurable per kind (`kinds.<type>.statuses`, a map). The protocol knows only the property `binding`. Every kind with lifecycle `decided` has at least one binding status, `superseded` is reserved. Content enums without protocol semantics (`reach`, thread kinds) are not part of the core (Sections 5 and 6).
28. **Resolution:** Decided. Fixed layer layout (one file per definition, the file stem is the key), merging with `null-without-target` and replacement on type conflicts, `extends` as `name@major` in depth-first order with the first occurrence counting, `lock.yaml` with package and plugin hashes, kind schemas as layer-relative paths embedded after merging, written-out defaults, ticket overrides as a last patch, the canonical form RFC 8785, and fixed error categories (Section 8). The test cases live in `schemas/tests/resolve/` (Section 14).
29. **Check:** Decided. Directory gates run against `resolved.json` when a step contains them, ticket gates against the snapshot, either the named step or every `required` step. Artifacts are found through the path patterns, and path and content must agree. A superseding artifact counts by its declared status. Findings carry fixed codes. Gates that need the process store fail until the event schema exists, `exec` and plugin gates fail as `unsupported` until step 8. Approvals count once per person, with the role checked against `actors`. `change-needs-binding` needs `changed_paths` and matches anchors by path segments and simple globs. The agent hook runs only `change-needs-binding` and never blocks edits to artifacts (Sections 5, 6, 7, 10, 12, and 14).
30. **MCP server:** Decided. One project per server (`hone mcp --root --packages`), the SDK `rmcp`, the tools `resolve`, `check`, `check_edit`, `check_artifact`, and `content_hash` without a prefix, with the same JSON as `--json`. A failed gate and an out-of-date `resolved.json` are normal results, "could not run" and invalid arguments are `isError`, never a protocol error. `check_artifact` and `content_hash` take path and content. Writing and comparing `resolved.json` moves into `hone-core` (Section 12).
31. **Event schema and local process store:** Decided. Scope IDs and the device ID are a prefix plus 8 characters of Crockford Base32, the device ID is created per project, so that a device cannot be linked across tenants. An event has no `date`: the UUIDv7 `id` carries the time of writing, and the order is topological by `after` and `seq`, then by `id`. `after` expresses causality only (the newest known event of every other device), references live in `payload`. The envelope is strict (only `x-` fields extra, `sig` reserved and absent), known types have fixed payloads (Section 4), unknown types are kept and ignored. Lines are RFC 8785 JSON, an interrupted last line of the open segment is ignored, invalid events are not read and are reported. Segments are numbered from 1, the highest is open. `step.changed` replaces `step.skipped`, the thread ID is derived from the opening event, a thread is answered by a message of another author, and `steps-complete` ignores the steps that contain it. The three process-store gates check for real, report invalid events as warnings, and fail with `process-store-unavailable` without a store. `approvals-valid` stays on the `review` block. hone stores locally in the XDG data directory (`HONE_DATA_DIR` overrides it), has no index yet, keeps the code in `hone-core`, and offers process commands for threads, steps, and events on the command line and over MCP, with the MCP author fixed at the start (Sections 4, 5, 7, 10, and 12).

32. **Plugin contract and `exec` gates:** Decided. Plugins are WASI preview 1 modules without host functions. They read the checked directory and the optional base state read-only under `/project` and `/base`, get no network, environment, or arguments, and must not use the clock or random numbers. `check` receives the configuration, the ticket record, the step, the parameters, every artifact (path, effective status, content hash, data, body), and optionally `changed_paths` and `tool_output`. It returns only `findings` with codes of its own choosing. `describe` is not called by `check`. A plugin that cannot run or returns an invalid output gets the finding `plugin-failed`. Before every run, `check` compares the `.wasm` file, the configuration, and a lockfile, which the caller may replace (`plugin-hash`, `plugin-capabilities`). Plugins are not frozen per ticket: after an update a ticket switches to the new configuration. `exec` gates come with step 8: only approved commands run (`exec-unapproved`), only with the named environment, with a default timeout of 10 minutes and outputs cut at 1 MiB. `exec-failed` covers start failures, timeouts, signals, and, without `evaluate`, an exit code other than 0. With `evaluate`, the plugin decides alone through `tool_output: { exit, stdout, stderr, truncated }`. hone approves commands as readable entries under `x-hone-exec` in `lock.yaml`, offers `--base` and `--lock` on the command line and over MCP, caches nothing, and builds Extism in always. Its plugins live in `plugins/` and are committed built (Sections 7, 8, 9, 12, and 14).

33. **Replication contract and shared folder:** Decided. A cursor is a JSON object from device to the highest contiguous `seq`, a missing device counts as 0. `read(cursor)` returns only valid events, per device in ascending `seq`, and the receiver orders the set itself. `append` stores in order and stops at the first event it cannot store, identical duplicates are accepted. Every store cuts its own segments, only `seq` is shared. A different event at the same device and `seq` is a `fork`, the same `id` with other content an `id-conflict`; the receiver keeps its copy, imports nothing later of that device, and reports it. A shared folder has the normative layout `<workspace>/<project>/log/…`; pull imports every device, the own one included, as a contiguous prefix; push writes only the own log, appends only to a byte prefix, and never overwrites. Stray files and directories are ignored, paths leaving the folder root are `unsafe-path`, a missing folder root is an error. The server enforces the author instead of rewriting it, speaks HTTP with NDJSON and Server-Sent Events, answers with fixed error codes (`unauthenticated`, `forbidden`, `scope-mismatch`, `identity-mismatch`, `role-mismatch`, `invalid-event`, `seq-gap`, `fork`, `id-conflict`), and learns the actors by its own means. A Git branch is a shared folder in a worktree. hone offers `hone sync --folder` and the MCP tool `sync` with the folder fixed at the start. The test cases live in `schemas/tests/read/` and `schemas/tests/sync/` (Sections 4, 12, and 14).

34. **Reference server:** Decided. The transport gains `invalid-request` (400), `not-found` (404, a project the server does not serve), and `too-large` (413), checked after the token. The body of `append` is `application/x-ndjson`, its last line may lack `\n`, and an empty line is an invalid event. A cursor in the query is strict: device IDs, `seq` without leading zeros, every device once. An event is checked for the reading rules, the scope, the identity, the role, the held event at its place, and the continuation, in this order, so the identity comes before duplicates, and a device belongs to the author of its first event on the server. A subscription continues every device without a gap or a repetition and carries only `data:` lines and heartbeat comments. hone keeps the checks of an append in `hone-core`, the HTTP client (reqwest on a thread of its own) too, and the server in the crate `hone-server` (axum), started by `hone serve --config <file>`: a YAML file with the listen address, the data directory in the layout of a shared folder, limits, identities with hashed static tokens, and per project the workspace, the members, and the actors. `hone sync --server <url>` takes the token from `HONE_TOKEN`, pushes in batches, then pulls, names problems by the local event, and reports a refused call as the error `refused`. `--watch` stays subscribed, pushes new own events every 2 seconds, and reconnects with a growing wait. The MCP server takes `--sync-server`. The append test cases live in `schemas/tests/append/`, and the read test cases also run against the server (Sections 4, 12, and 14).

35. **Unreadable entries in a shared folder:** Decided without review (listed in `docs/review/autonomous-decisions.md`). A file or directory of a shared folder that hone may not read, or a segment or directory it may not write during push, is skipped and reported as `permission-denied`, a code of its own, because `unsafe-path` means a path that leaves the folder. The sync goes on for every other device (Section 4).

36. **Desktop GUI:** Decided without review (listed in `docs/review/autonomous-decisions.md`). The GUI calls `hone … --json` as a child process and never links `hone-core`, as the maintainer asked, so the JSON of the command line becomes a public interface. For the ticket list and the event log, the command line gains `hone log`: every valid event of the store in the order of `read`, and the cursor. The GUI lives in its own workspace under `gui/` and uses the Dioxus components of rust-ui with hone's design tokens (Section 12).

37. **Approvals and condensation in the tool:** Decided without review (listed in `docs/review/autonomous-decisions.md`). `hone approve` writes `approval.given` with the current content hash and accepts only a role that may approve and that `actors` gives the author. `hone review condense` writes `approved_by` from the last decision of every person and role whose hash matches, with the UTC date of the event ID, plus an optional summary from a YAML file, refused under `approvals-only`. `hone review confirm` writes `review.confirmed` with the current content hash, for the role from `decides` or, without one, any role that may approve (Section 12).

38. **Git branch as a shared folder:** Decided without review (listed in `docs/review/autonomous-decisions.md`). `hone sync --git <branch> [--remote <name>]` keeps a worktree of the branch under the store directory, resets it to the remote branch or creates an orphan branch, syncs with the worktree as the folder, commits the own log, and pushes with up to three rebase retries. Git failures are `unavailable` (Section 12).

39. **Contexts:** Decided without review (listed in `docs/review/autonomous-decisions.md`). `contexts.yaml` in the platform's configuration directory holds per context a peer, an optional author, and the name of the token's environment variable, and binds project IDs to contexts. It holds no secret. `hone context set|use|show` edit and show it. Flags and their environment variables win; without them, `hone sync` takes the context's peer and the writing commands its author (Section 12).

40. **`module-anchors-resolve`:** Decided without review (listed in `docs/review/autonomous-decisions.md`). A plugin in `plugins/` that checks the anchors and `interface.code` of the artifacts of one kind against the checked state, resolving Rust symbols with `syn`. Errors `anchor-missing`, `symbol-missing`, `unparsable`; warnings `symbol-unchecked` and `other-repository` (Section 5).

41. **Specification layout:** Decided without review (listed in `docs/review/autonomous-decisions.md`). `spec/` is written chapter by chapter, one file per area, with BCP 14 key words. A written chapter is normative and wins over the concept, whose section then points to it; areas without a chapter stay specified by the concept. The first chapters are the conventions and levels, and conformance with every test case format, moved out of Section 14 (Section 16, step 11).

42. **Contexts in the MCP server:** Confirmed by the maintainer (listed in `docs/review/autonomous-decisions.md`). `hone mcp` reads the bound context once at its start and fills only what its flags and their environment variables leave out: the author, and the peer of the tool `sync` with the token from `token_env`. A context it cannot use stops the start with exit code 1. `--sync-remote` names the remote of `--sync-git` (Section 12, "Contexts").

43. **`export` and `forget`:** Confirmed by the maintainer (listed in `docs/review/autonomous-decisions.md`). `hone export --to <dir>` writes every device's log in the layout of a shared folder into a new or empty directory, from which a sync imports it again. `hone forget --confirm <project>` removes the local store and the project's context binding; copies in a shared folder or on a server stay. Neither is an MCP tool (Section 12, "Tenant separation").

44. **Tokens in the OS keyring:** Confirmed by the maintainer (listed in `docs/review/autonomous-decisions.md`). `hone context login <name>` stores a server's token, read from standard input, in the OS keyring under the context and the server; `hone context logout <name>` removes it. The token variable still wins when it is set. `contexts.yaml` gains no field (Section 12, "Contexts").

45. **The commands of a ticket:** Decided with the maintainer. `hone ticket start` writes the record, the snapshot, and `ticket.started` with an author; the profile follows `select`, `--profile` overrides it visibly (`profile-not-selected`); attribute values read like YAML scalars; external tickets as `--external <system>:<key>[=<url>]`; a stale `resolved.json` is refused. `set-attributes` writes the record and `ticket.attributes_changed` and keeps the profile; `switch-config` writes a new snapshot, the record's `config`, and `ticket.config_changed`. The MCP server offers only `ticket_start` (Section 12, "Process commands").

46. **Language plugins:** Decided with the maintainer. `module-anchors-resolve` and `public-api-has-decision` become standard gates that call language plugins; a plugin names its extensions in its plugin file (`language: { extensions }`), and two plugins for one extension are a resolution error. `resolve` and `public_api` take every symbol or file of a gate in one call and return one result per entry; public APIs are strings compared as sets. A file without a plugin is a warning (`symbol-unchecked`, `api-unchecked`); a failing plugin is `plugin-failed` while the results of the others stay. `hone/standard` ships plugins for Rust, TypeScript and JavaScript, Python, PHP, and Dart (Section 9, "Language plugins").

## 16. Next steps

1. Done: format (Question 1) and the artifact schema with tests (`schemas/`), adapted to all decisions (ID format, `reach`, lifecycles instead of fixed types, presets under `schemas/standard/`, module with `interface`, `context.observed`, hash normalization).
2. Done: README, license files (`LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE-CC0`), and `CONTRIBUTING.md` with DCO.
3. Done: the schema of the resolved configuration (`schemas/config.schema.json`) with valid, invalid, and reference test cases, configurable statuses (Question 27), and the Level 1 example as a test case.
4. Done: build `resolve` in Rust: layers, merging, test cases (`schemas/tests/resolve/`), the workspace `crates/hone-core` and `crates/hone-cli` with `hone resolve [--check] [--json]`. This is the core of Level 2 and pins down Level 1. The configuration fixtures and the reference check move from `schemas/tests/run.py` to `cargo test`.
5. Done: build `check` with the standard gates (including `ids-unique` and `change-needs-binding`) and connect it as an agent hook (`hone hook claude-code`). With that, the tool can be used alone right away, without a team and without Git integration. The artifact fixtures and the content hash test moved to `cargo test`, and `schemas/tests/run.py` is deleted. The gates that need the process store follow with step 7, `exec` and plugin gates with step 8.
6. Done: the MCP server (stdio) as the crate `hone-mcp`, a thin adapter over `hone-core`, started by `hone mcp`, with the tools `resolve`, `check`, `check_edit`, `check_artifact`, and `content_hash` (Question 30).
7. Done: scope IDs and the event schema (`schemas/event.schema.json`, Question 31) with valid and invalid fixtures, the local process store in `hone-core` (segments per device, deterministic order, no index yet), the commands `hone thread`, `hone step`, and `hone events` and the matching MCP tools, and the gates `no-open-blocking-threads`, `threads-answered`, and `steps-complete` against the store.
8. Done: the plugin contract (Question 32) with Extism as the runtime and `schemas/plugin.schema.json`, the gate `public-api-has-decision` in Rust under `plugins/`, capabilities and hash pinning checked on every run against a lockfile the caller may replace, `exec` gates with approved commands and `evaluate`, and `--base` and `--lock` on the command line and over MCP.
9. Done: the replication contract in the decided scope (Question 26, Question 33, Question 34, Question 35: unreadable entries of a shared folder are skipped and reported) with the test cases `schemas/tests/read/`, `schemas/tests/sync/`, and `schemas/tests/append/`, the shared folder as the first reference backend (`hone sync --folder`), the reference server as the crate `hone-server` (`hone serve`: HTTP, NDJSON, Server-Sent Events, bearer authentication, every error code of Section 4), and the client (`hone sync --server [--watch]`, the MCP tool `sync` with `--sync-folder` or `--sync-server`).
10. In progress: the desktop GUI `hone-gui` over the command line and `hone log` (Question 36), approvals and condensation as commands (Question 37), the Git branch as a shared folder (Question 38), contexts (Question 39) also in the MCP server (Question 42), the Rust plugin `module-anchors-resolve` (Question 40), `export` and `forget` (Question 43), and tokens in the OS keyring (Question 44) are done. In progress: the commands of a ticket (Question 45) and language plugins (Question 46). Open: delivery.
11. Done: the normative specification in `spec/` (Question 41). Written: the conventions and levels (`spec/README.md`), conformance with every test case format (`spec/conformance.md`), the process store (`spec/process-store.md`), replication (`spec/replication.md`), and review (`spec/review.md`), artifacts with lifecycle and status (`spec/artifacts.md`), steps, gates, and profiles (`spec/gates.md`), the configuration (`spec/configuration.md`), and plugins with `exec` gates (`spec/plugins.md`). Every area of the protocol now has a chapter.

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
