# Replication

Level: Extended. Test cases: `schemas/tests/read/`, `schemas/tests/sync/`, `schemas/tests/append/` ([conformance](conformance.md)). Terms (event, cursor, reading rules) are those of the [process store](process-store.md).

## Contract

1. Per project, a peer offers `append(events)`, `read(cursor) → events`, and `sync(peer)`. Events MUST NOT be changed, and duplicate IDs are ignored.
2. `read(cursor)` returns every valid event with a `seq` greater than the cursor's value for its device, up to that device's own cursor. The events of one device come in ascending `seq`; the devices MAY come in any order. Invalid events MUST NOT be returned.
3. `append(events)` takes events in order. An event already present with the same `id` and the same bytes is accepted and changes nothing. An event is stored only when it continues its device's log (`seq` is the cursor plus 1). The first event that cannot be stored ends the call: everything before it is stored, nothing after it.
4. `sync(peer)` is `read` and `append` in both directions, each side with the other's cursor.
5. Only `seq` is shared. Every store cuts the segments of every log with its own thresholds. A device's own log in a shared folder is written byte for byte.
6. An incoming event is compared with the event the receiver holds at the same device and `seq`. The same bytes are a duplicate and are ignored. The same `id` with other content, or an `id` the receiver holds at another device or `seq`, is an **`id-conflict`**. Another `id` at the same device and `seq` is a **`fork`**. In every conflict, the receiver MUST keep its copy, MUST NOT store the incoming event or any later one of that device from this peer, and MUST report it.

## Shared folder

7. Below the folder root, a shared folder holds process stores in the layout of the local store: `<workspace>/<project>/log/<device>/<number>.ndjson`. The reading rules apply unchanged.
8. **Pull.** For every device, including the own one, the receiver imports the valid events after its cursor while their `seq` is contiguous. It stops at the first gap, the first invalid line, or the first conflict, and reports it.
9. **Push.** A device MUST write only its own log into the folder. For every segment of its own log, the folder copy MUST be a byte prefix of the local segment: the missing rest is appended, a missing file is created. If the folder copy is not a prefix, that is a `fork`, and the file MUST stay untouched. Push MUST NOT overwrite, rename, or delete anything in the folder, and MUST push only complete lines.
10. **Untrusted content.** The folder is untrusted. Only regular files are opened, never a FIFO or a device. Every path is resolved to its canonical form and MUST stay below the folder root; otherwise it is neither read nor written and is reported as **`unsafe-path`**. A file or directory that may not be read or written is skipped and reported as **`permission-denied`**, and the sync goes on for every other device. A line that breaks the reading rules is reported as **`invalid-event`**.
11. **Missing folder.** If the folder root does not exist, sync MUST fail and MUST create nothing. The project directory below the root is created on the first push. A missing local store is created; no device ID is created for a device that has written nothing.

## Server transport

12. HTTP with JSON. The project is part of the path, and every request carries `Authorization: Bearer <token>`.

| Request                                                     | Meaning                                                                                                                                              |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /v1/projects/{project}/cursor`                         | The server's cursor as JSON                                                                                                                          |
| `POST /v1/projects/{project}/events`                        | `append`: the body is NDJSON, one event per line in its canonical form. Answer `200` with `{"cursor": …}`, the server's cursor after storing         |
| `GET /v1/projects/{project}/events?cursor=<device>:<seq>,…` | `read`: the answer is NDJSON (`application/x-ndjson`). An empty `cursor` means the empty cursor                                                      |
| The same request with `Accept: text/event-stream`           | Subscription (Server-Sent Events): first what `read` returns, then every new event, one event per `data:` line, as long as the connection stays open |

13. **Body of `append`.** `Content-Type: application/x-ndjson`. The last line MAY lack its `\n`. An empty line is an event that breaks the reading rules. An empty body stores nothing and is answered with the cursor.
14. **Cursor in the query.** Every entry is a device ID, a colon, and a `seq` in decimal without leading zeros; every device appears at most once; `0` means the same as a missing device. The cursor as JSON follows the same rules.
15. **Checks of a call,** in this order: the token (`unauthenticated`), the request (`invalid-request`), the project (`not-found`), the access (`forbidden`).
16. **Checks of an event,** in this order: the reading rules (`invalid-event`), the project of the path (`scope-mismatch`), the authenticated identity and the identity the device belongs to (`identity-mismatch`), the role of an approval according to `actors` (`role-mismatch`), the event held at the same device and `seq` (identical is accepted, otherwise `fork`; the same `id` elsewhere is `id-conflict`), and last the continuation of the log (`seq-gap`). A device belongs to the author of the first of its events the server holds. A server MUST NOT rewrite an event.
17. **Subscription.** Per device, the stream continues in ascending `seq` without a gap or a repetition, across the part `read` returns and the live part. It carries only `data:` lines, one event each, and comment lines as a heartbeat, no `id:` or `event:` fields. A server MAY close a subscriber that falls behind. An error before the stream starts is answered like any other.
18. **Errors** are answered with the status of the table and the body `{"code": …, "message": …, "seq"?: …, "event"?: …, "cursor"?: …}`. `seq` and `event` name the rejected event, `cursor` is the server's cursor after the events stored before it.

| Code                | Status | Meaning                                                                                                                                                     |
| ------------------- | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `invalid-request`   | 400    | The request is malformed: a cursor against the rules above, a body that is not UTF-8, a wrong media type or method, a project path that is not a project ID |
| `unauthenticated`   | 401    | The bearer token is missing or invalid                                                                                                                      |
| `forbidden`         | 403    | The identity has no access to the project                                                                                                                   |
| `not-found`         | 404    | The server does not serve the project, or the path names no request of the table                                                                            |
| `too-large`         | 413    | The body exceeds the server's limit                                                                                                                         |
| `scope-mismatch`    | 422    | The event's `project` is not the project of the path                                                                                                        |
| `identity-mismatch` | 422    | `author.name` is not the authenticated identity, or the device belongs to another identity on the server                                                    |
| `role-mismatch`     | 422    | An `approval.given` names a role that `actors` does not give its author                                                                                     |
| `invalid-event`     | 422    | The line breaks the reading rules                                                                                                                           |
| `seq-gap`           | 409    | The event does not continue its device's log on the server                                                                                                  |
| `fork`              | 409    | The server holds another event at the same device and `seq`                                                                                                 |
| `id-conflict`       | 409    | The server holds an event with the same `id` and other content                                                                                              |

19. Permission management, administration, notifications, and how a client obtains its token (OAuth 2.0 or OpenID Connect) are not part of this specification.
