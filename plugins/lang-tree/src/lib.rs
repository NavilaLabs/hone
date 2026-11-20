//! The shared engine of hone's tree-sitter language plugins (concept Section 9, "Language
//! plugins"): it parses a file, collects its declarations with their nesting, and answers the
//! entry points `resolve` and `public_api` from them. A language contributes only its [`Rules`].
//!
//! A symbol is the names of the nested declarations joined by the language's separator, for
//! example `Fee.add` or `Fee::add`. The public API of a file is one string per public
//! declaration whose enclosing declarations are public too: its symbol, `: `, and its
//! signature, the declaration's text without its body and with whitespace collapsed.

#[cfg(feature = "dart")]
pub mod dart;
#[cfg(feature = "php")]
pub mod php;
#[cfg(feature = "python")]
pub mod python;
#[cfg(feature = "typescript")]
pub mod typescript;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use tree_sitter::{Node, Parser};

/// What a language contributes to the engine.
pub struct Rules {
    /// The grammar for a file with this extension (lowercase, without the dot).
    pub grammar: fn(&str) -> tree_sitter::Language,
    /// What joins the names of nested declarations in a symbol.
    pub separator: &'static str,
    /// The declaration a node is, if any.
    pub declaration: fn(Node<'_>, &str) -> Option<Declared>,
}

/// A node that declares a name.
pub struct Declared {
    /// The declared name.
    pub name: String,
    /// Whether other files may use it, given that its enclosing declarations are public.
    pub public: bool,
    /// Whether its children can declare members, for example a class.
    pub container: bool,
    /// The signature, or `None` for the node's text up to its `body` field.
    pub signature: Option<String>,
}

struct Found {
    symbol: String,
    public: bool,
    signature: String,
    line: usize,
}

/// The entry point `resolve`: `input` as the plugin contract defines it, the files read below
/// `/project` or `/base`. Returns the output as JSON text.
#[must_use]
pub fn resolve(rules: &Rules, input: &str) -> String {
    let input: Value = serde_json::from_str(input).unwrap_or_default();
    let root = mount(&input);
    let mut files: BTreeMap<String, Result<Vec<Found>, String>> = BTreeMap::new();
    let results: Vec<Value> = input["symbols"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            let path = entry["path"].as_str().unwrap_or_default();
            let symbol = entry["symbol"].as_str().unwrap_or_default();
            let declarations = files
                .entry(path.to_owned())
                .or_insert_with(|| read(rules, &format!("{root}/{path}"), path));
            match declarations {
                Ok(found) => match found.iter().find(|found| found.symbol == symbol) {
                    Some(found) => json!({ "status": "found", "line": found.line }),
                    None => json!({ "status": "missing" }),
                },
                Err(message) => json!({ "status": "unparsable", "message": message }),
            }
        })
        .collect();
    json!({ "results": results }).to_string()
}

/// The entry point `public_api`: `input` as the plugin contract defines it. Returns the output
/// as JSON text.
#[must_use]
pub fn public_api(rules: &Rules, input: &str) -> String {
    let input: Value = serde_json::from_str(input).unwrap_or_default();
    let root = mount(&input);
    let results: Vec<Value> = input["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|path| {
            let path = path.as_str().unwrap_or_default();
            match read(rules, &format!("{root}/{path}"), path) {
                Ok(found) => {
                    let symbols: BTreeSet<String> = found
                        .into_iter()
                        .filter(|found| found.public)
                        .map(|found| format!("{}: {}", found.symbol, found.signature))
                        .collect();
                    json!({ "status": "ok", "symbols": symbols })
                }
                Err(message) => json!({ "status": "unparsable", "message": message }),
            }
        })
        .collect();
    json!({ "results": results }).to_string()
}

fn mount(input: &Value) -> &'static str {
    if input["state"] == "base" {
        "/base"
    } else {
        "/project"
    }
}

fn read(rules: &Rules, file: &str, path: &str) -> Result<Vec<Found>, String> {
    let source = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_lowercase())
        .unwrap_or_default();
    declarations(rules, &extension, &source)
}

/// Every declaration of `source`, a file with the extension `extension`, as `(symbol, public,
/// signature, line)`.
///
/// # Errors
///
/// Returns a message naming the first line with a syntax error.
pub fn declarations_of(
    rules: &Rules,
    extension: &str,
    source: &str,
) -> Result<Vec<(String, bool, String, usize)>, String> {
    Ok(declarations(rules, extension, source)?
        .into_iter()
        .map(|found| (found.symbol, found.public, found.signature, found.line))
        .collect())
}

fn declarations(rules: &Rules, extension: &str, source: &str) -> Result<Vec<Found>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&(rules.grammar)(extension))
        .map_err(|error| error.to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "the parser gave up".to_owned())?;
    let root = tree.root_node();
    if root.has_error() {
        let line = first_error(root).map_or(1, |node| node.start_position().row + 1);
        return Err(format!("syntax error at line {line}"));
    }
    let mut found = Vec::new();
    collect(
        rules,
        root,
        source,
        &mut Vec::new(),
        true,
        false,
        &mut found,
    );
    Ok(found)
}

fn first_error(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.has_error())
        .find_map(first_error)
}

// `path` holds the names of the enclosing declarations, `public` whether all of them are public,
// and `container` whether `node` is a declaration whose body holds members.
fn collect(
    rules: &Rules,
    node: Node<'_>,
    source: &str,
    path: &mut Vec<String>,
    public: bool,
    container: bool,
    found: &mut Vec<Found>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let Some(declared) = (rules.declaration)(child, source) else {
            if container || !is_body(child) {
                collect(rules, child, source, path, public, false, found);
            }
            continue;
        };
        path.push(declared.name);
        let visible = public && declared.public;
        found.push(Found {
            symbol: path.join(rules.separator),
            public: visible,
            signature: declared
                .signature
                .unwrap_or_else(|| collapse(head(child, source))),
            line: child.start_position().row + 1,
        });
        if declared.container {
            collect(rules, child, source, path, visible, true, found);
        }
        path.pop();
    }
}

// The bodies of functions declare nothing other files can name.
fn is_body(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "statement_block" | "compound_statement" | "function_body" | "block_statement"
    )
}

/// The text of `node` up to its `body` field, or all of it without one.
#[must_use]
pub fn head<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    let end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    source[node.start_byte()..end].trim_end_matches([' ', '\t', '\n', '\r', ';', '=', '>'])
}

/// `text` with every run of whitespace replaced by one space.
#[must_use]
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of `node`.
#[must_use]
pub fn text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.byte_range()]
}

/// The text of the field `field` of `node`, if it has one.
#[must_use]
pub fn field<'s>(node: Node<'_>, field: &str, source: &'s str) -> Option<&'s str> {
    node.child_by_field_name(field)
        .map(|child| text(child, source))
}

/// The first named child of `node` of kind `kind`.
#[must_use]
pub fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}
