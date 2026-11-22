//! PHP (`php`).
//!
//! Declarations: functions, constants, classes, interfaces, traits, enums, and their methods,
//! properties (`$name`), class constants, and enum cases. A top-level declaration is public, a
//! member unless it is `private` or `protected`. Namespaces are not part of a symbol. Separator
//! `::`. Every parameter is passed by position or by name.

use tree_sitter::Node;

use crate::{
    Declared, Entry, Parameter, Rules, child_of_kind, children_of_kind, collapse, field, head,
    text, token_form,
};

/// The rules for PHP.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_php::LANGUAGE_PHP.into(),
    separator: "::",
    declaration,
};

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let kind = match node.kind() {
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "trait_declaration" => "trait",
        "enum_declaration" => "enum",
        "function_definition" => "function",
        "method_declaration" if field(node, "name", source) == Some("__construct") => "constructor",
        "method_declaration" => "method",
        "enum_case" => "variant",
        "const_element" => "constant",
        "property_element" => "field",
        _ => return None,
    };
    let container = matches!(kind, "class" | "interface" | "trait" | "enum");
    // A constant or a property shares its modifiers and type with the other elements of its
    // declaration.
    let owner = match node.kind() {
        "const_element" | "property_element" => node.parent()?,
        _ => node,
    };
    let name = match node.kind() {
        "const_element" => text(child_of_kind(node, "name")?, source),
        _ => field(node, "name", source)?,
    };
    let mut entry = Entry {
        kind,
        is_static: child_of_kind(owner, "static_modifier").is_some(),
        bases: bases(node, source),
        modifiers: ["abstract_modifier", "final_modifier", "readonly_modifier"]
            .iter()
            .filter_map(|modifier| child_of_kind(owner, modifier))
            .map(|modifier| text(modifier, source).to_owned())
            .collect(),
        attributes: attributes(owner, source),
        parameters: parameters(node, source),
        returns: node
            .child_by_field_name("return_type")
            .map(|kind| token_form(kind, source)),
        value_type: owner
            .child_by_field_name("type")
            .map(|kind| token_form(kind, source)),
        ..Entry::default()
    };
    match node.kind() {
        // The backing type of an enum, for example `string`.
        "enum_declaration" => {
            entry.value_type = child_of_kind(node, "primitive_type")
                .or_else(|| child_of_kind(node, "named_type"))
                .map(|kind| token_form(kind, source));
        }
        "enum_case" => {
            entry.value = node
                .child_by_field_name("value")
                .map(|value| token_form(value, source));
        }
        "const_element" => {
            entry.value = node.named_child(1).map(|value| token_form(value, source));
            entry.text = Some(collapse(&format!(
                "{} {name}",
                head_without_elements(owner, source)
            )));
        }
        "property_element" => {
            entry.text = Some(collapse(&format!(
                "{} {name}",
                head_without_elements(owner, source)
            )));
        }
        _ => {
            entry.text = Some(collapse(
                head(node, source).trim_end_matches(['{', ' ', ';']),
            ));
        }
    }
    let public = child_of_kind(owner, "visibility_modifier")
        .is_none_or(|modifier| text(modifier, source) == "public");
    Some(Declared {
        name: name.to_owned(),
        public,
        container,
        entry,
    })
}

// `extends` and `implements`.
fn bases(node: Node<'_>, source: &str) -> Vec<String> {
    ["base_clause", "class_interface_clause"]
        .iter()
        .filter_map(|clause| child_of_kind(node, clause))
        .flat_map(|clause| {
            let mut cursor = clause.walk();
            clause
                .named_children(&mut cursor)
                .map(|base| token_form(base, source))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn attributes(node: Node<'_>, source: &str) -> Vec<String> {
    let Some(list) = child_of_kind(node, "attribute_list") else {
        return Vec::new();
    };
    children_of_kind(list, "attribute_group")
        .into_iter()
        .flat_map(|group| children_of_kind(group, "attribute"))
        .map(|attribute| token_form(attribute, source))
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
            let default = parameter
                .child_by_field_name("default_value")
                .map(|value| token_form(value, source));
            Some(Parameter {
                name: token_form(parameter.child_by_field_name("name")?, source),
                value_type: parameter
                    .child_by_field_name("type")
                    .map(|kind| token_form(kind, source)),
                optional: default.is_some(),
                variadic: parameter.kind() == "variadic_parameter",
                default,
                passing: "both",
            })
        })
        .collect()
}

// The modifiers and type of a property or constant declaration, without its elements.
fn head_without_elements<'s>(owner: Node<'_>, source: &'s str) -> &'s str {
    let mut cursor = owner.walk();
    let end = owner
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "const_element" | "property_element"))
        .map_or(owner.end_byte(), |element| element.start_byte());
    source[owner.start_byte()..end].trim()
}
