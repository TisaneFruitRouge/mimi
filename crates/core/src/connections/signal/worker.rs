//! The Signal client itself, on a thread of its own.
//!
//! presage runs parts of its receive loop with `spawn_local`, and libsignal's stores
//! aren't `Send`, so each Signal connection gets a dedicated thread with a
//! single-threaded runtime. The daemon talks to it through channels: [`Command`]s in,
//! [`Report`]s out. Incoming messages are sorted on this thread; only Note to Self ever
//! leaves it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::oneshot as link_oneshot;
use presage::Manager;
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::prelude::{ServiceError, Uuid};
use presage::libsignal_service::proto::{BodyRange, DataMessage};
use presage::libsignal_service::protocol::{Aci, ServiceId};
use presage::manager::Registered;
use presage::model::messages::Received;
use presage::store::StateStore;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::classify::{Incoming, classify, pictures, recording};
use super::store::{SignalStore, SignalStoreError};
use crate::db::Db;

type SignalManager = Manager<SignalStore, Registered>;
type SignalError = presage::Error<SignalStoreError>;

/// How long codes keep being refreshed while nobody scans one.
const LINK_WINDOW: Duration = Duration::from_secs(60 * 60);
/// How many of Mimi's own message timestamps are remembered, to recognise echoes.
const OWN_SENT: usize = 256;
/// Refusals in a row before the device counts as unlinked (not a passing hiccup).
const REFUSALS: u32 = 3;
/// The Signal thread's stack.
const STACK: usize = 64 * 1024 * 1024;

/// What the worker should do first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Link as a new device: show codes until one is scanned.
    Link,
    /// Use the device already linked.
    Run,
}

/// The account Mimi is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub aci: Uuid,
    /// The user's Signal name, or their number when the profile has none.
    pub name: String,
}

/// What the worker tells the daemon.
#[derive(Debug)]
pub enum Report {
    /// A fresh linking code (the `sgnl://linkdevice` address to show as a QR code).
    Code(String),
    /// Couldn't get a code: Signal is unreachable for now.
    CodeFailed,
    /// Nobody scanned a code for a while, so linking stopped.
    LinkPaused,
    /// Linked to the user's account.
    Linked(Linked),
    /// Connected and caught up with what arrived meanwhile.
    Online,
    /// Lost the connection; retrying.
    Offline,
    /// The device was removed from the phone (or Signal stopped accepting it).
    Unlinked,
    /// Something the user wrote in Note to Self.
    Message(Incoming),
}

/// A message to send to Note to Self, piece by piece: text and its styles.
pub type Pieces = Vec<(String, Vec<BodyRange>)>;

enum Command {
    Send {
        pieces: Pieces,
        /// The timestamps of the messages sent (Signal's ids for them).
        reply: oneshot::Sender<Result<Vec<u64>, String>>,
    },
}

/// A handle on a running worker.
#[derive(Clone)]
pub struct Worker {
    commands: mpsc::UnboundedSender<Command>,
}

impl Worker {
    /// Whether both handles are for the same worker.
    pub fn same(&self, other: &Worker) -> bool {
        self.commands.same_channel(&other.commands)
    }

    /// Sends a message to Note to Self. The timestamps of the messages sent.
    pub async fn send(&self, pieces: Pieces) -> Result<Vec<u64>, String> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Send { pieces, reply })
            .map_err(|_| "Signal isn't running.".to_owned())?;
        answer
            .await
            .map_err(|_| "Signal isn't running.".to_owned())?
    }

    /// A worker that isn't running, for tests.
    #[cfg(test)]
    pub fn detached() -> (Self, mpsc::UnboundedReceiver<Pieces>) {
        let (commands, mut rx) = mpsc::unbounded_channel::<Command>();
        let (tx, out) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut stamp = 1_000;
            while let Some(Command::Send { pieces, reply }) = rx.recv().await {
                let stamps = pieces
                    .iter()
                    .map(|_| {
                        stamp += 1;
                        stamp
                    })
                    .collect();
                let _ = tx.send(pieces);
                let _ = reply.send(Ok(stamps));
            }
        });
        (Self { commands }, out)
    }
}

