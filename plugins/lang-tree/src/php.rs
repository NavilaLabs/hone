//! PHP (`php`).
//!
//! Declarations: functions, constants, classes, interfaces, traits, enums, and their methods,
//! properties (`$name`), class constants, and enum cases. A top-level declaration is public, a
//! member unless it is `private` or `protected`. Namespaces are not part of a symbol. Separator
//! `::`.

use tree_sitter::Node;

use crate::{Declared, Rules, child_of_kind, collapse, field, head, text};

/// The rules for PHP.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_php::LANGUAGE_PHP.into(),
    separator: "::",
    declaration,
};

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let container = matches!(
        node.kind(),
        "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration"
    );
    let (name, signature) = match node.kind() {
        _ if container => (field(node, "name", source)?, None),
        "function_definition" | "method_declaration" | "enum_case" => {
            (field(node, "name", source)?, None)
        }
        "const_element" => {
            let name = text(child_of_kind(node, "name")?, source);
            let owner = node.parent()?;
            (
                name,
                Some(collapse(&format!(
                    "{} {name}",
                    head_without_elements(owner, source)
                ))),
            )
        }
        "property_element" => {
            let name = field(node, "name", source)?;
            let owner = node.parent()?;
            (
                name,
                Some(collapse(&format!(
                    "{} {name}",
                    head_without_elements(owner, source)
                ))),
            )
        }
        _ => return None,
    };
    let modifier_owner = match node.kind() {
        "const_element" | "property_element" => node.parent()?,
        _ => node,
    };
    let public = child_of_kind(modifier_owner, "visibility_modifier")
        .is_none_or(|modifier| text(modifier, source) == "public");
    let signature =
        signature.unwrap_or_else(|| collapse(head(node, source).trim_end_matches(['{', ' ', ';'])));
    Some(Declared {
        name: name.to_owned(),
        public,
        container,
        signature: Some(signature),
    })
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
