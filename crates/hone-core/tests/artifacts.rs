//! The artifact test cases under `schemas/tests/` (concept Section 14): single files checked by
//! their content, and the content hash.

use std::path::{Path, PathBuf};

use hone_core::{Code, Finding, Level, Resolved, check_artifact, content_hash, parse_yaml};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn json(path: &Path) -> Value {
    serde_json::from_slice(&read(path))
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

// The kinds of the standard package and of the project layer `schemas/tests/kinds/`, embedded
// as `resolve` would, in a valid resolved configuration.
fn config() -> Resolved {
    let mut document = json(&schemas().join("tests/config/valid/base.json"));
    let mut kinds = serde_json::Map::new();
    for layer in [schemas().join("standard"), schemas().join("tests")] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(layer.join("kinds"))
            .expect("kinds directory")
            .map(|entry| entry.expect("entry").path())
            .collect();
        files.sort();
        for file in files {
            let mut kind = parse_yaml(&read(&file))
                .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
            if let Some(schema) = kind.get("schema").and_then(Value::as_str) {
                kind["schema"] = json(&layer.join(schema));
            }
            let name = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("UTF-8 name");
            kinds.insert(name.to_owned(), kind);
        }
    }
    document["kinds"] = Value::Object(kinds);
    Resolved::from_document(document).unwrap_or_else(|error| panic!("kinds: {error}"))
}

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.file_name().is_some_and(|name| name != "expected.txt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no files in {}", directory.display());
    files
}

fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 name")
        .to_owned()
}

fn findings(config: &Resolved, path: &Path) -> Vec<Finding> {
    check_artifact(config, &name(path), &read(path))
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.level == Level::Error)
        .collect()
}

// `<file> <code> <pointer>` per line, `-` for no pointer.
fn expectations(directory: &Path) -> Vec<(String, String, String)> {
    let text = String::from_utf8(read(&directory.join("expected.txt"))).expect("UTF-8");
    text.lines()
        .map(|line| {
            let mut parts = line.split(' ');
            let mut next = || parts.next().unwrap_or("-").to_owned();
            (next(), next(), next())
        })
        .collect()
}

fn summary(findings: &[&Finding]) -> String {
    match findings.first() {
        Some(first) => format!("{} error(s), first: {first}", findings.len()),
        None => "no error".into(),
    }
}

#[test]
fn valid_files_pass() {
    let config = config();
    let mut failures = Vec::new();
    for path in files(&schemas().join("tests/valid")) {
        let found = findings(&config, &path);
        let errors = errors(&found);
        println!("{}: {}", name(&path), summary(&errors));
        if !errors.is_empty() {
            failures.push(format!("{}: {}", name(&path), summary(&errors)));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn unknown_fields_are_warnings() {
    let path = schemas().join("tests/valid/extensions-and-unknown-fields.md");
    let warnings: Vec<String> = findings(&config(), &path)
        .into_iter()
        .filter(|finding| finding.code == Code::UnknownField)
        .map(|finding| finding.pointer.unwrap_or_default())
        .collect();
    assert_eq!(warnings, ["/future-field"]);
}

fn expected_failures(directory: &str, profile_must_pass: bool) {
    let config = config();
    let directory = schemas().join("tests").join(directory);
    let expected = expectations(&directory);
    let names: Vec<String> = files(&directory).iter().map(|path| name(path)).collect();
    let listed: Vec<&String> = expected.iter().map(|(file, _, _)| file).collect();
    assert_eq!(
        names.iter().collect::<Vec<_>>(),
        listed,
        "expected.txt lists every file"
    );
    let mut failures = Vec::new();
    for (file, code, pointer) in &expected {
        let found = findings(&config, &directory.join(file));
        let errors = errors(&found);
        println!("{file}: {}", summary(&errors));
        let matches = errors.iter().any(|finding| {
            finding.code.name() == code && finding.pointer.as_deref().unwrap_or("-") == pointer
        });
        let profile_failed = errors
            .iter()
            .any(|finding| finding.code == Code::YamlProfile);
        if !matches || (profile_must_pass && profile_failed) {
            failures.push(format!(
                "{file}: expected {code} {pointer}, got {}",
                summary(&errors)
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn invalid_files_fail_for_the_expected_reason() {
    expected_failures("invalid", true);
}

#[test]
fn profile_violations_fail_the_profile() {
    expected_failures("profile/invalid", false);
}

fn split(text: &str) -> (&str, &str) {
    let end = text[4..].find("\n---\n").expect("front matter") + 4 + 5;
    text.split_at(end)
}

#[test]
fn content_hash_covers_fields_and_text_but_not_notation() {
    let source = String::from_utf8(read(&schemas().join("tests/valid/decision-accepted.md")))
        .expect("UTF-8");
    let hash = |text: &str| content_hash("decision.md", text.as_bytes()).expect("profile");
    let stored = "sha256:6707fdc5fdd423057322a6f4d934968ab60848484affd95ee104d31c38905d6e";
    let original = hash(&source);
    assert_eq!(
        original, stored,
        "the approval in the review block matches the content"
    );

    let reformatted =
        String::from_utf8(read(&schemas().join("tests/hash/decision-reformatted.md")))
            .expect("UTF-8");
    assert_eq!(
        hash(&reformatted),
        stored,
        "YAML notation does not change the hash"
    );
    assert_eq!(
        hash(&source.replace('\n', "\r\n")),
        stored,
        "CRLF does not change the hash"
    );
    assert_eq!(
        hash(&source.replace("role: architect", "role: human")),
        stored,
        "the review block is not part of the hash"
    );
    assert_ne!(
        hash(&source.replace("status: accepted", "status: rejected")),
        stored,
        "a field changes the hash"
    );
    assert_ne!(
        hash(&format!("{source}\nOne more sentence.")),
        stored,
        "the body changes the hash"
    );

    let (front, body) = split(&source);
    let trailing = body.replace('\n', "  \t\n");
    assert_eq!(
        hash(&format!("{front}{trailing}")),
        stored,
        "trailing blanks do not count"
    );
    let decomposed: String = body.nfd().collect();
    assert_ne!(decomposed, body);
    assert_eq!(
        hash(&format!("{front}{decomposed}")),
        stored,
        "NFD and NFC hash the same"
    );
    let space = body.find("Synchronous processing").expect("sentence") + "Synchronous".len();
    let rewrapped = format!("{}\n{}", &body[..space], &body[space + 1..]);
    assert_ne!(
        hash(&format!("{front}{rewrapped}")),
        stored,
        "a new line break changes the hash"
    );
}

#[test]
fn every_language_package_ships_the_module_of_its_test_case() {
    let cases = [
        ("rust", "module-anchors-resolve-resolved", vec!["rs"]),
        (
            "typescript",
            "language-typescript",
            vec!["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        ),
        ("python", "language-python", vec!["py", "pyi"]),
        ("php", "language-php", vec!["php"]),
        ("dart", "language-dart", vec!["dart"]),
    ];
    for (language, case, extensions) in cases {
        let name = format!("lang-{language}");
        let shipped = read(&schemas().join(format!("{name}/plugins/{name}.wasm")));
        let tested = read(&schemas().join(format!(
            "tests/check/{case}/project/.hone/plugins/{name}.wasm"
        )));
        assert!(
            shipped == tested,
            "{name}: the package and its test case use the same module"
        );
        let definition = parse_yaml(&read(
            &schemas().join(format!("{name}/plugins/{name}.yaml")),
        ))
        .expect("strict YAML profile");
        assert_eq!(
            definition["language"]["extensions"],
            serde_json::json!(extensions),
            "{name}"
        );
    }
}
