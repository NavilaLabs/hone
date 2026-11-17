//! The configuration fixtures under `schemas/tests/config/`: resolved documents that must pass,
//! fail the shape check, or fail the reference check.

use std::path::{Path, PathBuf};

use hone_core::{Category, Resolved};

fn fixtures(group: &str) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/tests/config")
        .join(group);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("readable entry").path())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", directory.display());
    paths
}

fn check(path: &Path) -> Result<Resolved, hone_core::ResolveError> {
    let text = std::fs::read_to_string(path).expect("readable fixture");
    Resolved::from_document(serde_json::from_str(&text).expect("fixture is JSON"))
}

#[test]
fn valid_configurations_pass() {
    for path in fixtures("valid") {
        if let Err(error) = check(&path) {
            panic!("{}: {error}", path.display());
        }
    }
}

#[test]
fn invalid_configurations_fail_the_shape() {
    expect_failure("invalid", Category::Shape);
}

#[test]
fn invalid_references_fail_only_the_reference_check() {
    expect_failure("invalid-refs", Category::Reference);
}

fn expect_failure(group: &str, category: Category) {
    for path in fixtures(group) {
        let name = path.file_name().expect("file name").to_string_lossy();
        match check(&path) {
            Ok(_) => panic!("{name}: was wrongly accepted"),
            Err(error) => {
                assert!(
                    error.problems.iter().all(|p| p.category == category),
                    "{name}: expected only `{category}`, got:\n{error}"
                );
                println!("{group}/{name}: {}", error.problems[0]);
            }
        }
    }
}
