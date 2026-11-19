use serde_json::{Value, json};

use crate::artifact;

/// The `approved_by` entries for `artifact` at content hash `hash` (concept Section 12,
/// "Approvals and condensation"): for every person and role, the last `approval.given` in the
/// order of `events` counts, and enters when it approves `hash`. Sorted by name and role.
pub(crate) fn approved_by(events: &[Value], artifact: &str, hash: &str) -> Vec<Value> {
    let mut last: Vec<((String, String), &Value)> = Vec::new();
    for event in events {
        if event["type"] != "approval.given" || event["payload"]["artifact"] != artifact {
            continue;
        }
        let key = (
            event["author"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            event["author"]["role"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        );
        match last.iter_mut().find(|(known, _)| *known == key) {
            Some(entry) => entry.1 = event,
            None => last.push((key, event)),
        }
    }
    last.sort_by(|(left, _), (right, _)| left.cmp(right));
    last.into_iter()
        .filter(|(_, event)| {
            event["payload"]["decision"] == "approved" && event["payload"]["hash"] == hash
        })
        .map(|((name, role), event)| {
            let mut entry = json!({
                "name": name,
                "role": role,
                "hash": hash,
                "date": date_of(event["id"].as_str().unwrap_or_default()),
            });
            if let Some(proof) = event["payload"].get("proof") {
                entry["proof"] = proof.clone();
            }
            entry
        })
        .collect()
}

/// Checks a summary file's content: a list of `{ q, a }` or `{ rejected }` with text values.
pub(crate) fn summary(value: &Value) -> Result<Value, String> {
    let entries = value
        .as_array()
        .ok_or("the summary must be a list of entries `{ q, a }` or `{ rejected }`")?;
    for (index, entry) in entries.iter().enumerate() {
        let keys: Vec<&str> = entry
            .as_object()
            .map(|map| map.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let shaped = matches!(keys.as_slice(), ["a", "q"] | ["rejected"])
            && entry
                .as_object()
                .is_some_and(|map| map.values().all(Value::is_string));
        if !shaped {
            return Err(format!(
                "summary entry {} must be `{{ q, a }}` or `{{ rejected }}` with text values",
                index + 1
            ));
        }
    }
    Ok(value.clone())
}

/// `bytes` of the artifact at `path` with its `review` block replaced by `review`. Every byte
/// outside the block stays as it was.
///
/// # Errors
///
/// Returns a message if the file cannot be read as an artifact, or if the result would change
/// its content hash or not read back as `review`.
pub(crate) fn rewrite(path: &str, bytes: &[u8], review: &Value) -> Result<Vec<u8>, String> {
    let before = artifact::parse(path, bytes).map_err(|violations| violations.join("; "))?;
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    if text.contains('\r') {
        return Err("the file has carriage returns; condense it after converting to \\n".into());
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (start, end) = if path.ends_with(".md") {
        let close = lines
            .iter()
            .skip(1)
            .position(|line| line.trim_end_matches('\n') == "---")
            .map_or(lines.len(), |index| index + 1);
        (1, close)
    } else {
        (0, lines.len())
    };
    let mut front: Vec<String> = Vec::new();
    let mut inside = false;
    for line in &lines[start..end] {
        let bare = line.trim_end_matches('\n');
        if bare == "review:" || bare.starts_with("review: ") || bare.starts_with("review:\t") {
            inside = true;
            continue;
        }
        if inside && (bare.is_empty() || bare.starts_with([' ', '\t', '-'])) {
            continue;
        }
        inside = false;
        front.push((*line).to_owned());
    }
    if front.last().is_some_and(|line| !line.ends_with('\n')) {
        front.push("\n".into());
    }
    front.push(render(review));
    let mut result = lines[..start].concat();
    result.push_str(&front.concat());
    result.push_str(&lines[end..].concat());
    let after = artifact::parse(path, result.as_bytes()).map_err(|violations| {
        format!("the condensed file is not valid: {}", violations.join("; "))
    })?;
    if after.hash() != before.hash() || after.data.get("review") != Some(review) {
        return Err("condensing would change more than the `review` block".into());
    }
    Ok(result.into_bytes())
}

// Double-quoted strings and flow maps keep the block inside the strict YAML profile whatever
// the text holds. JSON string escapes are valid YAML double-quoted escapes.
fn render(review: &Value) -> String {
    const ORDER: [&str; 8] = [
        "name", "role", "hash", "date", "proof", "q", "a", "rejected",
    ];
    let flow = |entry: &Value| {
        let map = entry.as_object().cloned().unwrap_or_default();
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort_by_key(|key| {
            ORDER
                .iter()
                .position(|known| known == key)
                .unwrap_or(ORDER.len())
        });
        let fields: Vec<String> = keys
            .into_iter()
            .map(|key| format!("{key}: {}", scalar(&map[key])))
            .collect();
        format!("{{ {} }}", fields.join(", "))
    };
    let mut text = String::from("review:\n");
    for key in ["approved_by", "summary"] {
        let Some(entries) = review[key].as_array() else {
            continue;
        };
        if entries.is_empty() {
            text.push_str(&format!("  {key}: []\n"));
            continue;
        }
        text.push_str(&format!("  {key}:\n"));
        for entry in entries {
            text.push_str(&format!("    - {}\n", flow(entry)));
        }
    }
    text
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => serde_json::to_string(text).expect("a string serializes"),
        other => serde_json::to_string(&other.to_string()).expect("a string serializes"),
    }
}

/// The UTC date `YYYY-MM-DD` of a UUIDv7 event ID, from its first 48 bits (milliseconds since
/// the Unix epoch).
fn date_of(event_id: &str) -> String {
    let hex: String = event_id
        .trim_start_matches("ev-")
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(12)
        .collect();
    let millis = u64::from_str_radix(&hex, 16).unwrap_or(0);
    let days = i64::try_from(millis / 86_400_000).unwrap_or(0);
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_comes_from_the_uuid_timestamp() {
        // 0x0193a8f09b2c ms = 1733706160940, `date -u -d @1733706160 +%F` gives 2024-12-09.
        assert_eq!(
            date_of("ev-0193a8f0-9b2c-7e4d-a1f3-000000000001"),
            "2024-12-09"
        );
        assert_eq!(
            date_of("ev-00000000-0000-7000-8000-000000000000"),
            "1970-01-01"
        );
    }
}
