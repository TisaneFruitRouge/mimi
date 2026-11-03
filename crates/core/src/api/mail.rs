use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    DraftRequest, MailDraft, MailOverview, MailPreset, MailSummary, MailThread, MailThreadDetail,
    MarkRead,
};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail::{self, store};

/// Works out a mailbox's servers from its address, for the connect dialog.
pub async fn discover(
    State(state): State<Arc<AppState>>,
    Json(req): Json<mimi_protocol::MailDiscoverRequest>,
) -> ApiResult<mimi_protocol::MailDiscovery> {
    Ok(Json(
        mail::discover::discover(&req.email, &state.http).await,
    ))
}

pub async fn presets() -> ApiResult<Vec<MailPreset>> {
    Ok(Json(mail::presets()))
}

/// Part of the mail: one account (`account`), or one address it received mail at
/// (`address`). Neither means all of it.
#[derive(Deserialize, Default)]
pub struct ScopeQuery {
    account: Option<Uuid>,
    address: Option<String>,
}

impl ScopeQuery {
    fn scope(self) -> store::Scope {
        store::Scope {
            account: self.account,
            address: self
                .address
                .map(|a| a.trim().to_lowercase())
                .filter(|a| !a.is_empty()),
        }
    }
}

pub async fn overview(
    State(state): State<Arc<AppState>>,
    Query(scope): Query<ScopeQuery>,
) -> ApiResult<MailOverview> {
    mail::overview(&state, scope.scope())
        .await
        .map(Json)
        .map_err(AppError::internal)
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// See `ScopeQuery`. (Not flattened: that breaks numbers in query strings.)
    account: Option<Uuid>,
    address: Option<String>,
    /// A smart folder's conversations (instead of a view).
    folder: Option<i64>,
    /// needs_reply | important | other | inbox | sent | archive
    view: Option<String>,
    /// Words to search for.
    q: Option<String>,
    /// A person in the directory: conversations with any of their addresses.
    person: Option<Uuid>,
    /// Paging: conversations whose last message is older than this.
    before: Option<i64>,
    limit: Option<u32>,
}

pub async fn threads(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Vec<MailThread>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    if let Some(person) = q.person {
        return mail::threads_with(&state, person, limit)
            .await
            .map(Json)
            .map_err(AppError::bad_request);
    }
    let view = match q.view.as_deref() {
        None | Some("") => None,
        Some(v) => {
            Some(mail::parse_view(v).ok_or_else(|| AppError::bad_request("Unknown mail view."))?)
        }
    };
    let query = store::Query {
        view,
        search: q.q.filter(|s| !s.trim().is_empty()),
        before: q.before,
        folder: q.folder,
        limit,
        scope: ScopeQuery {
            account: q.account,
            address: q.address,
        }
        .scope(),
        ..Default::default()
    };
    mail::threads(&state, query)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

pub async fn thread(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailThreadDetail> {
    // Older mail found by a search stays a while after it was last opened.
    mail::older::opened(&state, id).await;
    mail::thread(&state, id)
        .await
        .map_err(AppError::internal)?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Conversation"))
}

pub async fn read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<MarkRead>,
) -> ApiResult<()> {
    mail::mark_read(&state, id, body.read)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(()))
}

