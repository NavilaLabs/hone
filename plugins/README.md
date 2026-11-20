# hone plugins

hone's own WebAssembly plugins (concept Section 9). They form a Cargo workspace of their own, because they build for the target `wasm32-wasip1` and the main workspace builds for the host.

| Plugin      | Purpose                                                                                                                                                                                                                                                                    |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lang-rust` | A language plugin (concept Section 9, "Language plugins"): resolves Rust symbols and computes the public API of `.rs` files for the standard gates `module-anchors-resolve` and `public-api-has-decision`. Shipped as the package `hone/lang-rust` (`schemas/lang-rust/`). |
| `probe`     | A test plugin for the gate test cases. The parameter `mode` selects its behaviour (findings, echo, read, write, trap, timeout, memory, invalid output).                                                                                                                    |

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

## Using `lang-rust`

The package `hone/lang-rust` holds the plugin file; copy `schemas/lang-rust/plugins/lang-rust.wasm` to `.hone/plugins/lang-rust.wasm` and approve it in `.hone/lock.yaml`:

```yaml
# schemas/lang-rust/plugins/lang-rust.yaml
capabilities: { read: [project], timeout_ms: 5000, memory_pages: 256 }
language: { extensions: [rs] }
```

The standard gates then use it for every `.rs` file:

```yaml
# gate entries in steps
anchors:
  gate: module-anchors-resolve
  params: { kind: module } # optional, module is the default
public-api:
  gate: public-api-has-decision
  params: { anchored: module, binding: decision }
```

`module-anchors-resolve` needs neither changed paths nor a base state; a symbol in a file that no language plugin handles is the warning `symbol-unchecked`. `public-api-has-decision` needs `--changed` and `--base <dir>`; a hit file that no language plugin handles is the warning `api-unchecked`. After a rebuild of `lang-rust`, also copy the module into `schemas/lang-rust/plugins/`.
