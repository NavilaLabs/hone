// The public API entries of a Rust file (spec/plugins.md, items 32 and 33).

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use serde_json::{Map, Value, json};
use syn::spanned::Spanned;
use syn::{
    Attribute, Fields, FnArg, Generics, ImplItem, Item, ItemImpl, ReturnType, Signature, TraitItem,
    Type, UseTree, Visibility,
};

// The public API entries of the Rust file at `path`. A missing file has none.
pub(crate) fn public_items(path: &str) -> Result<Vec<Value>, String> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{path}: {error}")),
    };
    let file = syn::parse_file(&source).map_err(|error| format!("{path}: {error}"))?;
    let mut collector = Collector {
        source: Source::new(&source),
        out: Vec::new(),
    };
    collector.items(&file.items, "");
    Ok(collector.out)
}

struct Collector<'s> {
    source: Source<'s>,
    out: Vec<Value>,
}

impl Collector<'_> {
    fn items(&mut self, items: &[Item], module: &str) {
        for item in items {
            match item {
                Item::Fn(function) if is_public(&function.vis) => {
                    let mut entry = new_entry(
                        &format!("{module}{}", function.sig.ident),
                        "function",
                        function.sig.ident.span(),
                        self.source.text(function.vis.span(), function.sig.span()),
                    );
                    signature(&mut entry, &function.sig, None);
                    attributes(&mut entry, &function.attrs);
                    self.out.push(Value::Object(entry));
                }
                Item::Struct(structure) if is_public(&structure.vis) => {
                    let name = format!("{module}{}", structure.ident);
                    let end = match &structure.fields {
                        Fields::Named(_) => or(structure.generics.span(), structure.ident.span()),
                        _ => structure.span(),
                    };
                    let mut entry = new_entry(
                        &name,
                        "struct",
                        structure.ident.span(),
                        self.source.text(structure.vis.span(), end),
                    );
                    generics(&mut entry, &structure.generics, None);
                    attributes(&mut entry, &structure.attrs);
                    self.out.push(Value::Object(entry));
                    for (index, field) in structure.fields.iter().enumerate() {
                        if !is_public(&field.vis) {
                            continue;
                        }
                        let member = field
                            .ident
                            .as_ref()
                            .map_or_else(|| index.to_string(), ToString::to_string);
                        let mut entry = new_entry(
                            &format!("{name}::{member}"),
                            "field",
                            field.span(),
                            self.source.text(field.vis.span(), field.ty.span()),
                        );
                        put(&mut entry, "type", token_form(&field.ty));
                        attributes(&mut entry, &field.attrs);
                        self.out.push(Value::Object(entry));
                    }
                }
                Item::Enum(enumeration) if is_public(&enumeration.vis) => {
                    let name = format!("{module}{}", enumeration.ident);
                    let mut entry = new_entry(
                        &name,
                        "enum",
                        enumeration.ident.span(),
                        self.source.text(
                            enumeration.vis.span(),
                            or(enumeration.generics.span(), enumeration.ident.span()),
                        ),
                    );
                    generics(&mut entry, &enumeration.generics, None);
                    attributes(&mut entry, &enumeration.attrs);
                    self.out.push(Value::Object(entry));
                    for variant in &enumeration.variants {
                        let mut entry = new_entry(
                            &format!("{name}::{}", variant.ident),
                            "variant",
                            variant.ident.span(),
                            self.source.text(
                                variant.ident.span(),
                                or(variant.fields.span(), variant.ident.span()),
                            ),
                        );
                        if !matches!(variant.fields, Fields::Unit) {
                            put(&mut entry, "type", token_form(&variant.fields));
                        }
                        if let Some((_, value)) = &variant.discriminant {
                            put(&mut entry, "value", token_form(value));
                        }
                        attributes(&mut entry, &variant.attrs);
                        self.out.push(Value::Object(entry));
                    }
                }
                Item::Trait(definition) if is_public(&definition.vis) => {
                    let name = format!("{module}{}", definition.ident);
                    let end = definition.supertraits.last().map_or_else(
                        || or(definition.generics.span(), definition.ident.span()),
                        Spanned::span,
                    );
                    let mut entry = new_entry(
                        &name,
                        "trait",
                        definition.ident.span(),
                        self.source.text(definition.vis.span(), end),
                    );
                    generics(&mut entry, &definition.generics, None);
                    list(
                        &mut entry,
                        "bases",
                        definition.supertraits.iter().map(token_form).collect(),
                    );
                    let mut modifiers = Vec::new();
                    if definition.unsafety.is_some() {
                        modifiers.push("unsafe".to_owned());
                    }
                    if definition.auto_token.is_some() {
                        modifiers.push("auto".to_owned());
                    }
                    list(&mut entry, "modifiers", modifiers);
                    attributes(&mut entry, &definition.attrs);
                    self.out.push(Value::Object(entry));
                    for member in &definition.items {
                        self.trait_item(&name, member);
                    }
                }
                Item::Type(alias) if is_public(&alias.vis) => {
                    let mut entry = new_entry(
                        &format!("{module}{}", alias.ident),
                        "type",
                        alias.ident.span(),
                        self.source.text(alias.vis.span(), alias.ty.span()),
                    );
                    generics(&mut entry, &alias.generics, None);
                    put(&mut entry, "type", token_form(&alias.ty));
                    attributes(&mut entry, &alias.attrs);
                    self.out.push(Value::Object(entry));
                }
                Item::Const(constant) if is_public(&constant.vis) => {
                    let mut entry = new_entry(
                        &format!("{module}{}", constant.ident),
                        "constant",
                        constant.ident.span(),
                        self.source.text(constant.vis.span(), constant.ty.span()),
                    );
                    generics(&mut entry, &constant.generics, None);
                    put(&mut entry, "type", token_form(&constant.ty));
                    put(&mut entry, "value", token_form(&constant.expr));
                    attributes(&mut entry, &constant.attrs);
                    self.out.push(Value::Object(entry));
                }
                Item::Static(value) if is_public(&value.vis) => {
                    let mut entry = new_entry(
                        &format!("{module}{}", value.ident),
                        "static",
                        value.ident.span(),
                        self.source.text(value.vis.span(), value.ty.span()),
                    );
                    if matches!(value.mutability, syn::StaticMutability::Mut(_)) {
                        list(&mut entry, "modifiers", vec!["mut".to_owned()]);
                    }
                    put(&mut entry, "type", token_form(&value.ty));
                    attributes(&mut entry, &value.attrs);
                    self.out.push(Value::Object(entry));
                }
                Item::Use(declaration) if is_public(&declaration.vis) => {
                    let text = self.source.text(declaration.vis.span(), declaration.span());
                    let mut uses = Vec::new();
                    flatten(&declaration.tree, &mut Vec::new(), &mut uses);
                    for (name, path) in uses {
                        let mut entry = new_entry(
                            &format!("{module}{name}"),
                            "use",
                            declaration.use_token.span(),
                            text.clone(),
                        );
                        put(&mut entry, "type", path);
                        attributes(&mut entry, &declaration.attrs);
                        self.out.push(Value::Object(entry));
                    }
                }
                Item::Mod(inner) if is_public(&inner.vis) => {
                    let mut entry = new_entry(
                        &format!("{module}{}", inner.ident),
                        "module",
                        inner.ident.span(),
                        self.source.text(inner.vis.span(), inner.ident.span()),
                    );
                    attributes(&mut entry, &inner.attrs);
                    self.out.push(Value::Object(entry));
                    if let Some((_, items)) = &inner.content {
                        self.items(items, &format!("{module}{}::", inner.ident));
                    }
                }
                Item::Impl(block) => self.implementation(block, module),
                _ => {}
            }
        }
    }

    fn trait_item(&mut self, owner: &str, member: &TraitItem) {
        match member {
            TraitItem::Fn(function) => {
                let mut entry = new_entry(
                    &format!("{owner}::{}", function.sig.ident),
                    "method",
                    function.sig.ident.span(),
                    self.source.text(function.sig.span(), function.sig.span()),
                );
                signature(&mut entry, &function.sig, None);
                attributes(&mut entry, &function.attrs);
                self.out.push(Value::Object(entry));
            }
            TraitItem::Const(constant) => {
                let mut entry = new_entry(
                    &format!("{owner}::{}", constant.ident),
                    "constant",
                    constant.ident.span(),
                    self.source
                        .text(constant.const_token.span(), constant.ty.span()),
                );
                put(&mut entry, "type", token_form(&constant.ty));
                attributes(&mut entry, &constant.attrs);
                self.out.push(Value::Object(entry));
            }
            TraitItem::Type(alias) => {
                let end = alias
                    .bounds
                    .last()
                    .map_or_else(|| alias.ident.span(), Spanned::span);
                let mut entry = new_entry(
                    &format!("{owner}::{}", alias.ident),
                    "type",
                    alias.ident.span(),
                    self.source.text(alias.type_token.span(), end),
                );
                generics(&mut entry, &alias.generics, None);
                list(
                    &mut entry,
                    "bases",
                    alias.bounds.iter().map(token_form).collect(),
                );
                attributes(&mut entry, &alias.attrs);
                self.out.push(Value::Object(entry));
            }
            _ => {}
        }
    }

    // A trait implementation is one entry under the name of its type; an inherent one gives an
    // entry per public function.
    fn implementation(&mut self, block: &ItemImpl, module: &str) {
        let owner = type_name(&block.self_ty);
        let self_type = token_form(&block.self_ty);
        if let Some((negative, path, _)) = &block.trait_ {
            let mut entry = new_entry(
                &format!("{module}{owner}"),
                "impl",
                block.impl_token.span(),
                self.source
                    .text(block.impl_token.span(), block.self_ty.span()),
            );
            generics(&mut entry, &block.generics, None);
            let negative = if negative.is_some() { "!" } else { "" };
            put(&mut entry, "self_type", self_type);
            if block.unsafety.is_some() {
                list(&mut entry, "modifiers", vec!["unsafe".to_owned()]);
            }
            attributes(&mut entry, &block.attrs);
            put(
                &mut entry,
                "type",
                format!("{negative}{}", token_form(path)),
            );
            self.out.push(Value::Object(entry));
            return;
        }
        for member in &block.items {
            let ImplItem::Fn(function) = member else {
                continue;
            };
            if !is_public(&function.vis) {
                continue;
            }
            let mut entry = new_entry(
                &format!("{module}{owner}::{}", function.sig.ident),
                "method",
                function.sig.ident.span(),
                self.source.text(function.vis.span(), function.sig.span()),
            );
            put(&mut entry, "self_type", self_type.clone());
            signature(&mut entry, &function.sig, Some(&block.generics));
            attributes(&mut entry, &function.attrs);
            self.out.push(Value::Object(entry));
        }
    }
}

