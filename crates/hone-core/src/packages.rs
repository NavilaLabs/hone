use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

use crate::canonical::{sha256, to_canonical};
use crate::error::{Category, Origin, Problem, file_problem};
use crate::manifest::{Lock, Reference, read_package_config};
use crate::source::PackageSource;

#[derive(Debug, Clone)]
pub(crate) struct Layer {
    pub(crate) origin: Origin,
    pub(crate) root: PathBuf,
}

pub(crate) fn order(
    extends: &[Reference],
    lock: &Lock,
    source: &dyn PackageSource,
) -> Result<Vec<Layer>, Vec<Problem>> {
    let mut walk = Walk {
        lock,
        source,
        seen: BTreeMap::new(),
        layers: Vec::new(),
        problems: Vec::new(),
    };
    for reference in extends {
        walk.visit(reference, &Origin::Project);
    }
    if walk.problems.is_empty() {
        Ok(walk.layers)
    } else {
        Err(walk.problems)
    }
}

struct Walk<'a> {
    lock: &'a Lock,
    source: &'a dyn PackageSource,
    seen: BTreeMap<String, String>,
    layers: Vec<Layer>,
    problems: Vec<Problem>,
}

impl Walk<'_> {
    fn visit(&mut self, reference: &Reference, referrer: &Origin) {
        if let Some(major) = self.seen.get(&reference.name) {
            if *major != reference.major {
                self.problems.push(file_problem(
                    Category::PackageConflict,
                    referrer,
                    "config.yaml",
                    format!("`{reference}` conflicts with `{}@{major}`", reference.name),
                ));
            }
            return;
        }
        self.seen
            .insert(reference.name.clone(), reference.major.clone());

        let missing = |message: String| {
            file_problem(Category::PackageMissing, referrer, "config.yaml", message)
        };
        let Some(pinned) = self.lock.packages.get(&reference.name) else {
            self.problems
                .push(missing(format!("`{reference}` has no entry in lock.yaml")));
            return;
        };
        if pinned.major() != reference.major {
            self.problems.push(missing(format!(
                "`{reference}` does not match the pinned version {}",
                pinned.version
            )));
            return;
        }
        let Some(root) = self.source.locate(&reference.name, &pinned.version) else {
            self.problems.push(missing(format!(
                "`{}` {} is not available",
                reference.name, pinned.version
            )));
            return;
        };
        let origin = Origin::Package(reference.name.clone());
        match package_hash(&root) {
            Ok(actual) if actual == pinned.hash => {}
            Ok(actual) => {
                self.problems.push(file_problem(
                    Category::PackageHash,
                    &origin,
                    "",
                    format!(
                        "content hash {actual} differs from {} in lock.yaml",
                        pinned.hash
                    ),
                ));
                return;
            }
            Err(error) => {
                self.problems.push(file_problem(
                    Category::PackageHash,
                    &origin,
                    "",
                    format!("cannot be hashed: {error}"),
                ));
                return;
            }
        }
        match read_package_config(&root, &origin) {
            Ok(dependencies) => {
                for dependency in &dependencies {
                    self.visit(dependency, &origin);
                }
            }
            Err(problems) => self.problems.extend(problems),
        }
        self.layers.push(Layer { origin, root });
    }
}

fn package_hash(root: &Path) -> std::io::Result<String> {
    let mut files = Map::new();
    collect_files(root, "", &mut files)?;
    Ok(sha256(to_canonical(&Value::Object(files)).as_bytes()))
}

fn collect_files(
    directory: &Path,
    prefix: &str,
    files: &mut Map<String, Value>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_string_lossy()
            .nfc()
            .collect::<String>();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, &relative, files)?;
        } else {
            files.insert(relative, Value::String(sha256(&std::fs::read(&path)?)));
        }
    }
    Ok(())
}
