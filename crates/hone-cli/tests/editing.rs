//! `hone artifact show/save/check/new` and `hone file show`, the commands of an artifact editor and a
//! code view (concept Section 12, "Desktop GUI").

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/check/no-open-blocking-threads-open/project";
const DECISION: &str = "design/decisions/dec-7k3m2q8p-payment-async.md";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copyable");
        }
    }
}

// A copy of a gate test project, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-cli-editing-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(&repository().join(CASE), &root.join("project"));
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn hone(&self, arguments: &[&str], input: Option<&str>) -> (Option<i32>, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hone"))
            .args(arguments)
            .arg("--root")
            .arg(self.project())
            .arg("--json")
            .env("HONE_CONFIG_DIR", self.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("hone runs");
        let mut stdin = child.stdin.take().expect("a pipe");
        stdin
            .write_all(input.unwrap_or_default().as_bytes())
            .expect("writable");
        drop(stdin);
        let output: Output = child.wait_with_output().expect("hone ends");
        let value = serde_json::from_slice(&output.stdout).expect("one JSON document");
        (output.status.code(), value)
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.project().join(path)).expect("readable")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn today() -> String {
    let output = Command::new("date")
        .args(["-u", "+%F"])
        .output()
        .expect("date runs");
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn show_gives_the_text_the_data_and_what_the_form_needs() {
    let scratch = Scratch::new("show");
    let (code, value) = scratch.hone(&["artifact", "show", "--id", "dec-7k3m2q8p"], None);
    assert_eq!(code, Some(0));
    let text = scratch.read(DECISION);
    assert_eq!(
        value,
        json!({
            "id": "dec-7k3m2q8p",
            "type": "decision",
            "path": DECISION,
            "text": text,
            "data": {
                "id": "dec-7k3m2q8p",
                "type": "decision",
                "ticket": "tk-9f2x4a8m",
                "status": "accepted",
                "created": "2026-11-16",
                "authors": ["alice"],
                "links": { "affects": ["mod-4t8w2n6c"] },
                "review": { "approved_by": [{
                    "name": "alice",
                    "role": "architect",
                    "hash": "sha256:678e7bc1cacb3c07fe77369e22fb499f58067c31ad2eeb66b929a32ce8ce45f6",
                    "date": "2026-11-16"
                }] }
            },
            "body": "Text.\n",
            "lifecycle": "decided",
            "statuses": [
                { "name": "accepted", "binding": true },
                { "name": "proposed", "binding": false },
                { "name": "rejected", "binding": false }
            ],
            "link_types": ["supersedes", "affects", "verifies", "depends-on", "relates"]
        })
    );
    let (code, value) = scratch.hone(&["artifact", "show", "--id", "tk-9f2x4a8m"], None);
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({ "status": "error", "message": "no artifact `tk-9f2x4a8m` in the project" })
    );
}

#[test]
fn kinds_name_what_a_new_artifact_needs() {
    let scratch = Scratch::new("kinds");
    let (code, value) = scratch.hone(&["artifact", "kinds"], None);
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!([
            { "type": "decision", "lifecycle": "decided", "id_prefix": "dec", "path": "design/decisions/{id}-{slug}.md", "first_status": "proposed", "required": [] },
            { "type": "module", "lifecycle": "living", "id_prefix": "mod", "path": "design/modules/{id}-{slug}.yaml", "required": [] },
            { "type": "spec", "lifecycle": "decided", "id_prefix": "spec", "path": "design/tickets/{ticket}/{id}-{slug}.md", "first_status": "proposed", "required": [] }
        ])
    );
}

#[test]
fn save_writes_without_errors_and_keeps_warnings() {
    let scratch = Scratch::new("save");
    let changed = scratch
        .read(DECISION)
        .replace("status: \"accepted\"", "status: \"proposed\"")
        .replace("Text.", "New text.");
    let (code, value) = scratch.hone(&["artifact", "save", "--path", DECISION], Some(&changed));
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({ "id": "dec-7k3m2q8p", "path": DECISION, "saved": true, "findings": [] })
    );
    assert_eq!(scratch.read(DECISION), changed);

    let unknown = changed.replace("authors:", "mood: \"calm\"\nauthors:");
    let (code, value) = scratch.hone(&["artifact", "save", "--path", DECISION], Some(&unknown));
    assert_eq!(code, Some(0));
    assert_eq!(
        value["findings"],
        json!([{ "level": "warning", "code": "unknown-field", "message": "unknown field `mood`", "path": DECISION, "pointer": "/mood" }])
    );
    assert_eq!(scratch.read(DECISION), unknown);
}

