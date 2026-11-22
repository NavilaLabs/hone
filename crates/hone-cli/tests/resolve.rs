//! `hone resolve --show` (concept Section 12, "Showing the resolved configuration").

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn case(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/tests/resolve")
        .join(name)
}

fn hone(case: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hone"))
        .args(["resolve", "--root"])
        .arg(case.join("project"))
        .arg("--packages")
        .arg(case.join("packages"))
        .args(arguments)
        .output()
        .expect("hone runs")
}

#[test]
fn show_prints_the_configuration_with_its_layers() {
    let case = case("null-removal");
    let output = hone(&case, &["--show", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let shown: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    assert_eq!(shown["status"], "shown");
    assert_eq!(shown["file"], "out-of-date");
    assert_eq!(
        shown["origins"]["steps"]["decision"],
        json!([{ "kind": "package", "name": "acme/base" }, { "kind": "project" }])
    );
    assert_eq!(shown["config"]["protocol"], 1);
    assert_eq!(shown["lock"]["packages"]["acme/base"]["version"], "1.0.0");
    assert!(
        !case.join("project/.hone/resolved.json").exists(),
        "nothing is written"
    );

    let output = hone(&case, &["--show"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8"),
        "sha256:06425703822e17cfb9101e73544c0549a2454a3d8f1b38a321f5682fbc8d0fd7 (.hone/resolved.json is out of date)\n\
         actors  -\n\
         kinds.decision  acme/base, project\n\
         profiles.standard  acme/base\n\
         roles.human  acme/base\n\
         select  acme/base\n\
         steps.decision  acme/base, project\n\
         ticket  acme/base\n"
    );
}

#[test]
fn a_failed_resolution_prints_its_problems_and_the_lockfile() {
    let output = hone(&case("error-plugin-hash"), &["--show", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let failed: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["problems"][0]["category"], "plugin-hash");
    assert_eq!(
        failed["lock"]["plugins"]["api-check"]["hash"],
        "sha256:1111111111111111111111111111111111111111111111111111111111111111"
    );
}

#[test]
fn show_excludes_check_and_ticket_needs_show() {
    let case = case("null-removal");
    for (arguments, message) in [
        (
            vec!["--show", "--check"],
            "the argument '--show' cannot be used with '--check'",
        ),
        (
            vec!["--ticket", "tk-9f2x4a8m"],
            "the following required arguments were not provided:\n  --show",
        ),
    ] {
        let output = hone(&case, &arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        let stderr = String::from_utf8(output.stderr).expect("UTF-8");
        assert!(stderr.contains(message), "{arguments:?}: {stderr}");
    }
}
