use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde_json::{Map, Value};

use crate::error::{Category, Origin, Problem, file_problem};
use crate::packages::Layer;
use crate::yaml;

const DEFINITION_DIRECTORIES: [&str; 5] = ["kinds", "roles", "steps", "profiles", "plugins"];
const SUBTREE_FILES: [(&str, &str); 3] = [
    ("select.yaml", "select"),
    ("actors.yaml", "actors"),
    ("ticket.yaml", "ticket"),
];
const PROJECT_ONLY_FILES: [&str; 3] = ["lock.yaml", "resolved.json", "resolved.origin.json"];

pub(crate) struct LoadedLayer {
    pub(crate) tree: Map<String, Value>,
    pub(crate) schemas: BTreeMap<String, Value>,
}

enum Entry {
    Definitions,
    Subtree(&'static str),
    Ignored,
    ProjectOnly,
    Unknown,
}

fn classify(name: &str, is_directory: bool, is_project: bool) -> Entry {
    let project_only = PROJECT_ONLY_FILES.contains(&name) || name == "snapshots";
    if is_directory {
        match name {
            _ if DEFINITION_DIRECTORIES.contains(&name) => Entry::Definitions,
            "schemas" => Entry::Ignored,
            "snapshots" if is_project => Entry::Ignored,
            _ if project_only && !is_project => Entry::ProjectOnly,
            _ => Entry::Unknown,
        }
    } else if let Some((_, key)) = SUBTREE_FILES.iter().find(|(file, _)| *file == name) {
        Entry::Subtree(key)
    } else if name == "config.yaml" || (is_project && PROJECT_ONLY_FILES.contains(&name)) {
        Entry::Ignored
    } else if project_only && !is_project {
        Entry::ProjectOnly
    } else {
        Entry::Unknown
    }
}

pub(crate) fn load(layer: &Layer) -> Result<LoadedLayer, Vec<Problem>> {
    let mut loader = Loader {
        layer,
        tree: Map::new(),
        schemas: BTreeMap::new(),
        problems: Vec::new(),
    };
    loader.load_root();
    if loader.problems.is_empty() {
        Ok(LoadedLayer {
            tree: loader.tree,
            schemas: loader.schemas,
        })
    } else {
        Err(loader.problems)
    }
}

struct Loader<'a> {
    layer: &'a Layer,
    tree: Map<String, Value>,
    schemas: BTreeMap<String, Value>,
    problems: Vec<Problem>,
}

impl Loader<'_> {
    fn origin(&self) -> &Origin {
        &self.layer.origin
    }

    fn layout(&mut self, path: &str, message: &str) {
        let problem = file_problem(Category::Layout, self.origin(), path, message);
        self.problems.push(problem);
    }

    fn load_root(&mut self) {
        let is_project = self.layer.origin == Origin::Project;
        for (name, is_directory) in self.entries(&self.layer.root.clone(), "") {
            match classify(&name, is_directory, is_project) {
                Entry::Definitions => self.load_definitions(&name),
                Entry::Subtree(key) => {
                    if let Some(value) = self.read_yaml(&name) {
                        self.tree.insert(key.to_owned(), value);
                    }
                }
                Entry::Ignored => {}
                Entry::ProjectOnly => {
                    self.layout(&name, "belongs only to the project, not to a package");
                }
                Entry::Unknown => self.layout(&name, "unknown entry"),
            }
        }
    }

    fn load_definitions(&mut self, directory: &str) {
        let mut definitions = Map::new();
        let path = self.layer.root.join(directory);
        for (name, is_directory) in self.entries(&path, directory) {
            let relative = format!("{directory}/{name}");
            if is_directory {
                self.layout(&relative, "definition directories have no subdirectories");
                continue;
            }
            if directory == "plugins" && name.ends_with(".wasm") && name.len() > ".wasm".len() {
                continue;
            }
            let Some(stem) = name.strip_suffix(".yaml").filter(|stem| !stem.is_empty()) else {
                self.layout(&relative, "unknown entry, definitions are `<name>.yaml`");
                continue;
            };
            if let Some(mut value) = self.read_yaml(&relative) {
                if directory == "kinds" {
                    self.take_schema(stem, &relative, &mut value);
                }
                definitions.insert(stem.to_owned(), value);
            }
        }
        if !definitions.is_empty() {
            self.tree
                .insert(directory.to_owned(), Value::Object(definitions));
        }
    }

    fn take_schema(&mut self, kind: &str, relative: &str, definition: &mut Value) {
        // The path stays in the tree as a leaf, the content is embedded after merging.
        let Some(schema) = definition.as_object().and_then(|d| d.get("schema")) else {
            return;
        };
        let problem = |message: String| {
            file_problem(Category::SchemaPath, &self.layer.origin, relative, message)
        };
        let result = match schema {
            Value::Null => return,
            Value::String(path) => self.read_schema(path),
            _ => Err("an inline schema is not allowed, `schema` is a path to a JSON file".into()),
        };
        match result {
            Ok(content) => {
                self.schemas.insert(kind.to_owned(), content);
            }
            Err(message) => {
                let problem = problem(message);
                self.problems.push(problem);
            }
        }
    }

    fn read_schema(&self, path: &str) -> Result<Value, String> {
        let relative = Path::new(path);
        let stays_inside = !path.contains('\\')
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        if !stays_inside {
            return Err(format!("`{path}` leaves the layer"));
        }
        let root = self
            .layer
            .root
            .canonicalize()
            .map_err(|error| format!("layer root cannot be read: {error}"))?;
        let file = root
            .join(relative)
            .canonicalize()
            .map_err(|_| format!("`{path}` does not exist"))?;
        if !file.starts_with(&root) {
            return Err(format!("`{path}` leaves the layer"));
        }
        let bytes =
            std::fs::read(&file).map_err(|error| format!("`{path}` cannot be read: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("`{path}` is not JSON: {error}"))
    }

    fn read_yaml(&mut self, relative: &str) -> Option<Value> {
        let path = self.layer.root.join(relative);
        let result = std::fs::read(&path)
            .map_err(|error| vec![format!("cannot be read: {error}")])
            .and_then(|bytes| {
                yaml::parse(&bytes)
                    .map_err(|violations| violations.iter().map(ToString::to_string).collect())
            });
        match result {
            Ok(value) => Some(value),
            Err(messages) => {
                for message in messages {
                    let problem =
                        file_problem(Category::YamlProfile, self.origin(), relative, message);
                    self.problems.push(problem);
                }
                None
            }
        }
    }

    fn entries(&mut self, directory: &Path, relative: &str) -> Vec<(String, bool)> {
        let listing = match std::fs::read_dir(directory) {
            Ok(listing) => listing,
            Err(error) => {
                self.layout(relative, &format!("cannot be read: {error}"));
                return Vec::new();
            }
        };
        let mut entries: Vec<(String, bool)> = listing
            .filter_map(Result::ok)
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path().is_dir(),
                )
            })
            .collect();
        entries.sort();
        entries
    }
}
