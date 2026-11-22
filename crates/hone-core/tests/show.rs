//! Showing the resolved configuration with the layers of every definition and the lockfile
//! (concept Section 12, "Showing the resolved configuration"), over the resolution test cases
//! under `schemas/tests/resolve/`.

use std::path::{Path, PathBuf};

use hone_core::{
    Category, Comparison, FileState, PackageDirectory, ShowError, SnapshotState, show_resolved,
    write_resolved,
};
use serde_json::{Value, json};

// The hash of `null-removal/expected.json`: `jq -cS . expected.json | tr -d '\n' | sha256sum`.
const NULL_REMOVAL_HASH: &str =
    "sha256:06425703822e17cfb9101e73544c0549a2454a3d8f1b38a321f5682fbc8d0fd7";

fn case(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/tests/resolve")
        .join(name)
}

fn package(name: &str) -> Value {
    json!({ "kind": "package", "name": name })
}

fn project() -> Value {
    json!({ "kind": "project" })
}

// A copy of a resolution test case, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str, from: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-show-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(&case(from), &root);
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn packages(&self) -> PackageDirectory {
        PackageDirectory::new(self.root.join("packages"))
    }
}

impl Drop for Scratch {
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

#[test]
fn every_definition_names_the_layers_that_contributed() {
    let case = case("null-removal");
    let packages = PackageDirectory::new(case.join("packages"));
    let shown = show_resolved(&case.join("project"), &packages, None).expect("shows");

    let expected: Value =
        serde_json::from_slice(&std::fs::read(case.join("expected.json")).expect("readable"))
            .expect("JSON");
    assert_eq!(shown.config, expected);
    assert_eq!(shown.hash, NULL_REMOVAL_HASH);
    assert_eq!(
        shown.comparison,
        Comparison::File(FileState::OutOfDate),
        "the case has no resolved.json"
    );
    // `kinds.note` and `roles.critic` come from the package and are removed by the project.
    assert_eq!(
        shown.origins,
        json!({
            "kinds": { "decision": [package("acme/base"), project()] },
            "roles": { "human": [package("acme/base")] },
            "steps": { "decision": [package("acme/base"), project()] },
            "profiles": { "standard": [package("acme/base")] },
            "plugins": {},
            "select": [package("acme/base")],
            "actors": [],
            "ticket": [package("acme/base")],
        })
    );
    assert_eq!(
        shown.lock,
        json!({ "packages": { "acme/base": {
            "version": "1.0.0",
            "hash": "sha256:f65422be6686b5ecc4b653c2e83e716ba16f2af09245b78583b94241f4346c56",
        } } })
    );
    let serialized = serde_json::to_value(&shown).expect("JSON");
    assert_eq!(serialized["status"], "shown");
    assert_eq!(serialized["file"], "out-of-date");
    assert!(serialized.get("snapshot").is_none(), "{serialized}");
}

#[test]
fn packages_are_listed_in_the_order_of_extends() {
    let case = case("extends-order");
    let packages = PackageDirectory::new(case.join("packages"));
    let shown = show_resolved(&case.join("project"), &packages, None).expect("shows");
    assert_eq!(
        shown.origins["kinds"]["decision"],
        json!([package("acme/c"), package("acme/a"), package("acme/b")])
    );
}

#[test]
fn a_written_resolved_json_is_up_to_date() {
    let scratch = Scratch::new("up-to-date", "null-removal");
    write_resolved(&scratch.project(), &scratch.packages()).expect("writes");
    let shown = show_resolved(&scratch.project(), &scratch.packages(), None).expect("shows");
    assert_eq!(shown.comparison, Comparison::File(FileState::UpToDate));

    std::fs::write(
        scratch.project().join(".hone/steps/decision.yaml"),
        "acts: [human]\n",
    )
    .expect("writable");
    let shown = show_resolved(&scratch.project(), &scratch.packages(), None).expect("shows");
    assert_eq!(shown.comparison, Comparison::File(FileState::OutOfDate));
}

#[test]
fn a_missing_lockfile_is_an_empty_lock() {
    let case = case("defaults");
    let shown = show_resolved(
        &case.join("project"),
        &PackageDirectory::new(case.join("packages")),
        None,
    )
    .expect("shows");
    assert!(!case.join("project/.hone/lock.yaml").exists());
    assert_eq!(shown.lock, json!({}));
}

#[test]
fn a_failed_resolution_keeps_a_readable_lockfile() {
    let case = case("error-plugin-hash");
    let packages = PackageDirectory::new(case.join("packages"));
    let Err(ShowError::Failed { problems, lock }) =
        show_resolved(&case.join("project"), &packages, None)
    else {
        panic!("expected a failed resolution");
    };
    assert!(
        problems
            .iter()
            .any(|problem| problem.category == Category::PluginHash),
        "{problems:?}"
    );
    let lock = lock.expect("the lockfile is readable");
    assert_eq!(
        lock["plugins"]["api-check"]["hash"],
        "sha256:1111111111111111111111111111111111111111111111111111111111111111"
    );

    let case = self::case("error-manifest-lock");
    let outcome = show_resolved(&case.join("project"), &packages, None);
    let Err(ShowError::Failed { problems, lock }) = &outcome else {
        panic!("expected a failed resolution, got {outcome:?}");
    };
    assert_eq!(problems[0].category, Category::Manifest);
    assert_eq!(*lock, None);
    let serialized = serde_json::to_value(outcome.expect_err("failed")).expect("JSON");
    assert_eq!(serialized["status"], "failed");
    assert!(serialized.get("lock").is_none(), "{serialized}");
}

// A ticket record for `tk-9f2x4a8m` whose snapshot is `null-removal/expected.json`, with
// `overrides` appended.
fn ticket(scratch: &Scratch, overrides: &str) {
    let record = scratch
        .project()
        .join("design/tickets/tk-9f2x4a8m/ticket.yaml");
    std::fs::create_dir_all(record.parent().expect("parent")).expect("creatable");
    std::fs::write(
        &record,
        format!(
            "id: \"tk-9f2x4a8m\"\ntype: \"ticket\"\nproject: \"prj-8m2k4d7q\"\nprofile: \"standard\"\nconfig: \"{NULL_REMOVAL_HASH}\"\n{overrides}"
        ),
    )
    .expect("writable");
    let snapshots = scratch.project().join(".hone/snapshots");
    std::fs::create_dir_all(&snapshots).expect("creatable");
    std::fs::copy(
        scratch.root.join("expected.json"),
        snapshots.join(format!(
            "{}.json",
            NULL_REMOVAL_HASH.trim_start_matches("sha256:")
        )),
    )
    .expect("copyable");
}

#[test]
fn a_ticket_without_overrides_has_the_same_snapshot() {
    let scratch = Scratch::new("ticket-same", "null-removal");
    ticket(&scratch, "");
    let shown =
        show_resolved(&scratch.project(), &scratch.packages(), Some("tk-9f2x4a8m")).expect("shows");
    assert_eq!(shown.comparison, Comparison::Snapshot(SnapshotState::Same));
    assert_eq!(shown.hash, NULL_REMOVAL_HASH);
    let serialized = serde_json::to_value(&shown).expect("JSON");
    assert_eq!(serialized["snapshot"], "same");
    assert!(serialized.get("file").is_none(), "{serialized}");
}

#[test]
fn the_overrides_of_a_ticket_are_the_top_layer() {
    let scratch = Scratch::new("ticket-overrides", "null-removal");
    // The project removes `roles.critic` of the package; the override adds it again, so only
    // the override contributes to it.
    ticket(
        &scratch,
        "overrides:\n  roles:\n    critic: { can: [comment] }\n  steps:\n    decision:\n      acts: [critic]\n",
    );
    let shown =
        show_resolved(&scratch.project(), &scratch.packages(), Some("tk-9f2x4a8m")).expect("shows");
    assert_eq!(
        shown.comparison,
        Comparison::Snapshot(SnapshotState::Differs)
    );
    let overrides = json!({ "kind": "overrides" });
    assert_eq!(shown.origins["roles"]["critic"], json!([overrides]));
    assert_eq!(
        shown.origins["steps"]["decision"],
        json!([package("acme/base"), project(), overrides])
    );
    assert_eq!(shown.config["steps"]["decision"]["acts"], json!(["critic"]));
}

#[test]
fn an_unknown_ticket_is_invalid() {
    let scratch = Scratch::new("ticket-unknown", "null-removal");
    let outcome = show_resolved(&scratch.project(), &scratch.packages(), Some("tk-2b4c6d8f"));
    let Err(ShowError::Invalid { message }) = outcome else {
        panic!("expected an invalid ticket, got {outcome:?}");
    };
    assert!(
        message.starts_with("design/tickets/tk-2b4c6d8f/ticket.yaml: "),
        "{message}"
    );
}
