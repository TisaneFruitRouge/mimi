//! A forgiving vCard (2.1 / 3.0 / 4.0) reader: names, emails, phones and messaging
//! handles. Address books are full of odd cards; anything unreadable is skipped.

use hearth_protocol::Channel;

use super::{CardHandle, ContactCard};

/// Every contact card in `text` (which may hold several). `fallback_ref` names cards
/// without a UID, e.g. the resource's href.
pub fn parse(text: &str, fallback_ref: &str) -> Vec<ContactCard> {
    let mut cards = Vec::new();
    let mut current: Option<Vec<Property>> = None;
    for line in unfold(text) {
        let Some(prop) = Property::parse(&line) else {
            continue;
        };
        match (prop.name.as_str(), prop.value.to_ascii_uppercase().as_str()) {
            ("BEGIN", "VCARD") => current = Some(Vec::new()),
            ("END", "VCARD") => {
                if let Some(props) = current.take() {
                    let index = cards.len();
                    if let Some(card) = to_card(&props, fallback_ref, index) {
                        cards.push(card);
                    }
                }
            }
            _ => {
                if let Some(props) = current.as_mut() {
                    props.push(prop);
                }
            }
        }
    }
    cards
}

struct Property {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Property {
    /// `item1.TEL;TYPE=CELL,VOICE:+41…` → name `TEL`, params, value.
    fn parse(line: &str) -> Option<Self> {
        let (head, value) = split_unquoted(line, ':')?;
        let mut parts = split_params(head).into_iter();
        let name = parts.next()?;
        // Apple groups properties ("item1.TEL"); the group doesn't matter here.
        let name = name
            .rsplit('.')
            .next()
            .unwrap_or(&name)
            .to_ascii_uppercase();
        let params = parts
            .map(|p| match p.split_once('=') {
                Some((k, v)) => (k.to_ascii_uppercase(), v.trim_matches('"').to_owned()),
                // vCard 2.1: bare parameters are types ("TEL;CELL:…").
                None => ("TYPE".to_owned(), p),
            })
            .collect();
        Some(Self {
            name,
            params,
            value: value.to_owned(),
        })
    }

    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn types(&self) -> Vec<String> {
        self.params
            .iter()
            .filter(|(k, _)| k == "TYPE")
            .flat_map(|(_, v)| v.split(','))
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    }

    fn text(&self) -> String {
        unescape(&self.value)
    }
}

/// Joins folded lines (continuations start with a space or tab).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t'))
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !line.is_empty() {
            out.push(line.to_owned());
        }
    }
    out
}

