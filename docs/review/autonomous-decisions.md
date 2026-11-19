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

## Approvals and condensation (concept Question 37)

36. **`hone approve` checks the role before writing.** The role's `can` must hold `approve`, and `actors` must give the author that role (`"*"` for persons not listed), the rule a server enforces with `role-mismatch`. Alternative: write any approval and let `approvals-valid` ignore it, rejected because the log is immutable.
37. **The last decision counts.** For every person and role, the last `approval.given` for the artifact in the order of Section 4 decides; `changes-requested` after `approved` withdraws. Alternative: any `approved` counts, rejected because a withdrawal would be impossible.
38. **`date` comes from the event ID.** The UTC date of the UUIDv7 timestamp. Alternative: the date of condensing, rejected because it would change on every run.
39. **The summary comes from a YAML file.** `--summary <file>` in the strict profile, a list of `{ q, a }` or `{ rejected }` with text values; the AI drafts the file. Under `approvals-only` a summary is refused instead of dropped silently.
40. **Condensing writes, it does not commit.** It replaces the `review` block (a top-level `review:` key and its indented lines), appends the new block at the end of the front matter or the YAML file, keeps every other byte, refuses files with carriage returns, and checks that the content hash and the parsed block are as intended. It writes a new file next to the artifact and renames it over the old one, so a link planted at the path is replaced, not written through. Strings are double-quoted and entries are flow maps in the order of the concept's example.
41. **`review.confirmed` binds the current content hash** and needs the role from `decides` of a step of the ticket's profile that outputs the artifact's kind, or, if none names `decides`, a role that may approve; `actors` must give the author that role. Alternative: bind the hash of the whole file including the block, rejected because the content hash is the one hash approvals use.
42. **No MCP tools for approvals yet.** An approval is a person's decision; an agent with a fixed author should not give one without an explicit design.
43. **Open: actors for every event.** An automated security review suggested checking `actors` for every written event, not only approvals and confirmations. That would change the decided behaviour of `hone thread` and `hone step` (any handle with an existing role may write today, because local authorship is trust-based), so it is listed here for review instead of being changed.
44. **Not covered by tests yet:** a role without `approve`, `approvals-only`, and `decides`, because the gate test projects have none of them; covering them needs a test project with its own snapshot.
45. **Open: condensing still has a small race.** After an automated security review, condensing checks the artifact's directory below the project root before creating the new file and again just before the rename. A writer of the project directory could still swap the directory for a link between the second check and the rename. Closing it needs directory handles (`openat`/`renameat`, for example through `rustix`), a new dependency; the threat is a concurrent writer inside the user's own checkout.

## Git branch as a shared folder (concept Question 38)

46. **Where the worktree lives.** `<store>/git/<branch with / as ->/`, below the store directory and outside the project's checkout, so the project's working tree never shows the branch. Alternative: a directory in `.git/`, rejected because Git owns it.
47. **Reset, not merge.** Before each sync the worktree is reset to the fetched branch; the own log is written again from the local store, so nothing is lost. A missing remote branch is created as an orphan branch.
48. **Named tracking ref.** Fetches go to `refs/remotes/<remote>/<branch>`, because `FETCH_HEAD` belongs to the worktree that fetched and a worktree cannot see the repository's.
49. **Commit and push.** Only `log/<own device>/` is staged; no change means no commit. Message `hone: log of <device>`. A rejected push is retried three times after a fetch and a rebase, which cannot conflict. Commits use the repository's Git identity.
50. **Names.** Remote and branch may hold only ASCII letters, digits, `/`, `.`, `_`, `-`, must not start with `-` and must not contain `..`, because both reach the Git command line as arguments.
51. **Errors.** A failing Git command ends the sync as `unavailable` with Git's message.
52. **MCP.** `hone mcp --sync-git <branch>` (or `HONE_SYNC_GIT`) uses the remote `origin`; there is no `--sync-remote` yet.
53. **Not covered by tests yet:** the push retry after a rejected push.

## Contexts (concept Question 39)

