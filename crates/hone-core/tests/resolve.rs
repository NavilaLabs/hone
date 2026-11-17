//! The resolution test cases under `schemas/tests/resolve/` (concept Section 14).

use std::path::{Path, PathBuf};

use hone_core::{Location, Origin, PackageDirectory, Problem, Resolved, resolve};
use serde_json::Value;

fn cases() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/tests/resolve");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no cases in {}", directory.display());
    cases
}

// The location as `expected-error.txt` writes it: a JSON Pointer, or a path relative to the
// case directory.
fn render(location: &Location) -> String {
    let join = |root: String, path: &str| {
        if path.is_empty() {
            root
        } else {
            format!("{root}/{path}")
        }
    };
    match location {
        Location::Pointer(pointer) => pointer.clone(),
        Location::File {
            origin: Origin::Project,
            path,
        } => join("project/.hone".into(), path),
        Location::File {
            origin: Origin::Package(name),
            path,
        } => join(format!("packages/{name}"), path),
    }
}

fn render_problem(problem: &Problem) -> String {
    format!("{} {}", problem.category, render(&problem.location))
}

fn read_overrides(case: &Path) -> Option<Value> {
    let path = case.join("overrides.yaml");
    path.exists().then(|| {
        let bytes = std::fs::read(&path).expect("readable overrides");
        hone_core::parse_yaml(&bytes).unwrap_or_else(|error| panic!("overrides: {error}"))
    })
}

#[test]
fn resolution_cases() {
    let mut failures = Vec::new();
    for case in cases() {
        let name = case
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let packages = PackageDirectory::new(case.join("packages"));
        let overrides = read_overrides(&case);
        let result = resolve(&case.join("project"), &packages, overrides.as_ref());
        let expected_document = case.join("expected.json");
        let expected_error = case.join("expected-error.txt");
        match (expected_document.exists(), expected_error.exists(), result) {
            (true, false, Ok(resolved)) => {
                let text = std::fs::read_to_string(&expected_document).expect("readable");
                let expected: Value = serde_json::from_str(&text).expect("expected.json is JSON");
                if let Err(error) = Resolved::from_document(expected.clone()) {
                    failures.push(format!("{name}: expected.json is not valid:\n{error}"));
                } else if *resolved.document() != expected {
                    failures.push(format!(
                        "{name}: document differs\n  got:      {}\n  expected: {}",
                        resolved.document(),
                        expected
                    ));
                } else {
                    println!("{name}: ok");
                }
            }
            (true, false, Err(error)) => failures.push(format!("{name}: failed:\n{error}")),
            (false, true, result) => {
                let expected = std::fs::read_to_string(&expected_error).expect("readable");
                let expected = expected.trim_end();
                match result {
                    Ok(_) => failures.push(format!("{name}: resolved, expected `{expected}`")),
                    Err(error) => {
                        let reported: Vec<String> =
                            error.problems.iter().map(render_problem).collect();
                        if reported.iter().any(|line| line == expected) {
                            println!(
                                "{name}: {} problem(s): {}",
                                error.problems.len(),
                                error.problems[0]
                            );
                        } else {
                            failures.push(format!(
                                "{name}: expected `{expected}`, got:\n  {}",
                                reported.join("\n  ")
                            ));
                        }
                    }
                }
            }
            _ => failures.push(format!(
                "{name}: needs exactly one of expected.json and expected-error.txt"
            )),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
