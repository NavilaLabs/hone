//! The example project `examples/payments` stays usable: its configuration matches its sources,
//! its events import from its shared folder, and its ticket fails exactly where its README says.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const TICKET: &str = "tk-y4075sb1";

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/payments")
}

// A data and configuration directory of its own, removed on drop.
struct Device {
    root: PathBuf,
}

impl Device {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("hone-cli-example-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Self { root }
    }

    fn hone(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hone"))
            .args(arguments)
            .current_dir(example())
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
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn shared_files() -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(directory).expect("readable") {
            let path = entry.expect("readable").path();
            if path.is_dir() {
                walk(&path, files);
            } else {
                let bytes = std::fs::read(&path).expect("readable");
                files.push((path, bytes));
            }
        }
    }
    let mut files = Vec::new();
    walk(&example().join("shared"), &mut files);
    files.sort();
    files
}

fn failed_gates(report: &Value) -> Vec<String> {
    report["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .filter(|gate| gate["ok"] == false)
        .map(|gate| gate["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn the_example_works_as_its_readme_says() {
    let device = Device::new();
    let (code, resolved) = device.json(&["resolve", "--check", "--packages", "packages"]);
    assert_eq!((code, &resolved["status"]), (Some(0), &"up-to-date".into()));

    let before = shared_files();
    let (code, synced) = device.json(&["sync", "--folder", "shared"]);
    assert_eq!(code, Some(0), "{synced}");
    assert_eq!(synced["received"]["dev-3dzrsw0x"], 8);
    assert!(
        before == shared_files(),
        "a device without events writes nothing"
    );

    let (code, report) = device.json(&["check", "--ticket", TICKET]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(
        failed_gates(&report),
        [
            "decision/no-blockers",
            "implementation/binding",
            "interfaces/public-api"
        ]
    );
    let blocking = report["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .find(|gate| gate["name"] == "decision/no-blockers")
        .expect("the gate");
    assert_eq!(blocking["findings"][0]["code"], "thread-blocking-open");

    let (code, report) = device.json(&[
        "check",
        "--ticket",
        TICKET,
        "--step",
        "interfaces",
        "--changed",
        "src/payments/fee.rs",
        "--base",
        "base",
    ]);
    assert_eq!(code, Some(0), "{report}");
}
