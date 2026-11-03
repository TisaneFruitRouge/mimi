//! The assistant's Markdown as one would say it: formatting, code and addresses left out,
//! and cut into parts so reading can start after the first sentence instead of after the
//! whole reply.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use whatlang::{Detector, Lang};

/// Longest part, in characters: a few sentences, a few seconds to make.
const PART_CHARS: usize = 240;
/// Most text read at once (a long reply is cut short; the rest is on screen).
pub const MAX_CHARS: usize = 6_000;

/// What's said for a Markdown text, as plain sentences.
pub fn plain(markdown: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;
    let mut in_image = false;
    let mut cells = 0;
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Text(text) if !in_code_block && !in_image => out.push_str(&text),
            // Short inline code is usually a name or a command: say it.
            Event::Code(code) if code.chars().count() <= 40 => out.push_str(&code),
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => end_sentence(&mut out),
            Event::Start(Tag::TableRow | Tag::TableHead) => cells = 0,
            Event::Start(Tag::TableCell) => {
                if cells > 0 {
                    out.push_str(", ");
                }
                cells += 1;
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableRow
                | TagEnd::TableHead
                | TagEnd::BlockQuote(_),
            ) => end_sentence(&mut out),
            _ => {}
        }
    }
    tidy(&out)
}

/// Ends a sentence that has no punctuation of its own (a heading, a list item).
fn end_sentence(out: &mut String) {
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return;
    }
    let ends = trimmed
        .chars()
        .last()
        .is_some_and(|c| ".!?…:;。！？".contains(c));
    out.truncate(trimmed.len());
    if !ends {
        out.push('.');
    }
    out.push('\n');
}

/// Addresses become "a link", emoji and symbols go, and spaces are evened out.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut words: Vec<String> = Vec::new();
        for w in line.split_whitespace() {
            let w: String =
                if w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www.") {
                    "a link".to_owned()
                } else {
                    w.chars().filter(|c| speakable_char(*c)).collect()
                };
            match words.last_mut() {
                // What's left of "🍝." is punctuation for the word before.
                Some(last) if !w.is_empty() && !w.chars().any(char::is_alphanumeric) => {
                    last.push_str(&w)
                }
                _ if !w.is_empty() => words.push(w),
                _ => {}
            }
        }
        if words.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&words.join(" "));
    }
    out
}

fn speakable_char(c: char) -> bool {
    let n = c as u32;
    // Emoji, pictographs, dingbats, arrows, box drawing, variation selectors.
    !(matches!(n, 0x1F000..=0x1FAFF | 0x2190..=0x21FF | 0x2500..=0x27BF | 0xFE00..=0xFE0F | 0x200D)
        || matches!(c, '*' | '_' | '#' | '`' | '|' | '~' | '>' | '<'))
}

/// The text cut into parts at sentence ends. The first part is short so speech starts
/// quickly; the rest are up to [`PART_CHARS`].
pub fn parts(plain: &str) -> Vec<String> {
    let plain: String = plain.chars().take(MAX_CHARS).collect();
    let sentences = sentences(&plain);
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for sentence in sentences {
        let limit = if parts.is_empty() { 80 } else { PART_CHARS };
        if !current.is_empty() && current.chars().count() + sentence.chars().count() > limit {
            parts.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&sentence);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    // A sentence longer than a part is cut at a comma or a space.
    parts.into_iter().flat_map(|p| split_long(&p)).collect()
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            push_trimmed(&mut out, &mut current);
            continue;
        }
        current.push(c);
        let ends = ".!?…。！？".contains(c);
        if ends && chars.peek().is_none_or(|n| n.is_whitespace()) {
            push_trimmed(&mut out, &mut current);
        }
    }
    push_trimmed(&mut out, &mut current);
    out
}

fn push_trimmed(out: &mut Vec<String>, current: &mut String) {
    let s = current.trim();
    if !s.is_empty() {
        out.push(s.to_owned());
    }
    current.clear();
}