#[test]
fn save_refuses_errors_and_writes_nothing() {
    let scratch = Scratch::new("refuse");
    let original = scratch.read(DECISION);
    let bogus = original.replace("status: \"accepted\"", "status: \"bogus\"");
    let (code, value) = scratch.hone(&["artifact", "save", "--path", DECISION], Some(&bogus));
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({
            "id": "dec-7k3m2q8p",
            "path": DECISION,
            "saved": false,
            "findings": [{ "level": "error", "code": "status", "message": "`bogus` is not a status of the kind", "path": DECISION, "pointer": "/status" }]
        })
    );
    let renamed = original.replace("id: \"dec-7k3m2q8p\"", "id: \"dec-2h6r9t1x\"");
    let (code, value) = scratch.hone(&["artifact", "save", "--path", DECISION], Some(&renamed));
    assert_eq!(code, Some(1));
    assert_eq!(
        value["findings"],
        json!([{ "level": "error", "code": "path-mismatch", "message": "the path says `dec-7k3m2q8p`, the file says `dec-2h6r9t1x`", "path": DECISION, "pointer": "/id" }])
    );
    let (code, value) = scratch.hone(
        &["artifact", "save", "--path", DECISION],
        Some("no front matter\n"),
    );
    assert_eq!(code, Some(1));
    assert_eq!(value["findings"][0]["code"], "yaml-profile");
    assert_eq!(scratch.read(DECISION), original);

    for (path, message) in [
        (
            "../outside.md",
            "`../outside.md` is not a path relative to the project root",
        ),
        (
            "notes/readme.md",
            "`notes/readme.md` matches no artifact pattern",
        ),
        (
            "design/tickets/tk-9f2x4a8m/ticket.yaml",
            "`design/tickets/tk-9f2x4a8m/ticket.yaml` is a ticket record: change it with hone ticket set-attributes",
        ),
    ] {
        let (code, value) = scratch.hone(&["artifact", "save", "--path", path], Some(&original));
        assert_eq!(code, Some(2), "{path}");
        assert_eq!(value, json!({ "status": "error", "message": message }));
    }
    assert!(!scratch.project().join("notes").exists());
    assert!(!scratch.root.join("outside.md").exists());
}

#[test]
fn check_finds_errors_and_unresolved_links_of_the_text_and_writes_nothing() {
    let scratch = Scratch::new("check");
    let original = scratch.read(DECISION);
    let edited = original
        .replace("status: \"accepted\"", "status: \"bogus\"")
        .replace("[\"mod-4t8w2n6c\"]", "[\"mod-4t8w2n6c\", \"mod-9c3d6h4v\"]");
    let (code, value) = scratch.hone(&["artifact", "check", "--path", DECISION], Some(&edited));
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({
            "id": "dec-7k3m2q8p",
            "path": DECISION,
            "findings": [
                { "level": "error", "code": "status", "message": "`bogus` is not a status of the kind", "path": DECISION, "pointer": "/status" },
                { "level": "error", "code": "link-unresolved", "message": "no artifact `mod-9c3d6h4v`", "path": DECISION, "pointer": "/links/affects/1" },
            ]
        })
    );
    assert_eq!(scratch.read(DECISION), original);

    // The file on disk links nowhere; the checked text replaces it, so only the text counts.
    let broken = original.replace("[\"mod-4t8w2n6c\"]", "[\"mod-9c3d6h4v\"]");
    std::fs::write(scratch.project().join(DECISION), &broken).expect("writable");
    let (code, value) = scratch.hone(&["artifact", "check", "--path", DECISION], Some(&original));
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({ "id": "dec-7k3m2q8p", "path": DECISION, "findings": [] })
    );

    // A file that does not exist yet is checked against the directory as well.
    let draft = "design/decisions/dec-2h6r9t1x-retry.md";
    let text = original
        .replace("dec-7k3m2q8p", "dec-2h6r9t1x")
        .replace("tk-9f2x4a8m", "tk-7m2k4d8q");
    let (code, value) = scratch.hone(&["artifact", "check", "--path", draft], Some(&text));
    assert_eq!(code, Some(1));
    assert_eq!(
        value["findings"],
        json!([{ "level": "error", "code": "link-unresolved", "message": "no ticket record `tk-7m2k4d8q`", "path": draft, "pointer": "/ticket" }])
    );
    assert!(!scratch.project().join(draft).exists());

    let (code, value) = scratch.hone(
        &[
            "artifact",
            "check",
            "--path",
            "design/tickets/tk-9f2x4a8m/ticket.yaml",
        ],
        Some(&original),
    );
    assert_eq!(code, Some(2));
    assert_eq!(
        value,
        json!({ "status": "error", "message": "`design/tickets/tk-9f2x4a8m/ticket.yaml` is a ticket record: change it with hone ticket set-attributes" })
    );
}

