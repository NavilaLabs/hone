//! Python (`py`, `pyi`).
//!
//! Declarations: functions, classes and their methods, and assignments to a single name at the
//! top level of a module or a class. A name is public unless it starts with `_`; `__name__` is
//! public. `__all__` is not read. Separator `.`.
//!
//! A parameter is passed by position or by name, only by position before `/`, and only by name
//! after `*` or `*args`. The first parameter of a method is its receiver and not part of the
//! entry, unless the method is a `@staticmethod`. `@staticmethod` and `@classmethod` make a
//! method static, `@property` and `@<name>.setter` make it a property and a setter; these
//! decorators are not listed as attributes.

use tree_sitter::Node;

use crate::{Declared, Entry, Parameter, Rules, collapse, field, head, keywords, text, token_form};

/// The rules for Python.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_python::LANGUAGE.into(),
    separator: ".",
    declaration,
};

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let in_class = in_class_body(node);
    let (name, container, entry) = match node.kind() {
        "class_definition" => {
            let bases = node
                .child_by_field_name("superclasses")
                .map(|list| {
                    let mut cursor = list.walk();
                    list.named_children(&mut cursor)
                        .map(|base| token_form(base, source))
                        .collect()
                })
                .unwrap_or_default();
            let entry = Entry {
                kind: "class",
                bases,
                attributes: decorators(node, source),
                ..Entry::default()
            };
            (field(node, "name", source)?, true, entry)
        }
        "function_definition" => (field(node, "name", source)?, false, function(node, source)),
        "assignment" if at_declaration_level(node) => {
            let left = node.child_by_field_name("left")?;
            if left.kind() != "identifier" {
                return None;
            }
            let name = text(left, source);
            let declared = node.child_by_field_name("type");
            let entry = Entry {
                kind: if in_class { "field" } else { "variable" },
                text: Some(declared.map_or_else(
                    || name.to_owned(),
                    |kind| format!("{name}: {}", text(kind, source)),
                )),
                value_type: declared.map(|kind| token_form(kind, source)),
                ..Entry::default()
            };
            (name, false, entry)
        }
        _ => return None,
    };
    let mut entry = entry;
    if entry.text.is_none() {
        entry.text = Some(collapse(head(node, source).trim_end_matches(':')));
    }
    Some(Declared {
        name: name.to_owned(),
        public: !name.starts_with('_') || (name.starts_with("__") && name.ends_with("__")),
        container,
        entry,
    })
}

fn function(node: Node<'_>, source: &str) -> Entry {
    let mut attributes = decorators(node, source);
    let mut take = |decorator: &str| {
        let found = attributes.iter().any(|attribute| attribute == decorator);
        attributes.retain(|attribute| attribute != decorator);
        found
    };
    let is_static_method = take("@staticmethod");
    let is_class_method = take("@classmethod");
    let is_property = take("@property");
    let setter = attributes
        .iter()
        .position(|attribute| attribute.ends_with(".setter"));
    let mut kind = if in_class_body(node) {
        "method"
    } else {
        "function"
    };
    if is_property {
        kind = "property";
    }
    if let Some(index) = setter {
        attributes.remove(index);
        kind = "setter";
    }
    let mut parameters = parameters(node, source);
    if kind != "function" && !is_static_method && !parameters.is_empty() {
        parameters.remove(0);
    }
    Entry {
        kind,
        is_static: is_static_method || is_class_method,
        modifiers: keywords(node, &["async"]),
        attributes,
        parameters,
        returns: node
            .child_by_field_name("return_type")
            .map(|kind| token_form(kind, source)),
        ..Entry::default()
    }
}

fn parameters(node: Node<'_>, source: &str) -> Vec<Parameter> {
    let Some(list) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut parameters: Vec<Parameter> = Vec::new();
    let mut only_named = false;
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        match child.kind() {
            "positional_separator" => {
                for parameter in &mut parameters {
                    parameter.passing = "positional";
                }
                continue;
            }
            "keyword_separator" => {
                only_named = true;
                continue;
            }
            _ => {}
        }
        // `*args` or `**kwargs`, typed or not.
        let splat = if child.kind().ends_with("splat_pattern") {
            Some(child)
        } else if child.kind() == "typed_parameter" {
            child
                .named_child(0)
                .filter(|inner| inner.kind().ends_with("splat_pattern"))
        } else {
            None
        };
        let name = match splat {
            Some(splat) => splat.named_child(0).map(|name| text(name, source)),
            None if child.kind() == "identifier" => Some(text(child, source)),
            None => field(child, "name", source).or_else(|| {
                child
                    .named_child(0)
                    .filter(|inner| inner.kind() == "identifier")
                    .map(|inner| text(inner, source))
            }),
        };
        let Some(name) = name else { continue };
        let default = child
            .child_by_field_name("value")
            .map(|value| token_form(value, source));
        let passing = match splat.map(|splat| splat.kind()) {
            Some("list_splat_pattern") => {
                only_named = true;
                "positional"
            }
            Some(_) => "named",
            None if only_named => "named",
            None => "both",
        };
        parameters.push(Parameter {
            name: name.to_owned(),
            value_type: child
                .child_by_field_name("type")
                .map(|kind| token_form(kind, source)),
            optional: default.is_some(),
            variadic: splat.is_some(),
            default,
            passing,
        });
    }
    parameters
}

// The decorators of a decorated definition, each in token form with its `@`.
fn decorators(node: Node<'_>, source: &str) -> Vec<String> {
    node.parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .map(|parent| {
            let mut cursor = parent.walk();
            parent
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "decorator")
                .map(|decorator| token_form(decorator, source))
                .collect()
        })
        .unwrap_or_default()
}

// The body of a class, possibly through a decorated definition.
fn in_class_body(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    if parent.is_some_and(|parent| parent.kind() == "decorated_definition") {
        parent = parent.and_then(|parent| parent.parent());
    }
    if node.kind() == "assignment" {
        parent = parent.and_then(|statement| statement.parent());
    }
    parent.is_some_and(|block| {
        block.kind() == "block"
            && block
                .parent()
                .is_some_and(|owner| owner.kind() == "class_definition")
    })
}

// A module or class body, not a function body.
fn at_declaration_level(node: Node<'_>) -> bool {
    let Some(statement) = node
        .parent()
        .filter(|parent| parent.kind() == "expression_statement")
    else {
        return false;
    };
    match statement.parent() {
        Some(parent) if parent.kind() == "module" => true,
        Some(parent) if parent.kind() == "block" => parent
            .parent()
            .is_some_and(|owner| owner.kind() == "class_definition"),
        _ => false,
    }
}
