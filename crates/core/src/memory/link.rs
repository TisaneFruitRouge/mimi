//! Linking notes to the people they're about. A note in `people/` is linked (its
//! `subject`) to someone in the people directory when that's unambiguous: the name or
//! nickname matches exactly, or only one person has that first name, or the user
//! @-mentioned the person in the conversation the note was learned from. Recall then
//! brings in what's known about everyone a message mentions or names.

use std::collections::{HashMap, HashSet};

use mimi_protocol::{Mention, MentionKind, PersonSummary};
use uuid::Uuid;

use super::store;
use crate::AppState;
use crate::db::DbError;
use crate::people::fold;

/// Most people whose notes one message brings in by name.
const MAX_NAMED: usize = 3;

/// The names a note goes by: its file name ("léa-dupont" → "lea dupont") and title.
fn note_names(path: &str, title: &str) -> Vec<String> {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .replace(['-', '_'], " ");
    let mut names = vec![fold(&stem)];
    let title = fold(title);
    if !names.contains(&title) {
        names.push(title);
    }
    names.retain(|n| !n.is_empty());
    names
}

fn first_name(folded: &str) -> &str {
    folded.split_whitespace().next().unwrap_or("")
}

/// The person a note is about, if that's clear. `hints` are people the user @-mentioned
/// where the note was written, which settle a shared first name.
pub fn person_for(
    path: &str,
    title: &str,
    body: &str,
    people: &[PersonSummary],
    hints: &[Uuid],
) -> Option<Uuid> {
    if !path.starts_with("people/") {
        return None;
    }
    let names = note_names(path, title);
    // Full name or nickname: strong. First name only: weak.
    let mut strong: Vec<Uuid> = Vec::new();
    let mut weak: Vec<&PersonSummary> = Vec::new();
    for p in people {
        let full = fold(&p.name);
        let nick = p.nickname.as_deref().map(fold).unwrap_or_default();
        if names
            .iter()
            .any(|n| *n == full || (!nick.is_empty() && *n == nick))
        {
            strong.push(p.id);
        } else if names
            .iter()
            .any(|n| !n.contains(' ') && n == first_name(&full))
        {
            weak.push(p);
        }
    }
    let hinted: Vec<Uuid> = strong
        .iter()
        .copied()
        .chain(weak.iter().map(|p| p.id))
        .filter(|id| hints.contains(id))
        .collect();
    if hinted.len() == 1 {
        return hinted.first().copied();
    }
    if strong.len() == 1 {
        return strong.first().copied();
    }
    if !strong.is_empty() {
        return None;
    }
    match weak.as_slice() {
        [one] => Some(one.id),
        [] => None,
        several => {
            // "Léa Martin" written in the note tells two Léas apart.
            let text = format!(" {} ", fold(body));
            let named: Vec<Uuid> = several
                .iter()
                .filter(|p| {
                    let full = fold(&p.name);
                    full.contains(' ') && text.contains(&format!(" {full} "))
                })
                .map(|p| p.id)
                .collect();
            (named.len() == 1).then(|| named[0])
        }
    }
}

async fn directory(state: &AppState) -> Result<Vec<PersonSummary>, DbError> {
    let everyone = state.db.call(|c| crate::people::store::all(c)).await?;
    Ok(everyone.into_iter().map(|p| p.summary).collect())
}

/// Links every unlinked note in `people/` whose person is clear, and re-links notes
/// whose person was removed or merged away. Returns how many links changed.
pub async fn relink(state: &AppState, hints: &[Uuid]) -> Result<usize, DbError> {
    let notes = store::people_notes(&state.db).await?;
    if notes.is_empty() {
        return Ok(0);
    }
    let people = directory(state).await?;
    let ids: HashSet<String> = people.iter().map(|p| p.id.to_string()).collect();
    let mut changed = 0;
    for note in notes {
        if note.subject.as_ref().is_some_and(|s| ids.contains(s)) {
            continue;
        }
        // Someone merged into another person: the note follows them.
        let merged = match note.subject.as_ref().and_then(|s| s.parse::<Uuid>().ok()) {
            Some(old) => state
                .db
                .call(move |c| crate::people::merge::resolve(c, old))
                .await?
                .map(|id| id.to_string()),
            None => None,
        };
        let subject = merged.or_else(|| {
            person_for(&note.path, &note.title, &note.body, &people, hints).map(|id| id.to_string())
        });
        if subject != note.subject && store::set_subject(&state.db, &note.path, subject).await? {
            changed += 1;
        }
    }
    Ok(changed)
}

