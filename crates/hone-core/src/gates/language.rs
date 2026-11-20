// The standard gates that call language plugins (spec/gates.md, items 20 and 21):
// `module-anchors-resolve` and `public-api-has-decision`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde_json::{Value, json};

use super::{Scope, TicketCall, id_list};
use crate::anchor;
use crate::finding::{Code, Finding};
use crate::host::LanguageCall;
use crate::log;

// One anchor to check: the artifact's path, the pointer of the anchor, its kind and reference.
struct Anchor<'a> {
    artifact: &'a str,
    pointer: String,
    kind: &'a str,
    reference: &'a str,
}

impl Scope<'_> {
    pub(super) fn module_anchors_resolve(
        &self,
        params: &Value,
        call: &TicketCall<'_>,
    ) -> Vec<Finding> {
        let kind = params["kind"].as_str().unwrap_or("module");
        let root = call.host.project_root();
        let languages = languages(self.config);
        let mut findings = Vec::new();
        // Per language plugin: the anchors whose symbols it resolves.
        let mut batches: BTreeMap<&str, Vec<(Anchor<'_>, &str, &str)>> = BTreeMap::new();
        for anchor in self.anchors_of_kind(kind) {
            let problem = |level: fn(Code, String) -> Finding, code: Code, message: String| {
                level(code, message)
                    .at_path(anchor.artifact)
                    .at_pointer(&anchor.pointer)
            };
            let reference = anchor.reference;
            if reference
                .split('#')
                .next()
                .unwrap_or_default()
                .contains(':')
            {
                findings.push(problem(
                    Finding::warning,
                    Code::OtherRepository,
                    format!(
                        "`{reference}` lies in another repository, which this check cannot see"
                    ),
                ));
                continue;
            }
            match anchor.kind {
                "path" => {
                    if !exists_below(root, reference.trim_end_matches('/'), false) {
                        findings.push(problem(
                            Finding::error,
                            Code::AnchorMissing,
                            format!("no file or directory `{reference}`"),
                        ));
                    }
                }
                "glob" => {
                    if !glob_matches(root, reference) {
                        findings.push(problem(
                            Finding::error,
                            Code::AnchorMissing,
                            format!("no file matches `{reference}`"),
                        ));
                    }
                }
                "symbol" => {
                    let (file, symbol) = reference.split_once('#').unwrap_or((reference, ""));
                    if !exists_below(root, file, true) {
                        findings.push(problem(
                            Finding::error,
                            Code::AnchorMissing,
                            format!("no file `{file}`"),
                        ));
                    } else if symbol.is_empty() {
                        // `path#` without a symbol names only the file, which exists.
                    } else if let Some(plugin) = language_of(&languages, file) {
                        batches
                            .entry(plugin)
                            .or_default()
                            .push((anchor, file, symbol));
                    } else {
                        findings.push(problem(
                            Finding::warning,
                            Code::SymbolUnchecked,
                            format!("`{symbol}` in `{file}` is not checked: no language plugin handles the file"),
                        ));
                    }
                }
                _ => {}
            }
        }
        for (plugin, anchors) in batches {
            let symbols: Vec<Value> = anchors
                .iter()
                .map(|(_, path, symbol)| json!({ "path": path, "symbol": symbol }))
                .collect();
            let input = json!({ "protocol": 1, "state": "project", "symbols": symbols });
            let results = match call.host.language(
                self.config,
                plugin,
                LanguageCall::Resolve,
                &input,
                anchors.len(),
            ) {
                Ok(results) => results,
                Err(failed) => {
                    findings.push(failed);
                    continue;
                }
            };
            for ((anchor, file, symbol), result) in anchors.iter().zip(results) {
                let finding = match result["status"].as_str() {
                    Some("missing") => Finding::error(
                        Code::SymbolMissing,
                        format!("no symbol `{symbol}` in `{file}`"),
                    ),
                    Some("unparsable") => Finding::error(
                        Code::Unparsable,
                        format!(
                            "`{file}`: {}",
                            result["message"].as_str().unwrap_or_default()
                        ),
                    ),
                    _ => continue,
                };
                findings.push(finding.at_path(anchor.artifact).at_pointer(&anchor.pointer));
            }
        }
        findings
    }

    pub(super) fn public_api_has_decision(
        &self,
        params: &Value,
        call: &TicketCall<'_>,
    ) -> Vec<Finding> {
        let Some(changed) = call.changed_paths else {
            return vec![Finding::error(
                Code::ChangedPathsMissing,
                "the caller supplied no changed paths",
            )];
        };
        let Some(base) = call.host.base() else {
            return vec![Finding::error(
                Code::BaseMissing,
                "the caller supplied no base state to compare the public API with",
            )];
        };
        let root = call.host.project_root();
        let anchored = params["anchored"].as_str().unwrap_or_default();
        let binding = params["binding"].as_str().unwrap_or_default();
        let languages = languages(self.config);
        let mut findings = Vec::new();
        // Per changed file: the IDs of the anchored artifacts whose anchors hit it.
        let mut hits: Vec<(&str, Vec<&str>)> = Vec::new();
        for path in changed.iter().filter(|path| !path.contains(':')) {
            let ids: Vec<&str> = self
                .artifacts()
                .filter(|(_, artifact)| artifact.type_name() == Some(anchored))
                .filter(|(_, artifact)| {
                    artifact
                        .content
                        .data
                        .get("anchors")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .any(|anchor| anchor::hits(anchor, path))
                })
                .filter_map(|(_, artifact)| artifact.id())
                .collect();
            if !ids.is_empty() {
                hits.push((path, ids));
            }
        }
        let mut by_plugin: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, (path, _)) in hits.iter().enumerate() {
            match language_of(&languages, path) {
                Some(plugin) => by_plugin.entry(plugin).or_default().push(index),
                None => findings.push(
                    Finding::warning(
                        Code::ApiUnchecked,
                        format!("the public API of `{path}` is not checked: no language plugin handles the file"),
                    )
                    .at_path(path),
                ),
            }
        }
        for (plugin, indices) in by_plugin {
            let paths: Vec<&str> = indices.iter().map(|index| hits[*index].0).collect();
            let checked = match self.public_api(call, plugin, "project", root, &paths) {
                Ok(apis) => apis,
                Err(failed) => {
                    findings.push(failed);
                    continue;
                }
            };
            let base_apis = match self.public_api(call, plugin, "base", base, &paths) {
                Ok(apis) => apis,
                Err(failed) => {
                    findings.push(failed);
                    continue;
                }
            };
            for ((index, now), before) in indices.iter().zip(checked).zip(base_apis) {
                let (path, ids) = &hits[*index];
                let now = match now {
                    Ok(symbols) => symbols,
                    Err(message) => {
                        findings.push(Finding::error(Code::Unparsable, message).at_path(path));
                        continue;
                    }
                };
                // A base that does not parse counts as an empty API: every public item is new.
                let before = before.unwrap_or_default();
                if now == before {
                    continue;
                }
                let unbound: Vec<&str> = ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        !self.artifacts().any(|(candidate_index, candidate)| {
                            candidate.type_name() == Some(binding)
                                && id_list(candidate, "affects").any(|target| target == *id)
                                && self.effectively_binding(candidate_index)
                        })
                    })
                    .collect();
                if !unbound.is_empty() {
                    findings.push(
                        Finding::error(
                            Code::PublicApiUnbound,
                            format!(
                                "the public API changed ({}) without a binding `{binding}` that affects {}",
                                difference(&now, &before),
                                unbound.join(", ")
                            ),
                        )
                        .at_path(path),
                    );
                }
            }
        }
        findings
    }

    // The public API of every path in `state`, one entry per path: the symbols, or the message
    // of a file that does not parse. A file missing in the state has an empty API and is not
    // passed to the plugin.
    #[allow(clippy::type_complexity)]
    fn public_api(
        &self,
        call: &TicketCall<'_>,
        plugin: &str,
        state: &str,
        root: &Path,
        paths: &[&str],
    ) -> Result<Vec<Result<BTreeSet<String>, String>>, Finding> {
        let present: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|path| exists_below(root, path, true))
            .collect();
        let mut apis: BTreeMap<&str, Result<BTreeSet<String>, String>> = BTreeMap::new();
        if !present.is_empty() {
            let input = json!({ "protocol": 1, "state": state, "paths": present });
            let results = call.host.language(
                self.config,
                plugin,
                LanguageCall::PublicApi,
                &input,
                present.len(),
            )?;
            for (path, result) in present.iter().zip(results) {
                let api = if result["status"] == "ok" {
                    Ok(result["symbols"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect())
                } else {
                    Err(format!(
                        "`{path}`: {}",
                        result["message"].as_str().unwrap_or_default()
                    ))
                };
                apis.insert(path, api);
            }
        }
        Ok(paths
            .iter()
            .map(|path| apis.remove(path).unwrap_or_else(|| Ok(BTreeSet::new())))
            .collect())
    }

    fn anchors_of_kind<'s>(&'s self, kind: &str) -> Vec<Anchor<'s>> {
        let mut anchors = Vec::new();
        for (_, artifact) in self.artifacts() {
            if artifact.type_name() != Some(kind) {
                continue;
            }
            let data = &artifact.content.data;
            let listed = data
                .get("anchors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten();
            for (index, anchor) in listed.enumerate() {
                if let (Some(kind), Some(reference)) =
                    (anchor["kind"].as_str(), anchor["ref"].as_str())
                {
                    anchors.push(Anchor {
                        artifact: &artifact.path,
                        pointer: format!("/anchors/{index}"),
                        kind,
                        reference,
                    });
                }
            }
            if let Some(reference) = data
                .get("interface")
                .and_then(|interface| interface.get("code"))
                .and_then(Value::as_str)
            {
                anchors.push(Anchor {
                    artifact: &artifact.path,
                    pointer: "/interface/code".into(),
                    kind: "symbol",
                    reference,
                });
            }
        }
        anchors
    }
}

