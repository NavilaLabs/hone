# Example: payments

A small project to look at hone as it stands: a Rust library, its design artifacts, one ticket in progress, and the events of that ticket.

| What                                                                                         | Where                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Configuration: roles, steps, two profiles, selection                                         | `.hone/`, on top of the packages `hone/standard` and `hone/lang-rust`      |
| Packages                                                                                     | `packages/hone/` links to `schemas/`, so they are the repository's presets |
| Code                                                                                         | `src/payments/fee.rs`; `base/` is the state before `Fee::cents` was added  |
| Module, spec, decision                                                                       | `design/`; the decision is accepted, approved by alice, and condensed      |
| Ticket `tk-y4075sb1` (`risk: high`, profile `standard`)                                      | `design/tickets/tk-y4075sb1/`, snapshot in `.hone/snapshots/`              |
| Events: start, approvals, a resolved question, an open blocking concern, step `specify` done | `shared/`, a shared folder with the log of one device                      |

The kind `decision` comes from `hone/standard` and gets one more status from the project, so the configuration shows a definition with two layers.

## Try it

Build the command line at the repository root (`cargo build -p hone-cli`) and run everything below in this directory, with `hone` meaning `../../target/debug/hone`. The events live in a process store outside the repository; a data directory of its own keeps them apart from your real projects.

```sh
export HONE_DATA_DIR=/tmp/hone-example HONE_CONFIG_DIR=/tmp/hone-example/config
hone sync --folder shared                  # import the ticket's events
hone resolve --show --packages packages    # every definition with its layers, and the lockfile
hone check --ticket tk-y4075sb1            # decision/no-blockers fails: bob's concern is open
hone check --ticket tk-y4075sb1 --step interfaces --changed src/payments/fee.rs --base base
                                           # the public API changed, and the accepted decision names the module
hone events --ticket tk-y4075sb1           # threads and steps
```

Resolve the concern to see the ticket's gates pass, as alice, the architect:

```sh
hone events --ticket tk-y4075sb1 --json    # the ID of the open thread
hone thread resolve <thread> --author alice --role architect
```

A sync after such a write puts your device's log into `shared/`; `git checkout examples/payments/shared` and a new data directory start over.

## The GUI

```sh
cd ../../gui && HONE_BIN=../target/debug/hone cargo run -- ../examples/payments
```

with the same `HONE_DATA_DIR` and `HONE_CONFIG_DIR`. On the Settings screen, set the packages directory to the absolute path of `examples/payments/packages`, so that Profiles, Plugins, and New ticket can resolve. Sync takes `shared` as the folder.
