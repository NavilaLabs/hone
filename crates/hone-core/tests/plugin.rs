//! Plugin and `exec` gates beyond the gate test cases: the input a plugin receives, and the
//! conditions under which `check` cannot run (concept Sections 9 and 12).

use std::path::{Path, PathBuf};

use hone_core::{CheckRequest, check};
use jsonschema::Registry;
use serde_json::Value;

fn case(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/tests/check")
        .join(name)
}

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

fn interfaces() -> CheckRequest {
    CheckRequest {
        ticket: Some("tk-9f2x4a8m".into()),
        step: Some("interfaces".into()),
        ..CheckRequest::default()
    }
}

// A copy of a case's project in a fresh temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn of(name: &str, test: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("hone-plugin-test-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        copy(&case(name).join("project"), &directory);
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create directory");
    for entry in std::fs::read_dir(from).expect("readable directory") {
        let entry = entry.expect("readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

#[test]
fn the_input_of_check_matches_the_plugin_schema() {
    let request = CheckRequest {
        base: Some(case("plugin-input-schema/base")),
        changed_paths: Some(vec!["src/payments/fee.rs".into()]),
        ..interfaces()
    };
    let report = check(&case("plugin-input-schema/project"), &request).expect("runs");
    let dump = report
        .gates
        .iter()
        .flat_map(|gate| &gate.findings)
        .find(|finding| finding.code.name() == "input")
        .expect("the probe dumps its input");
    let input: Value = serde_json::from_str(&dump.message).expect("the dump is JSON");

    let read = |file: &str| -> Value {
        let text = std::fs::read_to_string(schemas().join(file)).expect("readable schema");
        serde_json::from_str(&text).expect("schema is JSON")
    };
    let registry = [
        "common.schema.json",
        "config.schema.json",
        "plugin.schema.json",
    ]
    .into_iter()
    .map(read)
    .fold(Registry::new(), |registry, schema| {
        let id = schema["$id"].as_str().expect("$id").to_owned();
        registry.add(id, schema).expect("valid resource")
    })
    .prepare()
    .expect("registry resolves");
    let validator = jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&serde_json::json!({
            "$ref": "https://hone.invalid/schema/1/plugin.schema.json#/$defs/CheckInput"
        }))
        .expect("valid schema");
    let errors: Vec<String> = validator
        .iter_errors(&input)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    assert_eq!(input["step"], "interfaces");
    assert_eq!(input["params"], serde_json::json!({ "mode": "dump" }));
    assert_eq!(input["ticket"]["id"], "tk-9f2x4a8m");
    assert_eq!(
        input["changed_paths"],
        serde_json::json!(["src/payments/fee.rs"])
    );
    assert!(input.get("tool_output").is_none());
    let decision = input["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .find(|artifact| artifact["data"]["id"] == "dec-7k3m2q8p")
        .expect("the decision is an artifact");
    assert_eq!(decision["body"], "Text.\n");
    assert!(decision["data"].get("body").is_none());
}

#[test]
fn without_changed_paths_the_input_has_none() {
    let report = check(&case("plugin-input-schema/project"), &interfaces()).expect("runs");
    let dump = report
        .gates
        .iter()
        .flat_map(|gate| &gate.findings)
        .find(|finding| finding.code.name() == "input")
        .expect("the probe dumps its input");
    let input: Value = serde_json::from_str(&dump.message).expect("the dump is JSON");
    assert!(input.get("changed_paths").is_none());
}

#[test]
fn a_malformed_lockfile_stops_a_check_with_a_plugin_gate() {
    let project = Scratch::of("plugin-trap", "malformed-with-plugin");
    std::fs::write(project.0.join(".hone/lock.yaml"), "plugins: [probe]\n").expect("write");
    let error = check(&project.0, &interfaces()).expect_err("cannot run");
    assert!(error.message.contains("lock.yaml"), "{}", error.message);
}

#[test]
fn a_malformed_lockfile_does_not_matter_without_a_plugin_gate() {
    let project = Scratch::of("plugin-trap", "malformed-without-plugin");
    std::fs::write(project.0.join(".hone/lock.yaml"), "plugins: [probe]\n").expect("write");
    let request = CheckRequest {
        step: Some("specify".into()),
        ..interfaces()
    };
    let report = check(&project.0, &request).expect("runs");
    assert!(report.gates.iter().all(|gate| gate.gate != "probe"));
}

#[test]
fn a_missing_lockfile_or_base_stops_the_check() {
    let project = case("plugin-trap/project");
    let missing = case("plugin-trap/no-such-lock.yaml");
    let request = CheckRequest {
        lock: Some(missing),
        ..interfaces()
    };
    let error = check(&project, &request).expect_err("cannot run");
    assert!(error.message.contains("lockfile"), "{}", error.message);
    let request = CheckRequest {
        base: Some(case("plugin-trap/no-such-base")),
        ..interfaces()
    };
    let error = check(&project, &request).expect_err("cannot run");
    assert!(error.message.contains("base"), "{}", error.message);
}

#[test]
fn without_a_lockfile_nothing_is_approved() {
    let project = Scratch::of("plugin-trap", "no-lockfile");
    std::fs::remove_file(project.0.join(".hone/lock.yaml")).expect("remove");
    let report = check(&project.0, &interfaces()).expect("runs");
    let codes: Vec<&str> = report
        .gates
        .iter()
        .flat_map(|gate| &gate.findings)
        .map(|finding| finding.code.name())
        .collect();
    assert_eq!(codes, ["plugin-hash"]);
}
