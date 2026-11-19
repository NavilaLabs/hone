# Process store

Level: Flow. Schema: `schemas/event.schema.json`. Test cases: `schemas/tests/events/` ([conformance](conformance.md)).

## Logs and segments

1. The process state of a project is an append-only event log per device. Only that device appends to its log. Merging means reading all logs and taking the union.
2. A device's log consists of segments `log/<device>/<number>.ndjson`. The number is decimal without leading zeros and starts at 1. The segment with the highest number is open, every lower one is closed.
3. When a segment is closed is up to the implementation. A closed segment MUST NOT change. It MAY be compressed (`.ndjson.zst`). Events MUST NOT be rewritten, and there is no compaction. Archiving and dropping operate on whole segments of completed tickets.
4. Every line of a segment is one event as JSON in the canonical form of RFC 8785, ending with `\n`.
5. A last line without `\n` in the open segment is an interrupted write: a reader MUST ignore it, and the writer MUST cut it off before it appends again. In a closed segment, such a line is an error.
6. Below `log/`, only directories named by a device ID and, in them, files named `<number>.ndjson` count. A reader MUST ignore everything else.
7. The local store is a directory per workspace and project holding `log/`. Without a server, the workspace is `local`. All clones and worktrees of a project share it. If the directory does not exist, the store is unavailable; if it exists without events, the store is empty.

## Events

8. The envelope fields `id`, `project`, `ticket`, `device`, `seq`, `type`, `author` (`name`, a handle, and `role`, a role key), and `payload` are REQUIRED, `after` is OPTIONAL. Further fields exist only as `x-<name>`.
9. `id` is a UUIDv7 (RFC 9562) in lowercase, prefixed `ev-`. Its timestamp is the time of writing and the only time of an event.
10. `seq` starts at 1 in each device's log and grows by exactly 1. A gap is allowed only where whole segments are missing.
11. `after` lists, for every other device, the newest event the writer had read when it wrote. It expresses causality only.
12. The field `sig` is reserved. An event with `sig` is invalid in version 1.
13. The types of version 1 and their payloads:

| Type                        | Payload                                                                    |
| --------------------------- | -------------------------------------------------------------------------- |
| `thread.opened`             | `kind`, `blocking`, `anchor`, `body`                                       |
| `thread.message`            | `thread`, `body`                                                           |
| `thread.resolved`           | `thread`                                                                   |
| `thread.reopened`           | `thread`                                                                   |
| `approval.given`            | `artifact`, `hash`, `decision` (`approved`, `changes-requested`), `proof?` |
| `review.confirmed`          | `artifact`, `hash`                                                         |
| `step.changed`              | `step`, `status` (`open`, `done`, `skipped`), `reason?`                    |
| `ticket.started`            | `profile`, `config`                                                        |
| `ticket.attributes_changed` | `attributes`                                                               |
| `ticket.config_changed`     | `config`                                                                   |

14. A known type has the payload schema of `schemas/event.schema.json`. A custom type is named `x-<name>` and has a free payload. An event of a type that is neither known nor `x-…` MUST be kept and ordered, and MUST be ignored by every computed status.

## Reading

15. An event is invalid if it violates the schema, lies in the log of another device, belongs to another project, breaks the rules of `seq`, or has the `id` of another event with different content. A reader MUST NOT read an invalid event and MUST report it.
16. The same event twice (same `id`, same content) counts once.

## Order

17. The order of the events is deterministic: topological by `after` and, within one device, by `seq`. Ties are broken by `id`.
18. A reference in `after` to an event that is not present does not count. Events in a cycle are ordered by `id`.

## Cursor

19. The cursor of a state is a JSON object from device ID to `seq`. A missing device counts as 0.
20. The cursor names, for every device, the highest `seq` up to which its valid events are contiguous, counted from its first valid event. A valid event behind a hole does not raise it.
