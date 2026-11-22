//! Dart (`dart`).
//!
//! Declarations: top-level functions and variables, classes, mixins, extensions, enums, and their
//! methods, getters, setters, constructors, fields, and enum values. A name is public unless it
//! starts with `_`. Separator `.`. A parameter in `{…}` is passed by name, any other by
//! position.

use tree_sitter::Node;

use crate::{
    Declared, Entry, Parameter, Rules, child_of_kind, children_of_kind, collapse, field, head,
    keywords, text, token_form,
};

/// The rules for Dart.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_dart::LANGUAGE.into(),
    separator: ".",
    declaration,
};

const SIGNATURES: [&str; 6] = [
    "function_signature",
    "getter_signature",
    "setter_signature",
    "constructor_signature",
    "factory_constructor_signature",
    "constant_constructor_signature",
];

const CLASS_MODIFIERS: [&str; 6] = ["abstract", "base", "final", "interface", "mixin", "sealed"];

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let (name, container, entry) = match node.kind() {
        "class_declaration"
        | "mixin_declaration"
        | "extension_declaration"
        | "enum_declaration" => {
            let entry = Entry {
                kind: match node.kind() {
                    "class_declaration" => "class",
                    "mixin_declaration" => "mixin",
                    "extension_declaration" => "extension",
                    _ => "enum",
                },
                type_parameters: type_parameters(node, source),
                self_type: node
                    .child_by_field_name("class")
                    .map(|kind| token_form(kind, source)),
                bases: bases(node, source),
                modifiers: keywords(node, &CLASS_MODIFIERS),
                attributes: annotations(node, source),
                text: Some(collapse(head(node, source))),
                ..Entry::default()
            };
            (field(node, "name", source)?, true, entry)
        }
        "function_declaration" | "method_declaration" => {
            let signature = node.child_by_field_name("signature")?;
            let (name, mut entry) = function(signature, source)?;
            entry.attributes = annotations(annotated_owner(node), source);
            entry.text = Some(collapse(text(signature, source)));
            (name, false, entry)
        }
        // An abstract method or a constructor without a body.
        "declaration"
            if SIGNATURES
                .iter()
                .any(|kind| child_of_kind(node, kind).is_some()) =>
        {
            let (name, mut entry) = function(node, source)?;
            entry.attributes = annotations(annotated_owner(node), source);
            entry.text = Some(collapse(text(node, source)));
            (name, false, entry)
        }
        "initialized_identifier" | "static_final_declaration" => {
            let name = field(node, "name", source)
                .or_else(|| child_of_kind(node, "identifier").map(|name| text(name, source)))?;
            (name, false, variable(node, name, source))
        }
        "enum_entry" => {
            let entry = Entry {
                kind: "variant",
                value: child_of_kind(node, "arguments")
                    .map(|arguments| token_form(arguments, source)),
                attributes: annotations(node, source),
                text: Some(collapse(text(node, source))),
                ..Entry::default()
            };
            (field(node, "name", source)?, false, entry)
        }
        _ => return None,
    };
    Some(Declared {
        name: name.to_owned(),
        public: !name.starts_with('_'),
        container,
        entry,
    })
}

// `node` is a signature, possibly wrapping the inner signature of a method.
fn function<'s>(node: Node<'_>, source: &'s str) -> Option<(&'s str, Entry)> {
    let inner = if SIGNATURES.contains(&node.kind()) {
        node
    } else {
        SIGNATURES
            .iter()
            .find_map(|kind| child_of_kind(node, kind))?
    };
    let name = field(inner, "name", source)?;
    let in_type = node
        .parent()
        .is_some_and(|parent| parent.kind() != "source_file" && parent.kind() != "program");
    let kind = match inner.kind() {
        "getter_signature" => "getter",
        "setter_signature" => "setter",
        "function_signature" if in_type => "method",
        "function_signature" => "function",
        _ => "constructor",
    };
    let mut modifiers = keywords(node, &["abstract", "external"]);
    modifiers.extend(keywords(inner, &["const", "external", "factory"]));
    let entry = Entry {
        kind,
        type_parameters: type_parameters(inner, source),
        is_static: !keywords(node, &["static"]).is_empty()
            || !keywords(inner, &["static"]).is_empty(),
        modifiers,
        parameters: parameters(inner, source),
        returns: inner
            .child_by_field_name("return_type")
            .map(|kind| token_form(kind, source)),
        ..Entry::default()
    };
    Some((name, entry))
}

