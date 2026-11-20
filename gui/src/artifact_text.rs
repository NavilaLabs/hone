//! The text of an artifact file, changed one top-level field at a time: a changed field is
//! written anew in the strict YAML profile, every other field keeps its bytes, comments
//! included. The GUI parses no YAML; the values come from `hone artifact show`, and
//! `hone artifact save` checks the result.

use serde_json::Value;

/// An artifact file split into its top-level fields and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactText {
    /// The lines `---` around the front matter, with their line endings; `None` for a YAML file.
    delimiters: Option<(String, String)>,
    blocks: Vec<Block>,
    body: String,
}

/// The lines of one top-level field, with the comment and blank lines before it. A block
/// without a key holds lines before the first field or after the last one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    key: Option<String>,
    prefix: String,
    text: String,
}

impl ArtifactText {
    /// Splits `text`, a Markdown file with front matter when `markdown`, otherwise a YAML file.
    ///
    /// # Errors
    ///
    /// Returns a message when a Markdown file has no front matter between two `---` lines.
    pub fn parse(text: &str, markdown: bool) -> Result<Self, String> {
        if !markdown {
            return Ok(Self {
                delimiters: None,
                blocks: blocks(text),
                body: String::new(),
            });
        }
        let (open, front, close, body) = split_front_matter(text)
            .ok_or_else(|| "the file has no front matter between two `---` lines".to_owned())?;
        Ok(Self {
            delimiters: Some((open.to_owned(), close.to_owned())),
            blocks: blocks(front),
            body: body.to_owned(),
        })
    }

    /// The top-level keys in the order of the file.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.blocks
            .iter()
            .filter_map(|block| block.key.clone())
            .collect()
    }

    /// Replaces the body. A YAML file has none, so the call does nothing there.
    pub fn set_body(&mut self, body: &str) {
        if self.delimiters.is_some() {
            body.clone_into(&mut self.body);
        }
    }

    /// Writes the top-level field `key` with `value`, in place, or after the last field.
    pub fn set(&mut self, key: &str, value: &Value) {
        let text = field(key, value);
        if let Some(block) = self.block_mut(key) {
            block.text = text;
            return;
        }
        let at = self
            .blocks
            .iter()
            .rposition(|block| block.key.is_some())
            .map_or(self.blocks.len(), |index| index + 1);
        self.blocks.insert(
            at,
            Block {
                key: Some(key.to_owned()),
                prefix: String::new(),
                text,
            },
        );
    }

    /// Removes the top-level field `key` and the comments before it.
    pub fn remove(&mut self, key: &str) {
        self.blocks
            .retain(|block| block.key.as_deref() != Some(key));
    }

    /// The text of the whole file.
    #[must_use]
    pub fn text(&self) -> String {
        let front: String = self
            .blocks
            .iter()
            .flat_map(|block| [block.prefix.as_str(), block.text.as_str()])
            .collect();
        match &self.delimiters {
            Some((open, close)) => format!("{open}{front}{close}{}", self.body),
            None => front,
        }
    }

    fn block_mut(&mut self, key: &str) -> Option<&mut Block> {
        self.blocks
            .iter_mut()
            .find(|block| block.key.as_deref() == Some(key))
    }
}

/// The opening line, the front matter, the closing line, and the body of a Markdown file.
fn split_front_matter(text: &str) -> Option<(&str, &str, &str, &str)> {
    let open = ["---\n", "---\r\n"]
        .into_iter()
        .find(|open| text.starts_with(open))?;
    let rest = &text[open.len()..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            let body = &rest[offset + line.len()..];
            return Some((open, &rest[..offset], line, body));
        }
        offset += line.len();
    }
    None
}