54. **A local `contexts.yaml`, no secret.** In the platform's configuration directory (`HONE_CONFIG_DIR` replaces it). A context holds a peer (`server`, `folder`, or `git` with `remote`), an optional author, and `token_env`, the name of the variable that holds the token. Alternative: store the token, rejected because a file with tokens leaks them; an OS keyring is left for later.
55. **Precedence.** A flag or its environment variable wins; only without them does `hone sync` take the context's peer and a writing command the context's author. `--author` and `--role` now go together.
56. **The file is written anew.** `hone context set` and `use` rewrite it in the strict profile; comments are lost. Unknown keys, a peer with more than one kind, and a binding to an unknown context are rejected when reading.
57. **Names.** A context name is lowercase letters, digits, and `-`, not starting with `-`.
58. **The GUI leaves the author to the context** when its author or role field is empty, instead of passing empty flags.
59. **Not done:** the MCP server does not read contexts yet (it keeps `--author`, `--sync-*`); `export` and `forget`; tokens in an OS keyring.

## `module-anchors-resolve` (concept Question 40)

60. **What is checked.** Every anchor and `interface.code` of the artifacts of the kind in `params.kind` (default `module`), against `/project`. `path`: the file or directory exists; `glob`: at least one file matches (`*`, `?`, `**` as in `change-needs-binding`); `symbol`: the file exists and, in a `.rs` file, the symbol resolves.
61. **Rust symbol resolution.** `a::b` resolves through items (fn, struct, enum, trait, type, const, static, union, inline `mod`, `macro_rules!`), enum variants, trait items, named struct fields, and items of `impl` blocks of a type named like the last path segment, also when the type is defined elsewhere. Re-exports (`pub use`) and out-of-line `mod x;` files are not followed. Alternative: rust-analyzer-level resolution, rejected as far beyond a WASM plugin.
62. **Codes.** Errors `anchor-missing`, `symbol-missing`, `unparsable`; warnings `symbol-unchecked` (symbol in a non-Rust file whose file exists) and `other-repository` (`repository:path`, not visible to the check). Each finding carries the artifact's path and the anchor's pointer (`/anchors/<i>`, `/interface/code`).
63. **Test cases.** One cause per case, as the gate test suite requires. The test projects' module kind has no extra schema, so `interface` shows the warning `unknown-field` of `schema-valid`; the expected lines include it.
64. **A binding holds the checkout's root.** An automated security review noted that a context was bound by the project ID from the repository's own `.hone/config.yaml`, so a foreign checkout could claim a bound project's ID and inherit its peer and author. A binding is now `{ context, root }` with the canonical root; in another checkout `hone sync` and the writing commands fail with a message to bind that checkout. Alternative: ask for confirmation, rejected because hone has no interactive prompts.

## Specification (concept Question 41, step 11)

65. **Chapter by chapter.** `spec/` holds one file per area with BCP 14 key words; a written chapter is normative and wins over the concept, whose section then points to it. Alternative: translate the whole concept at once, rejected because a half-checked normative text is worse than a clear boundary between written and pending chapters.
66. **First chapters.** `spec/README.md` (status, conventions, levels, chapter map) and `spec/conformance.md` (test suites and every test case format, moved out of concept Section 14 so that it exists once).
67. **Levels of the suites.** `valid/`, `invalid/`, `profile/`, `hash/`, `check/`, and `config/` are Core, `events/` is Flow, `resolve/`, `read/`, `sync/`, and `append/` are Extended; a gate case belongs to the level of its gates. The concept named the levels but did not assign the suites.
68. **Tests outside the suite.** Transport codes, the subscription, and permission errors in a shared folder cannot be expressed as repository files; the conformance chapter says each implementation tests them itself.
69. **Process store and replication moved.** `spec/process-store.md` and `spec/replication.md` hold the rules of concept Section 4 as numbered, normative statements; the concept keeps the reasons (one log per device, canonical lines, full UUIDv7, no database format, the role of the server, identity, CI) and points to the chapters. No rule was changed in the move.
70. **Pending chapters:** artifacts, configuration, gates, plugins, review.