#[cfg(unix)]
#[test]
fn save_does_not_write_through_a_planted_link() {
    let scratch = Scratch::new("link");
    let outside = scratch.root.join("outside");
    std::fs::create_dir_all(&outside).expect("creatable");
    let decisions = scratch.project().join("design/decisions");
    std::fs::rename(&decisions, outside.join("decisions")).expect("movable");
    std::os::unix::fs::symlink(outside.join("decisions"), &decisions).expect("a link");
    let original = std::fs::read_to_string(outside.join("decisions/dec-7k3m2q8p-payment-async.md"))
        .expect("readable");
    let changed = original.replace("Text.", "Changed.");

    let (code, value) = scratch.hone(&["artifact", "save", "--path", DECISION], Some(&changed));
    assert_eq!(code, Some(2));
    assert_eq!(
        value,
        json!({ "status": "error", "message": format!("{DECISION}: the directory leads outside the project root") })
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("decisions/dec-7k3m2q8p-payment-async.md"))
            .expect("readable"),
        original
    );
    let (code, value) = scratch.hone(&["file", "show", "--path", DECISION], None);
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({ "status": "error", "message": format!("`{DECISION}` is not a regular file below the project root") })
    );
}

#[test]
fn new_creates_an_artifact_with_the_fields_of_its_lifecycle() {
    let scratch = Scratch::new("new");
    let (code, value) = scratch.hone(
        &[
            "artifact",
            "new",
            "--kind",
            "decision",
            "--ticket",
            "tk-9f2x4a8m",
            "--title",
            "Retry with backoff!",
            "--author",
            "bob",
        ],
        None,
    );
    assert_eq!(code, Some(0), "{value}");
    let id = value["id"].as_str().expect("an ID").to_owned();
    assert!(
        id.len() == 12
            && id.starts_with("dec-")
            && id[4..]
                .chars()
                .all(|c| "0123456789abcdefghjkmnpqrstvwxyz".contains(c)),
        "{id}"
    );
    let path = format!("design/decisions/{id}-retry-with-backoff.md");
    assert_eq!(
        value,
        json!({ "id": id, "path": path, "saved": true, "findings": [] })
    );
    assert_eq!(
        scratch.read(&path),
        format!(
            "---\nid: \"{id}\"\ntype: \"decision\"\nticket: \"tk-9f2x4a8m\"\nstatus: \"proposed\"\ncreated: \"{}\"\nauthors: [\"bob\"]\n---\n# Retry with backoff!\n",
            today()
        )
    );

    let (code, value) = scratch.hone(
        &[
            "artifact",
            "new",
            "--kind",
            "module",
            "--title",
            "Ledger",
            "--anchor",
            "path:src/ledger",
        ],
        None,
    );
    assert_eq!(code, Some(0), "{value}");
    let id = value["id"].as_str().expect("an ID").to_owned();
    assert_eq!(
        scratch.read(&format!("design/modules/{id}-ledger.yaml")),
        format!(
            "id: \"{id}\"\ntype: \"module\"\nanchors:\n  - {{ kind: \"path\", ref: \"src/ledger\" }}\n"
        )
    );
}

