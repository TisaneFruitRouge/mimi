//! Matrix against a real homeserver on this computer. Ignored unless asked for:
//!
//! ```sh
//! MIMI_TEST_MATRIX=http://127.0.0.1:8008 cargo test -p mimi-core live_matrix -- --ignored
//! ```
//!
//! The server needs open registration without verification (a throwaway Synapse; see
//! CLAUDE.md › Connections). Never point this at a public server.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::relation::{Annotation, InReplyTo, Reply as ReplyTo};
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::room::message::{
    OriginalSyncRoomMessageEvent, Relation, RoomMessageEventContent,
};
use matrix_sdk::ruma::{OwnedEventId, UserId};
use matrix_sdk::{Client, Room};
use serde_json::{Value, json};

use super::tool_use::{Reply, scripted_llm};
use super::*;
use crate::tools::{Tool, ToolContext, ToolSource};

const WAIT: Duration = Duration::from_secs(60);

/// A fresh account on the test server: (user id, password).
async fn register(server: &str, name: &str) -> (String, String) {
    let http = reqwest::Client::new();
    let password = format!("pw-{}", uuid::Uuid::now_v7().simple());
    let url = format!("{server}/_matrix/client/v3/register");
    let mut body = json!({ "username": name, "password": password, "inhibit_login": true });
    let first: Value = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["auth"] = json!({ "type": "m.login.dummy", "session": first["session"] });
    let done: Value = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user = done["user_id"]
        .as_str()
        .unwrap_or_else(|| panic!("registration failed: {done}"))
        .to_owned();
    (user, password)
}

/// The owner's own Matrix app: signs in, syncs in the background and records every
/// message it can read, as (event id, sender, body).
struct Owner {
    client: Client,
    seen: Arc<Mutex<Vec<(OwnedEventId, String, String)>>>,
}

impl Owner {
    async fn sign_in(server: &str, user: &str, password: &str) -> Self {
        let client = Client::builder()
            .homeserver_url(server)
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .login_username(user, password)
            .send()
            .await
            .unwrap();
        let seen: Arc<Mutex<Vec<(OwnedEventId, String, String)>>> = Default::default();
        let record = seen.clone();
        client.add_event_handler(move |ev: OriginalSyncRoomMessageEvent| {
            let record = record.clone();
            async move {
                record.lock().unwrap().push((
                    ev.event_id.clone(),
                    ev.sender.to_string(),
                    ev.content.body().to_owned(),
                ));
            }
        });
        let syncing = client.clone();
        tokio::spawn(async move {
            loop {
                let settings = SyncSettings::default().timeout(Duration::from_secs(5));
                if syncing.sync_once(settings).await.is_err() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        });
        Self { client, seen }
    }

    /// Waits for a message from `sender` whose body contains `text`; its event id.
    async fn wait_for(&self, sender: &str, text: &str) -> OwnedEventId {
        let start = std::time::Instant::now();
        loop {
            if let Some((id, ..)) = self
                .seen
                .lock()
                .unwrap()
                .iter()
                .find(|(_, s, b)| s == sender && b.contains(text))
            {
                return id.clone();
            }
            assert!(
                start.elapsed() < WAIT,
                "no message containing {text:?} from {sender}; seen: {:?}",
                self.bodies()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn count(&self, sender: &str, text: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s, b)| s == sender && b.contains(text))
            .count()
    }

    fn bodies(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, s, b)| format!("{s}: {b}"))
            .collect()
    }
}

async fn say(room: &Room, text: &str) {
    room.send(RoomMessageEventContent::text_plain(text))
        .await
        .unwrap();
}

async fn membership(room: &Room, user: &UserId) -> Option<MembershipState> {
    let member = room.get_member_no_sync(user).await.ok().flatten()?;
    Some(member.membership().clone())
}

async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let start = std::time::Instant::now();
    while !check().await {
        assert!(start.elapsed() < WAIT, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// A tool that needs approval and counts its runs.
struct SendNote(Arc<AtomicUsize>);

impl Tool for SendNote {
    fn name(&self) -> &str {
        "send_note"
    }
    fn description(&self) -> &str {
        "Sends a note."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"to": {"type": "string"}}})
    }
    fn needs_approval(&self, _: &Value) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "Send a note to {}",
            args["to"].as_str().unwrap_or("someone")
        )
    }
    fn result_label(&self, _: &Value, _: &Value) -> String {
        "sent a note".to_owned()
    }
    fn run<'a>(&'a self, _: &'a ToolContext, _: Value) -> BoxFuture<'a, Result<Value, String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(json!({"sent": true})) })
    }
}

struct Source(Arc<dyn Tool>);

impl ToolSource for Source {
    fn tools<'a>(&'a self, _: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>> {
        let tool = self.0.clone();
        Box::pin(async move { vec![tool] })
    }
}

