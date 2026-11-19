# hone-gui

The desktop GUI of hone for Linux and Windows (concept Section 12, "Desktop GUI"). It is built with Dioxus on the system web view and talks to hone only through the command line: every screen calls `hone … --json` as a child process. It never links `hone-core`, so the JSON of the command line is its interface.

It is a Cargo workspace of its own, because the web view needs system libraries that the protocol crates do not: on Linux WebKitGTK 4.1 (`webkit2gtk-4.1`, `libsoup-3.0`), on Windows the WebView2 runtime.

## Run

```sh
cargo build -p hone-cli                     # at the repository root: the hone binary
cd gui && HONE_BIN=../target/debug/hone cargo run -- <project root>
```

`hone` is found through `HONE_BIN`, otherwise next to `hone-gui`, otherwise on the `PATH`. The GUI passes its environment on, so `HONE_DATA_DIR`, `HONE_TOKEN`, `HONE_SYNC_FOLDER`, `HONE_SYNC_SERVER`, `HONE_AUTHOR`, and `HONE_ROLE` work as on the command line. It stores nothing itself.

## Screens

| Screen    | Command line                                             |
| --------- | -------------------------------------------------------- |
| Tickets   | `hone log`, `hone events --ticket`, `hone step set`      |
| Threads   | `hone events --ticket`, `hone thread open/reply/resolve` |
| Checks    | `hone check [--ticket]`                                  |
| Event log | `hone log`                                               |
| Sync      | `hone sync --folder` or `--server`, `hone log`           |
| Server    | `hone serve --config`                                    |

The source has one module per feature under `src/features/`, the command line client in `src/hone.rs`, and the components in `src/ui/`.

## Checks

After a change under `gui/`, in this directory:

```sh
npm run gui:css          # at the repository root, after a change to classes or to assets/input.css
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test               # needs the hone binary: cargo build -p hone-cli at the repository root
```

`tests/cli.rs` runs the real command line against a copy of a gate test case, so a change to the JSON of the command line turns it red.

## Components

The components in `src/ui/` come from [rust-ui](https://rust-ui.com/docs/components), the Dioxus registry, under the MIT License (Copyright (c) 2026 Max Wells). They keep rust-ui's Tailwind v4 classes; `assets/input.css` maps rust-ui's theme names onto hone's design tokens, removes every radius and shadow, and draws the focus ring as an outline. Taken: Alert, Badge, Button, Card, Checkbox, Input, Progress, Separator, Skeleton, Table, Tabs, Textarea. Changes: variants the GUI does not use are left out, the icon crate is replaced by inline SVG, router links and pointer handlers are left out, `Input` and `Textarea` report their value through `oninput`, and the malformed second selector of `TableCell` is fixed. Select is not taken, because it depends on an inline script.

`assets/hone.css` is generated from `assets/input.css` with the Tailwind CLI pinned in `package.json` and committed, so that `cargo build` needs no Node.js. The fonts Inter and JetBrains Mono are used when installed and fall back to the system fonts.