fn blocks(front: &str) -> Vec<Block> {
    let mut blocks = vec![Block {
        key: None,
        prefix: String::new(),
        text: String::new(),
    }];
    for line in front.split_inclusive('\n') {
        let current = blocks.last_mut().expect("there is always a block");
        match top_level_key(line) {
            Some(key) => {
                // Comments and blank lines at the end of a field belong to the next one.
                let moved = current.text.split_off(trailing_comments(&current.text));
                blocks.push(Block {
                    key: Some(key),
                    prefix: moved,
                    text: line.to_owned(),
                });
            }
            None => current.text.push_str(line),
        }
    }
    if let Some(last) = blocks.last_mut().filter(|block| block.key.is_some()) {
        let trailing = last.text.split_off(trailing_comments(&last.text));
        if !trailing.is_empty() {
            blocks.push(Block {
                key: None,
                prefix: String::new(),
                text: trailing,
            });
        }
    }
    blocks
}

/// Where the blank lines and the comments at the start of a line that end `text` begin. An
/// indented `#` line can be the content of a block scalar, so it stays.
fn trailing_comments(text: &str) -> usize {
    let mut start = text.len();
    for line in text.split_inclusive('\n').rev() {
        if !(line.starts_with('#') || line.trim().is_empty()) {
            break;
        }
        start -= line.len();
    }
    start
}

/// The key of a line that starts a top-level field: no indentation, no comment, no sequence
/// item, and a key followed by `:`.
fn top_level_key(line: &str) -> Option<String> {
    let first = line.chars().next()?;
    if first.is_whitespace() || matches!(first, '#' | '-' | '%') {
        return None;
    }
    if let Some(quote @ ('"' | '\'')) = line.chars().next() {
        let end = line[1..].find(quote)? + 1;
        let rest = &line[end + 1..];
        return rest.starts_with(':').then(|| line[1..end].to_owned());
    }
    let colon = line
        .char_indices()
        .find(|&(index, character)| {
            character == ':'
                && line[index + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })?
        .0;
    Some(line[..colon].to_owned())
}

/// `key: value` in the strict YAML profile: strings double-quoted, lists of scalars and empty
/// collections in flow style, maps in block style, a list of collections one item per line.
fn field(key: &str, value: &Value) -> String {
    let mut out = String::new();
    write_field(&mut out, "", key, value);
    out
}

fn write_field(out: &mut String, indent: &str, key: &str, value: &Value) {
    let key = yaml_key(key);
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str(&format!("{indent}{key}:\n"));
            let inner = format!("{indent}  ");
            for (key, value) in map {
                write_field(out, &inner, key, value);
            }
        }
        Value::Array(items) if items.iter().any(|item| item.is_object() || item.is_array()) => {
            out.push_str(&format!("{indent}{key}:\n"));
            for item in items {
                out.push_str(&format!("{indent}  - {}\n", flow(item)));
            }
        }
        value => out.push_str(&format!("{indent}{key}: {}\n", flow(value))),
    }
}

fn flow(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(flow).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) if map.is_empty() => "{}".to_owned(),
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(key, value)| format!("{}: {}", yaml_key(key), flow(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // A JSON string is a valid double-quoted YAML scalar.
        scalar => scalar.to_string(),
    }
}

