# hone plugins

hone's own WebAssembly plugins (concept Section 9). They form a Cargo workspace of their own, because they build for the target `wasm32-wasip1` and the main workspace builds for the host.

| Plugin                    | Purpose                                                                                                                                                        |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `public-api-has-decision` | A gate: a change to the public API of a Rust file under an anchored artifact (for example a module) needs a binding decision. It compares with the base state. |
| `probe`                   | A test plugin for the gate test cases. The parameter `mode` selects its behaviour (findings, echo, read, write, trap, timeout, memory, invalid output).        |

## Building

```sh
rustup target add wasm32-wasip1
cd plugins
cargo build --release --target wasm32-wasip1
cargo clippy --release --target wasm32-wasip1 -- -D warnings
cargo fmt --check
```

The modules land in `plugins/target/wasm32-wasip1/release/`. `cargo test` of the main workspace needs neither this target nor network: the gate test cases under `schemas/tests/check/` contain the built `.wasm` files, and their configurations and lockfiles name the hashes of exactly these bytes.

After a change to a plugin, rebuild it, copy the new module into every test case that uses it (`.hone/plugins/<name>.wasm`), and update the hash in the case's `resolved.json`, snapshot, ticket record, and lockfiles. A compiler update alone can change the bytes, so only rebuild when the source changes.

## Using `public-api-has-decision`

```yaml
# .hone/plugins/public-api-has-decision.yaml
capabilities: { read: [project], timeout_ms: 5000, memory_pages: 256 }
```

```yaml
# a gate entry in a step
public-api:
  gate: public-api-has-decision
  params: { anchored: module, binding: decision }
```

`hone check` then needs `--changed` and `--base <dir>`, for example a `git worktree` of the target branch. Without them, the gate fails with `changed-paths-missing` or `base-missing`.