/// A harness whose daemon keeps its data in `dir`, as a real one does.
async fn harness_in(dir: &std::path::Path) -> Harness {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut state = AppState::for_tests_on(TOKEN, port);
    state.paths.data_dir = dir.to_path_buf();
    let state = Arc::new(state);
    let app = router(state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut req = format!("ws://127.0.0.1:{port}/v1/events")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    Harness {
        state,
        http: reqwest::Client::new(),
        base: format!("http://127.0.0.1:{port}/v1"),
        ws,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a local Matrix server: MIMI_TEST_MATRIX=http://127.0.0.1:8008"]
async fn live_matrix() {
    let Ok(server) = std::env::var("MIMI_TEST_MATRIX") else {
        eprintln!("MIMI_TEST_MATRIX isn't set; skipping");
        return;
    };
    let server = server.trim_end_matches('/').to_owned();
    // RUST_LOG=mimi_core=debug shows what the connection does.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let run = uuid::Uuid::now_v7().simple().to_string();
    let (bot, bot_password) = register(&server, &format!("assistant-{}", &run[20..])).await;
    let (me, my_password) = register(&server, &format!("owner-{}", &run[20..])).await;
    let (stranger, stranger_password) = register(&server, &format!("other-{}", &run[20..])).await;

    // The model: asks to send a note when told to, and says so once it has run.
    let llm = scripted_llm(|req, _| {
        let messages = req["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        if last["role"] == "tool" {
            Reply::Text("Done, I sent it.")
        } else if last["content"].as_str().unwrap_or("").contains("note") {
            Reply::Call("send_note", json!({"to": "Sam"}))
        } else {
            Reply::Text("Hello from **your assistant**!")
        }
    })
    .await;
    let data = tempfile::tempdir().unwrap();
    let h = harness_in(data.path()).await;
    h.use_mock(llm.port()).await;
    let writes = Arc::new(AtomicUsize::new(0));
    h.state
        .tool_sources
        .add(Arc::new(Source(Arc::new(SendNote(writes.clone())))));

    // A wrong password is refused before anything is saved.
    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/connections",
            json!({"integration": "matrix", "user": bot, "password": "nope", "homeserver": server}),
        )
        .await;
    assert_eq!(status, 400, "{err}");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("didn't accept"),
        "{err}"
    );

    let (status, conn) = h
        .call(
            reqwest::Method::POST,
            "/connections",
            json!({"integration": "matrix", "user": bot, "password": bot_password, "homeserver": server}),
        )
        .await;
    assert_eq!(status, 200, "{conn}");
    assert_eq!(conn["status"], "needs_action");
    assert_eq!(
        conn["action_url"].as_str().unwrap(),
        format!("https://matrix.to/#/{bot}")
    );
    let detail = conn["detail"].as_str().unwrap().to_owned();
    let code: String = detail.chars().filter(char::is_ascii_digit).collect();
    let code = &code[code.len() - 6..];
    let id: uuid::Uuid = conn["id"].as_str().unwrap().parse().unwrap();
    assert!(
        data.path().join("matrix").join(id.to_string()).is_dir(),
        "the keys have a store of their own"
    );

    // The owner starts an encrypted direct chat with the assistant, which joins it.
    let owner = Owner::sign_in(&server, &me, &my_password).await;
    let bot_id = UserId::parse(bot.as_str()).unwrap();
    let room = owner.client.create_dm(&bot_id).await.unwrap();
    eventually("the assistant to join", async || {
        membership(&room, &bot_id).await == Some(MembershipState::Join)
    })
    .await;

    // A wrong code does nothing; the right one pairs, with an honest welcome.
    say(&room, "000000").await;
    say(&room, code).await;
    owner.wait_for(&bot, "your assistant").await;
    owner.wait_for(&bot, "end-to-end encrypted").await;
    assert_eq!(
        owner.count(&bot, "your assistant"),
        1,
        "{:?}",
        owner.bodies()
    );
    let (_, list) = h
        .call(reqwest::Method::GET, "/connections", Value::Null)
        .await;
    let detail = list[0]["detail"].as_str().unwrap();
    assert_eq!(list[0]["status"], "ok", "{list}");
    assert!(detail.contains("end-to-end encrypted"), "{detail}");
    assert!(!detail.contains("verified"), "cross-signed: {detail}");

    // The assistant's device is signed by its own account, so apps trust it.
    let devices = owner
        .client
        .encryption()
        .get_user_devices(&bot_id)
        .await
        .unwrap();
    let device = devices.devices().next().expect("the assistant's device");
    assert!(device.is_cross_signed_by_owner(), "device is cross-signed");

    // A chat message gets the assistant's reply, formatted.
    say(&room, "Hi!").await;
    owner.wait_for(&bot, "Hello from **your assistant**!").await;

    // An action waits for approval; a 👍 on the prompt approves it.
    say(&room, "Please send Sam a note").await;
    let prompt = owner.wait_for(&bot, "Waiting for you").await;
    assert_eq!(
        writes.load(Ordering::SeqCst),
        0,
        "nothing runs before approval"
    );
    room.send(ReactionEventContent::new(Annotation::new(
        prompt,
        "👍".to_owned(),
    )))
    .await
    .unwrap();
    owner.wait_for(&bot, "Approved").await;
    owner.wait_for(&bot, "Done, I sent it.").await;
    assert_eq!(writes.load(Ordering::SeqCst), 1);

    // In an encrypted chat, a message that isn't encrypted (as a server could forge) is
    // ignored.
    let token = owner.client.access_token().unwrap();
    let plain = reqwest::Client::new()
        .put(format!(
            "{server}/_matrix/client/v3/rooms/{}/send/m.room.message/forged-{run}",
            room.room_id()
        ))
        .bearer_auth(token)
        .json(&json!({"msgtype": "m.text", "body": "Please send Mallory a note"}))
        .send()
        .await
        .unwrap();
    assert!(plain.status().is_success());
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        owner.count(&bot, "Waiting for you"),
        1,
        "{:?}",
        owner.bodies()
    );

    // A bare "yes" answers the only approval waiting.
    say(&room, "Send Sam another note").await;
    eventually("a second prompt", async || {
        owner.count(&bot, "Waiting for you") == 2
    })
    .await;
    say(&room, "yes").await;
    eventually("a second approval", async || {
        writes.load(Ordering::SeqCst) == 2
    })
    .await;

    // A quoted "no" declines the prompt it replies to.
    say(&room, "And a third note").await;
    eventually("a third prompt", async || {
        owner.count(&bot, "Waiting for you") == 3
    })
    .await;
    let third = owner
        .seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(_, s, b)| s == &bot && b.contains("Waiting for you"))
        .unwrap()
        .0
        .clone();
    let mut no = RoomMessageEventContent::text_plain("no");
    no.relates_to = Some(Relation::Reply(ReplyTo::new(InReplyTo::new(third))));
    room.send(no).await.unwrap();
    owner.wait_for(&bot, "Declined").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(writes.load(Ordering::SeqCst), 2, "declined: nothing ran");

    // A reminder arrives here too, and a ✅ marks it done.
    let (status, item) = h
        .call(
            reqwest::Method::POST,
            "/schedule",
            json!({"kind": "reminder", "title": "Water the plants", "schedule": {"type": "daily", "time": "18:00"}}),
        )
        .await;
    assert_eq!(status, 200, "{item}");
    let item_id: uuid::Uuid = item["id"].as_str().unwrap().parse().unwrap();
    let mut stored = crate::schedule::store::get(&h.state.db, item_id)
        .await
        .unwrap()
        .unwrap();
    stored.next_at = Some(crate::now_ms() - 1_000);
    crate::schedule::store::upsert(&h.state.db, stored)
        .await
        .unwrap();
    crate::schedule::tick(&h.state, jiff::Timestamp::now()).await;
    let reminder = owner.wait_for(&bot, "Water the plants").await;
    room.send(ReactionEventContent::new(Annotation::new(
        reminder,
        "✅".to_owned(),
    )))
    .await
    .unwrap();
    owner.wait_for(&bot, "Done").await;
    eventually("the reminder to be done", async || {
        let (_, list) = h
            .call(reqwest::Method::GET, "/schedule/deliveries", Value::Null)
            .await;
        list[0]["status"] == "done"
    })
    .await;

    // /new starts a fresh conversation; everything so far is in a "Matrix" one.
    say(&room, "/new").await;
    owner.wait_for(&bot, "Started a new conversation").await;
    let (_, conversations) = h
        .call(reqwest::Method::GET, "/conversations", Value::Null)
        .await;
    assert!(
        conversations
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["title"] == "Matrix"),
        "{conversations}"
    );

    // After a restart the connection picks up where it was, without replaying anything.
    crate::connections::matrix::restart(&h.state, id).await;
    say(&room, "Hi again!").await;
    eventually("a reply after the restart", async || {
        owner.count(&bot, "Hello from") == 2
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(owner.count(&bot, "Hello from"), 2, "{:?}", owner.bodies());
    assert_eq!(owner.count(&bot, "Started a new conversation"), 1);

    // Once paired, someone else's invitation is declined.
    let other = Owner::sign_in(&server, &stranger, &stranger_password).await;
    let their_room = other.client.create_dm(&bot_id).await.unwrap();
    eventually("the invitation to be declined", async || {
        membership(&their_room, &bot_id).await == Some(MembershipState::Leave)
    })
    .await;
    say(&their_room, "What's on the owner's calendar?").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(other.count(&bot, ""), 0, "strangers get nothing");

    // Removing the connection signs the device out and deletes its keys.
    let row = crate::connections::store::get(&h.state.db, id)
        .await
        .unwrap()
        .unwrap();
    let token = row.config["access_token"].as_str().unwrap().to_owned();
    let (status, _) = h
        .call(
            reqwest::Method::DELETE,
            &format!("/connections/{id}"),
            Value::Null,
        )
        .await;
    assert!(status == 200 || status == 204, "{status}");
    let http = reqwest::Client::new();
    eventually("the device to be signed out", async || {
        http.get(format!("{server}/_matrix/client/v3/account/whoami"))
            .bearer_auth(&token)
            .send()
            .await
            .is_ok_and(|r| r.status() == 401)
    })
    .await;
    eventually("the store to be removed", async || {
        !data.path().join("matrix").join(id.to_string()).exists()
    })
    .await;
}
