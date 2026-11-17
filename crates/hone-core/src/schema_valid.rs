use std::collections::HashMap;
use std::sync::LazyLock;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{Registry, Validator};
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::directory::{Artifact, Directory, Place};
use crate::finding::{Code, Finding};
use crate::pointer::Pointer;

const COMMON_SCHEMA: &str = include_str!("../../../schemas/common.schema.json");
const ARTIFACT_SCHEMA: &str = include_str!("../../../schemas/artifact.schema.json");

static COMMON: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(COMMON_SCHEMA).expect("common.schema.json is JSON"));
static ARTIFACT: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(ARTIFACT_SCHEMA).expect("artifact.schema.json is JSON"));
static ARTIFACT_ID: LazyLock<&'static str> = LazyLock::new(|| schema_id(&ARTIFACT));
static LINK_TYPES: LazyLock<Vec<String>> =
    LazyLock::new(|| property_names(&COMMON["$defs"]["Links"]));
static REGISTRY: LazyLock<Registry<'static>> = LazyLock::new(|| {
    Registry::new()
        .add(schema_id(&COMMON), &*COMMON)
        .expect("common.schema.json is a valid resource")
        .add(*ARTIFACT_ID, &*ARTIFACT)
        .expect("artifact.schema.json is a valid resource")
        .prepare()
        .expect("the registry of the protocol schemas resolves")
});

fn schema_id(schema: &'static Value) -> &'static str {
    schema["$id"]
        .as_str()
        .expect("protocol schemas have an $id")
}

pub(crate) struct ContentRules<'a> {
    kinds: &'a Map<String, Value>,
    validators: HashMap<String, Result<Validator, String>>,
}

impl<'a> ContentRules<'a> {
    pub(crate) fn new(config: &'a Value) -> Self {
        static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
        Self {
            kinds: config["kinds"].as_object().unwrap_or(&EMPTY),
            validators: HashMap::new(),
        }
    }

    pub(crate) fn check(&mut self, data: &Map<String, Value>) -> Vec<Finding> {
        let type_name = data.get("type").and_then(Value::as_str);
        let lifecycle = match type_name {
            Some("ticket") => "ticket",
            Some(name) => match self.kinds.get(name) {
                Some(kind) => kind["lifecycle"].as_str().unwrap_or_default(),
                None => {
                    return vec![
                        Finding::error(Code::KindUnknown, format!("unknown kind `{name}`"))
                            .at_pointer("/type"),
                    ];
                }
            },
            None => "",
        };
        let mut findings = Vec::new();
        if let Some(kind) = type_name.and_then(|name| self.kinds.get(name)) {
            findings.extend(prefix_and_status(kind, data));
        }
        let key = type_name.unwrap_or_default().to_owned();
        let kinds = self.kinds;
        let validator = self
            .validators
            .entry(key.clone())
            .or_insert_with(|| build_validator(type_name.and_then(|name| kinds.get(name))));
        let instance = Value::Object(data.clone());
        match validator {
            Ok(validator) => findings.extend(validator.iter_errors(&instance).flat_map(|error| {
                schema_findings(
                    error.instance_path().as_str(),
                    error.kind(),
                    error.to_string(),
                )
            })),
            Err(message) => {
                findings.push(Finding::error(Code::Schema, message.clone()));
            }
        }
        if type_name.is_some() {
            findings.extend(unknown_fields(
                lifecycle,
                type_name.and_then(|name| kinds.get(name)),
                data,
            ));
        }
        findings
    }
}

pub(crate) fn check_directory(config: &Value, directory: &Directory) -> Vec<Finding> {
    let mut rules = ContentRules::new(config);
    let mut findings = Vec::new();
    for unreadable in &directory.unreadable {
        for violation in &unreadable.violations {
            findings.push(
                Finding::error(Code::YamlProfile, violation.clone()).at_path(&unreadable.path),
            );
        }
    }
    for path in &directory.unmatched {
        findings.push(
            Finding::warning(
                Code::UnmatchedFile,
                "the name starts with the prefix of a kind, but the path matches no pattern",
            )
            .at_path(path),
        );
    }
    for artifact in &directory.artifacts {
        let mut found = path_mismatches(artifact);
        // With a type other than the pattern's kind, the content checks would judge the file by
        // the wrong kind, for example its prefix.
        if !found
            .iter()
            .any(|finding| finding.pointer.as_deref() == Some("/type"))
        {
            found.extend(rules.check(&artifact.content.data));
        }
        findings.extend(
            found
                .into_iter()
                .map(|finding| finding.at_path(&artifact.path)),
        );
    }
    findings
}

