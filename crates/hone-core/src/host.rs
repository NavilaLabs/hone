use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use extism::{CompiledPlugin, Manifest, Plugin, PluginBuilder, Wasm};
use jsonschema::{Registry, Validator};
use serde_json::{Map, Value};

use crate::canonical::sha256;
use crate::complete::unapproved_capabilities;
use crate::exec;
use crate::finding::{Code, Finding, Level};
use crate::manifest::Lock;

const COMMON_SCHEMA: &str = include_str!("../../../schemas/common.schema.json");
const CONFIG_SCHEMA: &str = include_str!("../../../schemas/config.schema.json");
const PLUGIN_SCHEMA: &str = include_str!("../../../schemas/plugin.schema.json");
const ENTRY_POINT: &str = "check";

static OUTPUT_VALIDATOR: LazyLock<Validator> =
    LazyLock::new(|| plugin_validator("#/$defs/CheckOutput"));

// Runs the gates that are not standard gates (concept Section 9): plugin gates in the sandbox and
// `exec` gates on the host, both only when the lockfile approves them.
pub(crate) struct Host<'a> {
    project_root: &'a Path,
    base: Option<&'a Path>,
    lock: Lock,
    // Compiled once per plugin and hash, instantiated fresh for every gate.
    compiled: RefCell<BTreeMap<(String, String), Result<CompiledPlugin, String>>>,
}

impl<'a> Host<'a> {
    pub(crate) fn new(project_root: &'a Path, base: Option<&'a Path>, lock: Lock) -> Self {
        Self {
            project_root,
            base,
            lock,
            compiled: RefCell::default(),
        }
    }

    // `input` is the input of `check` without `tool_output`.
    pub(crate) fn plugin_gate(&self, config: &Value, plugin: &str, input: &Value) -> Vec<Finding> {
        self.run_plugin(config, plugin, input)
            .unwrap_or_else(|finding| vec![finding])
    }

    // `input` is the input of `check` for the plugin named by `evaluate`, without `tool_output`.
    pub(crate) fn exec_gate(&self, config: &Value, params: &Value, input: &Value) -> Vec<Finding> {
        let output = match exec::run(self.project_root, params, &self.lock.commands) {
            Ok(output) => output,
            Err(finding) => return vec![finding],
        };
        let Some(evaluate) = params["evaluate"].as_str() else {
            return output.exit_finding().into_iter().collect();
        };
        let mut input = input.clone();
        input["tool_output"] = output.to_json();
        self.plugin_gate(config, evaluate, &input)
    }

