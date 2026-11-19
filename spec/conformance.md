# Conformance

An implementation proves its level ([README](README.md#conformance-levels)) with the test cases under `schemas/tests/`. Conformance is proven by the test suite, not by a name.

## Test suites

| Suite                                     | Level    | Checks                                                       |
| ----------------------------------------- | -------- | ------------------------------------------------------------ |
| `valid/`, `invalid/`, `profile/`, `hash/` | Core     | The YAML profile, the artifact schemas, the content hash     |
| `check/`                                  | Core     | Gates over a checked directory (Flow and Extended gates too) |
| `config/`                                 | Core     | The shape and the references of the resolved configuration   |
| `events/`                                 | Flow     | Event fixtures and the reading rules of the process store    |
| `resolve/`                                | Extended | Resolution from layers and packages                          |
| `read/`, `sync/`, `append/`               | Extended | The replication contract                                     |

A gate test case belongs to the level of the gates it uses: a case with a plugin gate is Extended, a case with a process-store gate is Flow.

## Artifact files

The YAML profile, the schemas, and the content hash have test cases of their own, because two parsers MUST produce the same data and the same hash. They are single files under `schemas/tests/`, checked by their content without a place in a directory, with the kinds of `schemas/standard/kinds/` and `schemas/tests/kinds/`.

- `valid/`: every file MUST pass `schema-valid` without an error.
- `invalid/`: every file MUST pass the profile and MUST fail the schema checks. `invalid/expected.txt` has one line per file: the file name, the code, and the JSON Pointer.
- `profile/invalid/`: every file MUST violate the profile. `profile/invalid/expected.txt` has one line per file: the file name and the code `yaml-profile`.
- `hash/`: content hashes. `valid/decision-accepted.md` carries the hash of its content in its `review` block, and `hash/decision-reformatted.md` is the same content in other YAML notation; both MUST yield that hash.

## Resolution

A resolution test case is a directory `schemas/tests/resolve/<case>/`:

- `packages/<name>/`: the packages the case provides, by package name (`packages/acme/base/` for `acme/base`).
- `project/.hone/`: the project layer, including `config.yaml` and `lock.yaml`.
- `overrides.yaml` (OPTIONAL): the ticket overrides.
- Either `expected.json`, the resolved document, compared as data, or `expected-error.txt`, one line with the error category, a space, and the location: a JSON Pointer, or a file path relative to the case directory (`project/.hone/…` or `packages/<name>/…`).

A failing case contains exactly one error. An implementation MAY report several, but the expected one MUST be among them. Every `expected.json` passes the shape and reference checks of the resolved configuration.

## Gates

A gate test case is a directory `schemas/tests/check/<case>/`:

- `project/`: the checked directory, including `.hone/resolved.json`, the snapshots, and the plugins (`.wasm` and `lock.yaml`) the case needs.
- `base/` (OPTIONAL): the base state of the call.
- `lock.yaml` (OPTIONAL): the lockfile the caller supplies instead of `project/.hone/lock.yaml`, as CI does with the lockfile of the base state.
- `data/` (OPTIONAL): the base directory of the process store, with the store of the project's scope IDs.
- `input.yaml` (OPTIONAL): `ticket`, `step`, and `changed_paths` of the call, each OPTIONAL.
- `expected.txt`: one line per finding, `<level> <gate> <code> <path> <pointer>`, sorted. `<gate>` is the gate name of a directory gate or `<step>/<label>` of a ticket gate. `<path>` and `<pointer>` are `-` when the finding has none. An empty file means that every gate passed without a finding.

A failing case contains exactly one finding of level `error`, except for `ids-unique`, which reports every file involved. An implementation MUST report every line of `expected.txt`, and MUST report a finding of level `error` exactly when the file contains one. Beyond that, it MAY report more findings.

## Replication

The replication contract has three kinds of test cases. They compare what is read, not bytes, because every store cuts its segments itself.

A read test case is a directory `schemas/tests/read/<case>/`:

- `store/`: a process store of the project `prj-8m2k4d7q` (`log/<device>/<number>.ndjson`).
- `cursor.json`: the cursor of the call.
- `expected.ndjson`: the returned events, one canonical line each, sorted by device and then by `seq`. An empty file means that nothing is returned.

A sync test case is a directory `schemas/tests/sync/<case>/`:

- `local/`: the local store before the sync, with `device` when the device has written. A case without `local/` starts without a local store.
- `folder/`: the shared-folder root before the sync, `<workspace>/<project>/log/…`.
- `cursor.json`: the cursors after one sync, `{"local": …, "folder": …}`, each the cursor of the store as read afterwards.
- `expected.txt`: one line per reported problem, `<code> <path> <line>`, sorted. `<path>` is relative to the case directory (`folder/…` or `local/…`), `<line>` is `-` for a whole file. An empty file means a sync without problems.

An append test case checks what a server stores and answers. It is a directory `schemas/tests/append/<case>/`:

- `store/`: the server's store of the project `prj-8m2k4d7q` before the call. A case without `store/` starts with an empty store.
- `request.json`: `{"identity": …, "actors": …}`, the authenticated identity and the actors the server knows for the project.
- `events.ndjson`: the body of the request.
- `expected.json`: `{"status": …, "code"?: …, "seq"?: …, "event"?: …, "cursor": …}`, the HTTP status and the answer without `message`. `cursor` is the server's cursor afterwards, also after a rejection, so it shows what was stored.

The read test cases also run against a server, through `GET …/events`. The codes of a whole call (`invalid-request`, `unauthenticated`, `forbidden`, `not-found`, `too-large`) and the subscription concern the transport, not a store; each implementation tests them itself.

Permission errors in a shared folder (`permission-denied`) depend on the file system and cannot be expressed as files in a repository; implementations test them themselves.
