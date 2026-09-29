//! Turning the assistant's Markdown into Matrix messages, and reading the user's.
//!
//! Matrix messages carry a plain `body` and an HTML `formatted_body`. The HTML is made
//! here from Markdown; anything the model wrote as raw HTML is shown as text, never
//! passed through, and links only keep web and email addresses.

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::channels::{self, Outgoing};

/// Matrix events are capped at 64 KiB; stay well under it, HTML included.
const PIECE: usize = 12_000;

/// A message ready to send: the plain body and its HTML.
#[derive(Debug, Clone, PartialEq)]
pub struct Formatted {
    pub body: String,
    pub html: String,
}

/// A message as one or more Matrix messages.
pub fn outgoing(message: &Outgoing) -> Vec<Formatted> {
    let mut md = String::new();
    if let Some(title) = &message.title {
        md.push_str(&format!("**{}**\n\n", escape_markdown(title)));
    }
    md.push_str(&message.markdown);
    for link in &message.links {
        md.push_str(&format!(
            "\n\n[{}](<{}>)",
            escape_markdown(&link.label),
            link.url.replace(['<', '>'], "")
        ));
    }
    split(&md)
        .into_iter()
        .map(|piece| Formatted {
            html: markdown_to_html(&piece),
            body: piece,
        })
        .collect()
}

/// Splits long Markdown at line breaks, closing and reopening a code block cut in two.
pub fn split(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open_fence = false;
    for piece in channels::chunks(md, PIECE) {
        let mut piece = if open_fence {
            format!("```\n{piece}")
        } else {
            piece
        };
        let fences = piece
            .lines()
            .filter(|l| l.trim_start().starts_with("```"))
            .count();
        open_fence = fences % 2 == 1;
        if open_fence {
            if !piece.ends_with('\n') {
                piece.push('\n');
            }
            piece.push_str("```");
        }
        out.push(piece.trim_end().to_owned());
    }
    out.retain(|p| !p.is_empty());
    out
}

/// Markdown as the HTML subset Matrix clients show. Raw HTML becomes text.
pub fn markdown_to_html(md: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let mut skipped_links = 0usize;
    let events = Parser::new_ext(md, options).filter_map(|event| match event {
        // The model's HTML is shown, not interpreted.
        Event::Html(html) | Event::InlineHtml(html) => Some(Event::Text(html)),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            // Autolinked addresses (`<me@example.org>`) become `mailto:` links.
            if link_type == LinkType::Email || safe_link(&dest_url) {
                Some(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }))
            } else {
                skipped_links += 1;
                None
            }
        }
        Event::End(TagEnd::Link) if skipped_links > 0 => {
            skipped_links -= 1;
            None
        }
        // Matrix pictures must be uploaded to the server; show the description instead.
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        other => Some(other),
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html.trim_end().to_owned()
}

/// Web and email links only: no `javascript:`, `data:` or relative addresses.
fn safe_link(url: &CowStr) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

/// Escapes text so it reads literally inside Markdown.
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '!' | '|' | '~' | '&'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escapes text for HTML.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A reply's text without the quoted message older clients put in front of it.
pub fn strip_reply_fallback(body: &str) -> &str {
    if !body.starts_with("> ") {
        return body;
    }
    match body.find("\n\n") {
        Some(end) if body[..end].lines().all(|l| l.starts_with('>')) => &body[end + 2..],
        _ => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::Link;

    #[test]
    fn markdown_becomes_matrix_html() {
        let html = markdown_to_html(
            "# Plan\n\n**Monday**: call Sam & book `table`\n\n- one *thing*\n- [docs](https://example.com)",
        );
        assert_eq!(
            html,
            "<h1>Plan</h1>\n<p><strong>Monday</strong>: call Sam &amp; book <code>table</code></p>\n<ul>\n<li>one <em>thing</em></li>\n<li><a href=\"https://example.com\">docs</a></li>\n</ul>"
        );
    }

    #[test]
    fn raw_html_from_the_model_is_escaped() {
        let html = markdown_to_html(
            "Hi <b onclick=\"x()\">there</b>\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>",
        );
        assert!(!html.contains("<b "), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{html}"
        );
        assert!(html.contains("&lt;b onclick="), "{html}");
    }

    #[test]
    fn only_web_and_email_links_survive() {
        let html = markdown_to_html(
            "[click](javascript:alert(1)) [data](data:text/html,hi) [ok](https://a.example/x) ![pic](https://t.example/p.png) <me@example.org>",
        );
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("data:text"), "{html}");
        assert!(html.contains("click"), "{html}");
        assert!(
            html.contains("<a href=\"https://a.example/x\">ok</a>"),
            "{html}"
        );
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("pic"), "{html}");
        assert!(html.contains("mailto:me@example.org"), "{html}");
    }

    #[test]
    fn titles_and_links_are_added_literally() {
        let parts = outgoing(&Outgoing {
            title: Some("Daily *brief*".into()),
            markdown: "All clear.".into(),
            links: vec![Link {
                label: "Open it".into(),
                url: "https://calendar.example/new?a=1&b=2".into(),
            }],
        });
        assert_eq!(parts.len(), 1);
        assert!(parts[0].html.contains("<strong>Daily *brief*</strong>"));
        assert!(
            parts[0]
                .html
                .contains("<a href=\"https://calendar.example/new?a=1&amp;b=2\">Open it</a>"),
            "{}",
            parts[0].html
        );
    }

    #[test]
    fn long_replies_split_without_breaking_code_blocks() {
        let code = "let x = 1;\n".repeat(1500);
        let md = format!("Here:\n```\n{code}```\nDone.");
        let parts = split(&md);
        assert!(parts.len() > 1);
        for part in &parts {
            assert!(part.chars().count() <= PIECE + 8);
            let fences = part.lines().filter(|l| l.starts_with("```")).count();
            assert_eq!(fences % 2, 0, "{part}");
        }
        assert!(parts.last().unwrap().ends_with("Done."));
    }

    #[test]
    fn reply_fallbacks_are_removed() {
        assert_eq!(
            strip_reply_fallback("> <@mimi:example.org> Waiting for you\n> Send it?\n\nyes"),
            "yes"
        );
        assert_eq!(strip_reply_fallback("yes"), "yes");
        assert_eq!(
            strip_reply_fallback("> a quote of my own\nand more"),
            "> a quote of my own\nand more"
        );
    }
}