// The language plugins of the configuration by extension.
pub(crate) fn languages(config: &Value) -> BTreeMap<String, &str> {
    let mut map = BTreeMap::new();
    for (name, plugin) in config["plugins"].as_object().into_iter().flatten() {
        for extension in plugin["language"]["extensions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(extension) = extension.as_str() {
                map.insert(extension.to_owned(), name.as_str());
            }
        }
    }
    map
}

pub(crate) fn language_of<'c>(
    languages: &BTreeMap<String, &'c str>,
    path: &str,
) -> Option<&'c str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    languages.get(&extension.to_lowercase()).copied()
}

// A path of the untrusted checked state: relative, without `..`, and below the root after
// following symbolic links.
fn exists_below(root: &Path, relative: &str, file: bool) -> bool {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return false;
    }
    let Ok(canonical) = root.join(relative).canonicalize() else {
        return false;
    };
    log::below(&canonical, root) && (!file || canonical.is_file())
}

// Walks only below the pattern's literal prefix, and follows no symbolic link to a directory.
fn glob_matches(root: &Path, pattern: &str) -> bool {
    let segments: Vec<&str> = pattern.split('/').collect();
    let literal = segments
        .iter()
        .take_while(|segment| !segment.contains(['*', '?']))
        .count()
        .min(segments.len().saturating_sub(1));
    let prefix = segments[..literal].join("/");
    if !prefix.is_empty() && !exists_below(root, &prefix, false) {
        return false;
    }
    let mut files = Vec::new();
    walk(root, &prefix, &mut files);
    files.iter().any(|file| anchor::glob_matches(pattern, file))
}

fn walk(root: &Path, relative: &str, files: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &path, files),
            Ok(kind) if kind.is_file() => files.push(path),
            _ => {}
        }
    }
}

fn difference(checked: &BTreeSet<String>, base: &BTreeSet<String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in checked.difference(base).take(3) {
        parts.push(format!("added `{item}`"));
    }
    for item in base.difference(checked).take(3) {
        parts.push(format!("removed `{item}`"));
    }
    parts.join(", ")
}
