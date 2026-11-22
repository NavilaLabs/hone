//! The shared engine of hone's tree-sitter language plugins (concept Section 9, "Language
//! plugins"): it parses a file, collects its declarations with their nesting, and answers the
//! entry points `resolve` and `public_api` from them. A language contributes only its [`Rules`].
//!
//! A symbol is the names of the nested declarations joined by the language's separator, for
//! example `Fee.add` or `Fee::add`. The public API of a file is one entry (`spec/plugins.md`,
//! item 32) per public declaration whose enclosing declarations are public too.

#[cfg(feature = "dart")]
pub mod dart;
#[cfg(feature = "php")]
pub mod php;
#[cfg(feature = "python")]
pub mod python;
#[cfg(feature = "typescript")]
pub mod typescript;

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
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
    /// What its public API entry says about it.
    pub entry: Entry,
}

/// The fields of a public API entry other than `symbol` and `line` (`spec/plugins.md`, items 32
/// and 33). An empty list and a `false` flag are left out of the entry.
#[derive(Default)]
pub struct Entry {
    /// The kind of declaration, for example `function` or `class`.
    pub kind: &'static str,
    /// The source text without the body, or `None` for the node's text up to its `body` field.
    pub text: Option<String>,
    /// The generic parameters in order, each in token form.
    pub type_parameters: Vec<String>,
    /// Whether the member is called through its type instead of an instance.
    pub is_static: bool,
    /// The type the declaration extends, for example the type of a Dart extension.
    pub self_type: Option<String>,
    /// The supertypes in order, each in token form.
    pub bases: Vec<String>,
    /// The keywords other than visibility and `static`.
    pub modifiers: Vec<String>,
    /// The attributes, decorators, and annotations, each in token form.
    pub attributes: Vec<String>,
    /// The parameters in order.
    pub parameters: Vec<Parameter>,
    /// The return type in token form.
    pub returns: Option<String>,
    /// The type in token form.
    pub value_type: Option<String>,
    /// The value of a constant or an enum member in token form.
    pub value: Option<String>,
}

/// A parameter of a public API entry.
pub struct Parameter {
    /// The name, or the pattern of a destructured parameter in token form.
    pub name: String,
    /// The type in token form.
    pub value_type: Option<String>,
    /// Whether a caller may leave it out.
    pub optional: bool,
    /// Whether it takes any number of arguments.
    pub variadic: bool,
    /// The default value in token form.
    pub default: Option<String>,
    /// How a caller passes it: `positional`, `named`, or `both`.
    pub passing: &'static str,
}

struct Found {
    symbol: String,
    public: bool,
    line: usize,
    entry: Value,
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
                    let symbols: Vec<Value> = found
                        .into_iter()
                        .filter(|found| found.public)
                        .map(|found| found.entry)
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

/// Every declaration of `source`, a file with the extension `extension`, as whether it is public
/// and its public API entry.
///
/// # Errors
///
/// Returns a message naming the first line with a syntax error.
pub fn declarations_of(
    rules: &Rules,
    extension: &str,
    source: &str,
) -> Result<Vec<(bool, Value)>, String> {
    Ok(declarations(rules, extension, source)?
        .into_iter()
        .map(|found| (found.public, found.entry))
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
        let symbol = path.join(rules.separator);
        let line = child.start_position().row + 1;
        let text = declared
            .entry
            .text
            .clone()
            .unwrap_or_else(|| collapse(head(child, source)));
        found.push(Found {
            entry: to_json(&symbol, line, text, declared.entry),
            symbol,
            public: visible,
            line,
        });
        if declared.container {
            collect(rules, child, source, path, visible, true, found);
        }
        path.pop();
    }
}

fn to_json(symbol: &str, line: usize, text: String, entry: Entry) -> Value {
    let mut out = Map::new();
    out.insert("symbol".into(), symbol.into());
    out.insert("kind".into(), entry.kind.into());
    list(&mut out, "type_parameters", entry.type_parameters);
    if entry.is_static {
        out.insert("static".into(), true.into());
    }
    optional(&mut out, "self_type", entry.self_type);
    list(&mut out, "bases", entry.bases);
    list(&mut out, "modifiers", entry.modifiers);
    list(&mut out, "attributes", entry.attributes);
    let parameters: Vec<Value> = entry
        .parameters
        .into_iter()
        .map(|parameter| {
            let mut out = Map::new();
            out.insert("name".into(), parameter.name.into());
            optional(&mut out, "type", parameter.value_type);
            if parameter.optional {
                out.insert("optional".into(), true.into());
            }
            if parameter.variadic {
                out.insert("variadic".into(), true.into());
            }
            optional(&mut out, "default", parameter.default);
            out.insert("passing".into(), parameter.passing.into());
            Value::Object(out)
        })
        .collect();
    if !parameters.is_empty() {
        out.insert("parameters".into(), parameters.into());
    }
    optional(&mut out, "returns", entry.returns);
    optional(&mut out, "type", entry.value_type);
    optional(&mut out, "value", entry.value);
    out.insert("line".into(), line.into());
    out.insert("text".into(), text.into());
    Value::Object(out)
}

fn list(out: &mut Map<String, Value>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        out.insert(key.into(), values.into());
    }
}

fn optional(out: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        out.insert(key.into(), value.into());
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
    let text = source[node.start_byte()..end].trim_end();
    // The `=>` of an arrow body, but not the `>` that closes a generic type.
    let text = text.strip_suffix("=>").unwrap_or(text);
    text.trim_end_matches([' ', '\t', '\n', '\r', ';', '='])
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

/// The text of `node` in token form (`spec/plugins.md`, item 32), without comments. A string
/// literal is one token.
#[must_use]
pub fn token_form(node: Node<'_>, source: &str) -> String {
    let mut tokens = Vec::new();
    leaves(node, source, &mut tokens);
    token_form::join(tokens)
}

/// The texts of `nodes` in token form, joined as one token list.
#[must_use]
pub fn token_form_of(nodes: &[Node<'_>], source: &str) -> String {
    let mut tokens = Vec::new();
    for node in nodes {
        leaves(*node, source, &mut tokens);
    }
    token_form::join(tokens)
}

fn leaves<'s>(node: Node<'_>, source: &'s str, tokens: &mut Vec<&'s str>) {
    if node.kind().contains("comment") {
        return;
    }
    if node.child_count() == 0 || (node.is_named() && node.kind().contains("string")) {
        let token = text(node, source);
        if !token.is_empty() {
            tokens.push(token);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        leaves(child, source, tokens);
    }
}

/// The named children of `node` of kind `kind`.
#[must_use]
pub fn children_of_kind<'t>(node: Node<'t>, kind: &str) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .collect()
}

/// The texts of the unnamed children of `node` that are one of `keywords`, in order.
#[must_use]
pub fn keywords(node: Node<'_>, keywords: &[&str]) -> Vec<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| !child.is_named() && keywords.contains(&child.kind()))
        .map(|child| child.kind().to_owned())
        .collect()
}