/// Person ids among `mentions`.
pub fn mentioned(mentions: &[Mention]) -> Vec<Uuid> {
    mentions
        .iter()
        .filter(|m| m.kind == MentionKind::Person)
        .filter_map(|m| m.id.parse().ok())
        .collect()
}

/// People named in `text`: by full name or nickname, or by first name when only one
/// person has it.
pub fn named_in(text: &str, people: &[PersonSummary]) -> Vec<Uuid> {
    let text = format!(" {} ", fold(text));
    let mut first_names: HashMap<String, usize> = HashMap::new();
    for p in people {
        *first_names
            .entry(first_name(&fold(&p.name)).to_owned())
            .or_default() += 1;
    }
    let has = |name: &str| name.chars().count() >= 3 && text.contains(&format!(" {name} "));
    people
        .iter()
        .filter(|p| {
            let full = fold(&p.name);
            let first = first_name(&full);
            let nick = p.nickname.as_deref().map(fold).unwrap_or_default();
            (full.contains(' ') && has(&full))
                || has(&nick)
                || (first_names.get(first) == Some(&1) && has(first))
        })
        .map(|p| p.id)
        .take(MAX_NAMED)
        .collect()
}

/// The people a message is about: those @-mentioned, then those named, as note
/// subjects.
pub async fn people_in(state: &AppState, text: &str, mentions: &[Mention]) -> Vec<String> {
    let mut ids = mentioned(mentions);
    match directory(state).await {
        Ok(people) => {
            for id in named_in(text, &people) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Err(e) => tracing::warn!("reading the people directory failed: {e}"),
    }
    ids.into_iter().map(|id| id.to_string()).collect()
}

/// Everyone's name by id, for showing which person a note is linked to.
pub async fn names(state: &AppState) -> HashMap<String, String> {
    directory(state)
        .await
        .map(|people| {
            people
                .into_iter()
                .map(|p| (p.id.to_string(), p.name))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, nickname: Option<&str>) -> PersonSummary {
        PersonSummary {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            nickname: nickname.map(str::to_owned),
            channels: Vec::new(),
            reach: Vec::new(),
        }
    }

    #[test]
    fn notes_link_only_when_the_person_is_clear() {
        let lea = person("Léa Martin", None);
        let sam = person("Samuel Keller", Some("Sam"));
        let lea2 = person("Léa Dubois", None);
        let alone = [lea.clone(), sam.clone()];

        // First name, accents ignored; nickname; full name in the file name.
        assert_eq!(
            person_for("people/lea.md", "Lea", "", &alone, &[]),
            Some(lea.id)
        );
        assert_eq!(
            person_for("people/sam.md", "Sam", "", &alone, &[]),
            Some(sam.id)
        );
        assert_eq!(
            person_for("people/samuel-keller.md", "Samuel", "", &alone, &[]),
            Some(sam.id)
        );
        // Only notes about people.
        assert_eq!(person_for("habits/lea.md", "Léa", "", &alone, &[]), None);
        assert_eq!(person_for("people/tom.md", "Tom", "", &alone, &[]), None);

        // Two Léas: not without a clue.
        let both = [lea.clone(), lea2.clone(), sam.clone()];
        assert_eq!(
            person_for("people/léa.md", "Léa", "- A nurse", &both, &[]),
            None
        );
        // The user @-mentioned one of them…
        assert_eq!(
            person_for("people/léa.md", "Léa", "", &both, &[lea2.id]),
            Some(lea2.id)
        );
        // …or the note says which.
        assert_eq!(
            person_for(
                "people/léa.md",
                "Léa",
                "- Léa Martin is the user's sister",
                &both,
                &[]
            ),
            Some(lea.id)
        );
    }

    #[test]
    fn people_are_found_by_name_in_a_message() {
        let lea = person("Léa Martin", None);
        let sam = person("Samuel Keller", Some("Sam"));
        let al1 = person("Alex Moor", None);
        let al2 = person("Alex Stone", None);
        let everyone = [lea.clone(), sam.clone(), al1.clone(), al2.clone()];

        assert_eq!(
            named_in("What should I get lea for her birthday?", &everyone),
            [lea.id]
        );
        assert_eq!(named_in("Is Sam free on Friday?", &everyone), [sam.id]);
        // A shared first name needs the full name.
        assert!(named_in("Did Alex call?", &everyone).is_empty());
        assert_eq!(named_in("Did Alex Stone call?", &everyone), [al2.id]);
        // Parts of words don't count.
        assert!(named_in("Samples of leaves", &everyone).is_empty());
    }
}
