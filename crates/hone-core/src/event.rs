use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonschema::{Registry, Validator};
use serde_json::Value;

const COMMON_SCHEMA: &str = include_str!("../../../schemas/common.schema.json");
const EVENT_SCHEMA: &str = include_str!("../../../schemas/event.schema.json");
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

pub(crate) const THREAD_OPENED: &str = "thread.opened";
pub(crate) const THREAD_MESSAGE: &str = "thread.message";
pub(crate) const THREAD_RESOLVED: &str = "thread.resolved";
pub(crate) const THREAD_REOPENED: &str = "thread.reopened";
pub(crate) const STEP_CHANGED: &str = "step.changed";
pub(crate) const APPROVAL_GIVEN: &str = "approval.given";
pub(crate) const REVIEW_CONFIRMED: &str = "review.confirmed";
pub(crate) const TICKET_STARTED: &str = "ticket.started";
pub(crate) const TICKET_ATTRIBUTES_CHANGED: &str = "ticket.attributes_changed";
pub(crate) const TICKET_CONFIG_CHANGED: &str = "ticket.config_changed";

static VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    let common: Value = serde_json::from_str(COMMON_SCHEMA).expect("common.schema.json is JSON");
    let event: Value = serde_json::from_str(EVENT_SCHEMA).expect("event.schema.json is JSON");
    let common_id = common["$id"].as_str().expect("$id").to_owned();
    let registry = Registry::new()
        .add(common_id, common)
        .expect("common.schema.json is a valid resource")
        .prepare()
        .expect("the registry of the event schema resolves");
    jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&event)
        .expect("event.schema.json is a valid schema")
});

pub(crate) fn schema_violations(event: &Value) -> Vec<String> {
    let mut violations: Vec<String> = VALIDATOR
        .iter_errors(event)
        .map(|error| {
            let at = error.instance_path().to_string();
            if at.is_empty() {
                error.to_string()
            } else {
                format!("{at}: {error}")
            }
        })
        .collect();
    violations.sort();
    violations.dedup();
    violations
}

pub(crate) fn new_event_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes[6..]).expect("the operating system provides random bytes");
    bytes[..6].copy_from_slice(&(millis as u64).to_be_bytes()[2..]);
    bytes[6] = 0x70 | (bytes[6] & 0x0f);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "ev-{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub(crate) fn new_device_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    let random: String = bytes
        .iter()
        .map(|byte| char::from(CROCKFORD[usize::from(byte & 0x1f)]))
        .collect();
    format!("dev-{random}")
}

pub(crate) fn new_ticket_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    let random: String = bytes
        .iter()
        .map(|byte| char::from(CROCKFORD[usize::from(byte & 0x1f)]))
        .collect();
    format!("tk-{random}")
}

pub(crate) fn thread_of(event_id: &str) -> String {
    format!("th-{}", event_id.trim_start_matches("ev-"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn event_ids_are_uuid_version_7_with_the_current_time() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_millis();
        let id = new_event_id();
        let hex: String = id[3..].replace('-', "");
        let millis = u128::from_str_radix(&hex[..12], 16).expect("hex");
        assert!(millis >= before && millis < before + 60_000, "{id}");
        assert_eq!(&hex[12..13], "7", "version: {id}");
        assert!("89ab".contains(&hex[16..17]), "variant: {id}");
        let event = json!({
            "id": id, "project": "prj-8m2k4d7q", "ticket": "tk-9f2x4a8m",
            "device": new_device_id(), "seq": 1, "type": "thread.resolved",
            "author": { "name": "alice", "role": "human" },
            "payload": { "thread": thread_of(&id) },
        });
        assert_eq!(schema_violations(&event), Vec::<String>::new());
    }
}
