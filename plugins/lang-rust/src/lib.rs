//! hone language plugin `lang-rust` (concept Section 9, "Language plugins").
//!
//! `resolve` finds a symbol `a::b` in a Rust file: an item, an item of an inline `mod`, a variant
//! of an enum, an item of a trait, a named field of a struct, or an item of an `impl` block of a
//! type named like the last but one segment, also when the type is defined elsewhere. Re-exports
//! and out-of-line `mod x;` files are not followed.
//!
//! `public_api` lists the public items of a Rust file: function signatures, structs with their
//! `pub` fields, enums, traits without the bodies of default methods, type aliases, constants and
//! statics with their types, `pub use`, the `pub` functions of inherent `impl` blocks, and the
//! headers of trait `impl` blocks, each inside its `pub mod` path. Bodies, private items, and doc
//! comments do not count.

mod api;
mod symbol;

use std::collections::BTreeMap;

use extism_pdk::{FnResult, plugin_fn};
use serde_json::{Value, json};

/// The entry point `describe` of the plugin contract.
#[plugin_fn]
pub fn describe() -> FnResult<String> {
    Ok(json!({
        "name": "lang-rust",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1,
        "entry_points": ["describe", "resolve", "public_api"],
        "capabilities": { "read": ["project"], "net": [], "timeout_ms": 5000, "memory_pages": 256 },
    })
    .to_string())
}

/// The entry point `resolve`: one result per symbol, in the order of the input.
#[plugin_fn]
pub fn resolve(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    let root = mount(&input);
    let mut parsed: BTreeMap<String, Result<syn::File, String>> = BTreeMap::new();
    let results: Vec<Value> = input["symbols"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            let path = entry["path"].as_str().unwrap_or_default();
            let symbol = entry["symbol"].as_str().unwrap_or_default();
            let file = parsed
                .entry(path.to_owned())
                .or_insert_with(|| parse(&format!("{root}/{path}")));
            match file {
                Ok(file) if symbol::exists(file, symbol) => json!({ "status": "found" }),
                Ok(_) => json!({ "status": "missing" }),
                Err(message) => json!({ "status": "unparsable", "message": message }),
            }
        })
        .collect();
    Ok(json!({ "results": results }).to_string())
}

/// The entry point `public_api`: one result per path, in the order of the input.
#[plugin_fn]
pub fn public_api(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    let root = mount(&input);
    let results: Vec<Value> = input["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|path| {
            let path = path.as_str().unwrap_or_default();
            match api::public_items(&format!("{root}/{path}")) {
                Ok(symbols) => json!({ "status": "ok", "symbols": symbols }),
                Err(message) => json!({ "status": "unparsable", "message": message }),
            }
        })
        .collect();
    Ok(json!({ "results": results }).to_string())
}

fn mount(input: &Value) -> &'static str {
    if input["state"] == "base" {
        "/base"
    } else {
        "/project"
    }
}

fn parse(path: &str) -> Result<syn::File, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    syn::parse_file(&source).map_err(|error| error.to_string())
}
