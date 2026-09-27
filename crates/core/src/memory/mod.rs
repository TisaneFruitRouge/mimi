//! Long-term memory: what the assistant knows about the user.
//!
//! Two tiers keep the prompt small however much is remembered:
//! - the **profile** (`profile.md`): a short summary that goes with every message,
//!   hard-capped at [`PROFILE_LIMIT`] characters;
//! - the **library**: short markdown notes addressed by path (`people/sam.md`,
//!   `habits/mornings.md`…), found on demand through a full-text index and, when the
//!   user turns it on, by meaning ([`semantic`]). A few notes relevant to each message
//!   are added to the prompt ([`recall`]), including notes about the people it mentions
//!   ([`link`]), and the model can search and read more with tools ([`tools`]).
//!
//! Memories are written by the assistant during a conversation (visible in the chat and
//! undoable), learned in the background from finished conversations ([`learn`]), or
//! edited by the user in the Memory screen. Everything lives in the encrypted database.

pub mod learn;
pub mod link;
pub mod recall;
pub mod semantic;
pub mod store;
pub mod tools;

/// Where the always-loaded summary lives.
pub const PROFILE_PATH: &str = "profile.md";

/// Most characters in the profile. Local models have small contexts; the profile rides
/// along with every message.
pub const PROFILE_LIMIT: usize = 1200;

/// Most characters in one library note. Long notes should be split by topic.
pub const NOTE_LIMIT: usize = 4000;

/// The library's folders, with the names the user sees. Paths outside them are filed
/// under `notes/` so the library stays tidy.
pub const FOLDERS: &[(&str, &str)] = &[
    ("people", "People"),
    ("habits", "Habits and routines"),
    ("preferences", "Likes and preferences"),
    ("places", "Places"),
    ("work", "Work"),
    ("interests", "Interests"),
    ("health", "Health"),
    ("notes", "Other"),
];

/// Normalizes a path the model or user gave: lowercase `folder/name.md`, with the name
/// made of letters, digits and dashes. Returns `None` if nothing usable is left.
pub fn normalize_path(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches('/').to_lowercase();
    if raw == PROFILE_PATH || raw == "profile" {
        return Some(PROFILE_PATH.to_owned());
    }
    let raw = raw.strip_suffix(".md").unwrap_or(&raw);
    let mut parts: Vec<String> = raw.split('/').map(slug).filter(|s| !s.is_empty()).collect();
    let name = parts.pop()?;
    let folder = parts.first().cloned().unwrap_or_default();
    let folder = match folder.as_str() {
        "person" | "family" | "friends" | "contacts" => "people",
        "habit" | "routines" | "routine" => "habits",
        "likes" | "preference" | "food" => "preferences",
        "place" | "home" => "places",
        "job" | "career" => "work",
        "hobbies" | "hobby" | "interest" => "interests",
        f if FOLDERS.iter().any(|(k, _)| *k == f) => f,
        _ => "notes",
    };
    let name: String = name.chars().take(60).collect();
    Some(format!("{folder}/{}.md", name.trim_matches('-')))
}

/// `Sam Carter` → `sam-carter`, keeping letters of any alphabet.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// A readable title from a path: `people/sam-carter.md` → `Sam Carter`.
pub fn title_from_path(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    name.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A person's note is best titled by their name as written in it ("Léa is the user's
/// sister" → "Léa"), which keeps accents a file name may have lost.
pub fn guess_title(path: &str, body: &str) -> Option<String> {
    if !path.starts_with("people/") {
        return None;
    }
    let first = facts(body).into_iter().next()?;
    let cut = [
        " is ", "'s ", "’s ", " has ", " lives ", " works ", " was ", " loves ", " likes ",
    ]
    .iter()
    .filter_map(|sep| first.find(sep))
    .min()?;
    let name = first[..cut].trim();
    let words: Vec<&str> = name.split_whitespace().collect();
    let looks_like_a_name = !words.is_empty()
        && words.len() <= 3
        && words
            .iter()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase))
        && !name.starts_with("The ");
    looks_like_a_name.then(|| name.to_owned())
}

/// Whether text looks like a secret that must never be remembered: passwords, codes,
/// card or account numbers, keys.
pub fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    const WORDS: &[&str] = &[
        "password",
        "passwort",
        "mot de passe",
        "passcode",
        "pin code",
        "pin is",
        "api key",
        "secret key",
        "private key",
        "seed phrase",
        "recovery phrase",
        "access token",
        "2fa",
        "one-time code",
        "verification code",
        "iban",
        "cvv",
        "security code",
    ];
    if WORDS.iter().any(|w| lower.contains(w)) {
        return true;
    }
    // Long digit runs: card, account or social security numbers.
    let mut run = 0;
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= 12 {
                return true;
            }
        } else if c != ' ' && c != '-' {
            run = 0;
        }
    }
    // Key-like tokens: long, mixed letters and digits, no spaces.
    text.split_whitespace().any(|w| {
        w.len() >= 24
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars().any(|c| c.is_ascii_alphabetic())
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_=+/.:".contains(c))
            && !w.contains("://")
    })
}

/// The bullet facts in a note body.
pub fn facts(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .map(|f| f.trim().to_owned())
        })
        .filter(|f| !f.is_empty())
        .collect()
}

