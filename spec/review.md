# Approvals, threads, and condensation

Level: Flow. Events: [process store](process-store.md). Gates: `approvals-valid`, `no-open-blocking-threads`, `threads-answered` ([gates](gates.md)).

## Approvals

1. An approval is the event `approval.given` with the payload `artifact`, `hash` (the content hash that was approved, without the `review` block), `decision` (`approved` or `changes-requested`), and an OPTIONAL `proof`.
2. An approval binds to the content hash. If the artifact's content changes afterwards, the approval no longer counts.
3. `proof` is free. Without a proof, an approval is trust-based. The standard gate checks only that the field exists; a plugin checks its content.
4. `approvals-valid` MUST read the approvals from the artifact's `review.approved_by`, with or without a process store, so that a local check and CI give the same result.
5. An entry of `approved_by` counts when its `role` is the gate's role, its `hash` is the artifact's current content hash, and `actors` gives the person that role; a person not listed in `actors` gets the roles of `"*"`. Every person counts once. With `require_proof: true`, an entry counts only if it has a field `proof`.
6. If the step has no output artifact in the ticket, `approvals-valid` passes.

## Threads

7. A thread consists of the events `thread.opened`, `thread.message`, `thread.resolved`, and `thread.reopened`. The thread ID is `th-` followed by the UUID of the opening event. `thread.opened` does not repeat it; the other three events name it in `payload.thread`.
8. The anchor of `thread.opened` is one of:

| `kind`       | Fields                                                    |
| ------------ | --------------------------------------------------------- |
| `artifact`   | `artifact`, `hash` (at the time of the comment), `quote?` |
| `code`       | `ref`, a reference `path#symbol`                          |
| `change`     | `ref`, a reference to an external change                  |
| `transcript` | `url`, `position?`                                        |

9. **Status.** A thread is open after `thread.opened`. The last of its `thread.resolved` and `thread.reopened` in the order of the process store decides whether it is resolved or open again. A message does not change the status. Events of a thread whose opening event is not present count for no status.
10. A thread whose anchored content changed (the hash no longer matches) is outdated. It stays visible and MUST still be closed explicitly.
11. A thread with `blocking: true` makes `no-open-blocking-threads` fail while it is open.
12. **Answered.** A thread is answered when it has at least one `thread.message` whose author `name` differs from the author of `thread.opened`. `threads-answered` fails for every thread of the ticket, opened by its role, that is not answered.
13. Approvals and threads are independent: an approval counts while its hash matches, and an open blocking thread blocks regardless.

## Condensation

14. Before an artifact is completed, the tool writes a `review` block into it: `approved_by`, a list of `{ name, role, hash, date, proof? }`, and OPTIONAL `summary`, a list of `{ q, a }` or `{ rejected }`. The confirmation of the deciding role is the event `review.confirmed` with `artifact` and `hash`.
15. The `review` block is outside the content hash.
16. The profile's `review_summary` decides what goes into the file: `full` (approvals and summary, the default) or `approvals-only` (only approvals).
17. The profile's `retention` decides what happens to the raw events of a completed ticket: `keep`, `archive` (the default; segments are kept but not loaded into an index), or `drop`.
