//! Email signatures (Settings › General › Signature): the lines at the end of each email
//! the user writes, after a `-- ` line, per sending address or the same for all.
//!
//! They live in the `settings` table, row `mail_signatures` (the encrypted database),
//! loaded only when needed: they can carry a small picture, so they stay out of
//! `Settings`, which every client reads.
//!
//! The editor puts the signature into the drafts the user writes (`features/mail/
//! signature.ts`); emails the assistant sends with `mail_send` get it here, in
//! `tools.rs`: the approval card shows it, and the model never writes it.

use std::collections::HashSet;

use base64::Engine;
use mimi_protocol::{AddressSignature, MailDraft, MailSignature, MailSignatures};
use rusqlite::OptionalExtension;

use super::smtp;
use crate::AppState;
use crate::db::Db;

/// Row in the `settings` table.
const ROW: &str = "mail_signatures";
/// The line before a signature, as mail apps write it (RFC 3676).
pub const SEPARATOR: &str = "-- ";
/// The same line in a formatted email.
pub const SEPARATOR_HTML: &str = "<div>-- </div>";
const BLANK_HTML: &str = "<div><br></div>";
/// Longest signature text, in characters.
pub const MAX_TEXT: usize = 2_000;
/// Longest signature HTML, in bytes.
const MAX_HTML: usize = 20_000;
/// Most pictures in one signature, and their size together (as files).
pub const MAX_PICTURES: usize = 3;
pub const MAX_PICTURE_BYTES: usize = 200 * 1024;
/// Most addresses with their own signature.
const MAX_ADDRESSES: usize = 50;

/// The saved signatures, or none.
pub async fn load(db: &Db) -> MailSignatures {
    db.call(|c| {
        c.query_row("SELECT value FROM settings WHERE key = ?1", [ROW], |r| {
            r.get::<_, String>(0)
        })
        .optional()
    })
    .await
    .ok()
    .flatten()
    .and_then(|raw| {
        serde_json::from_str(&raw)
            .inspect_err(|e| tracing::error!("saved signatures are unreadable: {e}"))
            .ok()
    })
    .unwrap_or_default()
}

/// Checks and cleans the signatures, saves them and returns them as saved.
pub async fn save(state: &AppState, input: MailSignatures) -> Result<MailSignatures, String> {
    let clean = check(input)?;
    let raw = serde_json::to_string(&clean).map_err(|e| e.to_string())?;
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                (ROW, raw),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(clean)
}

/// The signatures as they're kept: each one cleaned, addresses lowercase and once each.
pub fn check(input: MailSignatures) -> Result<MailSignatures, String> {
    let mut seen = HashSet::new();
    let mut addresses = Vec::new();
    for a in input.addresses {
        let address = a.address.trim().to_lowercase();
        if !address.contains('@') || !seen.insert(address.clone()) {
            continue;
        }
        addresses.push(AddressSignature {
            address,
            signature: clean(a.signature)?,
        });
    }
    if addresses.len() > MAX_ADDRESSES {
        return Err(format!(
            "At most {MAX_ADDRESSES} addresses can have their own signature."
        ));
    }
    Ok(MailSignatures {
        same_for_all: input.same_for_all,
        all: clean(input.all)?,
        addresses,
        in_replies: input.in_replies,
    })
}

