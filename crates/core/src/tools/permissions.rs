//! What the assistant may do without asking first, as the user chose in Settings ›
//! Permissions ([`mimi_protocol::Permissions`]).
//!
//! [`KINDS`] is the one place that defines the kinds of action a permission covers.
//! Adding a kind means adding a [`Governs`] variant and its [`Kind`] here, then pointing
//! tools at it with [`Tool::governed_by`] (and [`Tool::call_targets`] when its exceptions
//! are about people or calendars). Clients render the catalog `GET /v1/permissions`
//! serves, so they need no change.
//!
//! Each kind has a default plus exceptions for particular people or calendars. The most
//! specific wins; a call about several (an email to three people) runs on its own only
//! if every one of them allows it. The choices change only through the user's own API
//! calls, never through a tool.

use std::collections::{HashMap, HashSet};

use mimi_protocol::{
    Autonomy, Channel, KindPermission, PermissionKind, PermissionRule, PermissionRuleView,
    PermissionTarget, PermissionTargetKind, Permissions,
};
use serde_json::Value;
use uuid::Uuid;

use super::Tool;
use crate::AppState;
use crate::people::normalize::match_key;

/// The kinds of action a permission covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Governs {
    SendMail,
    AddEvents,
    ChangeEvents,
    Schedule,
}

/// How a kind of action is described and decided.
#[derive(Debug)]
pub struct Kind {
    pub governs: Governs,
    /// Stable: it's the key in the stored settings.
    pub id: &'static str,
    pub title: &'static str,
    pub ask_detail: &'static str,
    pub automatic_detail: &'static str,
    /// A safety net that holds whatever the user chooses.
    pub note: Option<&'static str>,
    /// A lucide icon name and the tile's colour.
    pub icon: &'static str,
    pub color: &'static str,
    pub default: Autonomy,
    pub targets: PermissionTargetKind,
    /// Automatic still asks unless everyone the call reaches (email recipients, an
    /// event's guests: its [`CallTarget::Email`]s) is someone the user knows (see
    /// [`crate::mail::known`]): a hostile email can't make the assistant write to, or
    /// invite, its sender's accomplice, whatever the settings say. A kind about people
    /// (email) must reach someone; a calendar kind reaching nobody is just about the
    /// calendar.
    pub recipients_must_be_known: bool,
}

pub const KINDS: &[Kind] = &[
    Kind {
        governs: Governs::SendMail,
        id: "send_mail",
        title: "Send emails",
        ask_detail: "Shows you the whole email to approve before it goes out.",
        automatic_detail: "Sends on its own to your contacts and people you've emailed before, invitations included. Still asks before writing to anyone new.",
        note: Some(
            "An email to someone new, who isn't in your contacts and hasn't had an email from you, always waits for your OK.",
        ),
        icon: "send",
        color: "#0a84ff",
        default: Autonomy::Ask,
        targets: PermissionTargetKind::Person,
        recipients_must_be_known: true,
    },
    Kind {
        governs: Governs::AddEvents,
        id: "add_events",
        title: "Add calendar events",
        ask_detail: "Shows you each event to approve before it's added.",
        automatic_detail: "Adds events to your calendars on its own. Invitations are only emailed when you say so.",
        note: Some(
            "An event with a guest who isn't in your contacts and hasn't had an email from you always waits for your OK.",
        ),
        icon: "calendar-plus",
        color: "#ff3b30",
        default: Autonomy::Ask,
        targets: PermissionTargetKind::Calendar,
        recipients_must_be_known: true,
    },
    Kind {
        governs: Governs::ChangeEvents,
        id: "change_events",
        title: "Change or remove calendar events",
        ask_detail: "Shows you each change or removal to approve first.",
        automatic_detail: "Moves, renames and removes events on its own. Guests are only emailed when you say so.",
        note: Some(
            "Inviting someone who isn't in your contacts and hasn't had an email from you always waits for your OK.",
        ),
        icon: "calendar-cog",
        color: "#5856d6",
        default: Autonomy::Ask,
        targets: PermissionTargetKind::Calendar,
        recipients_must_be_known: true,
    },
    Kind {
        governs: Governs::Schedule,
        id: "schedule",
        title: "Set reminders and routines",
        ask_detail: "Shows you each reminder or routine, and each change, to approve first.",
        automatic_detail: "Sets, changes and cancels them on its own. Each one shows in the chat with Undo.",
        note: None,
        icon: "bell-ring",
        color: "#ff9f0a",
        default: Autonomy::Automatic,
        targets: PermissionTargetKind::None,
        recipients_must_be_known: false,
    },
];