fn new_entry(symbol: &str, kind: &str, name: Span, text: String) -> Map<String, Value> {
    let mut entry = Map::new();
    entry.insert("symbol".into(), symbol.into());
    entry.insert("kind".into(), kind.into());
    entry.insert("line".into(), name.start().line.into());
    entry.insert("text".into(), text.into());
    entry
}

// The generics, receiver, modifiers, parameters, and return type of a function. The generics of
// an enclosing `impl` block come first.
fn signature(entry: &mut Map<String, Value>, signature: &Signature, outer: Option<&Generics>) {
    generics(entry, &signature.generics, outer);
    let mut modifiers = Vec::new();
    if signature.constness.is_some() {
        modifiers.push("const".to_owned());
    }
    if signature.asyncness.is_some() {
        modifiers.push("async".to_owned());
    }
    if signature.unsafety.is_some() {
        modifiers.push("unsafe".to_owned());
    }
    if let Some(abi) = &signature.abi {
        modifiers.push(token_form(abi));
    }
    list(entry, "modifiers", modifiers);
    let mut parameters = Vec::new();
    let mut receiver = None;
    for argument in &signature.inputs {
        match argument {
            FnArg::Receiver(this) => {
                let mut tokens = TokenStream::new();
                if this.colon_token.is_some() {
                    this.self_token.to_tokens(&mut tokens);
                    this.colon_token.to_tokens(&mut tokens);
                    this.ty.to_tokens(&mut tokens);
                } else {
                    if let Some((and, lifetime)) = &this.reference {
                        and.to_tokens(&mut tokens);
                        lifetime.to_tokens(&mut tokens);
                        // A binding `mut self` does not change the API, `&mut self` does.
                        this.mutability.to_tokens(&mut tokens);
                    }
                    this.self_token.to_tokens(&mut tokens);
                }
                receiver = Some(token_form(&tokens));
            }
            FnArg::Typed(typed) => parameters.push(json!({
                "name": token_form(&typed.pat),
                "type": token_form(&typed.ty),
                "passing": "positional",
            })),
        }
    }
    match receiver {
        Some(receiver) => put(entry, "receiver", receiver),
        None if entry["kind"] == "method" => {
            entry.insert("static".into(), true.into());
        }
        None => {}
    }
    if !parameters.is_empty() {
        entry.insert("parameters".into(), parameters.into());
    }
    if let ReturnType::Type(_, returned) = &signature.output {
        put(entry, "returns", token_form(returned));
    }
}