fn split_long(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = part.trim();
    while rest.chars().count() > PART_CHARS {
        let limit = rest
            .char_indices()
            .nth(PART_CHARS)
            .map_or(rest.len(), |(i, _)| i);
        let head = &rest[..limit];
        let cut = head
            .rfind([',', ';', ':'])
            .map(|i| i + 1)
            .or_else(|| head.rfind(' '))
            .filter(|&i| i > 0)
            .unwrap_or(limit);
        out.push(rest[..cut].trim().to_owned());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_owned());
    }
    out
}

/// The language a text is in, as an ISO 639-1 code, among those Mimi has voices for.
/// `None` when it can't tell.
pub fn language(text: &str) -> Option<&'static str> {
    const LANGUAGES: &[(Lang, &str)] = &[
        (Lang::Eng, "en"),
        (Lang::Fra, "fr"),
        (Lang::Deu, "de"),
        (Lang::Spa, "es"),
        (Lang::Ita, "it"),
        (Lang::Por, "pt"),
        (Lang::Nld, "nl"),
        (Lang::Pol, "pl"),
        (Lang::Rus, "ru"),
        (Lang::Ukr, "uk"),
        (Lang::Swe, "sv"),
        (Lang::Dan, "da"),
        (Lang::Nob, "nb"),
        (Lang::Fin, "fi"),
        (Lang::Ces, "cs"),
        (Lang::Slk, "sk"),
        (Lang::Slv, "sl"),
        (Lang::Hun, "hu"),
        (Lang::Ron, "ro"),
        (Lang::Lav, "lv"),
        (Lang::Ell, "el"),
        (Lang::Vie, "vi"),
        (Lang::Cmn, "zh"),
    ];
    let detector = Detector::with_allowlist(LANGUAGES.iter().map(|(l, _)| *l).collect());
    let info = detector.detect(text)?;
    // Short texts are often unsure; a clear winner is still better than a guess.
    if !info.is_reliable() && info.confidence() < 0.3 {
        return None;
    }
    LANGUAGES
        .iter()
        .find(|(l, _)| *l == info.lang())
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_read_as_sentences() {
        let md = "## Tomorrow\n\nYou have **two** things:\n\n- Dentist at 9:00\n- Lunch with [Sam](https://example.org/sam) 🍝\n\n```\nlet x = 1;\n```\n\nSee https://example.org for more. Run `/newbot` first.";
        assert_eq!(
            plain(md),
            "Tomorrow.\nYou have two things:\nDentist at 9:00.\nLunch with Sam.\nSee a link for more. Run /newbot first."
        );
    }

    #[test]
    fn tables_are_read_row_by_row() {
        let md = "| Day | Time |\n|---|---|\n| Monday | 9:00 |\n| Tuesday | 10:00 |";
        assert_eq!(plain(md), "Day, Time.\nMonday, 9:00.\nTuesday, 10:00.");
    }

    #[test]
    fn the_first_part_is_short_and_the_rest_are_bounded() {
        let text = "Sure! ".to_owned()
            + &*"This sentence is here to make the reply long enough to need parts. ".repeat(12);
        let parts = parts(&text);
        assert!(parts.len() > 3, "{parts:?}");
        assert!(parts[0].chars().count() <= 80, "{}", parts[0]);
        assert!(parts.iter().all(|p| p.chars().count() <= PART_CHARS));
        // Nothing is lost or reordered.
        let joined = parts.join(" ");
        assert_eq!(
            joined.split_whitespace().count(),
            text.split_whitespace().count()
        );
    }

    #[test]
    fn a_sentence_without_ends_is_still_cut() {
        let text = "word, ".repeat(100);
        let parts = parts(&text);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= PART_CHARS));
    }

    #[test]
    fn nothing_to_say_is_no_parts() {
        assert!(parts(&plain("```\ncode only\n```")).is_empty());
        assert!(parts("").is_empty());
    }

    #[test]
    fn languages_are_recognised() {
        assert_eq!(
            language("Bonjour ! Ton rendez-vous avec Sam est demain à quinze heures trente."),
            Some("fr")
        );
        assert_eq!(
            language("Your meeting with Sam is tomorrow at half past three in the afternoon."),
            Some("en")
        );
        assert_eq!(
            language("Morgen um drei Uhr hast du einen Termin beim Zahnarzt in der Stadt."),
            Some("de")
        );
    }
}