impl Governs {
    pub fn kind(self) -> &'static Kind {
        KINDS
            .iter()
            .find(|k| k.governs == self)
            .expect("every kind of action has a descriptor")
    }
}

pub fn kind_by_id(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// What a call is about, for matching exceptions: an email recipient, or a calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    /// An address as the model wrote it ("Sam <sam@x.org>" or bare).
    Email(String),
    /// A calendar id (`calendar::calendar_id`).
    Calendar(String),
}

/// The user's choice for a kind, or its default.
pub fn choice(permissions: &Permissions, kind: &Kind) -> KindPermission {
    permissions
        .0
        .get(kind.id)
        .cloned()
        .unwrap_or(KindPermission {
            autonomy: kind.default,
            rules: Vec::new(),
        })
}

/// Whether a call may end up on an approval card, so its arguments must be prepared in
/// the shape the card shows (see [`Tool::prepare`]).
pub fn may_ask(tool: &dyn Tool, args: &Value, permissions: &Permissions) -> bool {
    if tool.needs_approval(args) {
        return true;
    }
    let Some(kind) = tool.governed_by().map(Governs::kind) else {
        return false;
    };
    let choice = choice(permissions, kind);
    kind.recipients_must_be_known
        || choice.autonomy == Autonomy::Ask
        || choice.rules.iter().any(|r| r.autonomy == Autonomy::Ask)
}

/// Whether this call waits for the user's approval. `args` are the prepared arguments.
pub async fn requires_approval(
    state: &AppState,
    tool: &dyn Tool,
    args: &Value,
    permissions: &Permissions,
) -> bool {
    let Some(kind) = tool.governed_by().map(Governs::kind) else {
        return tool.needs_approval(args);
    };
    let targets = tool.call_targets(args);
    if decide(state, kind, &choice(permissions, kind), &targets).await == Autonomy::Ask {
        return true;
    }
    !reaches_only_known(state, kind, &targets).await
}

/// The safety net under every choice: a call reaching people (see
/// [`Kind::recipients_must_be_known`]) reaches only people the user knows.
async fn reaches_only_known(state: &AppState, kind: &Kind, targets: &[CallTarget]) -> bool {
    if !kind.recipients_must_be_known {
        return true;
    }
    let emails = emails(targets);
    if emails.is_empty() && kind.targets != PermissionTargetKind::Person {
        return true;
    }
    crate::mail::known::all_known(state, &emails).await
}

fn emails(targets: &[CallTarget]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|t| match t {
            CallTarget::Email(e) => Some(e.clone()),
            CallTarget::Calendar(_) => None,
        })
        .collect()
}

/// The kind's choice for these targets: automatic only if every one of them is.
async fn decide(
    state: &AppState,
    kind: &Kind,
    choice: &KindPermission,
    targets: &[CallTarget],
) -> Autonomy {
    if kind.targets == PermissionTargetKind::None || targets.is_empty() {
        return choice.autonomy;
    }
    let mut all_automatic = true;
    for target in targets {
        let matching = matching_rules(state, &choice.rules, target).await;
        let autonomy = if matching.is_empty() {
            choice.autonomy
        } else if matching.contains(&Autonomy::Ask) {
            // Someone with two exceptions (an address shared by two people): asking wins.
            Autonomy::Ask
        } else {
            Autonomy::Automatic
        };
        all_automatic &= autonomy == Autonomy::Automatic;
    }
    if all_automatic {
        Autonomy::Automatic
    } else {
        Autonomy::Ask
    }
}

/// The autonomy of every exception about this target.
async fn matching_rules(
    state: &AppState,
    rules: &[PermissionRule],
    target: &CallTarget,
) -> Vec<Autonomy> {
    match target {
        CallTarget::Calendar(id) => rules
            .iter()
            .filter(|r| matches!(&r.target, PermissionTarget::Calendar(c) if c == id))
            .map(|r| r.autonomy)
            .collect(),
        CallTarget::Email(raw) => {
            if !rules
                .iter()
                .any(|r| matches!(r.target, PermissionTarget::Person(_)))
            {
                return Vec::new();
            }
            let people = people_with_email(state, raw).await;
            rules
                .iter()
                .filter(|r| matches!(r.target, PermissionTarget::Person(p) if people.contains(&p)))
                .map(|r| r.autonomy)
                .collect()
        }
    }
}