fn path_mismatches(artifact: &Artifact) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut expect = |pointer: &str, field: Option<&str>, in_path: &str| {
        if field != Some(in_path) {
            findings.push(
                Finding::error(
                    Code::PathMismatch,
                    format!(
                        "the path says `{in_path}`, the file says `{}`",
                        field.unwrap_or("nothing")
                    ),
                )
                .at_pointer(pointer),
            );
        }
    };
    expect("/type", artifact.type_name(), artifact.place.type_name());
    match artifact.place {
        Place::Ticket => {
            if let Some(ticket) = &artifact.ticket_in_path {
                expect("/id", artifact.id(), ticket);
            }
        }
        Place::Kind(_) => {
            if let Some(id) = &artifact.id_in_path {
                expect("/id", artifact.id(), id);
            }
            if let Some(ticket) = &artifact.ticket_in_path {
                expect("/ticket", artifact.field("ticket"), ticket);
            }
        }
    }
    findings
}

fn prefix_and_status(kind: &Value, data: &Map<String, Value>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let (Some(prefix), Some(id)) = (
        kind["id_prefix"].as_str(),
        data.get("id").and_then(Value::as_str),
    ) && !id.starts_with(&format!("{prefix}-"))
    {
        findings.push(
            Finding::error(
                Code::IdPrefix,
                format!("`{id}` must start with `{prefix}-`"),
            )
            .at_pointer("/id"),
        );
    }
    if let (Some(statuses), Some(status)) = (
        kind["statuses"].as_object(),
        data.get("status").and_then(Value::as_str),
    ) && !statuses.contains_key(status)
    {
        findings.push(
            Finding::error(
                Code::Status,
                format!("`{status}` is not a status of the kind"),
            )
            .at_pointer("/status"),
        );
    }
    findings
}

fn lifecycle_definition(lifecycle: &str) -> Option<&'static str> {
    match lifecycle {
        "decided" => Some("Decided"),
        "living" => Some("Living"),
        "ticket" => Some("Ticket"),
        _ => None,
    }
}

fn build_validator(kind: Option<&Value>) -> Result<Validator, String> {
    let mut parts = vec![json!({ "$ref": *ARTIFACT_ID })];
    if let Some(kind) = kind {
        if let Some(definition) = kind["lifecycle"].as_str().and_then(lifecycle_definition) {
            parts.push(json!({ "$ref": format!("{}#/$defs/{definition}", *ARTIFACT_ID) }));
        }
        if let Some(schema) = kind.get("schema") {
            parts.push(schema.clone());
        }
    }
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "allOf": parts,
    });
    jsonschema::draft202012::options()
        .with_registry(&REGISTRY)
        .should_validate_formats(true)
        .build(&schema)
        .map_err(|error| format!("the schema of the kind is not valid: {error}"))
}

fn schema_findings(
    instance_path: &str,
    kind: &ValidationErrorKind,
    message: String,
) -> Vec<Finding> {
    let at = Pointer::from_segments(
        instance_path
            .split('/')
            .skip(1)
            .map(|segment| segment.replace("~1", "/").replace("~0", "~")),
    );
    match kind {
        ValidationErrorKind::Required { property } => {
            let key = property
                .as_str()
                .map_or_else(|| property.to_string(), str::to_owned);
            vec![Finding::error(Code::Schema, message).at_pointer(at.child(key))]
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
            .iter()
            .map(|key| Finding::error(Code::Schema, message.clone()).at_pointer(at.child(key)))
            .collect(),
        _ if at == Pointer::root() => vec![Finding::error(Code::Schema, message)],
        _ => vec![Finding::error(Code::Schema, message).at_pointer(at)],
    }
}

fn unknown_fields(
    lifecycle: &str,
    kind: Option<&Value>,
    data: &Map<String, Value>,
) -> Vec<Finding> {
    let mut known = lifecycle_definition(lifecycle)
        .map(|definition| property_names(&ARTIFACT["$defs"][definition]))
        .unwrap_or_default();
    let mut patterns = Vec::new();
    if let Some(schema) = kind.and_then(|kind| kind.get("schema")) {
        known.extend(property_names(schema));
        if let Some(pattern_properties) = schema["patternProperties"].as_object() {
            patterns.extend(
                pattern_properties
                    .keys()
                    .filter_map(|key| Regex::new(key).ok()),
            );
        }
    }
    let unknown = |key: &str, known: &[String]| {
        !key.starts_with("x-")
            && !known.iter().any(|name| name == key)
            && !patterns.iter().any(|pattern| pattern.is_match(key))
    };
    let mut findings = Vec::new();
    for key in data.keys().filter(|key| unknown(key, &known)) {
        findings.push(
            Finding::warning(Code::UnknownField, format!("unknown field `{key}`"))
                .at_pointer(Pointer::root().child(key)),
        );
    }
    if let Some(links) = data.get("links").and_then(Value::as_object) {
        for key in links
            .keys()
            .filter(|key| !key.starts_with("x-") && !LINK_TYPES.iter().any(|name| name == *key))
        {
            findings.push(
                Finding::warning(Code::UnknownField, format!("unknown link type `{key}`"))
                    .at_pointer(Pointer::from_segments(["links", key])),
            );
        }
    }
    findings
}

fn property_names(schema: &Value) -> Vec<String> {
    schema["properties"]
        .as_object()
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}
