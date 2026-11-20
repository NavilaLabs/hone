//! Python (`py`, `pyi`).
//!
//! Declarations: functions, classes and their methods, and assignments to a single name at the
//! top level of a module or a class. A name is public unless it starts with `_`; `__name__` is
//! public. `__all__` is not read. Separator `.`.

use tree_sitter::Node;

use crate::{Declared, Rules, collapse, field, head, text};

/// The rules for Python.
pub const RULES: Rules = Rules {
    grammar: |_| tree_sitter_python::LANGUAGE.into(),
    separator: ".",
    declaration,
};

fn declaration(node: Node<'_>, source: &str) -> Option<Declared> {
    let (name, container, signature) = match node.kind() {
        "class_definition" => (field(node, "name", source)?, true, None),
        "function_definition" => (field(node, "name", source)?, false, None),
        "assignment" if at_declaration_level(node) => {
            let left = node.child_by_field_name("left")?;
            if left.kind() != "identifier" {
                return None;
            }
            let name = text(left, source);
            let declared =
                field(node, "type", source).map_or(String::new(), |kind| format!(": {kind}"));
            (name, false, Some(format!("{name}{declared}")))
        }
        _ => return None,
    };
    let signature = signature.unwrap_or_else(|| collapse(head(node, source).trim_end_matches(':')));
    Some(Declared {
        name: name.to_owned(),
        public: !name.starts_with('_') || (name.starts_with("__") && name.ends_with("__")),
        container,
        signature: Some(signature),
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