/// Everyone in People with this email address. Anything that isn't an address matches
/// nobody.
async fn people_with_email(state: &AppState, raw: &str) -> HashSet<Uuid> {
    let Some(key) = crate::mail::known::address_of(raw).and_then(|e| match_key(Channel::Email, &e))
    else {
        return HashSet::new();
    };
    state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT person_id FROM person_handles
                 WHERE channel = 'email' AND match_key = ?1",
            )?;
            let ids = stmt.query_map([key], |r| r.get::<_, String>(0))?;
            Ok(ids
                .filter_map(|id| id.ok()?.parse().ok())
                .collect::<HashSet<Uuid>>())
        })
        .await
        .unwrap_or_default()
}

/// The exceptions an approval card can offer to add ("Don't ask again for Sam"), with
/// the choice's text. Only when it would make a difference next time: every target
/// resolves to one person or calendar with no exception of its own, and for email,
/// every recipient is someone the user knows (otherwise the safety net would still ask).
pub async fn always_offer(
    state: &AppState,
    tool: &dyn Tool,
    args: &Value,
    permissions: &Permissions,
) -> Option<(String, Vec<PermissionTarget>)> {
    let kind = tool.governed_by()?.kind();
    let choice = choice(permissions, kind);
    if choice.autonomy == Autonomy::Automatic && choice.rules.is_empty() {
        return None;
    }
    let targets = tool.call_targets(args);
    if targets.is_empty() {
        return None;
    }
    let mut out: Vec<PermissionTarget> = Vec::new();
    for target in &targets {
        let rule_target = match (kind.targets, target) {
            (PermissionTargetKind::Person, CallTarget::Email(raw)) => {
                let people = people_with_email(state, raw).await;
                if people.len() != 1 {
                    return None;
                }
                PermissionTarget::Person(people.into_iter().next()?)
            }
            (PermissionTargetKind::Calendar, CallTarget::Calendar(id)) => {
                PermissionTarget::Calendar(id.clone())
            }
            // An event's guests: the exception is about the calendar; the safety net
            // below still needs every guest to be known.
            (PermissionTargetKind::Calendar, CallTarget::Email(_)) => continue,
            _ => return None,
        };
        // The user chose to be asked about this one: don't offer to undo that here.
        if choice.rules.iter().any(|r| r.target == rule_target) {
            return None;
        }
        if !out.contains(&rule_target) {
            out.push(rule_target);
        }
    }
    if out.is_empty() || !reaches_only_known(state, kind, &targets).await {
        return None;
    }
    let labels = labels(state).await;
    let names: Vec<String> = out
        .iter()
        .map(|t| labels.get(t).cloned())
        .collect::<Option<_>>()?;
    Some((format!("Don't ask again for {}", join(&names)), out))
}

fn join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Adds exceptions making these targets automatic for `kind`: the user's own choice on
/// an approval card.
pub async fn allow_always(
    state: &AppState,
    kind_id: &str,
    targets: &[PermissionTarget],
) -> Result<(), crate::db::DbError> {
    let Some(kind) = kind_by_id(kind_id) else {
        return Ok(());
    };
    let mut settings = crate::settings::load(&state.db).await?;
    let mut choice = choice(&settings.permissions, kind);
    for target in targets {
        choice.rules.retain(|r| &r.target != target);
        choice.rules.push(PermissionRule {
            target: target.clone(),
            autonomy: Autonomy::Automatic,
        });
    }
    settings.permissions.0.insert(kind.id.to_owned(), choice);
    save(state, settings).await
}