/// Starts the Signal client for a connection on its own thread.
pub fn spawn(
    db: Db,
    connection: uuid::Uuid,
    device_name: String,
    mode: Mode,
    cancel: CancellationToken,
) -> (Worker, mpsc::UnboundedReceiver<Report>) {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (reports, report_rx) = mpsc::unbounded_channel();
    let started = std::thread::Builder::new()
        .name("signal".to_owned())
        // libsignal's futures are large (several MB unoptimised): the default 2 MB stack
        // overflows. Only the pages used are ever committed.
        .stack_size(STACK)
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    tracing::error!("couldn't start Signal: {e}");
                    return;
                }
            };
            let local = tokio::task::LocalSet::new();
            let client = Client {
                store: SignalStore::new(db, connection),
                reports,
                commands: command_rx,
                cancel,
                sent: VecDeque::new(),
                last_stamp: 0,
            };
            local.block_on(&runtime, client.run(device_name, mode));
        });
    if let Err(e) = started {
        tracing::error!("couldn't start Signal: {e}");
    }
    (Worker { commands }, report_rx)
}

struct Client {
    store: SignalStore,
    reports: mpsc::UnboundedSender<Report>,
    commands: mpsc::UnboundedReceiver<Command>,
    cancel: CancellationToken,
    /// Timestamps of Mimi's own recent messages.
    sent: VecDeque<u64>,
    last_stamp: u64,
}

impl Client {
    fn report(&self, report: Report) {
        let _ = self.reports.send(report);
    }

