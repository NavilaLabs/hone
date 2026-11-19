use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::log::Cursor;
use crate::manifest;

/// An error code of the server transport (concept Section 4, "Error codes").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerCode {
    /// The request is malformed.
    InvalidRequest,
    /// The bearer token is missing or invalid.
    Unauthenticated,
    /// The identity has no access to the project.
    Forbidden,
    /// The server does not serve the project, or the path names no request.
    NotFound,
    /// The body exceeds the server's limit.
    TooLarge,
    /// The event's `project` is not the project of the path.
    ScopeMismatch,
    /// `author.name` is not the authenticated identity.
    IdentityMismatch,
    /// An `approval.given` names a role that `actors` does not give its author.
    RoleMismatch,
    /// The line breaks the reading rules.
    InvalidEvent,
    /// The event does not continue its device's log on the server.
    SeqGap,
    /// The server holds another event at the same device and `seq`.
    Fork,
    /// The server holds an event with the same `id` and other content.
    IdConflict,
}

const CODES: [ServerCode; 12] = [
    ServerCode::InvalidRequest,
    ServerCode::Unauthenticated,
    ServerCode::Forbidden,
    ServerCode::NotFound,
    ServerCode::TooLarge,
    ServerCode::ScopeMismatch,
    ServerCode::IdentityMismatch,
    ServerCode::RoleMismatch,
    ServerCode::InvalidEvent,
    ServerCode::SeqGap,
    ServerCode::Fork,
    ServerCode::IdConflict,
];

impl ServerCode {
    /// The code as it appears in an error body, for example `seq-gap`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid-request",
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::TooLarge => "too-large",
            Self::ScopeMismatch => "scope-mismatch",
            Self::IdentityMismatch => "identity-mismatch",
            Self::RoleMismatch => "role-mismatch",
            Self::InvalidEvent => "invalid-event",
            Self::SeqGap => "seq-gap",
            Self::Fork => "fork",
            Self::IdConflict => "id-conflict",
        }
    }

    /// The HTTP status that answers the code.
    #[must_use]
    pub fn status(self) -> u16 {
        match self {
            Self::InvalidRequest => 400,
            Self::Unauthenticated => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::TooLarge => 413,
            Self::ScopeMismatch
            | Self::IdentityMismatch
            | Self::RoleMismatch
            | Self::InvalidEvent => 422,
            Self::SeqGap | Self::Fork | Self::IdConflict => 409,
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        CODES.into_iter().find(|code| code.as_str() == text)
    }
}

/// Whether `text` is a project ID (`prj-` and 8 characters of Crockford Base32).
#[must_use]
pub fn is_project_id(text: &str) -> bool {
    manifest::is_project_id(text)
}

/// Reads the cursor of a `cursor` query parameter, for example
/// `dev-3k9a5h2t:118,dev-7q2m8x4c:5` (concept Section 4, "Server transport"). An empty text is
/// the empty cursor.
///
/// # Errors
///
/// Returns a message if an entry is not a device ID, a colon, and a `seq` in decimal without
/// leading zeros, or if a device appears twice.
pub fn parse_cursor_query(text: &str) -> Result<Cursor, String> {
    let mut cursor = Cursor::new();
    if text.is_empty() {
        return Ok(cursor);
    }
    for entry in text.split(',') {
        let (device, seq) = entry
            .split_once(':')
            .ok_or_else(|| format!("the cursor entry `{entry}` has no `:`"))?;
        let seq = parse_seq(seq)
            .ok_or_else(|| format!("the cursor entry `{entry}` has no valid `seq`"))?;
        insert(&mut cursor, device, seq)?;
    }
    Ok(cursor)
}

pub(crate) fn cursor_query(cursor: &Cursor) -> String {
    cursor
        .iter()
        .map(|(device, seq)| format!("{device}:{seq}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn parse_cursor_json(value: &Value) -> Result<Cursor, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "the cursor is not a JSON object".to_owned())?;
    let mut cursor = Cursor::new();
    for (device, seq) in object {
        let seq = seq
            .as_u64()
            .ok_or_else(|| format!("the cursor of `{device}` is not a `seq`"))?;
        insert(&mut cursor, device, seq)?;
    }
    Ok(cursor)
}

fn parse_seq(text: &str) -> Option<u64> {
    let canonical = text == "0" || (!text.starts_with('0') && !text.is_empty());
    (canonical && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn insert(cursor: &mut BTreeMap<String, u64>, device: &str, seq: u64) -> Result<(), String> {
    if !manifest::is_device_id(device) {
        return Err(format!("`{device}` in the cursor is not a device ID"));
    }
    if cursor.insert(device.to_owned(), seq).is_some() {
        return Err(format!("`{device}` appears twice in the cursor"));
    }
    Ok(())
}