/// Replaces one kind's choice, as the user set it in Settings › Permissions. Refuses
/// unknown kinds and exceptions a kind can't have or about people or calendars that
/// don't exist; one exception per target, the last one wins.
pub async fn set(
    state: &AppState,
    kind_id: &str,
    mut new: KindPermission,
) -> Result<(), crate::api::error::AppError> {
    use crate::api::error::AppError;
    let kind = kind_by_id(kind_id).ok_or_else(|| AppError::not_found("Permission"))?;
    let known = labels(state).await;
    let mut settings = crate::settings::load(&state.db).await?;
    let before = choice(&settings.permissions, kind);
    let mut seen = HashSet::new();
    new.rules.reverse();
    new.rules.retain(|r| seen.insert(r.target.clone()));
    new.rules.reverse();
    for rule in &new.rules {
        let fits = matches!(
            (kind.targets, &rule.target),
            (PermissionTargetKind::Person, PermissionTarget::Person(_))
                | (
                    PermissionTargetKind::Calendar,
                    PermissionTarget::Calendar(_)
                )
        );
        if !fits {
            return Err(AppError::bad_request(
                "That kind of action can't have that exception.",
            ));
        }
        // Existing exceptions may point at something that's gone; new ones may not.
        let kept = before.rules.iter().any(|r| r.target == rule.target);
        if !kept && !known.contains_key(&rule.target) {
            return Err(AppError::bad_request(match rule.target {
                PermissionTarget::Person(_) => "That person isn't in People.",
                PermissionTarget::Calendar(_) => "That calendar isn't connected.",
            }));
        }
    }
    settings.permissions.0.insert(kind.id.to_owned(), new);
    save(state, settings).await?;
    Ok(())
}

async fn save(
    state: &AppState,
    settings: mimi_protocol::Settings,
) -> Result<(), crate::db::DbError> {
    crate::settings::save(&state.db, &settings).await?;
    state
        .events
        .publish(mimi_protocol::Event::SettingsChanged { settings });
    Ok(())
}

/// The names of everyone and every calendar an exception could be about.
async fn labels(state: &AppState) -> HashMap<PermissionTarget, String> {
    let mut out = HashMap::new();
    let accounts = crate::connections::calendar_accounts(state).await;
    for c in crate::connections::calendar::calendars(&accounts) {
        out.insert(PermissionTarget::Calendar(c.id), c.name);
    }
    let people: Vec<(String, String)> = state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM people")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .await
        .unwrap_or_default();
    for (id, name) in people {
        if let Ok(id) = id.parse() {
            out.insert(PermissionTarget::Person(id), name);
        }
    }
    out
}

