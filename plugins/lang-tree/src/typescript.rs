//! TypeScript and JavaScript (`ts`, `tsx`, `mts`, `cts`, `js`, `jsx`, `mjs`, `cjs`).
//!
//! Declarations: functions, classes, interfaces, enums and their members, type aliases, variables,
//! and namespaces; class members, interface members. A top-level declaration is public when it is
//! exported, a member unless it is `private`, `protected`, or `#name`. Separator `.`.

use tree_sitter::Node;

use crate::{Declared, Rules, child_of_kind, collapse, field, head, text};

/// The rules for TypeScript and JavaScript.
pub const RULES: Rules = Rules {
    grammar,
    separator: ".",
    declaration,
};

fn grammar(extension: &str) -> tree_sitter::Language {
    match extension {
        "tsx" | "jsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    }
}

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let container = matches!(
        node.kind(),
        "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "internal_module"
            | "module"
    );
    let leaf = matches!(
        node.kind(),
        "function_declaration"
            | "generator_function_declaration"
            | "function_signature"
            | "type_alias_declaration"
            | "variable_declarator"
            | "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "public_field_definition"
            | "property_signature"
            | "enum_assignment"
    ) || (node.kind() == "property_identifier"
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "enum_body"));
    if !container && !leaf {
        return None;
    }
    let name = if node.kind() == "property_identifier" {
        text(node, source)
    } else {
        field(node, "name", source)?
    };
    let signature = match node.kind() {
        // A variable's value is not its API, its declared type is.
        "variable_declarator" => {
            let kind = node
                .parent()
                .and_then(|list| list.child(0))
                .map_or("", |keyword| text(keyword, source));
            let declared = field(node, "type", source).unwrap_or("");
            Some(collapse(&format!("{kind} {name}{declared}")))
        }
        "public_field_definition" | "enum_assignment" | "property_identifier" => {
            let end = node
                .child_by_field_name("value")
                .map_or(node.end_byte(), |value| value.start_byte());
            Some(collapse(
                source[node.start_byte()..end].trim_end_matches([' ', '=']),
            ))
        }
        "type_alias_declaration" | "property_signature" | "method_signature" => {
            Some(collapse(text(node, source).trim_end_matches([';', ','])))
        }
        _ => Some(collapse(head(node, source))),
    };
    Some(Declared {
        name: name.to_owned(),
        public: public(node, source, name),
        container,
        signature,
    })
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
