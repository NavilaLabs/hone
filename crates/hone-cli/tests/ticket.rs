//! `hone ticket start`, `set-attributes`, and `switch-config` (concept Section 12, "Process
//! commands").

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/resolve/defaults/project";

// A copy of a resolve test case with a second profile, a selection rule, and a step that runs
// `steps-complete`, plus a data and a configuration directory, removed on drop.
struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-cli-ticket-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(CASE),
            &root.join("project"),
        );
        let hone = root.join("project/.hone");
        std::fs::write(
            hone.join("select.yaml"),
            "rules:\n  - { when: { risk: \"high\" }, profile: \"strict\" }\ndefault: \"only\"\n",
        )
        .expect("writable");
        std::fs::write(
            hone.join("steps/finish.yaml"),
            "gates: { complete: { gate: \"steps-complete\" } }\n",
        )
        .expect("writable");
        std::fs::write(
            hone.join("profiles/only.yaml"),
            "steps: { work: optional, finish: required }\n",
        )
        .expect("writable");
        std::fs::write(
            hone.join("profiles/strict.yaml"),
            "steps: { work: required, finish: required }\n",
        )
        .expect("writable");
        let project = Self { root };
        let resolved = project.hone(&["resolve"]);
        assert_eq!(resolved.status.code(), Some(0), "{resolved:?}");
        project
    }

    fn path(&self) -> PathBuf {
        self.root.join("project")
    }

    fn hone(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hone"))
            .args(arguments)
            .current_dir(self.path())
            .env("HONE_DATA_DIR", self.root.join("data"))
            .env("HONE_CONFIG_DIR", self.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .env_remove("HONE_ROLE")
            .output()
            .expect("hone runs")
    }

    fn json(&self, arguments: &[&str]) -> (Option<i32>, Value) {
        let mut all = arguments.to_vec();
        all.push("--json");
        let output = self.hone(&all);
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("one JSON document: {output:?}"));
        (output.status.code(), value)
    }

    fn written(&self) -> Vec<String> {
        let mut names: Vec<String> = ["tickets", ".hone/snapshots"]
            .iter()
            .flat_map(|directory| {
                std::fs::read_dir(self.path().join(directory))
                    .into_iter()
                    .flatten()
            })
            .map(|entry| {
                entry
                    .expect("readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("typed").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copyable");
        }
    }
}

// The content hash of the resolved configuration, computed with `sha256sum` over the file without
// its final newline, independently of hone.
fn config_hash(project: &Project) -> String {
    let bytes = std::fs::read(project.path().join(".hone/resolved.json")).expect("readable");
    let canonical = bytes.strip_suffix(b"\n").expect("ends with a newline");
    let scratch = project.root.join("canonical.json");
    std::fs::write(&scratch, canonical).expect("writable");
    let output = Command::new("sha256sum")
        .arg(&scratch)
        .output()
        .expect("sha256sum runs");
    let text = String::from_utf8(output.stdout).expect("text");
    format!("sha256:{}", text.split_whitespace().next().expect("a hash"))
}

const ALICE: [&str; 4] = ["--author", "alice", "--role", "human"];

fn with_author<'a>(arguments: &[&'a str]) -> Vec<&'a str> {
    let mut all = arguments.to_vec();
    all.extend(ALICE);
    all
}

