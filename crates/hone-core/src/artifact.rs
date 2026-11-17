use serde_json::{Map, Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::{canonical, yaml};

#[derive(Debug, Clone)]
pub(crate) struct Content {
    pub(crate) data: Map<String, Value>,
    pub(crate) body: String,
}

impl Content {
    pub(crate) fn hash(&self) -> String {
        let mut meta = self.data.clone();
        meta.remove("review");
        let document = json!({ "meta": meta, "body": normalize_body(&self.body) });
        canonical::sha256(canonical::to_canonical(&document).as_bytes())
    }
}

pub(crate) fn parse(path: &str, bytes: &[u8]) -> Result<Content, Vec<String>> {
    if bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(vec!["BOM not allowed".into()]);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|error| vec![format!("not valid UTF-8: {error}")])?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let markdown = path.ends_with(".md");
    let (front, body) = if markdown {
        split_front_matter(&text).map_err(|message| vec![message.to_owned()])?
    } else {
        (text, String::new())
    };
    let lines_before_front_matter = usize::from(markdown);
    let value = yaml::parse_str(&front).map_err(|violations| {
        violations
            .into_iter()
            .map(|mut violation| {
                if violation.line > 0 {
                    violation.line += lines_before_front_matter;
                }
                violation.to_string()
            })
            .collect::<Vec<_>>()
    })?;
    match value {
        Value::Object(data) => Ok(Content { data, body }),
        _ => Err(vec!["the data model must be a map".into()]),
    }
}

fn split_front_matter(text: &str) -> Result<(String, String), &'static str> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines[0] != "---" {
        return Err("front matter missing: the first line must be ---");
    }
    let end = lines[1..]
        .iter()
        .position(|line| *line == "---")
        .ok_or("front matter not closed")?
        + 1;
    Ok((lines[1..end].join("\n"), lines[end + 1..].join("\n")))
}

fn normalize_body(body: &str) -> String {
    let text: String = body.nfc().collect();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect();
    lines.join("\n").trim_matches('\n').to_owned()
}
