# hone specification

Status: draft, built chapter by chapter from the protocol concept (`docs/protocol-concept.md`). Nothing is stable before version 1.

The concept explains why; this specification states what an implementation must do. Where a chapter exists here, it is normative and the concept section points to it; where the two disagree, this specification wins and the disagreement is a bug in the concept. Areas without a chapter yet are specified by the concept.

## Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this specification are to be interpreted as described in BCP 14 ([RFC 2119](https://www.rfc-editor.org/rfc/rfc2119), [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174)) when, and only when, they appear in all capitals.

Every rule of the protocol can be checked deterministically, without AI. Rules that need Git, a tracker, a server, or an agent belong to a tool and are not part of this specification.

Examples are informative. The JSON Schemas under `schemas/` and the test cases under `schemas/tests/` are normative: a schema states the shape of a format, a test case states the result an implementation must produce.

## Conformance levels

An implementation claims one level and proves it with the test cases of that level ([conformance](conformance.md)).

1. **Core:** the artifact format, IDs, kinds with lifecycle and anchors, links, the effective status, reading the resolved configuration, and the standard gates.
2. **Flow:** Core, plus scope IDs, steps, profiles, selection rules, skipping, roles and actors, the event log per device, approvals, threads, and condensation.
3. **Extended:** Flow, plus resolution from layers and packages, frozen snapshots, WebAssembly plugins (gates, generators, language plugins), and the replication contract.

## Chapters

| Chapter                           | Content                                                          | State             |
| --------------------------------- | ---------------------------------------------------------------- | ----------------- |
| [Conformance](conformance.md)     | Levels, the test suites, and the format of every test case       | Written           |
| [Artifacts](artifacts.md)         | Format, YAML profile, content hash, IDs, links, lifecycle        | Written           |
| Configuration                     | The resolved configuration, layers, packages, snapshots          | Concept Section 8 |
| [Gates](gates.md)                 | Steps, profiles, the standard gates and their codes              | Written           |
| Plugins                           | The plugin contract, capabilities, and trust                     | Concept Section 9 |
| [Process store](process-store.md) | Logs, segments, events, the reading rules, the order, the cursor | Written           |
| [Replication](replication.md)     | `read`, `append`, `sync`, the shared folder, the HTTP transport  | Written           |
| [Review](review.md)               | Approvals, threads, and condensation                             | Written           |

## License

This specification is dedicated to the public domain under CC0 1.0 Universal ([LICENSE-CC0](../LICENSE-CC0)).
