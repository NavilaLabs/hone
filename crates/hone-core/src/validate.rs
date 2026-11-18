use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{Registry, Validator};
use serde_json::{Map, Value};

use crate::error::{Category, Problem, pointer_problem};
use crate::pointer::Pointer;

const COMMON_SCHEMA: &str = include_str!("../../../schemas/common.schema.json");
const CONFIG_SCHEMA: &str = include_str!("../../../schemas/config.schema.json");

pub(crate) const STANDARD_GATES: [&str; 10] = [
    "artifact-exists",
    "status-is",
    "schema-valid",
    "ids-unique",
    "links-resolve",
    "no-open-blocking-threads",
    "threads-answered",
    "approvals-valid",
    "steps-complete",
    "change-needs-binding",
];

static CONFIG_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    let common: Value = serde_json::from_str(COMMON_SCHEMA).expect("common.schema.json is JSON");
    let config: Value = serde_json::from_str(CONFIG_SCHEMA).expect("config.schema.json is JSON");
    let common_id = common["$id"]
        .as_str()
        .expect("common.schema.json has an $id")
        .to_owned();
    let registry = Registry::new()
        .add(&common_id, &common)
        .expect("common.schema.json is a valid resource")
        .prepare()
        .expect("the registry of the protocol schemas resolves");
    jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&config)
        .expect("config.schema.json is a valid schema")
});

pub(crate) fn validate(document: &Value) -> Vec<Problem> {
    let problems = shape(document);
    if !problems.is_empty() {
        return problems;
    }
    let Value::Object(document) = document else {
        unreachable!("the schema requires an object")
    };
    References::check(document)
}

fn shape(document: &Value) -> Vec<Problem> {
    let mut problems = Vec::new();
    for error in CONFIG_VALIDATOR.iter_errors(document) {
        let at = Pointer::from_segments(
            error
                .instance_path()
                .as_str()
                .split('/')
                .skip(1)
                .map(|segment| segment.replace("~1", "/").replace("~0", "~")),
        );
        let message = error.to_string();
        match error.kind() {
            ValidationErrorKind::Required { property } => {
                let key = property
                    .as_str()
                    .map_or_else(|| property.to_string(), str::to_owned);
                problems.push(pointer_problem(Category::Shape, &at.child(key), message));
            }
            ValidationErrorKind::AdditionalProperties { unexpected } => {
                for key in unexpected {
                    problems.push(pointer_problem(
                        Category::Shape,
                        &at.child(key),
                        message.clone(),
                    ));
                }
            }
            _ => problems.push(pointer_problem(Category::Shape, &at, message)),
        }
    }
    problems
}

// The references that JSON Schema cannot check, listed in the description of the configuration
// schema. The accessors expect a document whose shape is valid.
struct References<'a> {
    kinds: &'a Map<String, Value>,
    roles: &'a Map<String, Value>,
    steps: &'a Map<String, Value>,
    plugins: Option<&'a Map<String, Value>>,
    problems: Vec<Problem>,
}

impl<'a> References<'a> {
    fn check(document: &'a Map<String, Value>) -> Vec<Problem> {
        let map = |key: &str| document[key].as_object().expect("shape checked");
        let mut references = References {
            kinds: map("kinds"),
            roles: map("roles"),
            steps: map("steps"),
            plugins: document.get("plugins").and_then(Value::as_object),
            problems: Vec::new(),
        };
        references.prefixes();
        references.actors(map("actors"));
        references.steps();
        references.cycles();
        references.profiles(map("profiles"), &document["select"]);
        references.problems
    }

    fn fail(&mut self, at: &Pointer, message: String) {
        self.problems
            .push(pointer_problem(Category::Reference, at, message));
    }

    fn need_role(&mut self, at: &Pointer, role: &Value, capability: Option<&str>) {
        let name = text(role);
        match self.roles.get(name) {
            None => self.fail(at, format!("unknown role `{name}`")),
            Some(definition) => {
                if let Some(capability) = capability {
                    let can = definition["can"].as_array().expect("shape checked");
                    if !can.iter().any(|item| item == capability) {
                        self.fail(at, format!("role `{name}` cannot `{capability}`"));
                    }
                }
            }
        }
    }

    fn need_kind(&mut self, at: &Pointer, kind: &Value, decided: bool) {
        let name = text(kind);
        match self.kinds.get(name) {
            None => self.fail(at, format!("unknown kind `{name}`")),
            Some(definition) if decided && definition["lifecycle"] != "decided" => {
                self.fail(at, format!("kind `{name}` must have lifecycle `decided`"));
            }
            Some(_) => {}
        }
    }