fn generics(entry: &mut Map<String, Value>, inner: &Generics, outer: Option<&Generics>) {
    let all: Vec<&Generics> = outer.into_iter().chain([inner]).collect();
    list(
        entry,
        "type_parameters",
        all.iter()
            .flat_map(|generics| generics.params.iter().map(token_form))
            .collect(),
    );
    list(
        entry,
        "where",
        all.iter()
            .filter_map(|generics| generics.where_clause.as_ref())
            .flat_map(|clause| clause.predicates.iter().map(token_form))
            .collect(),
    );
}

// Doc comments are attributes, but they do not change the API.
fn attributes(entry: &mut Map<String, Value>, attributes: &[Attribute]) {
    list(
        entry,
        "attributes",
        attributes
            .iter()
            .filter(|attribute| !attribute.path().is_ident("doc"))
            .map(token_form)
            .collect(),
    );
}

// Each name a `use` tree makes visible, with the path it names: `C` for `a::b::C`, `D` for
// `a::C as D`, and `*` for `a::*`.
fn flatten(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<(String, String)>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten(&path.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let path = [prefix.clone(), vec![name.ident.to_string()]].concat();
            out.push((name.ident.to_string(), path.join("::")));
        }
        UseTree::Rename(rename) => {
            let path = [prefix.clone(), vec![rename.ident.to_string()]].concat();
            out.push((rename.rename.to_string(), path.join("::")));
        }
        UseTree::Glob(_) => {
            let path = [prefix.clone(), vec!["*".to_owned()]].concat();
            out.push(("*".to_owned(), path.join("::")));
        }
        UseTree::Group(group) => {
            for item in &group.items {
                flatten(item, prefix, out);
            }
        }
    }
}

