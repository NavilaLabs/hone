//! The catalog of a checked directory and the overview of a ticket (concept Section 12,
//! "Desktop GUI"): what a user interface lists and how it orders a profile's steps.

use std::path::{Path, PathBuf};

use hone_core::{Resolved, catalog, ticket_overview};
use serde_json::{Value, json};

const TICKET: &str = "tk-9f2x4a8m";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

fn copy_directory(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copyable");
        }
    }
}

// A copy of a gate test project, removed on drop.
struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-catalog-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        copy_directory(
            &schemas().join("tests/check/no-open-blocking-threads-open/project"),
            &root,
        );
        Self { root }
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("creatable");
        std::fs::write(path, contents).expect("writable");
    }

    // Freezes `change` applied to the current configuration as the ticket's snapshot.
    fn freeze(&self, change: impl FnOnce(&mut Value)) {
        let current: Value = serde_json::from_slice(
            &std::fs::read(self.root.join(".hone/resolved.json")).expect("readable"),
        )
        .expect("JSON");
        let mut document = current;
        change(&mut document);
        let snapshot = Resolved::from_document(document).expect("a valid configuration");
        let hash = snapshot.hash();
        let hex = hash.strip_prefix("sha256:").expect("a sha256 hash");
        self.write(
            &format!(".hone/snapshots/{hex}.json"),
            std::str::from_utf8(&snapshot.file_contents()).expect("UTF-8"),
        );
        self.write(
            &format!("design/tickets/{TICKET}/ticket.yaml"),
            &format!(
                "id: \"{TICKET}\"\ntype: \"ticket\"\nproject: \"prj-8m2k4d7q\"\nprofile: \"standard\"\nconfig: \"{hash}\"\nattributes: {{ risk: \"high\" }}\nexternal:\n  - {{ system: \"github\", key: \"12\", url: \"https://github.com/acme/payments/issues/12\" }}\n"
            ),
        );
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_catalog_lists_every_artifact_with_title_and_effective_status() {
    let project = Project::new("listing");
    project.write(
        "design/decisions/dec-2h6r9t1x-payment-sync.md",
        "---\nid: \"dec-2h6r9t1x\"\ntype: \"decision\"\nticket: \"tk-9f2x4a8m\"\nstatus: \"accepted\"\ncreated: \"2026-11-17\"\nauthors: [\"alice\"]\nlinks:\n  supersedes: [\"dec-7k3m2q8p\"]\n---\nIntro.\n\n# Payments run synchronously\n\nText.\n",
    );
    project.write("design/decisions/dec-broken-x.md", "no front matter\n");

    let catalog = catalog(&project.root).expect("listable");
    assert_eq!(catalog.project, "prj-8m2k4d7q");
    assert_eq!(
        serde_json::to_value(&catalog.artifacts).expect("serializable"),
        json!([
            {
                "id": "dec-2h6r9t1x",
                "type": "decision",
                "path": "design/decisions/dec-2h6r9t1x-payment-sync.md",
                "ticket": "tk-9f2x4a8m",
                "title": "Payments run synchronously",
                "status": "accepted",
                "effective": "accepted",
                "links": { "supersedes": ["dec-7k3m2q8p"] }
            },
            {
                "id": "dec-7k3m2q8p",
                "type": "decision",
                "path": "design/decisions/dec-7k3m2q8p-payment-async.md",
                "ticket": "tk-9f2x4a8m",
                "status": "accepted",
                "effective": "superseded",
                "links": { "affects": ["mod-4t8w2n6c"] }
            },
            {
                "id": "mod-4t8w2n6c",
                "type": "module",
                "path": "design/modules/mod-4t8w2n6c-fee.yaml"
            },
            {
                "id": "spec-3b8n1c5w",
                "type": "spec",
                "path": "design/tickets/tk-9f2x4a8m/spec-3b8n1c5w-async.md",
                "ticket": "tk-9f2x4a8m",
                "status": "accepted",
                "effective": "accepted"
            },
            {
                "id": "tk-9f2x4a8m",
                "type": "ticket",
                "path": "design/tickets/tk-9f2x4a8m/ticket.yaml",
                "profile": "standard"
            }
        ])
    );
    assert!(catalog.unreadable.is_empty(), "{:?}", catalog.unreadable);
}

#[test]
fn an_unreadable_artifact_is_reported_not_listed() {
    let project = Project::new("unreadable");
    project.write(
        "design/decisions/dec-2h6r9t1x-broken.md",
        "id: dec-2h6r9t1x\n",
    );

    let catalog = catalog(&project.root).expect("listable");
    assert_eq!(
        serde_json::to_value(&catalog.unreadable).expect("serializable"),
        json!([{
            "path": "design/decisions/dec-2h6r9t1x-broken.md",
            "message": "front matter missing: the first line must be ---"
        }])
    );
    assert_eq!(catalog.artifacts.len(), 4);
}

#[test]
fn a_profile_orders_its_steps_by_inputs_outputs_and_requires() {
    let project = Project::new("order");
    project.freeze(|config| {
        let steps = &mut config["steps"];
        // specify → decision → interfaces → implementation by kinds; completion after
        // implementation by `requires`; settled has no order and keeps its name's place.
        steps["specify"]["outputs"] = json!(["spec"]);
        steps["specify"]["title"] = json!("Specification");
        steps["decision"]["inputs"] = json!(["spec", "ticket"]);
        steps["decision"]["outputs"] = json!(["decision"]);
        steps["interfaces"]["inputs"] = json!(["decision"]);
        steps["interfaces"]["outputs"] = json!(["module"]);
        steps["implementation"]["inputs"] = json!(["module"]);
        steps["completion"]["requires"] = json!(["implementation"]);
        config["roles"]["critic"] = json!({ "kind": "ai", "can": ["comment"] });
    });

    let overview = ticket_overview(&project.root, TICKET).expect("readable");
    assert_eq!(
        serde_json::to_value(&overview).expect("serializable"),
        json!({
            "ticket": TICKET,
            "path": "design/tickets/tk-9f2x4a8m/ticket.yaml",
            "profile": "standard",
            "config": overview.config,
            "attributes": { "risk": "high" },
            "external": [
                { "system": "github", "key": "12", "url": "https://github.com/acme/payments/issues/12" }
            ],
            "roles": ["architect", "critic", "human"],
            "steps": [
                { "step": "settled", "requirement": "optional", "after": [] },
                { "step": "specify", "title": "Specification", "requirement": "required", "after": [] },
                { "step": "decision", "requirement": "required", "after": ["specify"] },
                { "step": "interfaces", "requirement": "optional", "after": ["decision"] },
                { "step": "implementation", "requirement": "required", "after": ["interfaces"] },
                { "step": "completion", "requirement": "optional", "after": ["implementation"] }
            ]
        })
    );
}

#[test]
fn a_cycle_of_inputs_and_outputs_puts_its_steps_at_the_end() {
    let project = Project::new("cycle");
    project.freeze(|config| {
        let steps = &mut config["steps"];
        steps["decision"]["inputs"] = json!(["spec"]);
        steps["specify"]["inputs"] = json!(["decision"]);
    });

    let overview = ticket_overview(&project.root, TICKET).expect("readable");
    let order: Vec<&str> = overview
        .steps
        .iter()
        .map(|step| step.step.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "completion",
            "implementation",
            "interfaces",
            "settled",
            "decision",
            "specify"
        ]
    );
}

#[test]
fn an_unknown_ticket_names_the_missing_record() {
    let project = Project::new("unknown");
    let error = ticket_overview(&project.root, "tk-2b7n5k1q").expect_err("no such ticket");
    assert!(
        error
            .message
            .starts_with("design/tickets/tk-2b7n5k1q/ticket.yaml: "),
        "{}",
        error.message
    );
}
