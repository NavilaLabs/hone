//! The preview of a Markdown body as HTML that the window can show without risk: raw HTML is
//! shown as text, a link is underlined text that navigates nowhere, and an image is its
//! description, so a body can neither run a script nor make the window load anything.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// The HTML of the Markdown `body`.
#[must_use]
pub fn preview(body: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let events = Parser::new_ext(body, options).map(|event| match event {
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link { dest_url, .. }) => Event::InlineHtml(CowStr::from(format!(
            "<span class=\"underline\" title=\"{}\">",
            escaped(&dest_url)
        ))),
        Event::Start(Tag::Image { dest_url, .. }) => Event::InlineHtml(CowStr::from(format!(
            "<span class=\"text-ink-muted\" title=\"{}\">[image: ",
            escaped(&dest_url)
        ))),
        Event::End(TagEnd::Link) => Event::InlineHtml(CowStr::from("</span>")),
        Event::End(TagEnd::Image) => Event::InlineHtml(CowStr::from("]</span>")),
        event => event,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            character => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::preview;

    #[test]
    fn markdown_becomes_html() {
        assert_eq!(
            preview("# Fees\n\nThe **fee** is `1`."),
            "<h1>Fees</h1>\n<p>The <strong>fee</strong> is <code>1</code>.</p>\n"
        );
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        assert_eq!(
            preview("<script>alert(1)</script>\n\nA <b onclick=\"x()\">b</b>."),
            "&lt;script&gt;alert(1)&lt;/script&gt;\n<p>A &lt;b onclick=\"x()\"&gt;b&lt;/b&gt;.</p>\n"
        );
    }

    #[test]
    fn links_and_images_neither_navigate_nor_load() {
        assert_eq!(
            preview(
                "[the \"spec\"](https://example.org/?a=1&b=\"2\") ![a chart](https://example.org/c.png)"
            ),
            "<p><span class=\"underline\" title=\"https://example.org/?a=1&amp;b=&quot;2&quot;\">the \"spec\"</span> \
             <span class=\"text-ink-muted\" title=\"https://example.org/c.png\">[image: a chart]</span></p>\n"
        );
    }
}
