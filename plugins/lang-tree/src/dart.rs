//! Dart (`dart`).
//!
//! Declarations: top-level functions and variables, classes, mixins, extensions, enums, and their
//! methods, getters, setters, constructors, fields, and enum values. A name is public unless it
//! starts with `_`. Separator `.`.

use tree_sitter::Node;

use crate::{Declared, Rules, child_of_kind, collapse, field, head, text};

/// The rules for Dart.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_dart::LANGUAGE.into(),
    separator: ".",
    declaration,
};

const SIGNATURES: [&str; 5] = [
    "function_signature",
    "getter_signature",
    "setter_signature",
    "constructor_signature",
    "factory_constructor_signature",
];

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let container = matches!(
        node.kind(),
        "class_declaration" | "mixin_declaration" | "extension_declaration" | "enum_declaration"
    );
    let (name, signature) = match node.kind() {
        _ if container => (field(node, "name", source)?, collapse(head(node, source))),
        "function_declaration" | "method_declaration" => {
            let signature = node.child_by_field_name("signature")?;
            (
                signature_name(signature, source)?,
                collapse(text(signature, source)),
            )
        }
        // An abstract method or a constructor without a body.
        "declaration"
            if SIGNATURES
                .iter()
                .any(|kind| child_of_kind(node, kind).is_some()) =>
        {
            let signature = SIGNATURES
                .iter()
                .find_map(|kind| child_of_kind(node, kind))?;
            (
                field(signature, "name", source)?,
                collapse(text(node, source)),
            )
        }
        "initialized_identifier" | "static_final_declaration" => {
            let name = field(node, "name", source)
                .or_else(|| child_of_kind(node, "identifier").map(|name| text(name, source)))?;
            (
                name,
                collapse(&format!("{} {name}", declared_type(node, source))),
            )
        }
        "enum_entry" => (field(node, "name", source)?, collapse(text(node, source))),
        _ => return None,
    };
    Some(Declared {
        name: name.to_owned(),
        public: !name.starts_with('_'),
        container,
        signature: Some(signature),
    })
}

fn signature_name<'s>(signature: Node<'_>, source: &'s str) -> Option<&'s str> {
    field(signature, "name", source).or_else(|| {
        SIGNATURES
            .iter()
            .find_map(|kind| child_of_kind(signature, kind))
            .and_then(|inner| field(inner, "name", source))
    })
}

// The modifiers and the type before the names of a variable declaration, for example `final int`.
fn declared_type<'s>(node: Node<'_>, source: &'s str) -> &'s str {
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
    let mut cursor = declaration.walk();
    let end = declaration
        .named_children(&mut cursor)
        .find(|child| child.kind().ends_with("_list") || child.kind() == "initialized_identifier")
        .map_or(declaration.start_byte(), |list| list.start_byte());
    source[declaration.start_byte()..end].trim()
}
