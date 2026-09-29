//! Markdown as Signal text. Signal has no markup: styles are ranges over the plain text
//! (bold, italic, monospace, strikethrough), counted in UTF-16 code units. The Markdown
//! syntax itself is removed.

use presage::libsignal_service::proto::BodyRange;
use presage::libsignal_service::proto::body_range::{AssociatedValue, Style as ProtoStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Monospace,
    Strikethrough,
}

/// A styled range, in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub style: Style,
}

/// Plain text with its styles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Styled {
    pub text: String,
    pub spans: Vec<Span>,
    /// Length of `text` in characters.
    chars: usize,
}

impl Styled {
    pub fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.chars += text.chars().count();
    }

    fn push_char(&mut self, c: char) {
        self.text.push(c);
        self.chars += 1;
    }

    /// Writes what `f` writes, in `style`.
    pub fn styled(&mut self, style: Style, f: impl FnOnce(&mut Self)) {
        let start = self.chars;
        f(self);
        let len = self.chars - start;
        if len > 0 {
            self.spans.push(Span { start, len, style });
        }
    }

    pub fn append(&mut self, other: &Styled) {
        let offset = self.chars;
        self.push(&other.text);
        self.spans.extend(other.spans.iter().map(|s| Span {
            start: s.start + offset,
            ..*s
        }));
    }

    /// Removes trailing whitespace, and styles over nothing.
    fn trim_end(&mut self) {
        let kept = self.text.trim_end().len();
        self.text.truncate(kept);
        self.chars = self.text.chars().count();
        let end = self.chars;
        self.spans.retain_mut(|s| {
            s.len = s.len.min(end.saturating_sub(s.start));
            s.len > 0
        });
    }

    /// The styles as Signal's body ranges, in UTF-16 code units.
    pub fn body_ranges(&self) -> Vec<BodyRange> {
        let mut utf16 = Vec::with_capacity(self.chars + 1);
        let mut at = 0u32;
        for c in self.text.chars() {
            utf16.push(at);
            at += c.len_utf16() as u32;
        }
        utf16.push(at);
        self.spans
            .iter()
            .map(|s| {
                let start = utf16[s.start];
                let end = utf16[(s.start + s.len).min(self.chars)];
                let style = match s.style {
                    Style::Bold => ProtoStyle::Bold,
                    Style::Italic => ProtoStyle::Italic,
                    Style::Monospace => ProtoStyle::Monospace,
                    Style::Strikethrough => ProtoStyle::Strikethrough,
                };
                BodyRange {
                    start: Some(start),
                    length: Some(end - start),
                    associated_value: Some(AssociatedValue::Style(style as i32)),
                }
            })
            .collect()
    }

    /// Cuts the text into pieces of at most `max` characters, at line breaks where it
    /// can, keeping each piece's styles.
    pub fn split(&self, max: usize) -> Vec<Styled> {
        let max = max.max(1);
        let mut pieces = Vec::new();
        let mut cuts = Vec::new();
        let (mut start, mut line_start, mut n) = (0usize, 0usize, 0usize);
        let chars: Vec<char> = self.text.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            n = i + 1;
            if n - start > max {
                // Cut at the last line break in this piece, else right here.
                let cut = if line_start > start { line_start } else { i };
                cuts.push((start, cut));
                start = cut;
            }
            if *c == '\n' {
                line_start = i + 1;
            }
        }
        if n > start || cuts.is_empty() {
            cuts.push((start, n));
        }
        for (from, to) in cuts {
            let text: String = chars[from..to].iter().collect();
            let spans = self
                .spans
                .iter()
                .filter_map(|s| {
                    let start = s.start.max(from);
                    let end = (s.start + s.len).min(to);
                    (end > start).then(|| Span {
                        start: start - from,
                        len: end - start,
                        style: s.style,
                    })
                })
                .collect();
            let mut piece = Styled {
                chars: to - from,
                text,
                spans,
            };
            piece.trim_end();
            // Drop the line break a cut left at the start.
            if piece.text.starts_with('\n') {
                piece.text.remove(0);
                piece.chars -= 1;
                piece.spans.retain_mut(|s| {
                    if s.start == 0 {
                        s.len -= 1;
                    } else {
                        s.start -= 1;
                    }
                    s.len > 0
                });
            }
            if !piece.text.is_empty() {
                pieces.push(piece);
            }
        }
        pieces
    }
}

/// Converts the Markdown models write into Signal text.
pub fn markdown(md: &str) -> Styled {
    let mut out = Styled::default();
    let mut in_code = false;
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.styled(Style::Monospace, |o| o.push(line));
            out.push("\n");
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        let heading = trimmed.trim_start_matches('#');
        if heading.len() < trimmed.len() && heading.starts_with(' ') {
            out.styled(Style::Bold, |o| inline(o, heading.trim()));
        } else if let Some(item) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        {
            out.push(indent);
            out.push("• ");
            inline(&mut out, item);
        } else if let Some(quote) = trimmed.strip_prefix('>') {
            out.push("│ ");
            inline(&mut out, quote.trim_start());
        } else if !trimmed.is_empty()
            && trimmed.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
            && trimmed.chars().filter(|c| *c != ' ').count() >= 3
        {
            out.push("———");
        } else {
            inline(&mut out, line);
        }
        out.push("\n");
    }
    out.trim_end();
    out
}

fn starts(chars: &[char], at: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(k, p)| chars.get(at + k) == Some(&p))
}

/// Where `pat` next starts, from `from` on.
fn find(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    (from..chars.len()).find(|&i| starts(chars, i, pat))
}

