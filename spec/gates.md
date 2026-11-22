# Steps, gates, and profiles

Level: Core for findings, the directory gates, and the gates over artifacts; Flow for steps, profiles, and the gates that read the process store ([README](README.md#conformance-levels)). Schema: `schemas/config.schema.json` (`steps`, `profiles`). Test cases: `schemas/tests/check/` ([conformance](conformance.md#gates)). Artifacts and their status: [artifacts](artifacts.md). Plugin gates and `exec`: [plugins](plugins.md).

## Steps

1. A step is an entry of `steps` in the resolved configuration; its key is its name. It has OPTIONAL `title`, `acts` (the roles that work), OPTIONAL `decides` (the role that decides and confirms the condensation), `inputs` and `outputs` (kinds, where `ticket` stands for the ticket record), `requires` (further steps that come first), and `gates`.
2. A step names roles, never people, and contains no instructions for a specific agent.
3. `gates` is a map from a label to an entry `{ gate, params }`. The same gate MAY appear under several labels. `gate` names a standard gate, `exec`, or a key of `plugins`. A plugin MUST NOT carry the name of a standard gate or `exec`.
4. The order of the steps follows from inputs and outputs: a step is ready when its inputs are done or skipped. `requires` adds a fixed order and MUST NOT form a cycle. No gate checks readiness.

## Profiles

5. A profile has `steps`, a map from a step to `required` or `optional`, plus `retention` and `review_summary` ([review](review.md#condensation)).
6. A `required` step MUST be done or explicitly skipped; otherwise `steps-complete` fails. A step that the profile does not list is not part of the ticket's flow.
7. A profile sets only steps and their strictness. There is no strictness per gate.
8. The selection rules choose the profile for a ticket ([configuration](configuration.md#roles-actors-and-selection)). A human MAY change the result per ticket; `steps-complete` then reports the warning `profile-not-selected`, so the deviation stays visible.

## Gates

9. A gate is a deterministic function over a checked directory. Its input is the checked state, an OPTIONAL base state, and an OPTIONAL list `changed_paths`, the last two supplied by the caller. Its output is a list of findings.
10. A finding has `level` (`error` or `warning`), `code`, OPTIONAL `path` (the file relative to the project root), OPTIONAL `pointer` (a JSON Pointer into the file's data model), and `message` (free text, not normative). A gate fails exactly when it has a finding of level `error`.
11. The codes of the standard gates are fixed by this chapter. A plugin gate additionally reports the codes its plugin chooses.
12. Every implementation MUST provide the standard gates of its conformance level natively. A gate the implementation does not provide fails with `unsupported`.

## The standard gates

13. The standard gates and their parameters:

| Gate                       | Parameters                                            | Checks                                                                                                                                                                                    |
| -------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `artifact-exists`          | `type`, `status?`                                     | The ticket has at least one artifact of kind `type`, with `status` one whose effective status is in the set                                                                               |
| `status-is`                | `type`, `status`                                      | Every artifact of kind `type` in the ticket has an effective status in the set. Passes if there is none                                                                                   |
| `schema-valid`             | –                                                     | Every artifact matches the schema of its lifecycle and kind, its prefix, and the statuses of its kind ([artifacts](artifacts.md#schema))                                                  |
| `ids-unique`               | –                                                     | No two files carry the same ID                                                                                                                                                            |
| `links-resolve`            | –                                                     | All ID links point to existing artifacts                                                                                                                                                  |
| `no-open-blocking-threads` | –                                                     | No blocking thread of the ticket is open ([review](review.md#threads))                                                                                                                    |
| `threads-answered`         | `role`                                                | Every thread of the ticket opened by role `role` is answered ([review](review.md#threads))                                                                                                |
| `approvals-valid`          | `role`, `min?` (1), `require_proof?` (false), `type?` | Every artifact of the step's `outputs` with lifecycle `decided` (only kind `type`, if given) has at least `min` valid approvals of role `role` ([review](review.md#approvals))            |
| `steps-complete`           | –                                                     | Every required step of the ticket's profile is done or skipped, except the steps that contain `steps-complete` themselves                                                                 |
| `change-needs-binding`     | `anchored`, `binding`                                 | If `changed_paths` hits the anchors of an artifact of kind `anchored`, a binding artifact of kind `binding` names it in `affects`                                                         |
| `module-anchors-resolve`   | `kind?` (`module`)                                    | Every anchor and `interface.code` of the artifacts of kind `kind` resolves in the checked state, symbols through the language plugin of the file ([plugins](plugins.md#language-plugins)) |
| `public-api-has-decision`  | `anchored`, `binding`                                 | If the public API of a changed file under the anchors of an artifact of kind `anchored` differs from the base state, a binding artifact of kind `binding` names it in `affects`           |

14. `status` is one status or a list of them (set membership). It MUST be a status of the kind or `superseded`. Unknown parameters are an error.
15. An artifact belongs to the ticket when its field `ticket` holds the ticket ID. The kind of `artifact-exists` and `status-is` and the kind `binding` of `change-needs-binding` MUST have lifecycle `decided`. The role of `approvals-valid` MUST be able to `approve`.
16. `artifact-exists` and `status-is` use the effective status, computed over the whole checked directory.
17. `links-resolve` checks the ID links (`supersedes`, `affects`, `verifies`, `depends-on`, `relates`), the field `ticket` (the ticket record exists), and `context.observed[].module` of the ticket record. It does not check `code`, `transcript`, `x-*` links, or fields of a kind's own schema.
18. `change-needs-binding` looks at every artifact of kind `anchored` in the checked directory whose anchors hit a changed path. For each, an artifact of kind `binding` anywhere in the checked directory, from any ticket, MUST list it in `links.affects` and have a binding effective status.
19. A `path` anchor hits the same path and everything below it, compared by whole path segments. A `glob` anchor knows `*` (any characters within one segment), `**` (any number of whole segments), and `?` (one character within a segment), and nothing else. A `symbol` anchor hits its file. An anchor with `repo:` hits only changed paths with the same `repo:` prefix; a path without a prefix belongs to the repository of the checked directory.
20. `module-anchors-resolve` and `public-api-has-decision` belong to level Extended, because they call language plugins. A `path` anchor of `module-anchors-resolve` resolves when the file or directory exists, a `glob` anchor when at least one file matches, a `symbol` anchor `path#symbol` when the file exists and the language plugin of the file returns `found`. Without a language plugin for the file, a symbol whose file exists is the warning `symbol-unchecked`. An anchor `repository:path` is the warning `other-repository`. Every finding names the artifact's path and the pointer of the anchor (`/anchors/<i>`, `/interface/code`).
21. `public-api-has-decision` takes every changed path that an anchor of an artifact of kind `anchored` hits (item 19). For a file with a language plugin, it compares the `public_api` of the base state and the checked state as [plugins](plugins.md#language-plugins) item 34 defines; a file missing in a state has an empty public API. If they differ, an artifact of kind `binding` with a binding effective status MUST list the anchored artifact in `links.affects`. The message of `public-api-unbound` names the added, removed, and changed symbols. A hit file without a language plugin is the warning `api-unchecked`. The kind `binding` MUST have lifecycle `decided`.
22. The state of a step is the `status` of its last `step.changed` in the order of the process store, and `open` without one. A `step.changed` for a step that the profile does not list is kept and ignored.

## The process store in gates

23. `no-open-blocking-threads`, `threads-answered`, and `steps-complete` read the events of the ticket from the process store. Without a process store, they fail with `process-store-unavailable`.
24. An invalid event is not read and is reported as the warning `event-invalid`.

## What `check` runs

25. `schema-valid`, `ids-unique`, and `links-resolve` are directory gates. Each runs at most once per call, if at least one step of the current resolved configuration (`.hone/resolved.json`) contains it, and checks the whole checked directory against that configuration.
26. Every other gate is a ticket gate. Ticket gates run only when the caller names a ticket, against the ticket's snapshot: the gates of the step the caller names, or otherwise of every step that the ticket's profile lists as `required`. A directory gate inside a ticket step does not run a second time.
27. The ticket record is found through `ticket.path`, its snapshot through `config`. If `resolved.json` or the snapshot is missing, fails the checks of the resolved configuration, or does not match its hash, `check` does not run. That is not a finding.
28. A file that violates the YAML profile is reported by `schema-valid` and is invisible to every other gate.

## Codes

29. The codes of the standard gates, `exec`, and plugin gates:

| Gate                                                                    | Code                        | Level   | Meaning                                                                                     |
| ----------------------------------------------------------------------- | --------------------------- | ------- | ------------------------------------------------------------------------------------------- |
| `schema-valid`                                                          | `yaml-profile`              | error   | The file violates the strict YAML profile or the file shape                                 |
| `schema-valid`                                                          | `kind-unknown`              | error   | `type` names neither `ticket` nor a kind of the configuration                               |
| `schema-valid`                                                          | `path-mismatch`             | error   | `id`, `ticket`, or `type` disagrees with the path                                           |
| `schema-valid`                                                          | `id-prefix`                 | error   | `id` does not start with the `id_prefix` of the kind                                        |
| `schema-valid`                                                          | `status`                    | error   | The declared status is not a status of the kind                                             |
| `schema-valid`                                                          | `schema`                    | error   | The data does not match the schema of its lifecycle and kind                                |
| `schema-valid`                                                          | `unknown-field`             | warning | A key without `x-` that no schema names                                                     |
| `schema-valid`                                                          | `unmatched-file`            | warning | A file looks like an artifact but matches no pattern                                        |
| `ids-unique`                                                            | `id-duplicate`              | error   | Another file carries the same ID. Every file involved gets a finding                        |
| `links-resolve`                                                         | `link-unresolved`           | error   | A link points to an artifact or a ticket record that does not exist                         |
| `artifact-exists`                                                       | `artifact-missing`          | error   | The ticket has no matching artifact                                                         |
| `status-is`                                                             | `status-mismatch`           | error   | An artifact of the ticket has an effective status outside the set                           |
| `approvals-valid`                                                       | `approvals-missing`         | error   | An artifact has fewer valid approvals than `min`                                            |
| `change-needs-binding`                                                  | `binding-missing`           | error   | An anchored artifact hit by `changed_paths` has no binding artifact through it              |
| `change-needs-binding`                                                  | `changed-paths-missing`     | error   | The caller supplied no `changed_paths`. An empty list is allowed and passes                 |
| `no-open-blocking-threads`                                              | `thread-blocking-open`      | error   | A blocking thread of the ticket is open                                                     |
| `threads-answered`                                                      | `thread-unanswered`         | error   | A thread of the ticket opened by the role is not answered                                   |
| `steps-complete`                                                        | `step-incomplete`           | error   | A required step is neither done nor skipped                                                 |
| `steps-complete`                                                        | `profile-not-selected`      | warning | The ticket's profile is not the result of `select` for its attributes                       |
| `module-anchors-resolve`                                                | `anchor-missing`            | error   | No file, directory, or match for an anchor                                                  |
| `module-anchors-resolve`                                                | `symbol-missing`            | error   | The file exists, the language plugin does not find the symbol                               |
| `module-anchors-resolve`                                                | `symbol-unchecked`          | warning | No language plugin handles the file of a symbol anchor                                      |
| `module-anchors-resolve`                                                | `other-repository`          | warning | The anchor names another repository, which the checked state cannot see                     |
| `public-api-has-decision`                                               | `public-api-unbound`        | error   | The public API of a hit file changed and no binding artifact names the anchored one         |
| `public-api-has-decision`                                               | `api-unchecked`             | warning | No language plugin handles a hit file                                                       |
| `public-api-has-decision`                                               | `changed-paths-missing`     | error   | The caller supplied no `changed_paths`                                                      |
| `public-api-has-decision`                                               | `base-missing`              | error   | The caller supplied no base state                                                           |
| the two above                                                           | `unparsable`                | error   | The language plugin cannot parse a file of the checked state                                |
| the three above                                                         | `process-store-unavailable` | error   | The gate needs the process store, which this call does not have                             |
| the three above                                                         | `event-invalid`             | warning | An event of the ticket (or one whose ticket cannot be read) is invalid                      |
| plugin gates, `exec` with `evaluate`                                    | `plugin-hash`               | error   | The `.wasm` file, its lock entry, or the hash disagree                                      |
| plugin gates, `exec` with `evaluate`                                    | `plugin-capabilities`       | error   | The capabilities exceed the approved ones in the lockfile                                   |
| plugin gates, `exec` with `evaluate`, the two gates of language plugins | `plugin-failed`             | error   | The plugin could not run or returned an invalid output                                      |
| `exec`                                                                  | `exec-unapproved`           | error   | The command, working directory, or environment is not approved                              |
| `exec`                                                                  | `exec-failed`               | error   | The command did not start, timed out, ended through a signal, or failed                     |
| any                                                                     | `unsupported`               | error   | The implementation does not provide this gate, for example one beyond its conformance level |
