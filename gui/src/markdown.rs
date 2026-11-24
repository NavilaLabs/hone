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

/// A passage of a body that a thread quotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    /// The number the preview shows after the passage.
    pub number: usize,
    pub text: String,
    /// Whether the passage is drawn as the chosen one.
    pub chosen: bool,
}

/// The HTML of the Markdown `body` with each quote marked where it first occurs in one piece of
/// text, followed by its number, and the numbers of the quotes found. A quote that spans
/// formatting (for example a code span inside it) or that overlaps an earlier quote is not found.
#[must_use]
pub fn preview_quoted(body: &str, quotes: &[Quote]) -> (String, Vec<usize>) {
    let html = preview(body);
    let mut marks: Vec<(usize, usize, &Quote)> = Vec::new();
    for quote in quotes {
        let needle = text_escaped(quote.text.trim());
        if needle.is_empty() {
            continue;
        }
        let found = html.match_indices(&needle).map(|(at, _)| at).find(|&at| {
            let end = at + needle.len();
            outside_tags(&html, at)
                && !marks
                    .iter()
                    .any(|&(start, stop, _)| at < stop && start < end)
        });
        if let Some(at) = found {
            marks.push((at, at + needle.len(), quote));
        }
    }
    let found = marks.iter().map(|(_, _, quote)| quote.number).collect();
    marks.sort_by_key(|&(start, _, _)| std::cmp::Reverse(start));
    let mut out = html;
    for (start, end, quote) in marks {
        let class = if quote.chosen {
            "hone-quote hone-quote-chosen"
        } else {
            "hone-quote"
        };
        out.insert_str(
            end,
            &format!(
                "</mark><sup class=\"hone-quote-number\">{}</sup>",
                quote.number
            ),
        );
        out.insert_str(start, &format!("<mark class=\"{class}\">"));
    }
    (out, found)
}

/// Whether `at` lies in text, not inside a tag.
fn outside_tags(html: &str, at: usize) -> bool {
    let before = &html[..at];
    match (before.rfind('<'), before.rfind('>')) {
        (Some(open), Some(close)) => close > open,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// `text` as `pulldown-cmark` escapes body text.
fn text_escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
    use super::{Quote, preview, preview_quoted};

    fn quote(number: usize, text: &str, chosen: bool) -> Quote {
        Quote {
            number,
            text: text.into(),
            chosen,
        }
    }

    #[test]
    fn a_quote_is_marked_where_it_first_occurs_in_text() {
        let body = "[pending](https://x.org/pending) state.\n\nThe pending state & more.\n\nThe pending state & more.";
        let (html, found) = preview_quoted(
            body,
            &[
                quote(1, "pending state & more", true),
                quote(2, "pending", false),
            ],
        );
        assert_eq!(
            html,
            "<p><span class=\"underline\" title=\"https://x.org/pending\"><mark class=\"hone-quote\">pending</mark><sup class=\"hone-quote-number\">2</sup></span> state.</p>\n\
             <p>The <mark class=\"hone-quote hone-quote-chosen\">pending state &amp; more</mark><sup class=\"hone-quote-number\">1</sup>.</p>\n\
             <p>The pending state &amp; more.</p>\n"
        );
        assert_eq!(found, [1, 2]);
    }

    #[test]
    fn a_quote_across_formatting_or_over_another_is_not_found() {
        let body = "Use `PaymentGateway` now. Pending ends.";
        let (html, found) = preview_quoted(
            body,
            &[
                quote(1, "Use PaymentGateway", false),
                quote(2, "Pending ends", false),
                quote(3, "ends", false),
                quote(4, "  ", false),
            ],
        );
        assert_eq!(found, [2]);
        assert_eq!(
            html,
            "<p>Use <code>PaymentGateway</code> now. <mark class=\"hone-quote\">Pending ends</mark><sup class=\"hone-quote-number\">2</sup>.</p>\n"
        );
    }

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
