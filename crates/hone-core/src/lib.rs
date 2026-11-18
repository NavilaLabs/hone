//! Reference implementation of the hone protocol: resolution of the configuration and the
//! standard gates.
//!
//! [`resolve`] builds the resolved configuration (Level 1) of a project from its layers
//! (concept Section 8). [`Resolved::from_document`] reads an existing resolved document.
//! [`write_resolved`] and [`compare_resolved`] keep `.hone/resolved.json` in step with its
//! sources. [`check`] runs the gates over a checked directory (concept Section 7).
//! [`ProcessStore`] appends and reads the events of the process store (concept Section 4).

mod anchor;
mod artifact;
mod canonical;
mod check;
mod complete;
mod directory;
mod error;
mod event;
mod exec;
mod finding;
mod gates;
mod host;
mod layer;
mod log;
mod manifest;
mod merge;
mod packages;
mod pointer;
mod process;
mod replication;
mod resolved_file;
mod schema_valid;
mod source;
mod validate;
mod yaml;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

pub use check::{
    CheckError, CheckReport, CheckRequest, GateClass, GateReport, check, check_artifact,
    check_edit, content_hash,
};
pub use error::{Category, Location, Origin, Problem, ResolveError};
pub use finding::{Code, Finding, Level};
pub use log::{Cursor, InvalidEvent};
pub use process::{
    Appended, Author, Change, ProcessError, ProcessStore, StepState, StepStatus, ThreadAnchor,
    ThreadState, ThreadStatus, TicketEvents,
};
pub use replication::{SyncCode, SyncProblem, SyncReport};
pub use resolved_file::{ResolvedFile, ResolvedFileError, compare_resolved, write_resolved};
pub use source::{NoPackages, PackageDirectory, PackageSource};
pub use yaml::YamlProfileError;

use crate::error::pointer_problem;
use crate::packages::Layer;
use crate::pointer::Pointer;

/// A valid resolved configuration (Level 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    document: Value,
}

impl Resolved {
    /// Checks a resolved document, for example the content of `.hone/resolved.json` or of a
    /// snapshot, against the shape and the references of the resolved configuration.
    ///
    /// # Errors
    ///
    /// Returns the problems of the categories `shape` and `reference`.
    pub fn from_document(document: Value) -> Result<Self, ResolveError> {
        let problems = validate::validate(&document);
        if problems.is_empty() {
            Ok(Self { document })
        } else {
            Err(ResolveError { problems })
        }
    }

    /// The resolved document as data.
    #[must_use]
    pub fn document(&self) -> &Value {
        &self.document
    }

    /// The bytes of `resolved.json` and of a snapshot: the canonical form (RFC 8785) followed
    /// by one `\n`.
    #[must_use]
    pub fn file_contents(&self) -> Vec<u8> {
        let mut bytes = canonical::to_canonical(&self.document).into_bytes();
        bytes.push(b'\n');
        bytes
    }

    /// The identity of the content: `sha256:` plus the SHA-256 of the canonical form.
    #[must_use]
    pub fn hash(&self) -> String {
        canonical::sha256(canonical::to_canonical(&self.document).as_bytes())
    }
}

/// Parses one YAML document in the strict YAML profile (concept Section 5) into the JSON data
/// model, for example the `overrides` of a ticket record.
///
/// # Errors
///
/// Returns every violation of the profile that was found.
pub fn parse_yaml(bytes: &[u8]) -> Result<Value, YamlProfileError> {
    yaml::parse(bytes).map_err(|violations| YamlProfileError {
        violations: violations.iter().map(ToString::to_string).collect(),
    })
}