/// Words of a fact, for comparing facts loosely.
fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether two facts say the same thing, allowing small wording differences.
pub fn same_fact(a: &str, b: &str) -> bool {
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    if wa == wb {
        return true;
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count();
    let union = wa.len() + wb.len() - shared;
    shared as f64 / union as f64 >= 0.8
}

/// Adds facts to a note body as bullets, skipping ones it already says. Returns the new
/// body and how many were added.
pub fn add_facts(body: &str, new: &[String]) -> (String, usize) {
    let existing = facts(body);
    let mut out = body.trim_end().to_owned();
    let mut added = 0;
    for fact in new {
        let fact = fact.trim().trim_start_matches("- ").trim();
        if fact.is_empty() || existing.iter().any(|e| same_fact(e, fact)) {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("- ");
        out.push_str(fact);
        added += 1;
    }
    (out, added)
}

/// Removes the bullets matching any of `gone` (loosely, or by containing the text).
/// Returns the new body and how many were removed.
pub fn remove_facts(body: &str, gone: &[String]) -> (String, usize) {
    let mut removed = 0;
    let kept: Vec<&str> = body
        .lines()
        .filter(|line| {
            let l = line.trim();
            let Some(fact) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) else {
                return true;
            };
            let hit = gone.iter().any(|g| {
                let g = g.trim();
                !g.is_empty()
                    && (same_fact(fact, g) || fact.to_lowercase().contains(&g.to_lowercase()))
            });
            if hit {
                removed += 1;
            }
            !hit
        })
        .collect();
    (kept.join("\n").trim().to_owned(), removed)
}

/// Keeps the profile within [`PROFILE_LIMIT`], cutting at a line boundary.
pub fn cap_profile(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= PROFILE_LIMIT {
        return text.to_owned();
    }
    let mut out = String::new();
    for line in text.lines() {
        if out.chars().count() + line.chars().count() + 1 > PROFILE_LIMIT {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// A fact as the user reads it: "the user's brother" → "your brother".
pub fn for_the_user(fact: &str) -> String {
    let mut s = fact.trim().trim_end_matches('.').to_owned();
    for (from, to) in [
        ("The user's", "Your"),
        ("the user's", "your"),
        ("The user is", "You are"),
        ("the user is", "you are"),
        ("The user has", "You have"),
        ("the user has", "you have"),
        ("The user", "You"),
        ("the user", "you"),
    ] {
        s = s.replace(from, to);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_tidy() {
        assert_eq!(
            normalize_path("People/Sam Carter").as_deref(),
            Some("people/sam-carter.md")
        );
        assert_eq!(
            normalize_path("family/mum.md").as_deref(),
            Some("people/mum.md")
        );
        assert_eq!(
            normalize_path("random/thing").as_deref(),
            Some("notes/thing.md")
        );
        assert_eq!(normalize_path("coffee").as_deref(), Some("notes/coffee.md"));
        assert_eq!(
            normalize_path("habits/mornings.md").as_deref(),
            Some("habits/mornings.md")
        );
        assert_eq!(normalize_path("PROFILE.md").as_deref(), Some(PROFILE_PATH));
        assert_eq!(
            normalize_path("people/Zoë").as_deref(),
            Some("people/zoë.md")
        );
        assert_eq!(normalize_path("  / "), None);
        assert_eq!(title_from_path("people/sam-carter.md"), "Sam Carter");
    }

    #[test]
    fn secrets_are_refused() {
        assert!(looks_secret("My bank password is hunter2"));
        assert!(looks_secret("card 4111 1111 1111 1111"));
        assert!(looks_secret("token sk-proj-abc123def456ghi789jkl012"));
        assert!(looks_secret("Mon mot de passe est chat"));
        assert!(!looks_secret("Sam is the user's brother and lives in Lyon"));
        assert!(!looks_secret("Phone: +41 79 123 45 67"));
        assert!(!looks_secret(
            "Likes https://example.com/some-long-page-name-2024"
        ));
    }

    #[test]
    fn facts_merge_without_duplicates() {
        let body = "- Sam is the user's brother\n- Lives in Lyon";
        let (body, added) = add_facts(
            body,
            &[
                "Sam is the user's brother.".into(),
                "Plays the cello".into(),
            ],
        );
        assert_eq!(added, 1);
        assert_eq!(
            facts(&body),
            [
                "Sam is the user's brother",
                "Lives in Lyon",
                "Plays the cello"
            ]
        );
        let (body, removed) = remove_facts(&body, &["lives in lyon".into()]);
        assert_eq!(removed, 1);
        assert_eq!(facts(&body).len(), 2);
    }

    #[test]
    fn profile_is_capped_at_a_line() {
        let long = (0..200)
            .map(|i| format!("- fact number {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let capped = cap_profile(&long);
        assert!(capped.chars().count() <= PROFILE_LIMIT);
        assert!(capped.ends_with(|c: char| c.is_ascii_digit()));
    }

    #[test]
    fn people_notes_are_titled_by_name() {
        assert_eq!(
            guess_title("people/leaa.md", "- Léa is the user's sister").as_deref(),
            Some("Léa")
        );
        assert_eq!(
            guess_title("people/sam.md", "- Sam Carter's birthday is 3 May").as_deref(),
            Some("Sam Carter")
        );
        assert_eq!(
            guess_title("people/mum.md", "- The user's mother lives in Lyon"),
            None
        );
        assert_eq!(guess_title("habits/run.md", "- Runs every morning"), None);
    }

    #[test]
    fn facts_read_naturally() {
        assert_eq!(
            for_the_user("Sam is the user's brother."),
            "Sam is your brother"
        );
        assert_eq!(for_the_user("The user is vegetarian"), "You are vegetarian");
    }
}