fn variable(node: Node<'_>, name: &str, source: &str) -> Entry {
    let declaration = enclosing_declaration(node);
    let mut modifiers = keywords(declaration, &["const", "covariant", "final", "late"]);
    let is_static = !keywords(declaration, &["static"]).is_empty();
    let value_type = child_of_kind(declaration, "type").map(|kind| token_form(kind, source));
    let constant = modifiers.iter().any(|modifier| modifier == "const");
    let value = if constant {
        node.child_by_field_name("value")
            .map(|value| token_form(value, source))
    } else {
        None
    };
    modifiers.sort();
    let in_type = declaration.kind() == "declaration";
    let head = [
        keywords(
            declaration,
            &["static", "const", "covariant", "final", "late"],
        )
        .join(" "),
        value_type.clone().unwrap_or_default(),
        name.to_owned(),
    ]
    .join(" ");
    Entry {
        kind: if in_type { "field" } else { "variable" },
        is_static,
        modifiers,
        attributes: annotations(annotated_owner(declaration), source),
        value_type,
        value,
        text: Some(collapse(&head)),
        ..Entry::default()
    }
}

fn type_parameters(node: Node<'_>, source: &str) -> Vec<String> {
    node.child_by_field_name("type_parameters")
        .or_else(|| child_of_kind(node, "type_parameters"))
        .map(|list| {
            children_of_kind(list, "type_parameter")
                .into_iter()
                .map(|parameter| token_form(parameter, source))
                .collect()
        })
        .unwrap_or_default()
}

// `extends`, `with`, `implements`, and the `on` of a mixin, each type in order.
fn bases(node: Node<'_>, source: &str) -> Vec<String> {
    let mut bases = Vec::new();
    let mut cursor = node.walk();
    for clause in node.named_children(&mut cursor) {
        if !matches!(
            clause.kind(),
            "superclass" | "mixins" | "interfaces" | "mixin_application_class"
        ) && !(node.kind() == "mixin_declaration" && clause.kind() == "type")
        {
            continue;
        }
        collect_types(clause, source, &mut bases);
    }
    bases
}

fn collect_types(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if node.kind() == "type" {
        out.push(token_form(node, source));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_types(child, source, out);
    }
}

fn parameters(node: Node<'_>, source: &str) -> Vec<Parameter> {
    let Some(list) = node
        .child_by_field_name("parameters")
        .or_else(|| child_of_kind(node, "formal_parameter_list"))
    else {
        return Vec::new();
    };
    let mut parameters = Vec::new();
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        match child.kind() {
            "formal_parameter" => parameters.extend(parameter(child, source, false, "positional")),
            "optional_formal_parameters" => optional_parameters(child, source, &mut parameters),
            _ => {}
        }
    }
    parameters
}

// `[…]` holds optional positional parameters, `{…}` named ones, optional unless `required`. The
// grammar puts `required` and a default value next to a parameter, not inside it.
fn optional_parameters(node: Node<'_>, source: &str, out: &mut Vec<Parameter>) {
    let named = keywords(node, &["["]).is_empty();
    let passing = if named { "named" } else { "positional" };
    let mut required = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "required" => required = true,
            "formal_parameter" => {
                out.extend(parameter(child, source, !required, passing));
                required = false;
            }
            _ if node.field_name_for_child(child_index(node, child)) == Some("default_value") => {
                if let Some(last) = out.last_mut() {
                    last.default = Some(token_form(child, source));
                }
            }
            _ => {}
        }
    }
}

fn child_index(parent: Node<'_>, child: Node<'_>) -> u32 {
    let mut cursor = parent.walk();
    let index = parent
        .children(&mut cursor)
        .position(|candidate| candidate.id() == child.id())
        .unwrap_or_default();
    u32::try_from(index).unwrap_or_default()
}

fn parameter(
    node: Node<'_>,
    source: &str,
    optional: bool,
    passing: &'static str,
) -> Option<Parameter> {
    let name = field(node, "name", source)
        .or_else(|| {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find_map(|child| field(child, "name", source))
        })
        .map_or_else(|| token_form(node, source), str::to_owned);
    Some(Parameter {
        name,
        value_type: child_of_kind(node, "type").map(|kind| token_form(kind, source)),
        optional,
        variadic: false,
        default: None,
        passing,
    })
}

// The annotations before a declaration: children of the declaration, or of the member that
// wraps it.
fn annotations(node: Node<'_>, source: &str) -> Vec<String> {
    children_of_kind(node, "annotation")
        .into_iter()
        .map(|annotation| token_form(annotation, source))
        .collect()
}

fn annotated_owner(node: Node<'_>) -> Node<'_> {
    match node.parent() {
        Some(parent) if parent.kind() == "member_declaration" => parent,
        _ => node,
    }
}

// The declaration that holds the modifiers and the type of a variable.
fn enclosing_declaration(node: Node<'_>) -> Node<'_> {
    let mut declaration = node;
    while let Some(parent) = declaration.parent() {
        declaration = parent;
        if matches!(
            parent.kind(),
            "declaration" | "top_level_variable_declaration"
        ) {
            break;
        }
    }
    declaration
}