/// One signature made safe to keep and send: its HTML through the same cleaning as an
/// email's (`smtp::clean_html`), only pictures it shows, each a small picture.
fn clean(sig: MailSignature) -> Result<MailSignature, String> {
    let text = sig.text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim_end().trim_start_matches('\n').to_owned();
    if text.chars().count() > MAX_TEXT {
        return Err("A signature can be at most 2,000 characters.".to_owned());
    }
    let mut pictures = Vec::new();
    for p in sig.pictures {
        let Some(id) = p.content_id.clone().filter(|id| smtp::is_content_id(id)) else {
            continue;
        };
        let mime = p.mime.clone().unwrap_or_default().to_ascii_lowercase();
        if !smtp::INLINE_TYPES.contains(&mime.as_str()) {
            return Err("A signature's pictures can be PNG, JPEG, GIF or WebP.".to_owned());
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(p.data.trim())
            .map_err(|_| "A picture in the signature couldn't be read.".to_owned())?;
        if data.is_empty() || p.source.is_some() {
            return Err("A picture in the signature couldn't be read.".to_owned());
        }
        let name: String = p
            .name
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect();
        pictures.push(mimi_protocol::NewMailAttachment {
            name: if name.trim().is_empty() {
                "Picture".to_owned()
            } else {
                name
            },
            mime: Some(mime),
            data: p.data.trim().to_owned(),
            source: None,
            content_id: Some(id),
        });
    }
    let html = sig
        .html
        .filter(|h| !h.trim().is_empty())
        .map(|h| {
            let ids = pictures
                .iter()
                .filter_map(|p| p.content_id.clone())
                .collect();
            smtp::clean_html(&h, &ids)
        })
        .filter(|h| !h.trim().is_empty());
    if html.as_ref().is_some_and(|h| h.len() > MAX_HTML) {
        return Err("That signature is too long.".to_owned());
    }
    // Only the pictures it shows.
    pictures.retain(|p| {
        let id = p.content_id.as_deref().unwrap_or_default();
        html.as_ref()
            .is_some_and(|h| h.contains(&format!("src=\"cid:{id}\"")))
    });
    if pictures.len() > MAX_PICTURES {
        return Err(format!(
            "A signature can have at most {MAX_PICTURES} pictures."
        ));
    }
    let bytes: usize = pictures.iter().map(|p| size(&p.data)).sum();
    if bytes > MAX_PICTURE_BYTES {
        return Err(
            "The pictures in a signature can add up to 200 KB. Use a smaller picture, such as a logo."
                .to_owned(),
        );
    }
    if text.trim().is_empty() && pictures.is_empty() {
        return Ok(MailSignature::default());
    }
    Ok(MailSignature {
        text,
        html,
        pictures,
    })
}

/// A base64 file's size.
fn size(data: &str) -> usize {
    let data = data.trim();
    data.len() / 4 * 3 - data.bytes().rev().take_while(|b| *b == b'=').count()
}

/// Whether a signature has nothing in it.
pub fn is_empty(sig: &MailSignature) -> bool {
    sig.text.trim().is_empty() && sig.pictures.is_empty()
}

/// The signature an email from `from` gets: the address's own, else the one for all;
/// none for a reply or forward unless the user wants it there too, and none when empty.
pub fn pick<'a>(sigs: &'a MailSignatures, from: &str, reply: bool) -> Option<&'a MailSignature> {
    if reply && !sigs.in_replies {
        return None;
    }
    let from = from.trim().to_lowercase();
    let sig = match sigs.same_for_all {
        true => &sigs.all,
        false => sigs
            .addresses
            .iter()
            .find(|a| a.address == from)
            .map(|a| &a.signature)
            .unwrap_or(&sigs.all),
    };
    (!is_empty(sig)).then_some(sig)
}

/// The signature a draft gets, from the address it will go from (as `mail::prepare`
/// works it out).
pub async fn for_draft(state: &AppState, draft: &MailDraft) -> Option<MailSignature> {
    let sender = super::sender(state, draft).await.ok()?;
    let sigs = load(&state.db).await;
    let reply = draft.reply_to.is_some() || draft.forward_of.is_some();
    pick(&sigs, &sender.from, reply).cloned()
}

