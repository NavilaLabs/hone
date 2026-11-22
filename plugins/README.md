# hone plugins

hone's own WebAssembly plugins (concept Section 9). They form a Cargo workspace of their own, because they build for the target `wasm32-wasip1` and the main workspace builds for the host.

| Plugin                                                    | Purpose                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lang-rust`                                               | A language plugin (concept Section 9, "Language plugins"): resolves Rust symbols and computes the public API of `.rs` files for the standard gates `module-anchors-resolve` and `public-api-has-decision`. Shipped as the package `hone/lang-rust` (`schemas/lang-rust/`). |
| `lang-tree`                                               | The shared engine of the tree-sitter language plugins: parses a file, collects its nested declarations, and answers `resolve` and `public_api`. One module of rules per language.                                                                                          |
| `lang-typescript`, `lang-python`, `lang-php`, `lang-dart` | Language plugins on `lang-tree` for TypeScript and JavaScript, Python, PHP, and Dart, each shipped as the package `hone/lang-<language>` (`schemas/lang-<language>/`).                                                                                                     |
| `token-form`                                              | The token form of the types in a public API entry (`spec/plugins.md`, item 32), shared by `lang-tree` and `lang-rust`.                                                                                                                                                     |
| `probe`                                                   | A test plugin for the gate test cases. The parameter `mode` selects its behaviour (findings, echo, read, write, trap, timeout, memory, invalid output). As a language plugin, the first symbol or file name selects it (`trap`, `extra` for one result too many).          |

## Building

The grammars of the tree-sitter plugins are C code, so building for `wasm32-wasip1` needs the WASI libc. `.cargo/config.toml` points `CFLAGS_wasm32_wasip1` at `/usr/share/wasi-sysroot`, where Arch Linux and Debian install it (package `wasi-libc`); set the variable to use another sysroot, for example one of wasi-sdk. The rules of `lang-tree`, the entries of `lang-rust`, and the token form are tested natively: `cargo test -p lang-tree -p lang-rust -p token-form --target x86_64-unknown-linux-gnu`.

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

`module-anchors-resolve` needs neither changed paths nor a base state; a symbol in a file that no language plugin handles is the warning `symbol-unchecked`. `public-api-has-decision` needs `--changed` and `--base <dir>`; a hit file that no language plugin handles is the warning `api-unchecked`. After a rebuild of a language plugin, also copy its module into `schemas/lang-<language>/plugins/`; a test checks that the package and its test case carry the same bytes. A rebuilt `lang-rust`, or any change under `schemas/standard/`, also changes the example `examples/payments` (its lock entries, `resolved.json`, and the ticket's snapshot); the test `example` of `hone-cli` turns red until it is updated.

## Symbols per language

| Plugin            | Extensions                                           | A symbol                    | Public                                                                |
| ----------------- | ---------------------------------------------------- | --------------------------- | --------------------------------------------------------------------- |
| `lang-rust`       | `rs`                                                 | `Fee::add`                  | `pub` items in `pub mod` paths                                        |
| `lang-typescript` | `ts`, `tsx`, `mts`, `cts`, `js`, `jsx`, `mjs`, `cjs` | `Fee.add`, `Payments.Inner` | exported declarations, members not `private`, `protected`, or `#name` |
| `lang-python`     | `py`, `pyi`                                          | `Fee.add`                   | names without a leading `_` (`__name__` is public)                    |
| `lang-php`        | `php`                                                | `Fee::add`, `Fee::$amount`  | top-level declarations, members not `private` or `protected`          |
| `lang-dart`       | `dart`                                               | `Fee.add`                   | names without a leading `_`                                           |

The public API of a file is one string per public declaration: its symbol and its signature without the body. A file with a syntax error is `unparsable`.

## Public API entries per language

`public_api` gives one entry per public declaration (`spec/plugins.md`, items 32 to 34). The host compares them without `line`, `text`, `extra`, defaults, and the names of parameters passed by position, so how a language passes parameters decides whether renaming one counts.

| Plugin            | Kinds                                                                                                                                                   | Parameters passed                                                       |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `lang-rust`       | `function`, `method`, `struct`, `field`, `enum`, `variant`, `trait`, `type`, `constant`, `static`, `use`, `module`, `impl`                              | by position                                                             |
| `lang-typescript` | `function`, `generator`, `class`, `interface`, `enum`, `variant`, `namespace`, `type`, `variable`, `method`, `constructor`, `getter`, `setter`, `field` | by position                                                             |
| `lang-python`     | `function`, `class`, `method`, `property`, `setter`, `field`, `variable`                                                                                | by both; by position before `/`, `*args`; by name after `*`, `**kwargs` |
| `lang-php`        | `function`, `class`, `interface`, `trait`, `enum`, `variant`, `method`, `constructor`, `constant`, `field`                                              | by both                                                                 |
| `lang-dart`       | `function`, `class`, `mixin`, `extension`, `enum`, `variant`, `method`, `constructor`, `getter`, `setter`, `field`, `variable`                          | by position; by name inside `{…}`                                       |
