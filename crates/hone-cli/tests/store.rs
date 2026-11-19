//! `hone export` and `hone forget` (concept Section 12, "Tenant separation").

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/check/no-open-blocking-threads-open";
const PROJECT: &str = "prj-8m2k4d7q";
const SEEDED_DEVICE: &str = "dev-3k9a5h2t";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// A copy of a gate test case and a configuration directory, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-cli-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(&repository().join(CASE), &root.join("case"));
        Self { root }
    }

    fn hone(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hone"))
            .args(arguments)
            .current_dir(self.root.join("case/project"))
            .env("HONE_DATA_DIR", self.root.join("case/data"))
            .env("HONE_CONFIG_DIR", self.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .env_remove("HONE_ROLE")
            .env_remove("HONE_SYNC_FOLDER")
            .env_remove("HONE_SYNC_SERVER")
            .env_remove("HONE_SYNC_GIT")
            .output()
            .expect("hone runs")
    }

    fn store(&self) -> PathBuf {
        self.root.join("case/data/local").join(PROJECT)
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).display().to_string()
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

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON document")
}

fn own_device(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.store().join("device"))
        .expect("written on the first append")
        .trim()
        .to_owned()
}

#[test]
fn an_export_survives_forget_and_syncs_back() {
    let scratch = Scratch::new("round-trip");
    let written = scratch.hone(&[
        "step",
        "set",
        "--ticket",
        "tk-9f2x4a8m",
        "decision",
        "done",
        "--author",
        "alice",
        "--role",
        "human",
    ]);
    assert_eq!(written.status.code(), Some(0));
    let own = own_device(&scratch);
    let folder = scratch.path("bind-folder");
    std::fs::create_dir_all(&folder).expect("creatable");
    scratch.hone(&["context", "set", "acme", "--folder", &folder]);
    scratch.hone(&["context", "use", "acme"]);

    let export = scratch.path("export");
    let exported = scratch.hone(&["export", "--to", &export, "--json"]);
    assert_eq!(exported.status.code(), Some(0));
    assert_eq!(
        json_of(&exported),
        json!({
            "directory": format!("{export}/local/{PROJECT}"),
            "events": { SEEDED_DEVICE: 1, own.as_str(): 1 },
        })
    );
    for device in [SEEDED_DEVICE, own.as_str()] {
        let segment = format!("log/{device}/1.ndjson");
        assert_eq!(
            std::fs::read(
                Path::new(&export)
                    .join("local")
                    .join(PROJECT)
                    .join(&segment)
            )
            .expect("exported"),
            std::fs::read(scratch.store().join(&segment)).expect("stored"),
            "{segment} is copied byte for byte"
        );
    }

    let forgotten = scratch.hone(&["forget", "--confirm", PROJECT, "--json"]);
    assert_eq!(forgotten.status.code(), Some(0));
    assert_eq!(
        json_of(&forgotten),
        json!({
            "directory": scratch.store().display().to_string(),
            "events": { SEEDED_DEVICE: 1, own.as_str(): 1 },
            "context": "acme",
        })
    );
    assert!(
        !scratch.root.join("case/data/local").exists(),
        "the store and its empty workspace directory are gone"
    );
    let contexts =
        std::fs::read_to_string(scratch.root.join("config/contexts.yaml")).expect("readable");
    assert!(
        contexts.ends_with("projects: {}\n") && contexts.contains("acme:"),
        "the binding is gone, the context stays: {contexts}"
    );

    let again = scratch.hone(&["forget", "--confirm", PROJECT, "--json"]);
    assert_eq!(
        json_of(&again)["events"],
        json!({}),
        "nothing left to forget"
    );

    let synced = scratch.hone(&["sync", "--folder", &export, "--json"]);
    assert_eq!(synced.status.code(), Some(0));
    assert_eq!(
        json_of(&synced)["received"],
        json!({ SEEDED_DEVICE: 1, own.as_str(): 1 })
    );
}

#[test]
fn export_and_forget_refuse_before_touching_anything() {
    let scratch = Scratch::new("refused");
    let occupied = scratch.root.join("occupied");
    std::fs::create_dir_all(&occupied).expect("creatable");
    std::fs::write(occupied.join("keep.txt"), "mine").expect("writable");
    let into_occupied = scratch.hone(&["export", "--to", &scratch.path("occupied"), "--json"]);
    assert_eq!(into_occupied.status.code(), Some(1));
    assert_eq!(
        json_of(&into_occupied),
        json!({
            "status": "invalid",
            "message": format!("{} is not empty: export into a new or empty directory", occupied.display()),
        })
    );
    assert_eq!(
        std::fs::read_dir(&occupied).expect("readable").count(),
        1,
        "nothing is written next to the existing file"
    );

    let wrong = scratch.hone(&["forget", "--confirm", "prj-00000000", "--json"]);
    assert_eq!(wrong.status.code(), Some(1));
    assert_eq!(
        json_of(&wrong),
        json!({
            "status": "invalid",
            "message": format!("`prj-00000000` is not the project of the root: forgetting `{PROJECT}` takes --confirm {PROJECT}"),
        })
    );
    assert!(scratch.store().join("log").is_dir(), "the store stays");

    std::fs::create_dir_all(scratch.root.join("config")).expect("creatable");
    std::fs::write(scratch.root.join("config/contexts.yaml"), "token: \"x\"\n").expect("writable");
    let malformed = scratch.hone(&["forget", "--confirm", PROJECT, "--json"]);
    assert_eq!(malformed.status.code(), Some(1));
    assert_eq!(json_of(&malformed)["status"], "invalid");
    assert!(
        scratch.store().join("log").is_dir(),
        "a malformed contexts.yaml stops forget before the store goes"
    );

    std::fs::remove_dir_all(scratch.store()).expect("removable");
    let no_store = scratch.hone(&["export", "--to", &scratch.path("fresh"), "--json"]);
    assert_eq!(no_store.status.code(), Some(1));
    assert_eq!(
        json_of(&no_store),
        json!({
            "status": "unavailable",
            "message": format!("no process store at {}", scratch.store().display()),
        })
    );
    assert!(!scratch.root.join("fresh").exists(), "nothing is created");
}

#[cfg(unix)]
#[test]
fn forget_does_not_follow_a_symbolic_link_out_of_the_store() {
    let scratch = Scratch::new("link");
    let outside = scratch.root.join("outside");
    std::fs::create_dir_all(&outside).expect("creatable");
    std::fs::write(outside.join("precious.txt"), "keep").expect("writable");
    std::os::unix::fs::symlink(&outside, scratch.store().join("planted")).expect("linkable");
    std::os::unix::fs::symlink(&outside, scratch.store().join("log/dev-zzzzzzzz"))
        .expect("linkable");

    let forgotten = scratch.hone(&["forget", "--confirm", PROJECT, "--json"]);
    assert_eq!(
        forgotten.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&forgotten.stdout)
    );
    assert!(!scratch.store().exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("precious.txt")).expect("still there"),
        "keep"
    );
}