#[test]
fn start_writes_the_record_the_snapshot_and_the_event() {
    let project = Project::new("start");
    let config = config_hash(&project);
    let (code, started) = project.json(&with_author(&[
        "ticket",
        "start",
        "--attribute",
        "risk=high",
        "--attribute",
        "size=3",
        "--attribute",
        "urgent=true",
        "--attribute",
        "code=\"007\"",
        "--external",
        "github:8798=https://github.com/o/r/issues/8798",
    ]));
    assert_eq!(code, Some(0), "{started}");
    let ticket = started["ticket"].as_str().expect("an ID").to_owned();
    assert!(ticket.len() == 11 && ticket.starts_with("tk-"), "{ticket}");
    assert_eq!(started["profile"], "strict", "`select` matches risk: high");
    assert_eq!(started["selected"], "strict");
    assert_eq!(started["config"], config.as_str());
    assert_eq!(
        started["attributes"],
        json!({ "risk": "high", "size": 3, "urgent": true, "code": "007" })
    );
    assert_eq!(started["event"]["type"], "ticket.started");
    assert_eq!(
        started["event"]["payload"],
        json!({ "profile": "strict", "config": config })
    );
    assert_eq!(
        started["event"]["author"],
        json!({ "name": "alice", "role": "human" })
    );

    assert_eq!(
        std::fs::read_to_string(project.path().join(format!("tickets/{ticket}.yaml")))
            .expect("the record"),
        format!(
            "id: \"{ticket}\"\ntype: \"ticket\"\nproject: \"prj-8m2k4d7q\"\nprofile: \"strict\"\nconfig: \"{config}\"\nattributes: {{ \"code\": \"007\", \"risk\": \"high\", \"size\": 3, \"urgent\": true }}\nexternal:\n  - {{ system: \"github\", key: \"8798\", url: \"https://github.com/o/r/issues/8798\" }}\n"
        )
    );
    let hex = config.trim_start_matches("sha256:");
    assert_eq!(
        std::fs::read(project.path().join(format!(".hone/snapshots/{hex}.json")))
            .expect("the snapshot"),
        std::fs::read(project.path().join(".hone/resolved.json")).expect("resolved"),
        "the snapshot is the current resolved configuration"
    );

    let (_, checked) = project.json(&["check", "--ticket", &ticket]);
    let codes: Vec<&str> = checked["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .flat_map(|gate| gate["findings"].as_array().into_iter().flatten())
        .filter_map(|finding| finding["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        ["step-incomplete"],
        "the record is valid and the profile is the selected one"
    );
}

#[test]
fn a_profile_against_select_is_kept_visible() {
    let project = Project::new("profile");
    let (code, started) = project.json(&with_author(&[
        "ticket",
        "start",
        "--attribute",
        "risk=high",
        "--profile",
        "only",
    ]));
    assert_eq!(code, Some(0), "{started}");
    assert_eq!(started["profile"], "only");
    assert_eq!(started["selected"], "strict");
    let ticket = started["ticket"].as_str().expect("an ID");
    let (_, checked) = project.json(&["check", "--ticket", ticket]);
    let warnings: Vec<Value> = checked["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .flat_map(|gate| gate["findings"].as_array().cloned().unwrap_or_default())
        .filter(|finding| finding["level"] == "warning")
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0]["code"], "profile-not-selected");
    assert_eq!(
        warnings[0]["message"],
        "the ticket uses the profile `only`, `select` gives `strict` for its attributes"
    );
}

#[test]
fn start_refuses_before_writing_anything() {
    let project = Project::new("refused");
    let refusals = [
        (
            with_author(&["ticket", "start", "--profile", "lite"]),
            "the configuration has no profile `lite`",
        ),
        (
            vec!["ticket", "start", "--author", "bob", "--role", "reviewer"],
            "the role `reviewer` does not exist in the ticket's configuration",
        ),
        (
            with_author(&["ticket", "start", "--attribute", "risk"]),
            "`risk` is not `<key>=<value>`",
        ),
        (
            with_author(&["ticket", "start", "--external", "github"]),
            "`github` is not `<system>:<key>[=<url>]`",
        ),
    ];
    for (arguments, message) in refusals {
        let (code, refused) = project.json(&arguments);
        assert_eq!(code, Some(1), "{arguments:?}");
        assert_eq!(refused, json!({ "status": "invalid", "message": message }));
    }
    assert!(project.written().is_empty(), "{:?}", project.written());

    std::fs::write(
        project.path().join(".hone/profiles/only.yaml"),
        "steps: { work: required, finish: required }\n",
    )
    .expect("writable");
    let (code, stale) = project.json(&with_author(&["ticket", "start"]));
    assert_eq!(code, Some(1));
    assert_eq!(
        stale,
        json!({ "status": "invalid", "message": ".hone/resolved.json does not match its sources: run hone resolve" })
    );
    assert!(project.written().is_empty(), "{:?}", project.written());
}

#[test]
fn set_attributes_rewrites_only_the_attributes_and_keeps_the_profile() {
    let project = Project::new("attributes");
    let (_, started) = project.json(&with_author(&[
        "ticket",
        "start",
        "--attribute",
        "risk=high",
        "--attribute",
        "size=3",
    ]));
    let ticket = started["ticket"].as_str().expect("an ID").to_owned();
    let record = project.path().join(format!("tickets/{ticket}.yaml"));
    let before = std::fs::read_to_string(&record).expect("readable");

    let (code, changed) = project.json(&with_author(&[
        "ticket",
        "set-attributes",
        "--ticket",
        &ticket,
        "--attribute",
        "risk=low",
        "--unset",
        "size",
    ]));
    assert_eq!(code, Some(0), "{changed}");
    assert_eq!(changed["profile"], "strict", "the profile stays");
    assert_eq!(changed["selected"], "only");
    assert_eq!(changed["event"]["type"], "ticket.attributes_changed");
    assert_eq!(
        changed["event"]["payload"],
        json!({ "attributes": { "risk": "low" } })
    );
    let expected = before.replace("attributes: { \"risk\": \"high\", \"size\": 3 }\n", "")
        + "attributes: { \"risk\": \"low\" }\n";
    assert_eq!(
        std::fs::read_to_string(&record).expect("readable"),
        expected
    );

    let (code, unknown) = project.json(&with_author(&[
        "ticket",
        "set-attributes",
        "--ticket",
        "tk-00000000",
        "--attribute",
        "risk=low",
    ]));
    assert_eq!(code, Some(1));
    assert_eq!(unknown["status"], "invalid");
}

#[test]
fn switch_config_freezes_the_current_rules_and_refuses_a_no_op() {
    let project = Project::new("switch");
    let (_, started) = project.json(&with_author(&["ticket", "start"]));
    let ticket = started["ticket"].as_str().expect("an ID").to_owned();
    let old = started["config"].as_str().expect("a hash").to_owned();

    let (code, same) = project.json(&with_author(&[
        "ticket",
        "switch-config",
        "--ticket",
        &ticket,
    ]));
    assert_eq!(code, Some(1));
    assert_eq!(
        same["message"],
        format!("the ticket `{ticket}` already uses the current configuration")
    );

    std::fs::write(
        project.path().join(".hone/profiles/only.yaml"),
        "steps: { work: required, finish: required }\n",
    )
    .expect("writable");
    assert_eq!(project.hone(&["resolve"]).status.code(), Some(0));
    let new = config_hash(&project);
    assert_ne!(new, old);
    let (code, switched) = project.json(&with_author(&[
        "ticket",
        "switch-config",
        "--ticket",
        &ticket,
    ]));
    assert_eq!(code, Some(0), "{switched}");
    assert_eq!(switched["config"], new.as_str());
    assert_eq!(switched["event"]["type"], "ticket.config_changed");
    assert_eq!(switched["event"]["payload"], json!({ "config": new }));
    let record = std::fs::read_to_string(project.path().join(format!("tickets/{ticket}.yaml")))
        .expect("readable");
    assert!(
        record.ends_with(&format!("config: \"{new}\"\n")),
        "{record}"
    );
    assert!(!record.contains(&old), "{record}");
    let hex = new.trim_start_matches("sha256:");
    assert!(
        project
            .path()
            .join(format!(".hone/snapshots/{hex}.json"))
            .is_file()
    );
}
