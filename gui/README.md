# hone-gui

The desktop GUI of hone for Linux and Windows (concept Section 12, "Desktop GUI"). It is built with Dioxus on the system web view and talks to hone only through the command line: every screen calls `hone … --json` as a child process. It never links `hone-core`, so the JSON of the command line is its interface. The one exception is the ticket tracker, which is tool behaviour of the GUI alone (see below).

It is a Cargo workspace of its own, because the web view needs system libraries that the protocol crates do not: on Linux WebKitGTK 4.1 (`webkit2gtk-4.1`, `libsoup-3.0`), on Windows the WebView2 runtime.

## Run

```sh
cargo build -p hone-cli                     # at the repository root: the hone binary
cd gui && HONE_BIN=../target/debug/hone cargo run -- <project root>
```

`hone` is found through `HONE_BIN`, otherwise next to `hone-gui`, otherwise on the `PATH`. The GUI passes its environment on, so `HONE_DATA_DIR`, `HONE_CONFIG_DIR`, `HONE_TOKEN`, `HONE_SYNC_FOLDER`, `HONE_SYNC_SERVER`, `HONE_AUTHOR`, and `HONE_ROLE` work as on the command line.

## Screens

| Screen     | Command line                                                                                                                            |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Tickets    | `hone artifacts`, `hone log`, `hone ticket show`, `hone events`, `hone check --class pure`, `hone step set`                             |
| Artifacts  | `hone artifacts`; one artifact: `hone review show`, `hone approve [--changes-requested]`, `hone review condense`, `hone review confirm` |
| Link graph | `hone artifacts`                                                                                                                        |
| Threads    | `hone events --ticket`, `hone artifacts`, `hone thread open/reply/resolve/reopen`                                                       |
| Checks     | `hone check [--ticket]`                                                                                                                 |
| Event log  | `hone log`                                                                                                                              |
| Tracker    | none: GitHub or Jira over HTTPS, the setting in `gui.json`                                                                              |
| Sync       | `hone sync --folder` or `--server`, `hone log`                                                                                          |
| Server     | `hone serve --config`                                                                                                                   |
| Top bar    | `hone artifacts` (project ID), `hone context show` and `hone log` (authors), `hone ticket show` (roles), the search                     |

A ticket shows the steps of its profile in the order of `hone ticket show`: done, skipped with its reason, open, or waiting for an earlier step that is still open, the current step (the first open step that waits for nothing), and per step the pure gates of the last check. The GUI runs `hone check --ticket <tk> --class pure` when a ticket opens and after every write, so no `exec` gate runs a command behind the user's back; the full check stays on the Checks screen. A gate whose only error is `changed-paths-missing` is shown as needing changed paths, not as failed, because the GUI knows no change.

The link graph places artifacts in one column per kind, ordered so that the kinds that store more links than they receive stand left, and orders each column by the rows of each node's neighbours. A link across a column runs through the gap between two rows, so it never passes behind a node. Hovering or focusing a node shows its links and their types; click or Enter opens it. Ticket records and their links start hidden; the ticket filter shows one ticket's artifacts and their direct neighbours.

An artifact shows its stored links and the links other artifacts store towards it (marked computed), the threads anchored to it, and its review in its ticket: every person's last decision, whether it is for the current content, the condensed `review` block, and the actions Approve, Request changes (with an optional proof), Condense into the file, and Confirm the review block. They write as the chosen author, and the artifact's ticket becomes the open ticket, so the role list is that ticket's.

The search (Ctrl+K or Cmd+K, or the field in the top bar) finds tickets by ID and external key, artifacts by ID, kind, and title, threads by their first line, and, with a tracker, the tracker's tickets (after a pause in typing). Every word must match. A tracker ticket that a ticket record names opens the hone ticket; any other one opens the Tracker screen on it.

Errors, failed gates, refused writes, and invalid fields use `bad`, never the accent; an invalid field keeps a `bad` border and focus ring.

