//! The gate test cases under `schemas/tests/check/` (concept Section 14).

use std::path::{Path, PathBuf};

use hone_core::{CheckReport, CheckRequest, GateClass, Level, check, check_edit, parse_yaml};
use serde_json::Value;

fn cases_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/tests/check")
}

fn case(name: &str) -> PathBuf {
    cases_directory().join(name)
}

fn cases() -> Vec<PathBuf> {
    let directory = cases_directory();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no cases in {}", directory.display());
    cases
}

fn request(case: &Path) -> CheckRequest {
    let path = case.join("input.yaml");
    if !path.exists() {
        return CheckRequest::default();
    }
    let input = parse_yaml(&std::fs::read(&path).expect("readable input"))
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let text = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_owned);
    CheckRequest {
        ticket: text("ticket"),
        step: text("step"),
        changed_paths: input.get("changed_paths").map(|paths| {
            paths
                .as_array()
                .expect("changed_paths is a list")
                .iter()
                .map(|path| path.as_str().expect("a path is a string").to_owned())
                .collect()
        }),
        class: None,
        data_directory: Some(case.join("data")),
    }
}

// The lines of `expected.txt`: `<level> <gate> <code> <path> <pointer>`, sorted.
fn render(report: &CheckReport) -> Vec<String> {
    let mut lines: Vec<String> = report
        .gates
        .iter()
        .flat_map(|gate| {
            gate.findings.iter().map(|finding| {
                format!(
                    "{} {} {} {} {}",
                    finding.level,
                    gate.name,
                    finding.code,
                    finding.path.as_deref().unwrap_or("-"),
                    finding.pointer.as_deref().unwrap_or("-"),
                )
            })
        })
        .collect();
    lines.sort();
    lines
}

fn expected(case: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(case.join("expected.txt")).expect("expected.txt");
    text.lines().map(str::to_owned).collect()
}

#[test]
fn gate_cases() {
    let mut failures = Vec::new();
    for case in cases() {
        let name = case
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 name")
            .to_owned();
        let expected = expected(&case);
        let errors = expected
            .iter()
            .filter(|line| line.starts_with("error "))
            .count();
        assert!(
            errors <= 1 || name == "ids-unique-duplicate",
            "{name}: a failing case has one error"
        );
        match check(&case.join("project"), &request(&case)) {
            Ok(report) => {
                let actual = render(&report);
                let error_count = actual
                    .iter()
                    .filter(|line| line.starts_with("error "))
                    .count();
                println!(
                    "{name}: {error_count} error(s), first: {}",
                    actual.first().map_or("none", String::as_str)
                );
                if actual != expected {
                    failures.push(format!(
                        "{name}:\n  expected {expected:?}\n  actual   {actual:?}"
                    ));
                }
                if report.passed() != (errors == 0) {
                    failures.push(format!("{name}: passed() disagrees with the findings"));
                }
            }
            Err(error) => failures.push(format!("{name}: could not run: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn class_exec_runs_only_exec_gates() {
    let request = CheckRequest {
        class: Some(GateClass::Exec),
        step: Some("interfaces".into()),
        ..request(&case("exec-unsupported"))
    };
    let report = check(&case("exec-unsupported/project"), &request).expect("runs");
    let names: Vec<&str> = report.gates.iter().map(|gate| gate.name.as_str()).collect();
    assert_eq!(names, ["interfaces/compiles"]);
}

#[test]
fn class_pure_skips_exec_gates() {
    let request = CheckRequest {
        class: Some(GateClass::Pure),
        ..request(&case("exec-unsupported"))
    };
    let report = check(&case("exec-unsupported/project"), &request).expect("runs");
    assert!(report.passed(), "{report:?}");
    assert!(report.gates.iter().all(|gate| gate.gate != "exec"));
}

#[test]
fn a_step_needs_a_ticket() {
    let request = CheckRequest {
        step: Some("specify".into()),
        ..CheckRequest::default()
    };
    assert!(check(&case("pass-directory/project"), &request).is_err());
}

#[test]
fn a_step_outside_the_profile_is_an_error() {
    let request = CheckRequest {
        step: Some("nonexistent".into()),
        ..request(&case("pass-ticket"))
    };
    let error = check(&case("pass-ticket/project"), &request).expect_err("unknown step");
    assert!(error.message.contains("nonexistent"), "{error}");
}

fn copy_directory(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create directory");
    for entry in std::fs::read_dir(from).expect("readable directory") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

fn scratch_copy(name: &str, label: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!("hone-check-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target);
    copy_directory(&case(name).join("project"), &target);
    target
}

fn snapshot_file(project: &Path) -> PathBuf {
    std::fs::read_dir(project.join(".hone/snapshots"))
        .expect("snapshots")
        .next()
        .expect("one snapshot")
        .expect("entry")
        .path()
}

#[test]
fn a_missing_snapshot_stops_the_check() {
    let project = scratch_copy("pass-ticket", "missing-snapshot");
    std::fs::remove_file(snapshot_file(&project)).expect("remove");
    let error = check(&project, &request(&case("pass-ticket"))).expect_err("no snapshot");
    assert!(error.message.contains("snapshots"), "{error}");
    std::fs::remove_dir_all(project).expect("clean up");
}

#[test]
fn a_snapshot_with_another_hash_stops_the_check() {
    let project = scratch_copy("pass-ticket", "changed-snapshot");
    let path = snapshot_file(&project);
    let text = std::fs::read_to_string(&path).expect("snapshot");
    std::fs::write(&path, text.replace("\"archive\"", "\"keep\"")).expect("write");
    let error = check(&project, &request(&case("pass-ticket"))).expect_err("hash differs");
    assert!(error.message.contains("hash"), "{error}");
    std::fs::remove_dir_all(project).expect("clean up");
}

#[test]
fn a_missing_resolved_configuration_stops_the_check() {
    let project = scratch_copy("pass-directory", "missing-resolved");
    std::fs::remove_file(project.join(".hone/resolved.json")).expect("remove");
    assert!(check(&project, &CheckRequest::default()).is_err());
    std::fs::remove_dir_all(project).expect("clean up");
}

#[test]
fn an_edit_of_code_without_a_binding_decision_fails() {
    let project = case("change-needs-binding-missing/project");
    let report = check_edit(&project, "tk-9f2x4a8m", "src/payments/fee.rs").expect("runs");
    assert!(!report.passed());
    let names: Vec<&str> = report.gates.iter().map(|gate| gate.name.as_str()).collect();
    assert_eq!(names, ["implementation/binding"]);
}

#[test]
fn an_edit_of_unanchored_code_passes() {
    let project = case("change-needs-binding-missing/project");
    let report = check_edit(&project, "tk-9f2x4a8m", "src/ledger/export.rs").expect("runs");
    assert!(report.passed());
}

#[test]
fn an_edit_of_an_artifact_is_never_checked() {
    let project = case("schema-valid-schema/project");
    let report = check_edit(
        &project,
        "tk-9f2x4a8m",
        "design/decisions/dec-4h7r2x9b-no-authors.md",
    )
    .expect("runs");
    assert!(report.gates.is_empty());
}

#[test]
fn warnings_do_not_fail_a_gate() {
    let report = check(
        &case("schema-valid-unknown-field/project"),
        &CheckRequest::default(),
    )
    .expect("runs");
    assert!(report.passed());
    assert!(
        report
            .gates
            .iter()
            .any(|gate| gate.findings.iter().any(|f| f.level == Level::Warning))
    );
}
