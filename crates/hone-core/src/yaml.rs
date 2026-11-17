use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use saphyr_parser::{Event, Parser, ScalarStyle, Span};
use serde_json::{Map, Number, Value};

const MAX_INTEGER: i64 = (1 << 53) - 1;

static AMBIGUOUS_BOOLEAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:y|Y|yes|Yes|YES|n|N|no|No|NO|on|On|ON|off|Off|OFF|True|TRUE|False|FALSE)$")
        .expect("valid regex")
});
static AMBIGUOUS_NULL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:~|Null|NULL)$").expect("valid regex"));
static NUMBER_LIKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^[-+]?(?:\.[0-9]+|[0-9][0-9_]*(?:\.[0-9_]*)?)(?:[eE][-+]?[0-9]+)?$
        | ^0[xob][0-9a-fA-F_]+$
        | ^[-+]?\.(?:inf|Inf|INF)$
        | ^\.(?:nan|NaN|NAN)$
        | ^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+(?:\.[0-9_]*)?$",
    )
    .expect("valid regex")
});
static PLAIN_INTEGER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-?(?:0|[1-9][0-9]*)$").expect("valid regex"));

/// A document that violates the strict YAML profile (concept Section 5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct YamlProfileError {
    /// The violations, each with its line where known, never empty.
    pub violations: Vec<String>,
}

impl std::fmt::Display for YamlProfileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.violations.join("; "))
    }
}

impl std::error::Error for YamlProfileError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileViolation {
    pub(crate) line: usize,
    pub(crate) message: String,
}

impl std::fmt::Display for ProfileViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            formatter.write_str(&self.message)
        } else {
            write!(formatter, "line {}: {}", self.line, self.message)
        }
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Value, Vec<ProfileViolation>> {
    if bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(vec![violation(0, "BOM not allowed")]);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|error| vec![violation(0, format!("not valid UTF-8: {error}"))])?;
    parse_str(text)
}

pub(crate) fn parse_str(text: &str) -> Result<Value, Vec<ProfileViolation>> {
    let mut builder = Builder::default();
    builder.check_directives(text);
    for result in Parser::new_from_str(text) {
        match result {
            Ok((event, span)) => {
                if !builder.accept(event, span) {
                    break;
                }
            }
            Err(error) => {
                builder.violations.push(violation(
                    error.marker().line(),
                    format!("YAML syntax error: {}", error.info()),
                ));
                break;
            }
        }
    }
    builder.finish()
}

fn violation(line: usize, message: impl Into<String>) -> ProfileViolation {
    ProfileViolation {
        line,
        message: message.into(),
    }
}

enum Frame {
    Sequence(Vec<Value>),
    Mapping {
        entries: Map<String, Value>,
        keys: BTreeSet<String>,
        pending_key: Option<String>,
    },
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    documents: usize,
    root: Option<Value>,
    violations: Vec<ProfileViolation>,
}

impl Builder {
    fn check_directives(&mut self, text: &str) {
        // Directives are not visible in the event stream, so the prelude before the first
        // `---` is checked line by line.
        for (index, line) in text.lines().enumerate() {
            if line == "---" || line.starts_with("--- ") || line.starts_with("---\t") {
                return;
            }
            if let Some(directive) = line.strip_prefix('%') {
                let mut words = directive.split_whitespace();
                let accepted = words.next() == Some("YAML")
                    && words.next() == Some("1.2")
                    && words.next().is_none_or(|word| word.starts_with('#'));
                if !accepted {
                    self.violations
                        .push(violation(index + 1, format!("directive `{line}`")));
                }
            } else if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                return;
            }
        }
    }

    fn expects_key(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Frame::Mapping {
                pending_key: None,
                ..
            })
        )
    }

    fn accept(&mut self, event: Event<'_>, span: Span) -> bool {
        let line = span.start.line();
        match event {
            Event::DocumentStart(_) => {
                self.documents += 1;
                if self.documents > 1 {
                    self.violations
                        .push(violation(line, "several YAML documents"));
                    return false;
                }
            }
            Event::Alias(_) => {
                self.violations.push(violation(line, "alias"));
                self.add(Value::Null, line);
            }
            Event::Scalar(value, style, anchor, tag) => {
                self.check_node(anchor, tag.is_some(), line);
                if self.expects_key() {
                    if let Some(message) = check_scalar(&value, style, true) {
                        self.violations.push(violation(line, message));
                    }
                    self.add_key(value.into_owned(), line);
                } else {
                    let converted = match check_scalar(&value, style, false) {
                        Some(message) => {
                            self.violations.push(violation(line, message));
                            Value::Null
                        }
                        None => convert_scalar(&value, style),
                    };
                    self.add(converted, line);
                }
            }
            Event::SequenceStart(anchor, ref tag) | Event::MappingStart(anchor, ref tag) => {
                self.check_node(anchor, tag.is_some(), line);
                if self.expects_key() {
                    self.violations
                        .push(violation(line, "key must be a scalar"));
                    return false;
                }
                let frame = if matches!(event, Event::SequenceStart(..)) {
                    Frame::Sequence(Vec::new())
                } else {
                    Frame::Mapping {
                        entries: Map::new(),
                        keys: BTreeSet::new(),
                        pending_key: None,
                    }
                };
                self.stack.push(frame);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let value = match self.stack.pop() {
                    Some(Frame::Sequence(items)) => Value::Array(items),
                    Some(Frame::Mapping { entries, .. }) => Value::Object(entries),
                    None => return false,
                };
                self.add(value, line);
            }
            Event::StreamStart | Event::StreamEnd | Event::DocumentEnd | Event::Nothing => {}
        }
        true
    }

    fn check_node(&mut self, anchor: usize, tagged: bool, line: usize) {
        if anchor != 0 {
            self.violations.push(violation(line, "anchor"));
        }
        if tagged {
            self.violations.push(violation(line, "tag"));
        }
    }

    fn add_key(&mut self, key: String, line: usize) {
        if let Some(Frame::Mapping {
            keys, pending_key, ..
        }) = self.stack.last_mut()
        {
            if !keys.insert(key.clone()) {
                self.violations
                    .push(violation(line, format!("duplicate key `{key}`")));
            }
            *pending_key = Some(key);
        }
    }

    fn add(&mut self, value: Value, line: usize) {
        match self.stack.last_mut() {
            Some(Frame::Sequence(items)) => items.push(value),
            Some(Frame::Mapping {
                entries,
                pending_key,
                ..
            }) => {
                if let Some(key) = pending_key.take() {
                    entries.insert(key, value);
                }
            }
            None => {
                if self.root.replace(value).is_some() {
                    self.violations
                        .push(violation(line, "several YAML documents"));
                }
            }
        }
    }

    fn finish(mut self) -> Result<Value, Vec<ProfileViolation>> {
        if self.violations.is_empty() && self.root.is_none() {
            self.violations.push(violation(0, "empty document"));
        }
        match self.root {
            Some(root) if self.violations.is_empty() => Ok(root),
            _ => Err(self.violations),
        }
    }
}