// The name an `impl` block's type has in a symbol: the last segment of a path, otherwise the
// type in token form.
fn type_name(target: &Type) -> String {
    match target {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(|| token_form(target), |segment| segment.ident.to_string()),
        _ => token_form(target),
    }
}

// `span`, or `fallback` when `span` covers no source, as the span of empty generics does.
fn or(span: Span, fallback: Span) -> Span {
    if span.start() == span.end() {
        fallback
    } else {
        span
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn put(entry: &mut Map<String, Value>, key: &str, value: String) {
    entry.insert(key.into(), value.into());
}

fn list(entry: &mut Map<String, Value>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        entry.insert(key.into(), values.into());
    }
}

fn token_form(node: &impl ToTokens) -> String {
    let mut tokens = Vec::new();
    leaves(node.to_token_stream(), &mut tokens);
    token_form::join(tokens)
}

fn leaves(stream: TokenStream, out: &mut Vec<String>) {
    for tree in stream {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    proc_macro2::Delimiter::Parenthesis => ("(", ")"),
                    proc_macro2::Delimiter::Brace => ("{", "}"),
                    proc_macro2::Delimiter::Bracket => ("[", "]"),
                    proc_macro2::Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    out.push(open.to_owned());
                }
                leaves(group.stream(), out);
                if !close.is_empty() {
                    out.push(close.to_owned());
                }
            }
            TokenTree::Punct(punct) => out.push(punct.as_char().to_string()),
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Literal(literal) => out.push(literal.to_string()),
        }
    }
}

