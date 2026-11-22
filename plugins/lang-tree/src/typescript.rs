//! TypeScript and JavaScript (`ts`, `tsx`, `mts`, `cts`, `js`, `jsx`, `mjs`, `cjs`).
//!
//! Declarations: functions, classes, interfaces, enums and their members, type aliases, variables,
//! and namespaces; class members, interface members. A top-level declaration is public when it is
//! exported, a member unless it is `private`, `protected`, or `#name`. Separator `.`. Every
//! parameter is passed by position.

use tree_sitter::Node;

use crate::{
    Declared, Entry, Parameter, Rules, child_of_kind, children_of_kind, collapse, field, keywords,
    text, token_form,
};

/// The rules for TypeScript and JavaScript.
pub const RULES: Rules = Rules {
    grammar,
    separator: ".",
    declaration,
};

const MODIFIERS: [&str; 6] = [
    "abstract", "async", "const", "declare", "override", "readonly",
];

fn grammar(extension: &str) -> tree_sitter::Language {
    match extension {
        "tsx" | "jsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    }
}

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let enum_member = node.kind() == "property_identifier"
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "enum_body");
    let (kind, container) = match node.kind() {
        "class_declaration" | "abstract_class_declaration" => ("class", true),
        "interface_declaration" => ("interface", true),
        "enum_declaration" => ("enum", true),
        "internal_module" | "module" => ("namespace", true),
        "function_declaration" | "function_signature" => ("function", false),
        "generator_function_declaration" => ("generator", false),
        "type_alias_declaration" => ("type", false),
        "variable_declarator" => ("variable", false),
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            (method_kind(node, source), false)
        }
        "public_field_definition" | "property_signature" => ("field", false),
        "enum_assignment" => ("variant", false),
        _ if enum_member => ("variant", false),
        _ => return None,
    };
    let name = if enum_member {
        text(node, source)
    } else {
        field(node, "name", source)?
    };
    let mut entry = Entry {
        kind,
        type_parameters: type_parameters(node, source),
        is_static: keywords(node, &["static"]).len() == 1,
        bases: bases(node, source),
        modifiers: keywords(node, &MODIFIERS),
        attributes: decorators(node, source),
        parameters: parameters(node, source),
        returns: node
            .child_by_field_name("return_type")
            .and_then(|annotation| annotated(annotation, source)),
        value_type: node
            .child_by_field_name("type")
            .and_then(|annotation| annotated(annotation, source)),
        ..Entry::default()
    };
    if node.kind().contains("generator") || keywords(node, &["*"]).len() == 1 {
        entry.kind = "generator";
    }
    if keywords(node, &["?"]).len() == 1 {
        entry.modifiers.push("?".into());
    }
    match node.kind() {
        "type_alias_declaration" => {
            entry.value_type = node
                .child_by_field_name("value")
                .map(|value| type_text(value, source));
            entry.text = Some(collapse(text(node, source).trim_end_matches(';')));
        }
        "variable_declarator" => {
            let list = node.parent()?;
            entry.modifiers = list
                .child_by_field_name("kind")
                .map(|keyword| vec![text(keyword, source).to_owned()])
                .unwrap_or_default();
            let declared = field(node, "type", source).unwrap_or("");
            entry.text = Some(collapse(&format!(
                "{} {name}{declared}",
                entry.modifiers.join(" ")
            )));
        }
        "public_field_definition" | "enum_assignment" | "property_identifier" => {
            if node.kind() == "enum_assignment" {
                entry.value = node
                    .child_by_field_name("value")
                    .map(|value| token_form(value, source));
            }
            let end = node
                .child_by_field_name("value")
                .map_or(node.end_byte(), |value| value.start_byte());
            entry.text = Some(collapse(
                source[node.start_byte()..end].trim_end_matches([' ', '=']),
            ));
        }
        "property_signature" | "method_signature" => {
            entry.text = Some(collapse(text(node, source).trim_end_matches([';', ','])));
        }
        _ => {}
    }
    Some(Declared {
        name: name.to_owned(),
        public: public(node, source, name),
        container,
        entry,
    })
}