    fn need_statuses(&mut self, at: &Pointer, kind: &Value, selector: &Value) {
        let Some(definition) = self.kinds.get(text(kind)) else {
            return;
        };
        let Some(statuses) = definition.get("statuses").and_then(Value::as_object) else {
            self.fail(
                at,
                format!("kind `{}` has no statuses (lifecycle `living`)", text(kind)),
            );
            return;
        };
        let selected: Vec<(Pointer, &Value)> = match selector {
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, item)| (at.child(i), item))
                .collect(),
            single => vec![(at.clone(), single)],
        };
        for (here, status) in selected {
            let name = text(status);
            if name != "superseded" && !statuses.contains_key(name) {
                self.fail(
                    &here,
                    format!("`{name}` is not a status of the kind `{}`", text(kind)),
                );
            }
        }
    }

    fn prefixes(&mut self) {
        let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, kind) in self.kinds {
            let prefix = text(&kind["id_prefix"]);
            if let Some(owner) = owners.insert(prefix, name) {
                let at = Pointer::from_segments(["kinds", name, "id_prefix"]);
                self.fail(&at, format!("`{prefix}` is already used by `{owner}`"));
            }
        }
    }

    fn actors(&mut self, actors: &Map<String, Value>) {
        for (handle, roles) in actors {
            for (index, role) in list(roles).iter().enumerate() {
                let at = Pointer::from_segments(["actors", handle]).child(index);
                self.need_role(&at, role, None);
            }
        }
    }

    fn steps(&mut self) {
        let steps = self.steps;
        for (name, step) in steps {
            let at = Pointer::from_segments(["steps", name]);
            for (index, role) in list(&step["acts"]).iter().enumerate() {
                self.need_role(&at.child("acts").child(index), role, None);
            }
            if let Some(role) = step.get("decides") {
                self.need_role(&at.child("decides"), role, None);
            }
            for field in ["inputs", "outputs"] {
                for (index, kind) in list(&step[field]).iter().enumerate() {
                    if kind != "ticket" {
                        self.need_kind(&at.child(field).child(index), kind, false);
                    }
                }
            }
            for (index, required) in list(&step["requires"]).iter().enumerate() {
                if !steps.contains_key(text(required)) {
                    let message = format!("unknown step `{}`", text(required));
                    self.fail(&at.child("requires").child(index), message);
                }
            }
            for (label, entry) in step["gates"].as_object().expect("shape checked") {
                self.gate(&at.child("gates").child(label), entry);
            }
        }
    }

    fn gate(&mut self, at: &Pointer, entry: &Value) {
        let name = text(&entry["gate"]);
        let empty = Map::new();
        let params = entry
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let param = |key: &str| at.child("params").child(key);
        match name {
            "exec" => {
                if let Some(plugin) = params.get("evaluate")
                    && !self.has_plugin(text(plugin))
                {
                    self.fail(
                        &param("evaluate"),
                        format!("unknown plugin `{}`", text(plugin)),
                    );
                }
            }
            "artifact-exists" | "status-is" => {
                self.need_kind(&param("type"), &params["type"], true);
                if let Some(selector) = params.get("status") {
                    self.need_statuses(&param("status"), &params["type"], selector);
                }
            }
            "threads-answered" => self.need_role(&param("role"), &params["role"], None),
            "approvals-valid" => {
                self.need_role(&param("role"), &params["role"], Some("approve"));
                if let Some(kind) = params.get("type") {
                    self.need_kind(&param("type"), kind, true);
                }
            }
            "change-needs-binding" => {
                self.need_kind(&param("anchored"), &params["anchored"], false);
                self.need_kind(&param("binding"), &params["binding"], true);
            }
            standard if STANDARD_GATES.contains(&standard) => {}
            plugin if self.has_plugin(plugin) => {}
            unknown => self.fail(&at.child("gate"), format!("unknown gate `{unknown}`")),
        }
    }

    fn has_plugin(&self, name: &str) -> bool {
        self.plugins
            .is_some_and(|plugins| plugins.contains_key(name))
    }

    // Every step on a cycle is reported, so that the result does not depend on where the
    // search starts.
    fn cycles(&mut self) {
        let mut on_cycle = BTreeSet::new();
        for start in self.steps.keys() {
            let mut stack = vec![start.as_str()];
            let mut visited = BTreeSet::new();
            while let Some(current) = stack.pop() {
                for required in self.requires(current) {
                    if required == start {
                        on_cycle.insert(start.as_str());
                    } else if visited.insert(required) {
                        stack.push(required);
                    }
                }
            }
        }
        for name in on_cycle {
            let at = Pointer::from_segments(["steps", name, "requires"]);
            self.fail(&at, format!("step `{name}` lies on a cycle of `requires`"));
        }
    }

    fn requires(&self, step: &str) -> Vec<&'a str> {
        self.steps
            .get(step)
            .map(|step| list(&step["requires"]).iter().map(text).collect())
            .unwrap_or_default()
    }

    fn profiles(&mut self, profiles: &Map<String, Value>, select: &Value) {
        for (name, profile) in profiles {
            for step in profile["steps"].as_object().expect("shape checked").keys() {
                if !self.steps.contains_key(step) {
                    let at = Pointer::from_segments(["profiles", name, "steps", step]);
                    self.fail(&at, format!("unknown step `{step}`"));
                }
            }
        }
        for (index, rule) in list(&select["rules"]).iter().enumerate() {
            let profile = text(&rule["profile"]);
            if !profiles.contains_key(profile) {
                let at = Pointer::from_segments(["select", "rules"])
                    .child(index)
                    .child("profile");
                self.fail(&at, format!("unknown profile `{profile}`"));
            }
        }
        let default = text(&select["default"]);
        if !profiles.contains_key(default) {
            let at = Pointer::from_segments(["select", "default"]);
            self.fail(&at, format!("unknown profile `{default}`"));
        }
    }
}

fn text(value: &Value) -> &str {
    value.as_str().expect("shape checked")
}

fn list(value: &Value) -> &Vec<Value> {
    value.as_array().expect("shape checked")
}
