# Decisions for review

Decisions taken without the maintainer, during an autonomous run on the branch `feat/step-9-10-11`, and build-time rules that were reported but not yet confirmed. Each entry names the decision, the alternatives, and where it lives. Concept questions decided this way are marked "decided without review" in Section 15.

## Step 9: reference server (build-time rules, reported, not yet confirmed)

1. A store error on the server answers 500 with code `internal-error` and a generic message, so no server path leaks. Alternative: no body, or the real message.
2. A server answer the client cannot parse is the error `refused` with code `invalid-answer`. Alternative: `unavailable`.
3. A status without a known code is `refused` with code `http-<status>`.
4. A request without a `cursor` parameter means the empty cursor. Alternative: `invalid-request`.
5. An empty line in an append is reported as "the line is empty" (code `invalid-event`).
6. `--watch` stops pushing after the server rejected an own event, so the rejection is not repeated every 2 seconds.
7. `--watch` reconnects after 60 seconds without data on the stream.
8. A project without `actors` in the server configuration rejects every approval (fail closed).
9. A relative `data` path in `server.yaml` is resolved against the directory of the file.
10. `server.yaml` rejects unknown keys, members without an identity, two identities sharing a token, and limits that are not positive.
11. `hone serve` exits with 2 when it cannot start.
12. With both `HONE_SYNC_FOLDER` and `HONE_SYNC_SERVER` set, `hone sync` is a conflict even when only one flag is passed (clap counts environment values).
13. A server log with a hole rejects the next append to that device with `seq-gap`.
14. The server checks `Content-Length` against the limit before it reads the body.
15. An `id` the server holds at another place is `id-conflict`.
16. A subscriber also reads again at every heartbeat, so that events another process wrote into the data directory arrive.
17. An append test case without `store/` starts with an empty store (Section 14).
18. The client uses reqwest's default TLS (rustls with aws-lc-rs), which needs a C compiler to build.

## Step 9: rest

19. **Code `permission-denied`.** A file or directory of a shared folder that hone may not read, or may not write during push, is skipped and reported as `permission-denied` (Question 35), one code for reading and writing. Alternatives: `unsafe-path`, which would blur "leaves the folder"; two codes `unreadable` and `unwritable`. A permission error in the local store is skipped the same way, because the reader is shared.
20. **A denied segment ends the push.** When a segment of the own log cannot be written, push writes no later segment, as after a `fork` or an `unsafe-path`. Alternative: try the later segments, rejected because a later segment without its predecessor is a gap for every reader.
21. **A project directory that may not be examined ends the sync.** When hone may not look into the workspace directory, so that it cannot tell whether the project directory exists, sync reports `permission-denied` for the project directory and neither pulls nor pushes. Alternative: treat it as missing and try to create it, which fails the same way with a less precise report.
22. **A directory that may be listed but not entered.** When `log/` may be listed but its entries may not be examined, every device directory is reported as `permission-denied` instead of being ignored silently. A canonicalization refused for lack of permission is `permission-denied`, not `unsafe-path`.
23. **Permission tests under root.** The permission tests return early when locking a file has no effect, as for root. Alternative: fail, rejected because CI containers often run as root.