/// A key without quotes when no YAML parser can read it as anything but that string.
fn yaml_key(key: &str) -> String {
    const READ_AS_OTHER: [&str; 9] = ["y", "n", "yes", "no", "on", "off", "true", "false", "null"];
    let mut characters = key.chars();
    let plain = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && !READ_AS_OTHER.contains(&key.to_ascii_lowercase().as_str());
    if plain {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ArtifactText;

    const DECISION: &str = "---\n\
        # The decision on fees.\n\
        id: \"dec-7k3m2q8p\"\n\
        type: decision # the kind\n\
        status: \"proposed\"\n\
        links:\n  affects: [\"mod-4t8w2n6c\"]\n\
        \n\
        # Who wrote it.\n\
        authors:\n- \"alice\"\n\
        # trailing\n\
        ---\n\
        # Fees\n\nText.\n";

    #[test]
    fn untouched_text_comes_back_byte_for_byte() {
        let text = ArtifactText::parse(DECISION, true).expect("front matter");
        assert_eq!(text.text(), DECISION);
        assert_eq!(text.keys(), ["id", "type", "status", "links", "authors"]);
        let yaml = "id: \"mod-4t8w2n6c\"\n# note\nname: \"Fees\"\n";
        assert_eq!(ArtifactText::parse(yaml, false).expect("yaml").text(), yaml);
    }

    #[test]
    fn a_changed_field_is_rewritten_in_place_and_keeps_its_comments() {
        let mut text = ArtifactText::parse(DECISION, true).expect("front matter");
        text.set("status", &json!("accepted"));
        text.set(
            "links",
            &json!({ "affects": ["mod-4t8w2n6c"], "supersedes": ["dec-2b7n5k1q"] }),
        );
        text.set("authors", &json!(["alice", "bob"]));
        text.set_body("# Fees\n\nMore text.\n");
        assert_eq!(
            text.text(),
            "---\n\
             # The decision on fees.\n\
             id: \"dec-7k3m2q8p\"\n\
             type: decision # the kind\n\
             status: \"accepted\"\n\
             links:\n  affects: [\"mod-4t8w2n6c\"]\n  supersedes: [\"dec-2b7n5k1q\"]\n\
             \n\
             # Who wrote it.\n\
             authors: [\"alice\", \"bob\"]\n\
             # trailing\n\
             ---\n\
             # Fees\n\nMore text.\n"
        );
    }

    #[test]
    fn a_new_field_follows_the_last_field_and_a_removed_one_takes_its_comments() {
        let mut text = ArtifactText::parse(DECISION, true).expect("front matter");
        text.set(
            "anchors",
            &json!([{ "kind": "symbol", "ref": "src/fee.rs#Fee::add" }]),
        );
        text.remove("authors");
        text.remove("links");
        assert_eq!(
            text.text(),
            "---\n\
             # The decision on fees.\n\
             id: \"dec-7k3m2q8p\"\n\
             type: decision # the kind\n\
             status: \"proposed\"\n\
             anchors:\n  - { kind: \"symbol\", ref: \"src/fee.rs#Fee::add\" }\n\
             # trailing\n\
             ---\n\
             # Fees\n\nText.\n"
        );
    }

    #[test]
    fn values_are_written_in_the_strict_profile() {
        let mut text = ArtifactText::parse("", false).expect("yaml");
        text.set("created", &json!("2026-11-20"));
        text.set("answer", &json!("yes"));
        text.set("count", &json!(3));
        text.set("done", &json!(false));
        text.set("parent", &json!(null));
        text.set("secrets", &json!([]));
        text.set("x-note", &json!("a \"quoted\"\nline"));
        text.set("interface", &json!({ "code": "src/fee.rs#Fee", "x-y": {} }));
        text.set("on", &json!(1));
        assert_eq!(
            text.text(),
            "created: \"2026-11-20\"\n\
             answer: \"yes\"\n\
             count: 3\n\
             done: false\n\
             parent: null\n\
             secrets: []\n\
             x-note: \"a \\\"quoted\\\"\\nline\"\n\
             interface:\n  code: \"src/fee.rs#Fee\"\n  x-y: {}\n\
             \"on\": 1\n"
        );
    }

    #[test]
    fn keys_are_found_with_quotes_and_colons_in_values() {
        let yaml = "\"title\": \"a: b\"\nurl: https://example.org\n";
        let mut text = ArtifactText::parse(yaml, false).expect("yaml");
        text.set("title", &json!("c"));
        text.set("url", &json!("https://example.org/x"));
        assert_eq!(
            text.text(),
            "title: \"c\"\nurl: \"https://example.org/x\"\n"
        );
    }

    #[test]
    fn a_markdown_file_needs_front_matter() {
        assert_eq!(
            ArtifactText::parse("# Fees\n", true),
            Err("the file has no front matter between two `---` lines".to_owned())
        );
        let mut crlf = ArtifactText::parse("---\r\nid: \"a\"\r\n---\r\nText", true).expect("crlf");
        crlf.set_body("More");
        assert_eq!(crlf.text(), "---\r\nid: \"a\"\r\n---\r\nMore");
    }
}
