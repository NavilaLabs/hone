use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hone_core::HostedStore;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const DEFAULT_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_HEARTBEAT_SECONDS: u64 = 15;

/// The configuration of a reference server (concept Section 12, "Reference server"), read from
/// a YAML file in the strict profile.
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) listen: SocketAddr,
    pub(crate) request_bytes: usize,
    pub(crate) heartbeat: Duration,
    // Keyed by the SHA-256 of the token, so that no token is ever held.
    pub(crate) identities: BTreeMap<String, String>,
    pub(crate) projects: BTreeMap<String, Project>,
}

#[derive(Debug, Clone)]
pub(crate) struct Project {
    pub(crate) store: HostedStore,
    pub(crate) members: BTreeSet<String>,
    pub(crate) actors: BTreeMap<String, BTreeSet<String>>,
}

impl Config {
    /// Reads the configuration from `path`. A relative `data` directory is resolved against the
    /// directory of the file.
    ///
    /// # Errors
    ///
    /// Returns a message naming the file and the first problem: the file cannot be read, it
    /// breaks the YAML profile, a key is unknown or missing, a value has the wrong form, a
    /// member has no identity, or two identities share a token.
    pub fn load(path: &Path) -> Result<Self, String> {
        let fail = |message: String| format!("{}: {message}", path.display());
        let bytes = std::fs::read(path).map_err(|error| fail(error.to_string()))?;
        let document = hone_core::parse_yaml(&bytes).map_err(|error| fail(error.to_string()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        parse(&document, base).map_err(fail)
    }

    /// The configuration with `listen` replaced, for example by `127.0.0.1:0` for a free port.
    #[must_use]
    pub fn listening_on(mut self, listen: SocketAddr) -> Self {
        self.listen = listen;
        self
    }
}

fn parse(document: &Value, base: &Path) -> Result<Config, String> {
    let root = object(document, "the configuration")?;
    known_keys(
        root,
        "the configuration",
        &["listen", "data", "limits", "identities", "projects"],
    )?;
    let listen = text(root, "listen")?
        .parse()
        .map_err(|_| "`listen` is not an address with a port, such as 127.0.0.1:8470".to_owned())?;
    let data = base.join(PathBuf::from(text(root, "data")?));
    let (request_bytes, heartbeat_seconds) = match root.get("limits") {
        None => (DEFAULT_REQUEST_BYTES, DEFAULT_HEARTBEAT_SECONDS),
        Some(limits) => {
            let limits = object(limits, "`limits`")?;
            known_keys(limits, "`limits`", &["request_bytes", "heartbeat_seconds"])?;
            (
                positive(limits, "request_bytes")?.unwrap_or(DEFAULT_REQUEST_BYTES),
                positive(limits, "heartbeat_seconds")?.unwrap_or(DEFAULT_HEARTBEAT_SECONDS),
            )
        }
    };
    let mut identities = BTreeMap::new();
    for (name, identity) in object(required(root, "identities")?, "`identities`")? {
        let context = format!("the identity `{name}`");
        let identity = object(identity, &context)?;
        known_keys(identity, &context, &["token"])?;
        let token = text(identity, "token")?;
        let hash = token
            .strip_prefix("sha256:")
            .filter(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            .ok_or_else(|| {
                format!("the token of `{name}` is not `sha256:` and 64 lowercase hex digits")
            })?;
        if identities.insert(hash.to_owned(), name.clone()).is_some() {
            return Err(format!(
                "the token of `{name}` belongs to another identity too"
            ));
        }
    }
    let names: BTreeSet<&String> = identities.values().collect();
    let mut projects = BTreeMap::new();
    for (id, project) in object(required(root, "projects")?, "`projects`")? {
        let context = format!("the project `{id}`");
        let project = object(project, &context)?;
        known_keys(project, &context, &["workspace", "members", "actors"])?;
        let store = HostedStore::new(&data, text(project, "workspace")?, id)
            .map_err(|error| error.to_string())?;
        let members = strings(
            required(project, "members")?,
            &format!("the members of `{id}`"),
        )?;
        if let Some(stranger) = members.iter().find(|member| !names.contains(member)) {
            return Err(format!("the member `{stranger}` of `{id}` has no identity"));
        }
        let mut actors = BTreeMap::new();
        if let Some(map) = project.get("actors") {
            for (handle, roles) in object(map, &format!("the actors of `{id}`"))? {
                actors.insert(
                    handle.clone(),
                    strings(roles, &format!("the roles of `{handle}` in `{id}`"))?,
                );
            }
        }
        projects.insert(
            id.clone(),
            Project {
                store,
                members,
                actors,
            },
        );
    }
    Ok(Config {
        listen,
        request_bytes: usize::try_from(request_bytes).unwrap_or(usize::MAX),
        heartbeat: Duration::from_secs(heartbeat_seconds),
        identities,
        projects,
    })
}

pub(crate) fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{context} is not a mapping"))
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    map.get(key).ok_or_else(|| format!("`{key}` is missing"))
}

fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    required(map, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn positive(map: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    map.get(key)
        .map(|value| {
            value
                .as_u64()
                .filter(|number| *number > 0)
                .ok_or_else(|| format!("`{key}` is not a positive integer"))
        })
        .transpose()
}

fn strings(value: &Value, context: &str) -> Result<BTreeSet<String>, String> {
    value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or_else(|| format!("{context} are not a list of strings"))
}

fn known_keys(map: &Map<String, Value>, context: &str, known: &[&str]) -> Result<(), String> {
    match map.keys().find(|key| !known.contains(&key.as_str())) {
        Some(key) => Err(format!("{context} has the unknown key `{key}`")),
        None => Ok(()),
    }
}