    /// Sleeps, unless cancelled (then `false`).
    async fn pause(&self, how_long: Duration) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(how_long) => true,
            _ = self.cancel.cancelled() => false,
        }
    }

    async fn run(mut self, device_name: String, mode: Mode) {
        // Signal's client defaults to its own TLS backend unless one is chosen; use the
        // one the rest of the daemon uses.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let manager = match mode {
            Mode::Link => match self.link(&device_name).await {
                Some(manager) => manager,
                None => return,
            },
            Mode::Run if self.store.is_registered().await => {
                match Manager::load_registered(self.store.clone()).await {
                    Ok(manager) => manager,
                    Err(e) => {
                        tracing::warn!("couldn't load the Signal device: {e}");
                        self.report(Report::Unlinked);
                        return;
                    }
                }
            }
            Mode::Run => {
                self.report(Report::LinkPaused);
                return;
            }
        };
        self.serve(manager).await;
    }

    /// Shows codes until one is scanned (or an hour passes).
    async fn link(&mut self, device_name: &str) -> Option<SignalManager> {
        let started = Instant::now();
        let mut backoff = Duration::from_secs(5);
        loop {
            if started.elapsed() > LINK_WINDOW {
                self.report(Report::LinkPaused);
                return None;
            }
            let (tx, rx) = link_oneshot::channel::<url::Url>();
            let mut shown: Option<Instant> = None;
            let reports = self.reports.clone();
            let forward = async {
                if let Ok(url) = rx.await {
                    shown = Some(Instant::now());
                    let _ = reports.send(Report::Code(url.to_string()));
                }
            };
            let linking = Manager::link_secondary_device(
                self.store.clone(),
                SignalServers::Production,
                device_name.to_owned(),
                tx,
            );
            let result = tokio::select! {
                (result, ()) = futures::future::join(linking, forward) => result,
                _ = self.cancel.cancelled() => return None,
            };
            match result {
                Ok(manager) => {
                    let linked = self.linked(&manager).await;
                    self.report(Report::Linked(linked));
                    return Some(manager);
                }
                // A code that was shown and then expired: show a fresh one right away.
                Err(_) if shown.is_some_and(|at| at.elapsed() > Duration::from_secs(20)) => {
                    backoff = Duration::from_secs(5);
                }
                Err(e) => {
                    tracing::warn!("couldn't get a Signal linking code: {}", brief(&e));
                    self.report(Report::CodeFailed);
                    if !self.pause(backoff).await {
                        return None;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
        }
    }

    /// Who the device was linked to.
    async fn linked(&self, manager: &SignalManager) -> Linked {
        let data = manager.registration_data();
        let mut profile_manager = manager.clone();
        let profile =
            tokio::time::timeout(Duration::from_secs(15), profile_manager.retrieve_profile()).await;
        let name = match profile {
            Ok(Ok(profile)) => profile
                .name
                .map(|n| n.given_name)
                .filter(|n| !n.trim().is_empty()),
            _ => None,
        };
        Linked {
            aci: data.service_ids.aci,
            name: name.unwrap_or_else(|| {
                data.phone_number
                    .format()
                    .mode(presage::libsignal_service::prelude::phonenumber::Mode::International)
                    .to_string()
            }),
        }
    }

    /// Receives messages and sends Mimi's, reconnecting whenever the connection drops.
    async fn serve(&mut self, manager: SignalManager) {
        let account = manager.registration_data().service_ids.aci;
        let device = u32::from(manager.device_id());
        let mut sender = manager.clone();
        let mut backoff = Duration::from_secs(2);
        let mut refusals = 0;
        loop {
            let mut receiver = manager.clone();
            let stream = tokio::select! {
                stream = receiver.receive_messages() => stream,
                _ = self.cancel.cancelled() => return,
            };
            let stream = match stream {
                Ok(stream) => stream,
                Err(e) => {
                    if refused(&e) {
                        refusals += 1;
                        if refusals >= REFUSALS {
                            self.report(Report::Unlinked);
                            return;
                        }
                    } else {
                        refusals = 0;
                        tracing::warn!("couldn't connect to Signal: {}", brief(&e));
                    }
                    self.report(Report::Offline);
                    if !self
                        .wait_handling_commands(&mut sender, account, backoff)
                        .await
                    {
                        return;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(5 * 60));
                    continue;
                }
            };
            refusals = 0;
            futures::pin_mut!(stream);
            let mut online = false;
            loop {
                tokio::select! {
                    item = stream.next() => match item {
                        Some(Received::QueueEmpty) => {
                            if !online {
                                online = true;
                                backoff = Duration::from_secs(2);
                                self.report(Report::Online);
                            }
                        }
                        Some(Received::Content(content)) => {
                            let sent = &self.sent;
                            match classify(&content, account, device, |ts| sent.contains(&ts)) {
                                Incoming::Note { text, quote, timestamp, .. } => {
                                    // Only a note's pictures and recording are ever downloaded.
                                    let photos = download(&manager, &pictures(&content, account)).await;
                                    let voice = match recording(&content, account) {
                                        Some(pointer) => download_recording(&manager, &pointer).await,
                                        None => None,
                                    };
                                    if text.is_empty() && photos.is_empty() && voice.is_none() {
                                        continue;
                                    }
                                    self.report(Report::Message(Incoming::Note { text, quote, timestamp, photos, voice }));
                                }
                                incoming @ Incoming::Reaction { .. } => {
                                    self.report(Report::Message(incoming));
                                }
                                Incoming::OwnEcho | Incoming::Ignore => {}
                            }
                        }
                        Some(_) => {}
                        None => break,
                    },
                    command = self.commands.recv() => match command {
                        Some(command) => self.handle(&mut sender, account, command).await,
                        None => return,
                    },
                    _ = self.cancel.cancelled() => return,
                }
            }
            // The connection dropped (servers and networks close idle ones): reconnect.
            if !self
                .wait_handling_commands(&mut sender, account, Duration::from_secs(1))
                .await
            {
                return;
            }
        }
    }

    /// Waits before reconnecting, still sending what the daemon asks meanwhile.
    async fn wait_handling_commands(
        &mut self,
        sender: &mut SignalManager,
        account: Uuid,
        how_long: Duration,
    ) -> bool {
        let deadline = tokio::time::sleep(how_long);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => return true,
                command = self.commands.recv() => match command {
                    Some(command) => self.handle(sender, account, command).await,
                    None => return false,
                },
                _ = self.cancel.cancelled() => return false,
            }
        }
    }

    async fn handle(&mut self, sender: &mut SignalManager, account: Uuid, command: Command) {
        match command {
            Command::Send { pieces, reply } => {
                let result = self.send(sender, account, pieces).await;
                let _ = reply.send(result);
            }
        }
    }

    /// A timestamp for a new message: now, and always after the last one.
    fn stamp(&mut self) -> u64 {
        let now = crate::now_ms() as u64;
        self.last_stamp = now.max(self.last_stamp + 1);
        self.sent.push_back(self.last_stamp);
        while self.sent.len() > OWN_SENT {
            self.sent.pop_front();
        }
        self.last_stamp
    }

    /// Sends to Note to Self: Signal delivers it to the user's other devices as a
    /// message they sent themselves.
    async fn send(
        &mut self,
        sender: &mut SignalManager,
        account: Uuid,
        pieces: Pieces,
    ) -> Result<Vec<u64>, String> {
        let me = ServiceId::Aci(Aci::from(account));
        let mut stamps = Vec::new();
        for (text, ranges) in pieces {
            let timestamp = self.stamp();
            let message = DataMessage {
                body: Some(text),
                body_ranges: ranges,
                timestamp: Some(timestamp),
                ..Default::default()
            };
            if let Err(e) = sender.send_message(me, message, timestamp).await {
                tracing::warn!("sending to Signal failed: {}", brief(&e));
                return Err("Couldn't send to Signal.".to_owned());
            }
            stamps.push(timestamp);
        }
        Ok(stamps)
    }
}