/// Flags (stars) a conversation or takes the flag off. Fails, with nothing changed,
/// when the mail server doesn't take it.
pub async fn flag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<mimi_protocol::FlagThread>,
) -> ApiResult<()> {
    mail::flags::set_flagged(&state, id, body.flagged)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

pub async fn archive(State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> ApiResult<()> {
    mail::archive(&state, id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

/// One action on several conversations, chosen together in the Mail panel: the user's
/// own click. Answers which went through; some can fail while the rest still do.
pub async fn batch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<mimi_protocol::MailBatch>,
) -> ApiResult<mimi_protocol::MailBatchResult> {
    mail::batch::run(&state, body.ids, body.action)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// Moves a conversation to the Trash. The user's own click in the Mail panel.
pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> ApiResult<()> {
    mail::delete(&state, id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

pub async fn summarize(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailSummary> {
    let summary = mail::triage::summarize(&state, id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(MailSummary { summary }))
}

pub async fn draft(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<DraftRequest>>,
) -> ApiResult<MailDraft> {
    let instructions = body.and_then(|Json(b)| b.instructions);
    mail::triage::draft_reply(&state, id, instructions)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// Sends a message the user wrote and sent themselves, from the Mail panel or a draft
/// card: their click is the approval.
pub async fn send(
    State(state): State<Arc<AppState>>,
    Json(draft): Json<MailDraft>,
) -> ApiResult<()> {
    mail::send(&state, draft)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

/// An attachment, always as a download: never shown inline, so an HTML attachment can't
/// run in the web interface's origin.
pub async fn attachment(
    State(state): State<Arc<AppState>>,
    Path((message, index)): Path<(i64, usize)>,
) -> Result<axum::response::Response, AppError> {
    let found = mail::attachment(&state, message, index)
        .await
        .map_err(AppError::bad_request)?;
    let ascii: String = found
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = found
        .name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    axum::response::Response::builder()
        .header("content-type", "application/octet-stream")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"),
        )
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", "sandbox")
        .header("x-mimi-content-type", found.content_type)
        .body(axum::body::Body::from(found.data))
        .map_err(AppError::internal)
}

/// A message ready to show as sent or as formatted text, pictures from other servers
/// left out.
pub async fn content(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<mimi_protocol::MailContent> {
    mail::render::content(&state, id, None)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// The same with its pictures from other servers, fetched now: the user asked to load
/// them ("Load images").
pub async fn images(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<mimi_protocol::MailContent> {
    mail::render::content(&state, id, Some(&state.mail.images))
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// Saves the user's TypeSafe key, after checking it with TypeSafe, so Jev can sort mail.
pub async fn jev_connect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<mimi_protocol::JevKey>,
) -> ApiResult<()> {
    mail::jev::save_key(&state, &body.api_key)
        .await
        .map_err(AppError::bad_request)?;
    mail::changed(&state);
    Ok(Json(()))
}

/// Forgets the TypeSafe key; sorting goes back to the user's own model.
pub async fn jev_disconnect(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    mail::jev::remove_key(&state.db)
        .await
        .map_err(AppError::internal)?;
    let mut settings = crate::settings::load(&state.db).await?;
    if settings.mail_sorter == mimi_protocol::MailSorter::Jev {
        settings.mail_sorter = mimi_protocol::MailSorter::Model;
        crate::settings::save(&state.db, &settings).await?;
        state
            .events
            .publish(mimi_protocol::Event::SettingsChanged { settings });
    }
    mail::changed(&state);
    Ok(Json(()))
}

/// A folder's icon and colour, checked against the names the app knows.
fn folder_looks(
    input: &mimi_protocol::MailFolderInput,
) -> Result<(Option<String>, Option<String>), AppError> {
    use mail::folders::{COLORS, ICONS};
    let pick = |value: &Option<String>, allowed: &[&str], what: &str| match value.as_deref() {
        Some(v) if !allowed.contains(&v) => Err(AppError::bad_request(format!("Unknown {what}."))),
        v => Ok(v.map(str::to_owned)),
    };
    Ok((
        pick(&input.icon, ICONS, "icon")?,
        pick(&input.color, COLORS, "colour")?,
    ))
}

/// A folder name and description, trimmed and checked.
fn folder_fields(
    input: &mimi_protocol::MailFolderInput,
    required: bool,
) -> Result<(Option<String>, Option<String>), AppError> {
    use mail::folders::{MAX_DESCRIPTION, MAX_NAME};
    let name = input.name.as_deref().map(str::trim).map(str::to_owned);
    let description = input
        .description
        .as_deref()
        .map(str::trim)
        .map(str::to_owned);
    match &name {
        Some(n) if n.is_empty() || n.chars().count() > MAX_NAME => {
            return Err(AppError::bad_request(format!(
                "Give the folder a name of up to {MAX_NAME} characters."
            )));
        }
        None if required => return Err(AppError::bad_request("Give the folder a name.")),
        _ => {}
    }
    match &description {
        Some(d) if d.is_empty() || d.chars().count() > MAX_DESCRIPTION => {
            return Err(AppError::bad_request(format!(
                "Say what goes in the folder, in up to {MAX_DESCRIPTION} characters."
            )));
        }
        None if required => {
            return Err(AppError::bad_request("Say what goes in the folder."));
        }
        _ => {}
    }
    Ok((name, description))
}

pub async fn create_folder(
    State(state): State<Arc<AppState>>,
    Json(input): Json<mimi_protocol::MailFolderInput>,
) -> ApiResult<()> {
    let (name, description) = folder_fields(&input, true)?;
    let (name, description) = (name.unwrap_or_default(), description.unwrap_or_default());
    let (icon, color) = folder_looks(&input)?;
    let icon = icon.unwrap_or_else(|| mail::folders::ICONS[0].to_owned());
    let color = color.unwrap_or_else(|| mail::folders::COLORS[0].to_owned());
    let created = state
        .db
        .call(move |c| {
            let count: i64 = c.query_row("SELECT count(*) FROM mail_folders", [], |r| r.get(0))?;
            if count >= mail::folders::MAX_FOLDERS as i64 {
                return Ok(false);
            }
            mail::folders::create_with(c, &name, &description, &icon, &color).map(|_| true)
        })
        .await
        .map_err(AppError::internal)?;
    if !created {
        return Err(AppError::bad_request(format!(
            "You can have up to {} smart folders.",
            mail::folders::MAX_FOLDERS
        )));
    }
    mail::changed(&state);
    state.mail.triage_wake.notify_one();
    Ok(Json(()))
}

pub async fn update_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<mimi_protocol::MailFolderInput>,
) -> ApiResult<()> {
    let (name, description) = folder_fields(&input, false)?;
    let (icon, color) = folder_looks(&input)?;
    let found = state
        .db
        .call(move |c| {
            let found = mail::folders::update(c, id, name.as_deref(), description.as_deref())?;
            if found {
                mail::folders::restyle(c, id, icon.as_deref(), color.as_deref())?;
            }
            Ok(found)
        })
        .await
        .map_err(AppError::internal)?;
    if !found {
        return Err(AppError::not_found("Folder"));
    }
    mail::changed(&state);
    state.mail.triage_wake.notify_one();
    Ok(Json(()))
}

pub async fn delete_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<()> {
    let found = state
        .db
        .call(move |c| mail::folders::delete(c, id))
        .await
        .map_err(AppError::internal)?;
    if !found {
        return Err(AppError::not_found("Folder"));
    }
    mail::changed(&state);
    Ok(Json(()))
}

/// The user puts a conversation in a folder or takes it out; the sorter won't undo it.
pub async fn set_thread_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<mimi_protocol::FolderMembership>,
) -> ApiResult<()> {
    let ok = state
        .db
        .call(move |c| {
            let thread: bool = c
                .query_row("SELECT 1 FROM mail_threads WHERE id = ?1", [id], |_| Ok(()))
                .optional()?
                .is_some();
            if !thread || !mail::folders::exists(c, body.folder)? {
                return Ok(false);
            }
            mail::folders::set(c, body.folder, id, body.member, true).map(|_| true)
        })
        .await
        .map_err(AppError::internal)?;
    if !ok {
        return Err(AppError::not_found("Conversation or folder"));
    }
    mail::changed(&state);
    Ok(Json(()))
}

/// Checks every account for new mail now.
pub async fn refresh(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    for account in mail::accounts(&state).await {
        state.mail.poke(account.id);
    }
    Ok(Json(()))
}

/// Looks on the mail servers for mail older than what's kept here, and brings in the
/// newest matches (see `mail::older`).
pub async fn older(
    State(state): State<Arc<AppState>>,
    Json(req): Json<mimi_protocol::MailOlderSearch>,
) -> ApiResult<mimi_protocol::MailOlderResults> {
    let terms = mail::older::Terms::new(&req.q, Vec::new());
    mail::older::search(&state, &terms, req.account, mail::older::CAP)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}
