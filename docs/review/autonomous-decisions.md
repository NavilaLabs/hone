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

## Desktop GUI (concept Question 36)

24. **rust-ui's Dioxus components, taken as they are.** The handover assumed rust-ui supports only Leptos and asked for a port. rust-ui now publishes a Dioxus registry (MIT), so its Dioxus sources are taken and trimmed, not ported. Their Tailwind classes stay; hone's tokens stand behind rust-ui's theme names. Alternative: port the Leptos sources by hand, rejected because the result would be the same code with more room for mistakes.
25. **Theme mapping.** rust-ui's `primary` is hone's `accent` (the one primary action), rust-ui's `accent` (a hover fill) is hone's `surface-2`, `muted` and `secondary` are `surface`, `border` and `input` are `line`, `destructive` is `bad`. Every radius is 0 and every shadow is removed with one global rule, and the focus ring is an outline (2px, offset 2px). Alternative: rewrite the classes to hone's names, rejected because the handover asked to keep rust-ui's classes.
26. **A workspace of its own under `gui/`.** The web view needs WebKitGTK on Linux; in the main workspace every `cargo test` would build Dioxus and require those libraries. The precedent is `plugins/`. Alternative: `crates/hone-gui` in the main workspace.
27. **`hone log` is new.** The GUI needs the tickets and the event log, and the command line had no command that lists events. `hone log [--json]` prints `cursor` and `events` (the order of `read`). It has no filter; the GUI filters. Alternative: `hone events` without `--ticket`, rejected because its result also computes threads and steps of one ticket.
28. **Thread status is not recomputed in the GUI.** The ticket list shows the event count and the last event type from `hone log`; threads and their status come only from `hone events`, so the GUI never reimplements a protocol rule. The thread ID of an opening event (`th-` plus its UUID) is used to group a conversation, because concept Section 10 states it.
29. **Steps shown are the steps with a state.** The command line does not print a ticket's profile, so the step track shows the steps that have a `step.changed`, in the order of their first change, and setting a step takes its name as text. A `hone ticket` command that prints the profile's steps would let the track show every step; it is not added here.
30. **Gate classes in the check screen.** The ten standard gates of Section 7 are `pure`, `exec` is `exec`, every other gate is `plugin · wasm`. The command line does not print the class; adding it to `hone check --json` would remove this list from the GUI.
31. **Where `hone` is found.** `HONE_BIN`, then next to the GUI executable, then the `PATH`. The GUI passes its environment on and stores no token, no folder, and no server; writer and root are entered in the window and default to `HONE_AUTHOR`, `HONE_ROLE`, and the first argument.
32. **CSS is generated and committed.** `npm run gui:css` (Tailwind CLI 4.3.4, pinned) writes `gui/assets/hone.css`, which is committed and embedded with `include_str!`, so `cargo build` needs neither Node.js nor the Dioxus CLI. It is listed in `.prettierignore`.
33. **Fonts.** Inter and JetBrains Mono are named first and fall back to system fonts; no font files are committed and nothing is loaded from the network.
34. **Screens without a command.** Artifacts, the link graph, profiles, and plugins are left out until the command line offers them. The server screen starts `hone serve` as a child process and stops it by killing it.
35. **Windows is not tested on this machine.** The GUI uses no platform-specific code (paths through `std::path`, the executable suffix from `std::env::consts`), but it was built and run only on Linux.