// The source of a file, sliced by the line and column of spans.
struct Source<'s> {
    lines: Vec<&'s str>,
}

impl<'s> Source<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            lines: source.split('\n').collect(),
        }
    }

    // The text from the start of `from` to the end of `to`, whitespace collapsed. An empty
    // `from` starts at `to`.
    fn text(&self, from: Span, to: Span) -> String {
        let start = if from.start() == from.end() {
            to.start()
        } else {
            from.start()
        };
        let end = to.end();
        let mut text = String::new();
        for line in start.line..=end.line {
            let Some(content) = self.lines.get(line - 1) else {
                break;
            };
            let first = if line == start.line { start.column } else { 0 };
            let last = if line == end.line {
                end.column
            } else {
                content.chars().count()
            };
            text.extend(content.chars().skip(first).take(last.saturating_sub(first)));
            text.push(' ');
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    const SAMPLE: &str = r#"/// Doc.
#[derive(Clone, Debug)]
pub struct Fee<T: Copy> where T: Default {
    pub amount: u64,
    secret: u8,
}
pub struct Pair(pub u8, String);
pub enum State { Open = 1, Paid(u64), Closed { at: u64 } }
pub trait Priced: Clone + Send {
    const RATE: u32;
    type Item: Display;
    fn price(&self) -> u64 { 0 }
}
pub type Map<K> = std::collections::HashMap<
    K,
    u8,
>;
pub const MAX: u32 = 5;
pub static mut COUNT: u32 = 0;
pub use crate::a::{B, c as D, e::*};
pub mod inner { pub fn f() {} }
impl<T: Copy> Fee<T> {
    pub async unsafe fn add<'a>(&'a mut self, x: &'a str, (a, b): (u8, u8)) -> Result<(), E> where T: Debug { todo!() }
    pub fn new(mut self) -> Self { self }
    pub fn of() -> Self { todo!() }
    fn hidden(&self) {}
}
impl Display for Fee<u8> {}
impl !Send for Pair {}
pub extern "C" fn ffi(self: Box<Self>) {}
"#;

    // The expected entries were written by hand from the sample.
    #[test]
    fn entries_of_every_kind_of_item() {
        let directory = std::env::temp_dir().join("lang-rust-entries-of-every-kind-of-item");
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let file = directory.join("sample.rs");
        std::fs::write(&file, SAMPLE).expect("the sample");
        let entries = super::public_items(file.to_str().expect("a UTF-8 path"));
        std::fs::remove_dir_all(&directory).expect("removing the temporary directory");
        assert_eq!(
            entries.expect("the sample parses"),
            vec![
                json!({ "symbol": "Fee", "kind": "struct", "type_parameters": ["T:Copy"], "where": ["T:Default"], "attributes": ["#[derive(Clone, Debug)]"], "line": 3, "text": "pub struct Fee<T: Copy>" }),
                json!({ "symbol": "Fee::amount", "kind": "field", "type": "u64", "line": 4, "text": "pub amount: u64" }),
                json!({ "symbol": "Pair", "kind": "struct", "line": 7, "text": "pub struct Pair(pub u8, String);" }),
                json!({ "symbol": "Pair::0", "kind": "field", "type": "u8", "line": 7, "text": "pub u8" }),
                json!({ "symbol": "State", "kind": "enum", "line": 8, "text": "pub enum State" }),
                json!({ "symbol": "State::Open", "kind": "variant", "value": "1", "line": 8, "text": "Open" }),
                json!({ "symbol": "State::Paid", "kind": "variant", "type": "(u64)", "line": 8, "text": "Paid(u64)" }),
                json!({ "symbol": "State::Closed", "kind": "variant", "type": "{at:u64}", "line": 8, "text": "Closed { at: u64 }" }),
                json!({ "symbol": "Priced", "kind": "trait", "bases": ["Clone", "Send"], "line": 9, "text": "pub trait Priced: Clone + Send" }),
                json!({ "symbol": "Priced::RATE", "kind": "constant", "type": "u32", "line": 10, "text": "const RATE: u32" }),
                json!({ "symbol": "Priced::Item", "kind": "type", "bases": ["Display"], "line": 11, "text": "type Item: Display" }),
                json!({ "symbol": "Priced::price", "kind": "method", "receiver": "&self", "returns": "u64", "line": 12, "text": "fn price(&self) -> u64" }),
                json!({ "symbol": "Map", "kind": "type", "type_parameters": ["K"], "type": "std::collections::HashMap<K, u8>", "line": 14, "text": "pub type Map<K> = std::collections::HashMap< K, u8, >" }),
                json!({ "symbol": "MAX", "kind": "constant", "type": "u32", "value": "5", "line": 18, "text": "pub const MAX: u32" }),
                json!({ "symbol": "COUNT", "kind": "static", "modifiers": ["mut"], "type": "u32", "line": 19, "text": "pub static mut COUNT: u32" }),
                json!({ "symbol": "B", "kind": "use", "type": "crate::a::B", "line": 20, "text": "pub use crate::a::{B, c as D, e::*};" }),
                json!({ "symbol": "D", "kind": "use", "type": "crate::a::c", "line": 20, "text": "pub use crate::a::{B, c as D, e::*};" }),
                json!({ "symbol": "*", "kind": "use", "type": "crate::a::e::*", "line": 20, "text": "pub use crate::a::{B, c as D, e::*};" }),
                json!({ "symbol": "inner", "kind": "module", "line": 21, "text": "pub mod inner" }),
                json!({ "symbol": "inner::f", "kind": "function", "line": 21, "text": "pub fn f()" }),
                json!({ "symbol": "Fee::add", "kind": "method", "self_type": "Fee<T>", "type_parameters": ["T:Copy", "'a"], "where": ["T:Debug"], "modifiers": ["async", "unsafe"], "receiver": "&'a mut self", "parameters": [
                    { "name": "x", "type": "&'a str", "passing": "positional" },
                    { "name": "(a, b)", "type": "(u8, u8)", "passing": "positional" }
                ], "returns": "Result<(), E>", "line": 23, "text": "pub async unsafe fn add<'a>(&'a mut self, x: &'a str, (a, b): (u8, u8)) -> Result<(), E> where T: Debug" }),
                json!({ "symbol": "Fee::new", "kind": "method", "self_type": "Fee<T>", "type_parameters": ["T:Copy"], "receiver": "self", "returns": "Self", "line": 24, "text": "pub fn new(mut self) -> Self" }),
                json!({ "symbol": "Fee::of", "kind": "method", "self_type": "Fee<T>", "type_parameters": ["T:Copy"], "static": true, "returns": "Self", "line": 25, "text": "pub fn of() -> Self" }),
                json!({ "symbol": "Fee", "kind": "impl", "self_type": "Fee<u8>", "type": "Display", "line": 28, "text": "impl Display for Fee<u8>" }),
                json!({ "symbol": "Pair", "kind": "impl", "self_type": "Pair", "type": "!Send", "line": 29, "text": "impl !Send for Pair" }),
                json!({ "symbol": "ffi", "kind": "function", "modifiers": ["extern\"C\""], "receiver": "self:Box<Self>", "line": 30, "text": "pub extern \"C\" fn ffi(self: Box<Self>)" }),
            ]
        );
    }
}