/// The text without a signature its writer (a model) already put at its end: a block
/// after its own `-- ` line, or the signature's own lines. Anything else is left as it
/// is: a short sign-off ("Best, Vincent") is not a signature.
pub fn strip_duplicate(body: &str, signature: &str) -> String {
    let body = body.trim_end();
    let lines: Vec<&str> = body.lines().collect();
    // Its own separator, with a short block under it.
    if let Some(i) = lines.iter().rposition(|l| l.trim_end() == "--")
        && lines.len() - i - 1 <= 6
    {
        return lines[..i].join("\n").trim_end().to_owned();
    }
    let norm = |l: &str| {
        l.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let sig: Vec<String> = signature
        .lines()
        .map(norm)
        .filter(|l| !l.is_empty() && !l.starts_with("[image:"))
        .collect();
    if sig.is_empty() {
        return body.to_owned();
    }
    let (mut j, mut k) = (lines.len(), sig.len());
    while k > 0 && j > 0 {
        let l = norm(lines[j - 1]);
        if l.is_empty() {
            j -= 1;
            continue;
        }
        if l != sig[k - 1] {
            break;
        }
        k -= 1;
        j -= 1;
    }
    match k {
        0 => lines[..j].join("\n").trim_end().to_owned(),
        _ => body.to_owned(),
    }
}

/// Adds a signature at the end of a draft: below its text after a `-- ` line, in the
/// plain text and in the HTML (which a signature with formatting brings), with its
/// pictures in the text. A signature the text already ends with isn't repeated.
pub fn sign(draft: &mut MailDraft, sig: &MailSignature) {
    if is_empty(sig) {
        return;
    }
    let text = strip_duplicate(&draft.body, &sig.text);
    let html = match (&draft.html, &sig.html) {
        (None, None) => None,
        (Some(h), _) if text == draft.body.trim_end() => Some(h.clone()),
        _ => Some(text_html(&text)),
    };
    draft.body = format!("{text}\n\n{SEPARATOR}\n{}", sig.text);
    draft.html = html.map(|before| {
        let signature = sig.html.clone().unwrap_or_else(|| text_html(&sig.text));
        format!("{before}{BLANK_HTML}{SEPARATOR_HTML}{signature}")
    });
    if draft.html.is_some() {
        for p in &sig.pictures {
            if !draft
                .attachments
                .iter()
                .any(|a| a.content_id.is_some() && a.content_id == p.content_id)
            {
                draft.attachments.push(p.clone());
            }
        }
    }
}

/// Plain text as the mail editor writes it in HTML: a `<div>` per line.
pub fn text_html(text: &str) -> String {
    text.lines()
        .map(|l| match l {
            "" => BLANK_HTML.to_owned(),
            l => format!(
                "<div>{}</div>",
                l.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mimi_protocol::NewMailAttachment;

    use super::*;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn plain(text: &str) -> MailSignature {
        MailSignature {
            text: text.to_owned(),
            ..Default::default()
        }
    }

    fn logo(id: &str) -> NewMailAttachment {
        NewMailAttachment {
            name: "Logo.png".into(),
            mime: Some("image/png".into()),
            data: PNG.into(),
            source: None,
            content_id: Some(id.into()),
        }
    }

    fn sigs() -> MailSignatures {
        MailSignatures {
            same_for_all: false,
            all: plain("Vincent"),
            addresses: vec![AddressSignature {
                address: "billing@acme.example".into(),
                signature: plain("Acme Billing\n+33 1 23 45 67 89"),
            }],
            in_replies: true,
        }
    }

    #[test]
    fn the_signature_follows_the_from_address() {
        let s = sigs();
        assert_eq!(
            pick(&s, "Billing@Acme.example", false).unwrap().text,
            "Acme Billing\n+33 1 23 45 67 89"
        );
        // Any other address signs with the one for all.
        assert_eq!(pick(&s, "me@acme.example", false).unwrap().text, "Vincent");
        // The same for all: the address's own is kept, not used.
        let same = MailSignatures {
            same_for_all: true,
            ..sigs()
        };
        assert_eq!(
            pick(&same, "billing@acme.example", false).unwrap().text,
            "Vincent"
        );
        // Left out of replies when asked.
        let no_replies = MailSignatures {
            in_replies: false,
            ..sigs()
        };
        assert!(pick(&no_replies, "me@acme.example", true).is_none());
        assert!(pick(&no_replies, "me@acme.example", false).is_some());
        // An empty one is none.
        assert!(pick(&MailSignatures::default(), "me@acme.example", false).is_none());
    }

    #[test]
    fn plain_text_gets_the_separator_line() {
        let mut d = MailDraft {
            body: "Hi Sam,\n\nSee you at noon.\n".into(),
            ..Default::default()
        };
        sign(&mut d, &plain("Vincent\nwendling.example"));
        assert_eq!(
            d.body,
            "Hi Sam,\n\nSee you at noon.\n\n-- \nVincent\nwendling.example"
        );
        assert_eq!(d.html, None);
    }

    #[test]
    fn a_formatted_signature_brings_its_html_and_picture() {
        let sig = clean(MailSignature {
            text: "Vincent\n[image: Logo.png]".into(),
            html: Some(
                "<div><strong>Vincent</strong></div><div><img src=\"cid:logo@sig\" alt=\"Logo.png\" width=\"120\"></div>"
                    .into(),
            ),
            pictures: vec![logo("logo@sig")],
        })
        .unwrap();
        let mut d = MailDraft {
            body: "Hello <you> & co".into(),
            ..Default::default()
        };
        sign(&mut d, &sig);
        assert_eq!(
            d.body,
            "Hello <you> & co\n\n-- \nVincent\n[image: Logo.png]"
        );
        let html = d.html.unwrap();
        assert!(
            html.starts_with("<div>Hello &lt;you&gt; &amp; co</div><div><br></div><div>-- </div><div><strong>Vincent</strong></div>"),
            "{html}"
        );
        assert!(html.contains("src=\"cid:logo@sig\""), "{html}");
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(d.attachments[0].content_id.as_deref(), Some("logo@sig"));
    }

    #[test]
    fn a_signature_already_written_is_not_repeated() {
        let sig = "Vincent Wendling\nAcme";
        // Its own lines at the end.
        assert_eq!(
            strip_duplicate("Sounds good.\n\nVincent Wendling\nacme  \n", sig),
            "Sounds good."
        );
        // A block after its own separator.
        assert_eq!(
            strip_duplicate("Sounds good.\n\n--\nV. Wendling\nCEO", sig),
            "Sounds good."
        );
        // A sign-off is not a signature.
        assert_eq!(
            strip_duplicate("Sounds good.\n\nBest,\nVincent", sig),
            "Sounds good.\n\nBest,\nVincent"
        );
        let mut d = MailDraft {
            body: "Sounds good.\n\nVincent Wendling\nAcme".into(),
            ..Default::default()
        };
        sign(&mut d, &plain(sig));
        assert_eq!(d.body.matches("Vincent Wendling").count(), 1, "{}", d.body);
    }

    #[test]
    fn signatures_are_cleaned_like_the_mail_they_go_in() {
        let sig = clean(MailSignature {
            text: "Vincent".into(),
            html: Some(
                "<div style=\"color:red\" onclick=\"x()\">Vincent<script>alert(1)</script></div>\
                 <img src=\"https://tracker.example/p.gif\"><img src=\"cid:logo@sig\" onerror=\"x()\">\
                 <a href=\"javascript:x()\">site</a><iframe src=\"https://x.example\"></iframe>"
                    .into(),
            ),
            pictures: vec![logo("logo@sig"), logo("unused@sig")],
        })
        .unwrap();
        let html = sig.html.unwrap();
        for bad in [
            "style",
            "onclick",
            "script",
            "tracker",
            "onerror",
            "javascript",
            "iframe",
        ] {
            assert!(!html.contains(bad), "{bad} in {html}");
        }
        assert!(html.contains("src=\"cid:logo@sig\""), "{html}");
        // Only the pictures it shows are kept.
        assert_eq!(sig.pictures.len(), 1);

        // Not a picture, or too large: refused.
        let mut pdf = logo("a@sig");
        pdf.mime = Some("application/pdf".into());
        let shows = |id: &str| Some(format!("<img src=\"cid:{id}\">"));
        assert!(
            clean(MailSignature {
                text: "x".into(),
                html: shows("a@sig"),
                pictures: vec![pdf],
            })
            .is_err()
        );
        let mut big = logo("big@sig");
        big.data = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 300 * 1024]);
        assert!(
            clean(MailSignature {
                text: "x".into(),
                html: shows("big@sig"),
                pictures: vec![big],
            })
            .unwrap_err()
            .contains("200 KB")
        );
        // Nothing in it: none.
        assert_eq!(clean(plain("  \n ")).unwrap(), MailSignature::default());
    }

    #[test]
    fn a_signed_formatted_email_goes_with_its_picture_inline() {
        let sig = clean(MailSignature {
            text: "Vincent\n[image: Logo.png]".into(),
            html: Some(
                "<div>Vincent</div><div><img src=\"cid:logo@sig\" alt=\"Logo.png\"></div>".into(),
            ),
            pictures: vec![logo("logo@sig")],
        })
        .unwrap();
        let mut d = MailDraft {
            to: vec!["sam@example.com".into()],
            subject: "Hi".into(),
            body: "Hello".into(),
            ..Default::default()
        };
        sign(&mut d, &sig);
        let inline: Vec<smtp::Inline> = d
            .attachments
            .iter()
            .map(|a| smtp::Inline {
                content_id: a.content_id.clone().unwrap(),
                file: super::super::parse::Attachment {
                    name: a.name.clone(),
                    content_type: a.mime.clone().unwrap(),
                    data: base64::engine::general_purpose::STANDARD
                        .decode(&a.data)
                        .unwrap(),
                },
            })
            .collect();
        let built = smtp::build("me@example.org", &d, None, &[], &inline).unwrap();
        let raw = String::from_utf8(built.formatted).unwrap();
        assert!(raw.contains("multipart/related"), "{raw}");
        assert!(raw.contains("Content-ID: <logo@sig>"), "{raw}");
        // The text part keeps the separator's space.
        assert!(raw.contains("Hello\r\n\r\n-- \r\nVincent\r\n"), "{raw}");
    }
}
