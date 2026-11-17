use std::cmp::Ordering;
use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn to_canonical(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for byte in digest {
        write!(out, "{byte:02x}").expect("writing to a String never fails");
    }
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => {
            let float = number
                .as_f64()
                .expect("serde_json numbers without arbitrary precision are finite");
            out.push_str(ryu_js::Buffer::new().format_finite(float));
        }
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(entries) => {
            let mut keys: Vec<&String> = entries.keys().collect();
            keys.sort_by(|a, b| compare_utf16(a, b));
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, &entries[key]);
            }
            out.push('}');
        }
    }
}

fn compare_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            control if u32::from(control) < 0x20 => {
                write!(out, "\\u{:04x}", u32::from(control))
                    .expect("writing to a String never fails");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{sha256, to_canonical};

    #[test]
    fn sorts_keys_by_utf16_code_units() {
        // RFC 8785, Section 3.2.3: U+1F600 (surrogates D83D DE00) sorts before U+FB33.
        let value =
            json!({"\u{fb33}": 1, "\u{1f600}": 2, "\r": 3, "1": 4, "\u{80}": 5, "a": 6, "ö": 7});
        assert_eq!(
            to_canonical(&value),
            "{\"\\r\":3,\"1\":4,\"a\":6,\"\u{80}\":5,\"ö\":7,\"\u{1f600}\":2,\"\u{fb33}\":1}"
        );
    }

    #[test]
    fn escapes_like_ecmascript() {
        let value = json!("\u{0}\u{8}\t\n\u{b}\u{c}\r\u{1f}\"\\/\u{7f}\u{2028}é");
        assert_eq!(
            to_canonical(&value),
            "\"\\u0000\\b\\t\\n\\u000b\\f\\r\\u001f\\\"\\\\/\u{7f}\u{2028}é\""
        );
    }

    #[test]
    fn formats_numbers_like_ecmascript() {
        let value: serde_json::Value = serde_json::from_str(
            "[0, -0, 1, -1, 9007199254740991, 1.5, 0.000001, 1e-7, 1e21, 123456789012345680000, 4.5e-10, 1.0]",
        )
        .expect("valid JSON");
        assert_eq!(
            to_canonical(&value),
            "[0,0,1,-1,9007199254740991,1.5,0.000001,1e-7,1e+21,123456789012345680000,4.5e-10,1]"
        );
    }

    #[test]
    fn nests_without_whitespace() {
        let value = json!({"b": [true, false, null, {"d": "e", "c": []}], "a": {}});
        assert_eq!(
            to_canonical(&value),
            r#"{"a":{},"b":[true,false,null,{"c":[],"d":"e"}]}"#
        );
    }

    #[test]
    fn hashes_with_prefix() {
        assert_eq!(
            sha256(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