fn method_kind(node: Node<'_>, source: &str) -> &'static str {
    if field(node, "name", source) == Some("constructor") {
        "constructor"
    } else if keywords(node, &["get"]).len() == 1 {
        "getter"
    } else if keywords(node, &["set"]).len() == 1 {
        "setter"
    } else {
        "method"
    }
}

fn type_parameters(node: Node<'_>, source: &str) -> Vec<String> {
    node.child_by_field_name("type_parameters")
        .map(|list| {
            children_of_kind(list, "type_parameter")
                .into_iter()
                .map(|parameter| token_form(parameter, source))
                .collect()
        })
        .unwrap_or_default()
}

// `extends` and `implements` of a class, `extends` of an interface.
fn bases(node: Node<'_>, source: &str) -> Vec<String> {
    let mut bases = Vec::new();
    if let Some(heritage) = child_of_kind(node, "class_heritage") {
        let mut cursor = heritage.walk();
        for clause in heritage.named_children(&mut cursor) {
            if clause.kind() == "extends_clause" {
                bases.push(
                    token_form(clause, source)
                        .trim_start_matches("extends ")
                        .to_owned(),
                );
            } else {
                let mut cursor = clause.walk();
                bases.extend(
                    clause
                        .named_children(&mut cursor)
                        .map(|base| token_form(base, source)),
                );
            }
        }
    }
    if let Some(clause) = child_of_kind(node, "extends_type_clause") {
        let mut cursor = clause.walk();
        bases.extend(
            clause
                .named_children(&mut cursor)
                .map(|base| token_form(base, source)),
        );
    }
    bases
}

// A decorator of an exported class sits on the `export` statement.
fn decorators(node: Node<'_>, source: &str) -> Vec<String> {
    let mut decorators = children_of_kind(node, "decorator");
    if let Some(statement) = node
        .parent()
        .filter(|parent| parent.kind() == "export_statement")
    {
        decorators.extend(children_of_kind(statement, "decorator"));
    }
    decorators
        .into_iter()
        .map(|decorator| token_form(decorator, source))
        .collect()
}

fn parameters(node: Node<'_>, source: &str) -> Vec<Parameter> {
    let Some(list) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = list.walk();
    list.named_children(&mut cursor)
        .filter(|parameter| parameter.kind().ends_with("_parameter"))
        .filter_map(|parameter| {
            let pattern = parameter.child_by_field_name("pattern")?;
            let variadic = pattern.kind() == "rest_pattern";
            let name = if variadic {
                pattern.named_child(0).unwrap_or(pattern)
            } else {
                pattern
            };
            let default = parameter
                .child_by_field_name("value")
                .map(|value| token_form(value, source));
            Some(Parameter {
                name: token_form(name, source),
                value_type: parameter
                    .child_by_field_name("type")
                    .and_then(|annotation| annotated(annotation, source)),
                optional: parameter.kind() == "optional_parameter" || default.is_some(),
                variadic,
                default,
                passing: "positional",
            })
        })
        .collect()
}

// The type of a `: T` annotation.
fn annotated(annotation: Node<'_>, source: &str) -> Option<String> {
    annotation
        .named_child(0)
        .map(|kind| type_text(kind, source))
}

// A union or intersection over several lines starts with a `|` or `&` that only formatting adds.
fn type_text(node: Node<'_>, source: &str) -> String {
    let form = token_form(node, source);
    match form.strip_prefix(['|', '&']) {
        Some(rest) if matches!(node.kind(), "union_type" | "intersection_type") => rest.to_owned(),
        _ => form,
    }
}

fn public(node: Node<'_>, source: &str, name: &str) -> bool {
    if name.starts_with('#') {
        return false;
    }
    if let Some(modifier) = child_of_kind(node, "accessibility_modifier") {
        return text(modifier, source) == "public";
    }
    let mut parent = node.parent();
    // A variable declarator sits in a declaration list.
    if node.kind() == "variable_declarator" {
        parent = parent.and_then(|list| list.parent());
    }
    // At the top level and in a namespace, only an `export` makes a declaration visible.
    !matches!(
        parent.map(|parent| parent.kind()),
        Some("program" | "statement_block")
    )
}