Wherever the set of values is known, a field is a searchable list (`src/ui/combobox.rs`): tickets, steps, thread kinds (hone/standard's four plus the kinds in the log, or a new one), artifacts for an anchor, the event log's filters, the author (the context's author and the authors of the log, or a new handle) and the role (the roles of the open ticket's configuration). Arrow keys move, Enter selects, Escape closes.

The source has one module per feature under `src/features/`, the command line client in `src/hone.rs`, the tracker in `src/tracker.rs`, and the components in `src/ui/`.

## Tracker

The Tracker screen keeps, per project and checkout, where the project's tickets live: a GitHub repository (suggested from the Git remote `origin`) or a Jira project (base URL, project key, and the account email for Jira Cloud). It lists the open tickets or searches them, opens the hone ticket whose record names one in `external`, and copies the `external` entry for a ticket that no record names yet. A ticket whose record names a tracker ticket takes its title from the tracker.

The setting lives in `gui.json` in the hone configuration directory (`HONE_CONFIG_DIR`, otherwise `$XDG_CONFIG_HOME/hone/`, `%APPDATA%\hone\`, or `~/Library/Application Support/hone/`), next to `contexts.yaml`, keyed by project ID and the canonical root like a context binding:

```json
{
  "trackers": [
    {
      "project": "prj-8m2k4d7q",
      "root": "/home/alice/acme/payments",
      "tracker": { "kind": "github", "owner": "acme", "repo": "payments" }
    }
  ]
}
```

A Jira entry is `{ "kind": "jira", "base_url": "https://acme.atlassian.net", "project_key": "PAY", "email": "alice@acme.example" }`. Either may name `token_env`. The token never goes into a file: it comes from that variable (default `GITHUB_TOKEN` or `JIRA_API_TOKEN`) when set and not empty, otherwise from the OS keyring under the service `hone` and the account `tracker <API address>`, where the screen stores and removes it. GitHub works without a token for public repositories, at a low rate limit.

## Checks

After a change under `gui/`, in this directory:

```sh
npm run gui:css          # at the repository root, after a change to classes or to assets/input.css
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test               # needs the hone binary: cargo build -p hone-cli at the repository root
```

`tests/cli.rs` runs the real command line against a copy of a gate test case, so a change to the JSON of the command line turns it red. The tracker's requests and answers are unit-tested without a network; no test talks to GitHub, Jira, or the OS keyring.

## Components

The components in `src/ui/` come from [rust-ui](https://rust-ui.com/docs/components), the Dioxus registry, under the MIT License (Copyright (c) 2026 Max Wells). They keep rust-ui's Tailwind v4 classes; `assets/input.css` maps rust-ui's theme names onto hone's design tokens, removes every radius and shadow, draws the focus ring as an outline, and makes `dark:` follow the theme switch. Taken: Alert, Badge, Button, Card, Checkbox, Input, Progress, Separator, Skeleton, Table, Tabs, Textarea. Changes: variants the GUI does not use are left out, the icon crate is replaced by inline SVG, router links and pointer handlers are left out, `Input` and `Textarea` report their value through `oninput`, and the malformed second selector of `TableCell` is fixed. Select is not taken, because it depends on an inline script; `Combobox` is hone's own.

The accent is blue: `#6b8cff` in the dark theme and `#1d4ed8` in the light theme, with a hover shade (`#8ea8ff`, `#1e40af`) and a muted fill for the highlighted choice (`#1a2033`, `#ebeffb`). Accent text passes 4.5:1 on `bg`, `surface`, and the muted fill in both themes. The logo (`assets/logo-dark.svg`, `assets/logo-light.svg`) stands in the top bar in the variant of the theme; the window icon is `assets/icon.png`, the dark variant rendered at 256 px.

`assets/hone.css` is generated from `assets/input.css` with the Tailwind CLI pinned in `package.json` and committed, so that `cargo build` needs no Node.js. The fonts Inter and JetBrains Mono are used when installed and fall back to the system fonts.