/// The catalog as clients show it, with the user's choices.
pub async fn catalog(state: &AppState) -> Result<Vec<PermissionKind>, crate::db::DbError> {
    let settings = crate::settings::load(&state.db).await?;
    let labels = labels(state).await;
    Ok(KINDS
        .iter()
        .map(|kind| {
            let choice = choice(&settings.permissions, kind);
            PermissionKind {
                id: kind.id.to_owned(),
                title: kind.title.to_owned(),
                ask_detail: kind.ask_detail.to_owned(),
                automatic_detail: kind.automatic_detail.to_owned(),
                note: kind.note.map(str::to_owned),
                icon: kind.icon.to_owned(),
                color: kind.color.to_owned(),
                default_autonomy: kind.default,
                targets: kind.targets,
                autonomy: choice.autonomy,
                rules: choice
                    .rules
                    .into_iter()
                    .map(|r| {
                        let label = labels.get(&r.target).cloned();
                        PermissionRuleView {
                            missing: label.is_none(),
                            label: label.unwrap_or_else(|| {
                                match r.target {
                                    PermissionTarget::Person(_) => "Someone no longer in People",
                                    PermissionTarget::Calendar(_) => {
                                        "A calendar no longer connected"
                                    }
                                }
                                .to_owned()
                            }),
                            target: r.target,
                            autonomy: r.autonomy,
                        }
                    })
                    .collect(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::future::BoxFuture;
    use serde_json::json;

    use super::*;
    use crate::tools::ToolContext;

    struct Fake(bool, Option<Governs>);

    impl Tool for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        fn description(&self) -> &str {
            ""
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        fn needs_approval(&self, _: &Value) -> bool {
            self.0
        }
        fn governed_by(&self) -> Option<Governs> {
            self.1
        }
        fn call_targets(&self, args: &Value) -> Vec<CallTarget> {
            let list = |k: &str| args[k].as_array().cloned().unwrap_or_default();
            list("to")
                .into_iter()
                .filter_map(|v| v.as_str().map(|s| CallTarget::Email(s.to_owned())))
                .chain(
                    list("calendars")
                        .into_iter()
                        .filter_map(|v| v.as_str().map(|s| CallTarget::Calendar(s.to_owned()))),
                )
                .collect()
        }
        fn summary(&self, _: &Value) -> String {
            String::new()
        }
        fn run<'a>(&'a self, _: &'a ToolContext, _: Value) -> BoxFuture<'a, Result<Value, String>> {
            Box::pin(async { Ok(Value::Null) })
        }
    }

    fn perms(entries: &[(&str, Autonomy, Vec<PermissionRule>)]) -> Permissions {
        Permissions(
            entries
                .iter()
                .map(|(k, a, rules)| {
                    (
                        (*k).to_owned(),
                        KindPermission {
                            autonomy: *a,
                            rules: rules.clone(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn all(a: Autonomy) -> Permissions {
        perms(
            &KINDS
                .iter()
                .map(|k| (k.id, a, Vec::new()))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn every_kind_has_one_descriptor_and_a_unique_id() {
        for g in [
            Governs::SendMail,
            Governs::AddEvents,
            Governs::ChangeEvents,
            Governs::Schedule,
        ] {
            assert_eq!(KINDS.iter().filter(|k| k.governs == g).count(), 1);
        }
        let ids: HashSet<&str> = KINDS.iter().map(|k| k.id).collect();
        assert_eq!(ids.len(), KINDS.len());
        // The ids of the first version stay, so saved settings keep working.
        for id in ["send_mail", "add_events", "schedule"] {
            assert!(kind_by_id(id).is_some());
        }
    }

    #[tokio::test]
    async fn settings_decide_only_for_the_actions_they_cover() {
        let state = AppState::for_tests("t");
        let args = json!({});
        let defaults = Permissions::default();
        let all_automatic = all(Autonomy::Automatic);
        let all_ask = all(Autonomy::Ask);
        let ask = |tool: Fake, p: Permissions| {
            let state = &state;
            let args = &args;
            async move { requires_approval(state, &tool, args, &p).await }
        };

        // Tools no setting covers keep their own rule, whatever the settings say.
        assert!(ask(Fake(true, None), all_automatic.clone()).await);
        assert!(!ask(Fake(false, None), all_ask.clone()).await);

        // Events: ask by default, automatic when allowed.
        let event = || Fake(true, Some(Governs::AddEvents));
        assert!(ask(event(), defaults.clone()).await);
        assert!(!ask(event(), all_automatic.clone()).await);
        let change = || Fake(true, Some(Governs::ChangeEvents));
        assert!(ask(change(), defaults.clone()).await);
        assert!(!ask(change(), all_automatic.clone()).await);

        // Reminders: automatic by default, as before; asking when the user wants that,
        // with the arguments prepared for the card.
        let reminder = || Fake(false, Some(Governs::Schedule));
        assert!(!ask(reminder(), defaults.clone()).await);
        assert!(ask(reminder(), all_ask.clone()).await);
        assert!(may_ask(&reminder(), &args, &all_ask));
        assert!(!may_ask(&reminder(), &args, &defaults));

        // Automatic email still asks for strangers, for nobody, and for non-addresses.
        let send = |to: Value| {
            let state = &state;
            let all_automatic = all_automatic.clone();
            async move {
                requires_approval(
                    state,
                    &Fake(true, Some(Governs::SendMail)),
                    &json!({"to": to}),
                    &all_automatic,
                )
                .await
            }
        };
        assert!(send(json!(["stranger@example.net"])).await);
        assert!(send(json!([])).await);
        assert!(send(json!(["not an address"])).await);

        // Automatic events too, once they have guests: a stranger makes it ask; an
        // event with nobody on it is only about the calendar.
        for kind in [Governs::AddEvents, Governs::ChangeEvents] {
            let event = |to: Value| {
                let state = &state;
                let all_automatic = all_automatic.clone();
                async move {
                    requires_approval(
                        state,
                        &Fake(true, Some(kind)),
                        &json!({"to": to}),
                        &all_automatic,
                    )
                    .await
                }
            };
            assert!(event(json!(["stranger@example.net"])).await);
            assert!(event(json!(["not an address"])).await);
            assert!(!event(json!([])).await);
        }
    }

    #[tokio::test]
    async fn the_most_specific_choice_wins_and_every_calendar_must_allow_it() {
        let state = AppState::for_tests("t");
        let rule = |id: &str, a| PermissionRule {
            target: PermissionTarget::Calendar(id.to_owned()),
            autonomy: a,
        };
        let tool = Fake(true, Some(Governs::AddEvents));
        let asks = |p: Permissions, cals: Value| {
            let state = &state;
            let tool = &tool;
            async move { requires_approval(state, tool, &json!({"calendars": cals}), &p).await }
        };
        // Ask by default, but Work is automatic.
        let work_ok = perms(&[(
            "add_events",
            Autonomy::Ask,
            vec![rule("work", Autonomy::Automatic)],
        )]);
        assert!(!asks(work_ok.clone(), json!(["work"])).await);
        assert!(asks(work_ok.clone(), json!(["family"])).await);
        assert!(asks(work_ok.clone(), json!(["work", "family"])).await);
        // A rule for another kind changes nothing here.
        let elsewhere = perms(&[(
            "change_events",
            Autonomy::Ask,
            vec![rule("work", Autonomy::Automatic)],
        )]);
        assert!(asks(elsewhere, json!(["work"])).await);
        // Automatic by default, but always ask about Family.
        let family_asks = perms(&[(
            "add_events",
            Autonomy::Automatic,
            vec![rule("family", Autonomy::Ask)],
        )]);
        assert!(!asks(family_asks.clone(), json!(["work"])).await);
        assert!(asks(family_asks.clone(), json!(["family"])).await);
        assert!(may_ask(&tool, &json!({}), &family_asks));
        // No calendar named: the default decides.
        assert!(asks(work_ok, json!([])).await);
        assert!(!asks(family_asks, json!([])).await);
    }

    async fn person(state: &AppState, name: &str, email: &str) -> Uuid {
        let id = Uuid::now_v7();
        let (name, email) = (name.to_owned(), email.to_owned());
        let key = match_key(Channel::Email, &email).unwrap();
        state
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO people (id, name, created_at, updated_at) VALUES (?1, ?2, 0, 0)",
                    (id.to_string(), &name),
                )?;
                c.execute(
                    "INSERT INTO person_handles (id, person_id, channel, value, match_key, created_at)
                     VALUES (?1, ?2, 'email', ?3, ?4, 0)",
                    (Uuid::now_v7().to_string(), id.to_string(), &email, key),
                )?;
                Ok(())
            })
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn people_exceptions_cover_their_addresses_and_every_recipient_must_allow_it() {
        let state = AppState::for_tests("t");
        let sam = person(&state, "Sam", "sam@example.com").await;
        let lea = person(&state, "Léa", "lea@example.com").await;
        let rule = |p: Uuid, a| PermissionRule {
            target: PermissionTarget::Person(p),
            autonomy: a,
        };
        let tool = Fake(true, Some(Governs::SendMail));
        let decides = |p: Permissions, to: Value| {
            let state = &state;
            async move {
                let kind = Governs::SendMail.kind();
                let targets: Vec<CallTarget> = to
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| CallTarget::Email(v.as_str().unwrap().to_owned()))
                    .collect();
                decide(state, kind, &choice(&p, kind), &targets).await
            }
        };
        let sam_ok = perms(&[(
            "send_mail",
            Autonomy::Ask,
            vec![rule(sam, Autonomy::Automatic)],
        )]);
        use Autonomy::*;
        assert_eq!(
            decides(sam_ok.clone(), json!(["Sam <SAM@example.com>"])).await,
            Automatic
        );
        assert_eq!(
            decides(
                sam_ok.clone(),
                json!(["sam@example.com", "lea@example.com"])
            )
            .await,
            Ask
        );
        assert_eq!(
            decides(sam_ok.clone(), json!(["someone@else.org"])).await,
            Ask
        );
        assert_eq!(
            decides(sam_ok.clone(), json!(["not an address"])).await,
            Ask
        );
        let lea_asks = perms(&[(
            "send_mail",
            Automatic,
            vec![rule(lea, Ask), rule(Uuid::now_v7(), Ask)],
        )]);
        assert_eq!(
            decides(lea_asks.clone(), json!(["sam@example.com"])).await,
            Automatic
        );
        assert_eq!(
            decides(lea_asks, json!(["sam@example.com", "lea@example.com"])).await,
            Ask
        );

        // A recipient no exception covers, and whom the user doesn't know: the card
        // still comes.
        assert!(
            requires_approval(
                &state,
                &tool,
                &json!({"to": ["x@stranger.example"]}),
                &sam_ok
            )
            .await
        );
    }
}