fn split_unquoted(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            c if c == sep && !quoted => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

fn split_params(head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in head.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => out.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    out.push(current);
    out
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

/// Splits a structured value (`N:Carter;Sam;;;`) on unescaped semicolons.
fn components(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in v.chars() {
        if escaped {
            current.push('\\');
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ';' {
            out.push(unescape(&std::mem::take(&mut current)));
        } else {
            current.push(c);
        }
    }
    out.push(unescape(&current));
    out
}

fn to_card(props: &[Property], fallback_ref: &str, index: usize) -> Option<ContactCard> {
    let get = |name: &str| props.iter().find(|p| p.name == name);
    // Groups and lists aren't people.
    let kind = get("KIND")
        .or_else(|| get("X-ADDRESSBOOKSERVER-KIND"))
        .map(|p| p.text().to_ascii_lowercase());
    if matches!(kind.as_deref(), Some("group" | "org" | "location")) {
        return None;
    }

    let formatted = get("FN").map(Property::text).filter(|s| !s.is_empty());
    let structured = get("N").map(|p| {
        let c = components(&p.value);
        let given = c.get(1).cloned().unwrap_or_default();
        let family = c.first().cloned().unwrap_or_default();
        [given, family]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    });
    let org = get("ORG").map(|p| components(&p.value).into_iter().next().unwrap_or_default());
    let nickname = get("NICKNAME")
        .map(|p| p.text().split(',').next().unwrap_or("").trim().to_owned())
        .filter(|s| !s.is_empty());

    let mut handles = Vec::new();
    for p in props {
        let label = label_of(p);
        match p.name.as_str() {
            "EMAIL" => push(&mut handles, Channel::Email, &p.text(), label),
            "TEL" => {
                let types = p.types();
                if types.iter().any(|t| t == "fax" || t == "pager") {
                    continue;
                }
                push(&mut handles, Channel::Phone, &p.text(), label);
            }
            "IMPP" | "X-SOCIALPROFILE" | "URL" => {
                if let Some((channel, value)) = messaging_handle(p) {
                    push(&mut handles, channel, &value, None);
                }
            }
            "X-TELEGRAM" => push(&mut handles, Channel::Telegram, &p.text(), None),
            "X-SIGNAL" => push(&mut handles, Channel::Signal, &p.text(), None),
            _ => {}
        }
    }

    let name = formatted
        .or(structured.filter(|s| !s.is_empty()))
        .or(org.filter(|s| !s.is_empty()))
        .or_else(|| nickname.clone())
        .or_else(|| handles.first().map(|h: &CardHandle| h.value.clone()))?;

    let record = get("UID")
        .map(Property::text)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if index == 0 {
                fallback_ref.to_owned()
            } else {
                format!("{fallback_ref}#{index}")
            }
        });
    Some(ContactCard {
        record,
        name,
        nickname,
        handles,
    })
}

fn push(handles: &mut Vec<CardHandle>, channel: Channel, value: &str, label: Option<String>) {
    let value = super::normalize::clean(channel, value);
    if value.is_empty()
        || handles
            .iter()
            .any(|h| h.channel == channel && h.value == value)
    {
        return;
    }
    handles.push(CardHandle {
        channel,
        value,
        label,
    });
}

/// "mobile", "work", "home" from TYPE parameters; not the technical ones.
fn label_of(p: &Property) -> Option<String> {
    p.types().into_iter().find_map(|t| match t.as_str() {
        "cell" | "mobile" | "iphone" => Some("mobile".to_owned()),
        "work" => Some("work".to_owned()),
        "home" => Some("home".to_owned()),
        "main" => Some("main".to_owned()),
        _ => None,
    })
}

/// Telegram, Signal, WhatsApp and Matrix from IMPP, social profiles or links.
fn messaging_handle(p: &Property) -> Option<(Channel, String)> {
    let service = p
        .param("X-SERVICE-TYPE")
        .map(str::to_ascii_lowercase)
        // X-SOCIALPROFILE;TYPE=telegram:…
        .or_else(|| {
            (p.name == "X-SOCIALPROFILE")
                .then(|| p.types().into_iter().next())
                .flatten()
        })
        .unwrap_or_default();
    let value = p.text();
    let lower = value.to_ascii_lowercase();
    // Apple writes IMPP values as "x-apple:username".
    let bare = lower
        .split_once(':')
        .filter(|(scheme, _)| !scheme.starts_with("http"))
        .map(|(_, rest)| rest.to_owned())
        .unwrap_or_else(|| lower.clone());

    if service.contains("telegram")
        || lower.starts_with("telegram:")
        || lower.starts_with("tg:")
        || lower.contains("t.me/")
    {
        let raw = if lower.contains("t.me/") {
            value.clone()
        } else {
            bare
        };
        return super::normalize::telegram_username(&raw).map(|u| (Channel::Telegram, u));
    }
    if service.contains("signal") || lower.starts_with("sgnl:") || lower.starts_with("signal:") {
        return Some((Channel::Signal, bare));
    }
    if service.contains("whatsapp") || lower.starts_with("whatsapp:") || lower.contains("wa.me/") {
        let number = lower.rsplit('/').next().unwrap_or(&bare).to_owned();
        return Some((
            Channel::Whatsapp,
            if lower.contains("wa.me/") {
                number
            } else {
                bare
            },
        ));
    }
    if service.contains("matrix") || lower.starts_with("matrix:") || lower.contains("matrix.to/#/")
    {
        let id = value.rsplit("#/").next().unwrap_or(&bare).to_owned();
        return Some((
            Channel::Matrix,
            if lower.contains("matrix.to") {
                id
            } else {
                bare
            },
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARDS: &str = "BEGIN:VCARD\r
VERSION:3.0\r
UID:sam-1\r
FN:Sam Carter\r
N:Carter;Sam;;;\r
NICKNAME:Sammy\r
item1.TEL;type=CELL;type=VOICE;type=pref:+41 79 123 45 67\r
TEL;TYPE=FAX:+41 44 000 00 00\r
EMAIL;TYPE=INTERNET;TYPE=WORK:sam@work.example\r
IMPP;X-SERVICE-TYPE=Telegram:x-apple:samcarter\r
X-SOCIALPROFILE;TYPE=signal:+41791234567\r
NOTE:Line one\\nline two with a very long text that gets folded by the \r
 server across lines\r
END:VCARD\r
BEGIN:VCARD\r
VERSION:4.0\r
FN:Zoë Müller\r
TEL;VALUE=uri;TYPE=home:tel:0041-22-555-12-12\r
URL:https://t.me/zoem\r
END:VCARD\r
BEGIN:VCARD\r
VERSION:3.0\r
FN:Book club\r
X-ADDRESSBOOKSERVER-KIND:group\r
END:VCARD\r
BEGIN:VCARD\r
VERSION:2.1\r
N:;Grandma;;;\r
TEL;HOME:022 555 00 00\r
END:VCARD\r
";

    #[test]
    fn reads_names_and_handles() {
        let cards = parse(CARDS, "/addressbook/all.vcf");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Sam Carter", "Zoë Müller", "Grandma"]);

        let sam = &cards[0];
        assert_eq!(sam.record, "sam-1");
        assert_eq!(sam.nickname.as_deref(), Some("Sammy"));
        let handles: Vec<(Channel, &str, Option<&str>)> = sam
            .handles
            .iter()
            .map(|h| (h.channel, h.value.as_str(), h.label.as_deref()))
            .collect();
        assert_eq!(
            handles,
            [
                (Channel::Phone, "+41 79 123 45 67", Some("mobile")),
                (Channel::Email, "sam@work.example", Some("work")),
                (Channel::Telegram, "@samcarter", None),
                (Channel::Signal, "+41791234567", None),
            ]
        );

        let zoe = &cards[1];
        assert_eq!(zoe.handles[0].value, "0041-22-555-12-12");
        assert_eq!(zoe.handles[0].label.as_deref(), Some("home"));
        assert_eq!(
            (zoe.handles[1].channel, zoe.handles[1].value.as_str()),
            (Channel::Telegram, "@zoem")
        );
        // Cards without a UID get a stable reference from their location.
        assert_eq!(zoe.record, "/addressbook/all.vcf#1");
        assert_eq!(cards[2].handles[0].label.as_deref(), Some("home"));
    }
}
