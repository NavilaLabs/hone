//! hone language plugin `lang-typescript` (concept Section 9, "Language plugins") for TypeScript and JavaScript
//! (`ts, tsx, mts, cts, js, jsx, mjs, cjs`). The declarations it knows are documented in `lang_tree::typescript`.

use extism_pdk::{FnResult, plugin_fn};
use serde_json::json;

/// The entry point `describe` of the plugin contract.
#[plugin_fn]
pub fn describe() -> FnResult<String> {
    Ok(json!({
        "name": "lang-typescript",
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
    Ok(lang_tree::resolve(&lang_tree::typescript::RULES, &input))
}

/// The entry point `public_api`: one result per path, in the order of the input.
#[plugin_fn]
pub fn public_api(input: String) -> FnResult<String> {
    Ok(lang_tree::public_api(&lang_tree::typescript::RULES, &input))
}