/// Resolves the configuration of the project in `project_root` (the directory that contains
/// `.hone/`), following concept Section 8, "Resolution steps".
///
/// `packages` provides the packages named in `extends`. `overrides` is the field `overrides`
/// of a ticket record, a patch on the resolved tree, or `None` outside a ticket.
///
/// # Errors
///
/// Returns every problem of the first resolution step that found any. Each problem carries one
/// of the normative [`Category`] values.
pub fn resolve(
    project_root: &Path,
    packages: &dyn PackageSource,
    overrides: Option<&Value>,
) -> Result<Resolved, ResolveError> {
    let into_error = |problems| ResolveError { problems };
    let hone = project_root.join(".hone");

    let (extends, lock) = match (
        manifest::read_project_config(&hone),
        manifest::read_lock(&hone),
    ) {
        (Ok(extends), Ok(lock)) => (extends, lock),
        (config, lock) => {
            let mut problems = config.err().unwrap_or_default();
            problems.extend(lock.err().unwrap_or_default());
            return Err(into_error(problems));
        }
    };

    let mut layers = packages::order(&extends, &lock, packages).map_err(into_error)?;
    layers.push(Layer {
        origin: Origin::Project,
        root: hone.clone(),
    });

    let mut loaded = Vec::new();
    let mut problems = Vec::new();
    for layer in &layers {
        match layer::load(layer) {
            Ok(layer) => loaded.push(layer),
            Err(found) => problems.extend(found),
        }
    }
    if !problems.is_empty() {
        return Err(into_error(problems));
    }

    let mut tree = Map::new();
    let mut schemas = BTreeMap::new();
    for layer in loaded {
        merge::merge(&mut tree, layer.tree, &Pointer::root(), &mut problems);
        schemas.extend(layer.schemas);
    }
    if let Some(overrides) = overrides {
        apply_overrides(&mut tree, overrides, &mut problems);
    }
    if !problems.is_empty() {
        return Err(into_error(problems));
    }

    complete::embed_schemas(&mut tree, &schemas);
    let problems = complete::complete_plugins(&mut tree, &hone, &lock);
    if !problems.is_empty() {
        return Err(into_error(problems));
    }
    complete::write_defaults(&mut tree);

    Resolved::from_document(Value::Object(tree))
}

fn apply_overrides(tree: &mut Map<String, Value>, overrides: &Value, problems: &mut Vec<Problem>) {
    let Value::Object(overrides) = overrides else {
        problems.push(pointer_problem(
            Category::Shape,
            &Pointer::root(),
            "`overrides` must be a map",
        ));
        return;
    };
    let forbidden = |at: Pointer| {
        pointer_problem(
            Category::OverrideForbidden,
            &at,
            "a ticket override must not change this part of the configuration",
        )
    };
    let before = problems.len();
    for key in ["protocol", "ticket", "plugins"] {
        if overrides.contains_key(key) {
            problems.push(forbidden(Pointer::root().child(key)));
        }
    }
    if let Some(Value::Object(kinds)) = overrides.get("kinds") {
        for (name, kind) in kinds {
            if kind
                .as_object()
                .is_some_and(|kind| kind.contains_key("schema"))
            {
                problems.push(forbidden(Pointer::from_segments(["kinds", name, "schema"])));
            }
        }
    }
    if problems.len() == before {
        merge::merge(tree, overrides.clone(), &Pointer::root(), problems);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::Value;

    use crate::{Resolved, yaml};

    fn repository_file(path: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    #[test]
    fn concept_example_equals_its_fixture() {
        let concept = repository_file("docs/protocol-concept.md");
        let section = &concept[concept
            .find("### Level 1: the resolved configuration")
            .expect("Level 1 section")..];
        let start = section.find("```yaml\n").expect("YAML block") + "```yaml\n".len();
        let end = start
            + section[start..]
                .find("```\n")
                .expect("end of the YAML block");
        let example = yaml::parse_str(&section[start..end])
            .unwrap_or_else(|violations| panic!("profile: {violations:?}"));
        let fixture: Value = serde_json::from_str(&repository_file(
            "schemas/tests/config/valid/concept-example.json",
        ))
        .expect("fixture is JSON");
        assert_eq!(example, fixture);
        Resolved::from_document(example).expect("the example is valid");
    }

    #[test]
    fn canonical_form_is_stable() {
        let document: Value =
            serde_json::from_str(&repository_file("schemas/tests/config/valid/base.json"))
                .expect("fixture is JSON");
        let resolved = Resolved::from_document(document).expect("valid");
        let bytes = resolved.file_contents();
        assert!(bytes.ends_with(b"}\n") && !bytes[..bytes.len() - 1].contains(&b'\n'));
        assert_eq!(
            resolved.hash(),
            "sha256:b3ad610fc06c78503fdf4b23ca76d88c3d034eae585e2d854add069f3893940f"
        );
    }
}