fn check_scalar(value: &str, style: ScalarStyle, is_key: bool) -> Option<String> {
    if style != ScalarStyle::Plain {
        return None;
    }
    if value.is_empty() {
        return Some("empty value (null is written as `null`)".into());
    }
    if is_key && value == "<<" {
        return Some("merge key `<<`".into());
    }
    let integer = PLAIN_INTEGER.is_match(value);
    if integer || matches!(value, "true" | "false" | "null") {
        if is_key {
            return Some(format!("key `{value}` is not a string (quote it)"));
        }
        if integer && !value.parse::<i64>().is_ok_and(|n| n.abs() <= MAX_INTEGER) {
            return Some(format!("integer `{value}` outside ±(2^53-1)"));
        }
        return None;
    }
    if AMBIGUOUS_BOOLEAN.is_match(value)
        || AMBIGUOUS_NULL.is_match(value)
        || NUMBER_LIKE.is_match(value)
    {
        return Some(format!("ambiguous value `{value}` (quote it)"));
    }
    None
}

fn convert_scalar(value: &str, style: ScalarStyle) -> Value {
    if style != ScalarStyle::Plain {
        return Value::String(value.to_owned());
    }
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => match value.parse::<i64>() {
            Ok(number) if PLAIN_INTEGER.is_match(value) => Value::Number(Number::from(number)),
            _ => Value::String(value.to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse, parse_str};

    fn first_violation(text: &str) -> String {
        parse_str(text).expect_err("must violate the profile")[0]
            .message
            .clone()
    }

    #[test]
    fn reads_the_data_model() {
        let value = parse_str(
            "a: 1\nb: [true, false, null]\nc: \"010\"\nd: 2026-11-16x\ne: { f: -0 }\ng: |\n  text\n",
        )
        .expect("valid");
        assert_eq!(
            value,
            json!({"a": 1, "b": [true, false, null], "c": "010", "d": "2026-11-16x", "e": {"f": 0}, "g": "text\n"})
        );
        assert_eq!(parse_str("null").expect("valid"), json!(null));
        assert_eq!(
            parse_str("%YAML 1.2\n---\na: b\n").expect("valid"),
            json!({"a": "b"})
        );
    }

    #[test]
    fn rejects_what_the_profile_forbids() {
        let cases = [
            ("a: &x 1\nb: *x\n", "anchor"),
            ("a: !!str 1\n", "tag"),
            ("a: 1\na: 2\n", "duplicate key `a`"),
            ("a: 1.5\n", "ambiguous value `1.5` (quote it)"),
            ("a: yes\n", "ambiguous value `yes` (quote it)"),
            ("a: ~\n", "ambiguous value `~` (quote it)"),
            ("a: 0x1F\n", "ambiguous value `0x1F` (quote it)"),
            ("a: 1:30\n", "ambiguous value `1:30` (quote it)"),
            ("a: 010\n", "ambiguous value `010` (quote it)"),
            ("a:\n", "empty value (null is written as `null`)"),
            ("1: a\n", "key `1` is not a string (quote it)"),
            ("<<: {a: 1}\n", "merge key `<<`"),
            (
                "a: 9007199254740992\n",
                "integer `9007199254740992` outside ±(2^53-1)",
            ),
            ("? [a]\n: b\n", "key must be a scalar"),
            ("a: 1\n---\nb: 2\n", "several YAML documents"),
            ("%YAML 1.1\n---\na: 1\n", "directive `%YAML 1.1`"),
            (
                "%TAG ! tag:example.com,2026:\n---\na: 1\n",
                "directive `%TAG ! tag:example.com,2026:`",
            ),
            ("", "empty document"),
        ];
        for (text, expected) in cases {
            assert_eq!(first_violation(text), expected, "input: {text:?}");
        }
        assert_eq!(
            parse(b"\xef\xbb\xbfa: 1\n").expect_err("BOM")[0].message,
            "BOM not allowed"
        );
        assert!(
            parse(b"a: \xff\n").expect_err("UTF-8")[0]
                .message
                .starts_with("not valid UTF-8")
        );
        assert!(first_violation("a: [1\n").starts_with("YAML syntax error"));
    }
}