fn text_of(chars: &[char]) -> String {
    chars.iter().collect()
}

/// Inline Markdown: `code`, **bold**, *italic*, ~~strikethrough~~, [links](url).
fn inline(out: &mut Styled, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\\' && next.is_some_and(|n| n.is_ascii_punctuation()) {
            out.push_char(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`'
            && let Some(end) = find(&chars, i + 1, "`")
        {
            out.styled(Style::Monospace, |o| o.push(&text_of(&chars[i + 1..end])));
            i = end + 1;
            continue;
        }
        let pair = if starts(&chars, i, "**") {
            Some(("**", Style::Bold))
        } else if starts(&chars, i, "__") {
            Some(("__", Style::Bold))
        } else if starts(&chars, i, "~~") {
            Some(("~~", Style::Strikethrough))
        } else {
            None
        };
        if let Some((pat, style)) = pair
            && let Some(end) = find(&chars, i + 2, pat)
            && end > i + 2
        {
            out.styled(style, |o| inline(o, &text_of(&chars[i + 2..end])));
            i = end + 2;
            continue;
        }
        if c == '['
            && let Some(close) = find(&chars, i + 1, "](")
            && let Some(end) = find(&chars, close + 2, ")")
        {
            let label = text_of(&chars[i + 1..close]);
            let url = text_of(&chars[close + 2..end]);
            inline(out, &label);
            if (url.starts_with("https://") || url.starts_with("http://")) && url != label {
                out.push(&format!(" ({url})"));
            }
            i = end + 1;
            continue;
        }
        // *italic*, and _italic_ only at word boundaries (not snake_case).
        let opens = match c {
            '*' => true,
            '_' => i == 0 || !chars[i - 1].is_alphanumeric(),
            _ => false,
        };
        if opens
            && next.is_some_and(|n| !n.is_whitespace() && n != c)
            && let Some(end) = (i + 1..chars.len()).find(|&j| {
                chars[j] == c
                    && !chars[j - 1].is_whitespace()
                    && (c == '*' || chars.get(j + 1).is_none_or(|n| !n.is_alphanumeric()))
            })
        {
            out.styled(Style::Italic, |o| inline(o, &text_of(&chars[i + 1..end])));
            i = end + 1;
            continue;
        }
        out.push_char(c);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(s: &Styled) -> Vec<(String, Style)> {
        let chars: Vec<char> = s.text.chars().collect();
        s.spans
            .iter()
            .map(|sp| (text_of(&chars[sp.start..sp.start + sp.len]), sp.style))
            .collect()
    }

    #[test]
    fn markdown_becomes_styled_text() {
        let s = markdown(
            "# Plan\n\n**Monday**: call Sam & book `table`\n- one *thing*\n- [docs](https://example.com)\n\
             some_snake_case and _this_ ~~not~~\n```\nlet x = 1;\n```",
        );
        assert_eq!(
            s.text,
            "Plan\n\nMonday: call Sam & book table\n• one thing\n• docs (https://example.com)\n\
             some_snake_case and this not\nlet x = 1;"
        );
        assert_eq!(
            spans(&s),
            vec![
                ("Plan".into(), Style::Bold),
                ("Monday".into(), Style::Bold),
                ("table".into(), Style::Monospace),
                ("thing".into(), Style::Italic),
                ("this".into(), Style::Italic),
                ("not".into(), Style::Strikethrough),
                ("let x = 1;".into(), Style::Monospace),
            ]
        );
    }

    #[test]
    fn nested_and_unclosed_markup() {
        let s = markdown("**bold with *italic* inside** and a lone * star and 2*3");
        assert_eq!(s.text, "bold with italic inside and a lone * star and 2*3");
        assert_eq!(
            spans(&s),
            vec![
                ("italic".into(), Style::Italic),
                ("bold with italic inside".into(), Style::Bold),
            ]
        );
        assert_eq!(markdown(r"not \*emphasis\*").text, "not *emphasis*");
    }

    #[test]
    fn ranges_count_utf16() {
        let s = markdown("😀 **hé**");
        let ranges = s.body_ranges();
        // The emoji takes two UTF-16 units, then a space.
        assert_eq!(ranges[0].start, Some(3));
        assert_eq!(ranges[0].length, Some(2));
        assert_eq!(
            ranges[0].associated_value,
            Some(AssociatedValue::Style(ProtoStyle::Bold as i32))
        );
    }

    #[test]
    fn long_text_splits_at_lines_and_keeps_styles() {
        let md = format!(
            "**{}**\n{}\n{}",
            "a".repeat(30),
            "b".repeat(30),
            "c".repeat(100)
        );
        let s = markdown(&md);
        let pieces = s.split(64);
        assert!(pieces.iter().all(|p| p.text.chars().count() <= 64));
        assert_eq!(
            pieces[0].text,
            format!("{}\n{}", "a".repeat(30), "b".repeat(30))
        );
        assert_eq!(spans(&pieces[0]), vec![("a".repeat(30), Style::Bold)]);
        assert_eq!(
            pieces[1..]
                .iter()
                .map(|p| p.text.as_str())
                .collect::<String>(),
            "c".repeat(100)
        );
        // A style cut in two stays on both sides.
        let s = markdown(&format!("**{}**", "d".repeat(10)));
        let pieces = s.split(6);
        assert_eq!(pieces.len(), 2);
        assert_eq!(spans(&pieces[1]), vec![("dddd".into(), Style::Bold)]);
    }
}