#[test]
fn new_refuses_before_it_writes() {
    let scratch = Scratch::new("new-refused");
    let before: Vec<_> = std::fs::read_dir(scratch.project().join("design/decisions"))
        .expect("readable")
        .collect();
    for (arguments, message) in [
        (
            vec![
                "--kind",
                "decision",
                "--title",
                "No ticket",
                "--author",
                "bob",
            ],
            "the kind `decision` belongs to a ticket: pass --ticket",
        ),
        (
            vec![
                "--kind",
                "decision",
                "--ticket",
                "tk-2b7n5k1q",
                "--title",
                "X",
                "--author",
                "bob",
            ],
            "no ticket record `tk-2b7n5k1q` in the project",
        ),
        (
            vec![
                "--kind",
                "decision",
                "--ticket",
                "tk-9f2x4a8m",
                "--title",
                "X",
            ],
            "no author: pass --author, or set an author in the project's context",
        ),
        (
            vec![
                "--kind",
                "module",
                "--ticket",
                "tk-9f2x4a8m",
                "--title",
                "X",
            ],
            "the kind `module` is living and belongs to no ticket",
        ),
        (
            vec!["--kind", "idea", "--title", "X"],
            "the configuration has no kind `idea`",
        ),
        (
            vec!["--kind", "module", "--title", "***"],
            "the title `***` gives no slug: use letters or digits",
        ),
        (
            vec!["--kind", "module", "--title", "X", "--anchor", "src"],
            "`src` is not `<kind>:<ref>`",
        ),
    ] {
        let mut all = vec!["artifact", "new"];
        all.extend(arguments);
        let (code, value) = scratch.hone(&all, None);
        assert_eq!(code, Some(2), "{message}");
        assert_eq!(value, json!({ "status": "error", "message": message }));
    }
    let after: Vec<_> = std::fs::read_dir(scratch.project().join("design/decisions"))
        .expect("readable")
        .collect();
    assert_eq!(before.len(), after.len());
    assert_eq!(
        std::fs::read_dir(scratch.project().join("design/modules"))
            .expect("readable")
            .count(),
        1
    );
}

#[test]
fn file_show_reads_regular_files_below_the_root() {
    let scratch = Scratch::new("file");
    let (code, value) = scratch.hone(
        &[
            "file",
            "show",
            "--path",
            "design/modules/mod-4t8w2n6c-fee.yaml",
            "--symbol",
            "Fee",
        ],
        None,
    );
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({
            "path": "design/modules/mod-4t8w2n6c-fee.yaml",
            "text": scratch.read("design/modules/mod-4t8w2n6c-fee.yaml"),
            "symbol": "Fee",
            "resolved": "unchecked",
            "message": "no language plugin handles `design/modules/mod-4t8w2n6c-fee.yaml`"
        })
    );
    for (path, message) in [
        (
            "../project/design",
            "`../project/design` is not a path relative to the project root",
        ),
        (
            "design",
            "`design` is not a regular file below the project root",
        ),
        (
            "design/missing.rs",
            "design/missing.rs: No such file or directory (os error 2)",
        ),
    ] {
        let (code, value) = scratch.hone(&["file", "show", "--path", path], None);
        assert_eq!(code, Some(1), "{path}");
        assert_eq!(value, json!({ "status": "error", "message": message }));
    }
}

#[test]
fn file_show_asks_the_language_plugin_for_a_symbol() {
    let root = repository().join("schemas/tests/check/module-anchors-resolve-resolved/project");
    let show = |symbol: &str| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_hone"))
            .args([
                "file",
                "show",
                "--path",
                "src/payments/fee.rs",
                "--symbol",
                symbol,
                "--json",
                "--root",
            ])
            .arg(&root)
            .output()
            .expect("hone runs");
        serde_json::from_slice(&output.stdout).expect("one JSON document")
    };
    let found = show("Fee::amount");
    assert_eq!(
        (found["language"].as_str(), found["resolved"].as_str()),
        (Some("lang-rust"), Some("found"))
    );
    // The lines of fee.rs, counted by hand: the line of the symbol's name, not of its item.
    for (symbol, line) in [
        ("Fee::amount", 2),
        ("Fee::total", 7),
        ("Tier::High", 14),
        ("rounding::round", 18),
        ("compute", 23),
    ] {
        assert_eq!(show(symbol)["line"], line, "{symbol}");
    }
    let missing = show("Nope");
    assert_eq!(missing["resolved"], "missing");
    assert_eq!(
        missing["message"],
        "no symbol `Nope` in `src/payments/fee.rs`"
    );
}
