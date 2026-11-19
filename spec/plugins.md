# Plugins and `exec` gates

Level: Extended ([README](README.md#conformance-levels)). Schema: `schemas/plugin.schema.json`; plugins in the configuration: `schemas/config.schema.json`. Test cases: the plugin and `exec` cases of `schemas/tests/check/` ([conformance](conformance.md#gates)). Gates and codes: [gates](gates.md). The lockfile and the capability check of `resolve`: [configuration](configuration.md#plugins-in-the-configuration).

Only the contract of this chapter is normative. The reference runtime (Extism) is not part of the specification.

## Contract

1. A plugin is a WebAssembly module for WASI preview 1 that exports named entry points. Input and output are UTF-8 JSON in the shapes of `schemas/plugin.schema.json`.
2. The entry points of the protocol:

| Entry point        | Kind            | Input                                                                                            | Output                                                               |
| ------------------ | --------------- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| `describe()`       | all             | –                                                                                                | Name, version, protocol version, entry points, required capabilities |
| `check(input)`     | gate            | configuration, ticket, step, parameters, artifacts, optionally `changed_paths` and `tool_output` | `{ findings }`                                                       |
| `generate(input)`  | generator       | layer, parameters, resolved configuration so far                                                 | patch                                                                |
| `resolve(anchor)`  | language plugin | anchor (`path#symbol`)                                                                           | location or error                                                    |
| `public_api(path)` | language plugin | path                                                                                             | list of public symbols                                               |

3. Custom entry points are named `x-<name>`.

## Input of `check`

4. The input has `protocol` (`1`), `config`, `ticket`, `step`, `params`, and `artifacts`, OPTIONAL `changed_paths`, and, for a plugin named by `evaluate`, `tool_output`. It contains no `null`.
5. `config` is the configuration the gate runs against (the ticket's snapshot). `ticket` is the data of the ticket record. `step` is the name of the step. `params` is the `params` of the gate entry, `{}` when it has none; for a plugin named by `evaluate`, the `params` of the `exec` gate.
6. `artifacts` lists every artifact of the checked directory, not only the ticket's, including ticket records. Each entry has `path` (relative to the project root), `status` (the effective status, absent when the artifact has no declared status), `hash` (computed by the rule of the content hash for every entry, also for lifecycle `living` and the ticket record), `data` (the data model of the file), and `body` (the Markdown body, `""` for a YAML file). `id` and `type` live only in `data`. A file that violates the YAML profile is absent.
7. `changed_paths` is absent when the caller supplied none.

## Output

8. The output of `check` is `{ findings }`, a list of findings in the shape of [gates](gates.md#gates): `level`, `code`, OPTIONAL `path` and `pointer`, and `message`. There is no other field. A plugin chooses its own codes, each a key (lowercase with hyphens). The host sorts the findings.
9. The output of `describe` is `{ name, version, protocol, entry_points, capabilities }`, with `capabilities` in the shape of the capabilities below. `check` does not call `describe`. A plugin recognizes the protocol version from `protocol` in its input.

## Runtime

10. The host gives a plugin the checked directory read-only under `/project` and, if the caller supplied a base state, that state read-only under `/base`. `read: [project]` mounts the whole directory; a path such as `read: [src]` mounts only `/project/src` and `/base/src`. A path that does not exist in a state is not mounted there.
11. The host gives a plugin no network, no environment variables, no arguments, and no standard input. Standard output and standard error are discarded.
12. `memory_pages` (pages of 64 KiB) is the memory limit and `timeout_ms` the time limit of one call.
13. A plugin MUST NOT use the clock or random numbers. The host MUST call every gate with a fresh instance.
14. If the module cannot be loaded, lacks the entry point, traps, exceeds a limit, returns an error, or returns an output outside the schema, the gate gets the finding `plugin-failed`. The other gates run on.

## Capabilities and trust

15. A plugin in the resolved configuration has `wasm` (the path relative to the project root), `hash` (the SHA-256 of the module), and `capabilities` with `read` (`project` or paths relative to the project root), `net` (allowed hosts), `timeout_ms`, and `memory_pages`, all REQUIRED. A plugin cannot grant itself capabilities.
16. The resolved configuration contains only the plugins that `check` runs, gates and language plugins. Their `net` MUST be `[]`. Generators are listed in `.hone/config.yaml` and are not part of the resolved configuration.
17. Before a plugin runs, `check` MUST compare the SHA-256 of the `.wasm` file with `hash` in the configuration the gate runs against, and that `hash` and the capabilities with the entry in a lockfile. The lockfile is `.hone/lock.yaml`, unless the caller supplies another one.
18. A missing file, a missing lock entry, or a differing hash is the finding `plugin-hash`. Capabilities beyond the approved ones are the finding `plugin-capabilities`. In both cases the plugin does not run.

## `exec` gates

19. An `exec` gate has the parameters `command` (REQUIRED, an argv list, no shell), `cwd` (OPTIONAL, relative to the project root, default the project root), `env` (OPTIONAL, the names of environment variables passed through, default none), `timeout_ms` (OPTIONAL, default 600000), and `evaluate` (OPTIONAL, a plugin that turns the result into findings).
20. `exec` gates belong to the class `exec`, every other gate to the class `pure`. The caller MAY choose the class of a call.
21. The command, the working directory, and the allowed environment variables MUST be approved by the tool; how the tool pins them is not specified. An `exec` gate whose command is not approved does not run and gets the finding `exec-unapproved`.
22. The command receives exactly the variables in `env` that the caller has set, and no others. A program name without a path separator is looked up through the host's own `PATH`.
23. Standard input is empty. Standard output and standard error are kept up to 1 MiB each, from the start; bytes that are not UTF-8 are replaced.
24. A program that cannot start, a timeout (the host ends the process), or an end through a signal is the finding `exec-failed`, and a plugin named by `evaluate` is not called.
25. Without `evaluate`, exit code 0 passes and any other exit code is `exec-failed`.
26. With `evaluate`, the host passes the result as `tool_output: { exit, stdout, stderr, truncated }` to the plugin's `check`, where `truncated` is `true` when one of the outputs was cut. The plugin's findings alone decide. The plugin goes through the same pinning and the same failures as a plugin gate.
27. The host provides no host functions with which a plugin starts commands.