    fn run_plugin(
        &self,
        config: &Value,
        name: &str,
        input: &Value,
    ) -> Result<Vec<Finding>, Finding> {
        let definition = &config["plugins"][name];
        let hash = definition["hash"].as_str().unwrap_or_default();
        let capabilities = definition["capabilities"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let compiled = self.compile(name, definition, hash, &capabilities)?;
        let failed = |message: String| Finding::error(Code::PluginFailed, message);
        let input = serde_json::to_vec(input).expect("JSON serializes");
        // WASI blocks on the tokio runtime of the calling thread if there is one, which deadlocks
        // a caller inside a runtime, such as the MCP server. A thread of its own has none.
        let output = std::thread::scope(|scope| {
            scope
                .spawn(|| call(&compiled, name, &input))
                .join()
                .unwrap_or_else(|_| Err(format!("`{name}` made the host panic")))
        })
        .map_err(failed)?;
        let output: Value = serde_json::from_slice(&output)
            .map_err(|error| failed(format!("`{name}` returned no JSON: {error}")))?;
        if let Some(violation) = OUTPUT_VALIDATOR.iter_errors(&output).next() {
            return Err(failed(format!(
                "`{name}` returned an invalid output at `{}`: {violation}",
                violation.instance_path()
            )));
        }
        Ok(output["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(finding)
            .collect())
    }

    fn compile(
        &self,
        name: &str,
        definition: &Value,
        hash: &str,
        capabilities: &Map<String, Value>,
    ) -> Result<CompiledPlugin, Finding> {
        let key = (name.to_owned(), hash.to_owned());
        if let Some(compiled) = self.compiled.borrow().get(&key) {
            return compiled
                .clone()
                .map_err(|message| Finding::error(Code::PluginFailed, message));
        }
        let bytes = self.approved_module(name, definition, hash, capabilities)?;
        let compiled = PluginBuilder::new(self.manifest(bytes, capabilities))
            .with_wasi(true)
            .with_cache_disabled()
            .compile()
            .map_err(|error| format!("`{name}` cannot be loaded: {}", cause(&error)));
        self.compiled.borrow_mut().insert(key, compiled.clone());
        compiled.map_err(|message| Finding::error(Code::PluginFailed, message))
    }

    // The bytes of the module, after the checks of concept Section 9, "Checked on every run".
    // The same bytes are hashed and loaded, so the file cannot change in between.
    fn approved_module(
        &self,
        name: &str,
        definition: &Value,
        hash: &str,
        capabilities: &Map<String, Value>,
    ) -> Result<Vec<u8>, Finding> {
        let wasm = definition["wasm"].as_str().unwrap_or_default();
        let mismatch = |message: String| Finding::error(Code::PluginHash, message).at_path(wasm);
        let bytes = std::fs::read(self.project_root.join(wasm))
            .map_err(|error| mismatch(format!("`{name}` cannot be read: {error}")))?;
        let actual = sha256(&bytes);
        if actual != hash {
            return Err(mismatch(format!(
                "`{name}` has the hash {actual}, the configuration names {hash}"
            )));
        }
        let Some(approved) = self.lock.plugins.get(name) else {
            return Err(mismatch(format!("the lockfile has no entry for `{name}`")));
        };
        if approved.hash != hash {
            return Err(mismatch(format!(
                "the lockfile approves {} for `{name}`, the configuration names {hash}",
                approved.hash
            )));
        }
        if let Some((key, message)) = unapproved_capabilities(capabilities, &approved.capabilities)
            .into_iter()
            .next()
        {
            return Err(Finding::error(
                Code::PluginCapabilities,
                format!("`{name}`: `{key}`: {message}"),
            ));
        }
        Ok(bytes)
    }

    fn manifest(&self, bytes: Vec<u8>, capabilities: &Map<String, Value>) -> Manifest {
        let mut manifest = Manifest::new([Wasm::data(bytes)])
            .with_allowed_hosts(std::iter::empty())
            .with_allowed_paths(self.mounts(capabilities).into_iter());
        if let Some(pages) = capabilities["memory_pages"]
            .as_u64()
            .and_then(|pages| u32::try_from(pages).ok())
        {
            manifest = manifest.with_memory_max(pages);
        }
        if let Some(timeout) = capabilities["timeout_ms"].as_u64() {
            manifest = manifest.with_timeout(Duration::from_millis(timeout));
        }
        manifest
    }

    // The read-only mounts of concept Section 9, "Runtime": `/project` and `/base`, each with the
    // paths of `read` that exist in that state.
    fn mounts(&self, capabilities: &Map<String, Value>) -> Vec<(String, PathBuf)> {
        let entries: Vec<&str> = capabilities["read"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let states = [(Some(self.project_root), "/project"), (self.base, "/base")];
        let mut mounts = Vec::new();
        for (root, guest) in states {
            let Some(root) = root else {
                continue;
            };
            for entry in &entries {
                let (host, guest) = if *entry == "project" {
                    (root.to_path_buf(), PathBuf::from(guest))
                } else {
                    (root.join(entry), Path::new(guest).join(entry))
                };
                // A symbolic link in the checked state must not mount a directory outside it.
                if let (Ok(host), Ok(root)) = (host.canonicalize(), root.canonicalize())
                    && host.starts_with(&root)
                {
                    mounts.push((format!("ro:{}", host.display()), guest));
                }
            }
        }
        mounts
    }
}

// Calls `check` on a fresh instance, so that no state passes from one gate to the next.
fn call(compiled: &CompiledPlugin, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    let mut plugin = Plugin::new_from_compiled(compiled)
        .map_err(|error| format!("`{name}` cannot be instantiated: {}", cause(&error)))?;
    if !plugin.function_exists(ENTRY_POINT) {
        return Err(format!("`{name}` does not export `{ENTRY_POINT}`"));
    }
    plugin
        .call::<&[u8], &[u8]>(ENTRY_POINT, input)
        .map(<[u8]>::to_vec)
        .map_err(|error| format!("`{name}` failed: {}", cause(&error)))
}

// The output was validated against the schema, so every field has its shape.
fn finding(value: &Value) -> Finding {
    let text = |key: &str| value[key].as_str().map(str::to_owned);
    Finding {
        level: if value["level"] == "warning" {
            Level::Warning
        } else {
            Level::Error
        },
        code: Code::Plugin(text("code").unwrap_or_default()),
        path: text("path"),
        pointer: text("pointer"),
        message: text("message").unwrap_or_default(),
    }
}

fn cause(error: &extism::Error) -> String {
    error.root_cause().to_string()
}

fn plugin_validator(definition: &str) -> Validator {
    let registry = [COMMON_SCHEMA, CONFIG_SCHEMA, PLUGIN_SCHEMA]
        .into_iter()
        .map(|text| {
            let schema: Value = serde_json::from_str(text).expect("a protocol schema is JSON");
            let id = schema["$id"].as_str().expect("$id").to_owned();
            (id, schema)
        })
        .fold(Registry::new(), |registry, (id, schema)| {
            registry
                .add(id, schema)
                .expect("a protocol schema is a valid resource")
        })
        .prepare()
        .expect("the registry of the plugin schema resolves");
    let plugin_id = "https://hone.invalid/schema/1/plugin.schema.json";
    jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&serde_json::json!({ "$ref": format!("{plugin_id}{definition}") }))
        .expect("plugin.schema.json is a valid schema")
}