/// Downloads (and decrypts) the pictures of a note. One that can't be fetched is left
/// out: the note still goes through, and the assistant sees what did arrive.
async fn download(
    manager: &SignalManager,
    pointers: &[presage::libsignal_service::proto::AttachmentPointer],
) -> Vec<crate::attachments::Upload> {
    let mut out = Vec::new();
    for pointer in pointers {
        match tokio::time::timeout(Duration::from_secs(90), manager.get_attachment(pointer)).await {
            Ok(Ok(data)) if data.len() <= crate::attachments::MAX_UPLOAD_BYTES => {
                out.push(crate::attachments::Upload::new(
                    data,
                    pointer.file_name.clone(),
                    pointer.content_type.clone(),
                ));
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!("downloading a Signal photo failed: {}", brief(&e)),
            Err(_) => tracing::warn!("downloading a Signal photo timed out"),
        }
    }
    out
}

/// Downloads (and decrypts) a note's voice recording.
async fn download_recording(
    manager: &SignalManager,
    pointer: &presage::libsignal_service::proto::AttachmentPointer,
) -> Option<Vec<u8>> {
    match tokio::time::timeout(Duration::from_secs(90), manager.get_attachment(pointer)).await {
        Ok(Ok(data)) if data.len() <= crate::attachments::MAX_UPLOAD_BYTES => Some(data),
        Ok(Ok(_)) => None,
        Ok(Err(e)) => {
            tracing::warn!("downloading a Signal voice note failed: {}", brief(&e));
            None
        }
        Err(_) => {
            tracing::warn!("downloading a Signal voice note timed out");
            None
        }
    }
}

/// Whether Signal refused the device's credentials: it was unlinked.
fn refused(e: &SignalError) -> bool {
    if matches!(e, presage::Error::ServiceError(ServiceError::Unauthorized)) {
        return true;
    }
    // A refused connection only says so in the websocket handshake error's details.
    refused_text(&format!("{e:?}"))
}

fn refused_text(debug: &str) -> bool {
    debug.contains("UnexpectedStatusCode(401)") || debug.contains("UnexpectedStatusCode(403)")
}

/// An error, for the log: its kind, without anything a server sent back.
fn brief(e: &SignalError) -> String {
    let text = e.to_string();
    // "Unexpected response: HTTP 500: <body>" and the like: keep the part before the body.
    match text.match_indices(": ").nth(1) {
        Some((at, _)) => text[..at].to_owned(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_recognised() {
        assert!(refused(&presage::Error::ServiceError(
            ServiceError::Unauthorized
        )));
        assert!(refused_text(
            "ServiceError(WsError(Handshake(UnexpectedStatusCode(403))))"
        ));
        assert!(!refused_text(
            "ServiceError(WsError(Handshake(UnexpectedStatusCode(502))))"
        ));
        assert!(!refused(&presage::Error::ServiceError(
            ServiceError::Timeout { reason: "slow" }
        )));
    }
}
