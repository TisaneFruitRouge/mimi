//! Which incoming Signal messages are for the assistant.
//!
//! As a linked device Mimi receives everything on the account: other people's messages,
//! groups, calls, receipts, typing, stories. Only what the user writes in Note to Self
//! counts. Those arrive as "sent" transcripts from another of the user's own devices,
//! addressed to the account itself. Everything else is dropped here, unread and unlogged.

use presage::libsignal_service::content::ContentBody;
use presage::libsignal_service::prelude::{Content, Uuid};
use presage::libsignal_service::proto::{DataMessage, SyncMessage, sync_message};
use presage::libsignal_service::protocol::ServiceId;

/// What an incoming message is, for Mimi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// Something the user wrote in Note to Self. `quote` is the timestamp of the message
    /// it replies to, if any.
    Note {
        text: String,
        quote: Option<u64>,
        timestamp: u64,
    },
    /// A reaction the user put on a message in Note to Self (`target` is its timestamp).
    Reaction { emoji: String, target: u64 },
    /// One of Mimi's own messages coming back.
    OwnEcho,
    /// Anything else: not for the assistant.
    Ignore,
}

fn is_account(id: &ServiceId, account: Uuid) -> bool {
    matches!(id, ServiceId::Aci(_)) && id.raw_uuid() == account
}

/// Sorts an incoming message. `account` is the user's ACI, `device` this device's id and
/// `ours` says whether Mimi sent a message with that timestamp.
pub fn classify(
    content: &Content,
    account: Uuid,
    device: u32,
    ours: impl Fn(u64) -> bool,
) -> Incoming {
    // Only the user's own devices can write in Note to Self.
    if !is_account(&content.metadata.sender, account) {
        return Incoming::Ignore;
    }
    if u32::from(content.metadata.sender_device) == device {
        return Incoming::OwnEcho;
    }
    let (message, timestamp): (&DataMessage, Option<u64>) = match &content.body {
        ContentBody::SynchronizeMessage(SyncMessage {
            content: Some(sync_message::Content::Sent(sent)),
            ..
        }) => {
            // A message the user sent from another device: only the ones to themselves.
            if !sent
                .parse_destination_service_id()
                .is_some_and(|to| is_account(&to, account))
            {
                return Incoming::Ignore;
            }
            match &sent.message {
                Some(message) => (message, sent.timestamp.or(message.timestamp)),
                // Edits and anything without a plain message.
                None => return Incoming::Ignore,
            }
        }
        // Some apps may write to themselves directly rather than as a transcript.
        ContentBody::DataMessage(message) if is_account(&content.metadata.destination, account) => {
            (message, message.timestamp)
        }
        _ => return Incoming::Ignore,
    };
    if message.group_v2.is_some() || message.story_context.is_some() {
        return Incoming::Ignore;
    }
    let timestamp =
        timestamp.unwrap_or_else(|| content.metadata.client_timestamp.timestamp_millis() as u64);
    if ours(timestamp) {
        return Incoming::OwnEcho;
    }
    if let Some(reaction) = &message.reaction {
        return match (&reaction.emoji, reaction.target_sent_timestamp) {
            (Some(emoji), Some(target)) if !reaction.remove() => Incoming::Reaction {
                emoji: emoji.clone(),
                target,
            },
            _ => Incoming::Ignore,
        };
    }
    let text = message.body.as_deref().unwrap_or("").trim();
    if text.is_empty() {
        // Photos, files, stickers and control messages: not for the assistant.
        return Incoming::Ignore;
    }
    Incoming::Note {
        text: text.to_owned(),
        quote: message.quote.as_ref().and_then(|q| q.id),
        timestamp,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use presage::libsignal_service::content::Metadata;
    use presage::libsignal_service::proto::data_message::{Quote, Reaction};
    use presage::libsignal_service::proto::{
        CallMessage, EditMessage, GroupContextV2, ReceiptMessage, StoryMessage, TypingMessage,
    };
    use presage::libsignal_service::protocol::{Aci, DeviceId};

    use super::*;

    const ME: Uuid = Uuid::from_u128(0x1111);
    const FRIEND: Uuid = Uuid::from_u128(0x2222);
    const PHONE: u32 = 1;
    const MIMI: u32 = 3;

    fn aci(uuid: Uuid) -> ServiceId {
        ServiceId::Aci(Aci::from(uuid))
    }

    fn content(sender: Uuid, device: u32, body: impl Into<ContentBody>) -> Content {
        Content {
            metadata: Metadata {
                sender: aci(sender),
                destination: aci(ME),
                sender_device: DeviceId::try_from(device).unwrap(),
                pni_verified: None,
                client_timestamp: chrono::Utc.timestamp_millis_opt(1_000).unwrap(),
                server_timestamp: chrono::Utc.timestamp_millis_opt(1_000).unwrap(),
                needs_receipt: false,
                unidentified_sender: false,
                was_plaintext: false,
                server_guid: None,
            },
            body: body.into(),
        }
    }

    fn text(body: &str) -> DataMessage {
        DataMessage {
            body: Some(body.to_owned()),
            timestamp: Some(42),
            ..Default::default()
        }
    }

    fn sent_to(to: Uuid, message: DataMessage) -> SyncMessage {
        SyncMessage {
            content: Some(sync_message::Content::Sent(sync_message::Sent {
                destination_service_id: Some(aci(to).service_id_string()),
                timestamp: message.timestamp,
                message: Some(message),
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    fn sort(content: &Content) -> Incoming {
        classify(content, ME, MIMI, |ts| ts == 7)
    }

    #[test]
    fn note_to_self_from_the_phone_is_for_the_assistant() {
        let mut message = text("  what's on today? ");
        message.quote = Some(Quote {
            id: Some(99),
            ..Default::default()
        });
        assert_eq!(
            sort(&content(ME, PHONE, sent_to(ME, message))),
            Incoming::Note {
                text: "what's on today?".into(),
                quote: Some(99),
                timestamp: 42
            }
        );
        // From a desktop app linked to the same account, too.
        assert!(matches!(
            sort(&content(ME, 2, sent_to(ME, text("hi")))),
            Incoming::Note { .. }
        ));
    }

    #[test]
    fn reactions_in_note_to_self() {
        let reaction = |remove| DataMessage {
            reaction: Some(Reaction {
                emoji: Some("👍".into()),
                remove: Some(remove),
                target_sent_timestamp: Some(5),
                ..Default::default()
            }),
            timestamp: Some(43),
            ..Default::default()
        };
        assert_eq!(
            sort(&content(ME, PHONE, sent_to(ME, reaction(false)))),
            Incoming::Reaction {
                emoji: "👍".into(),
                target: 5
            }
        );
        assert_eq!(
            sort(&content(ME, PHONE, sent_to(ME, reaction(true)))),
            Incoming::Ignore
        );
    }

    #[test]
    fn mimis_own_messages_are_not_input() {
        assert_eq!(
            sort(&content(ME, MIMI, sent_to(ME, text("hi")))),
            Incoming::OwnEcho
        );
        let mut echo = text("hi");
        echo.timestamp = Some(7);
        assert_eq!(
            sort(&content(ME, PHONE, sent_to(ME, echo))),
            Incoming::OwnEcho
        );
    }

    #[test]
    fn everything_else_is_ignored() {
        let ignored = [
            // Someone else writing to the user.
            content(FRIEND, PHONE, text("hey, it's Sam")),
            // The user writing to someone else from their phone.
            content(ME, PHONE, sent_to(FRIEND, text("see you at 8"))),
            // Groups, even when the user wrote it.
            content(
                ME,
                PHONE,
                sent_to(
                    ME,
                    DataMessage {
                        group_v2: Some(GroupContextV2::default()),
                        ..text("group chat")
                    },
                ),
            ),
            content(
                FRIEND,
                PHONE,
                DataMessage {
                    group_v2: Some(GroupContextV2::default()),
                    ..text("group chat")
                },
            ),
            // Receipts, typing, calls, stories, edits.
            content(FRIEND, PHONE, ReceiptMessage::default()),
            content(ME, PHONE, ReceiptMessage::default()),
            content(FRIEND, PHONE, TypingMessage::default()),
            content(FRIEND, PHONE, CallMessage::default()),
            content(FRIEND, PHONE, StoryMessage::default()),
            content(ME, PHONE, EditMessage::default()),
            // Other sync messages (read receipts, contacts…).
            content(ME, PHONE, SyncMessage::default()),
            // A photo saved to Note to Self, with no text.
            content(ME, PHONE, sent_to(ME, DataMessage::default())),
        ];
        for message in &ignored {
            assert_eq!(sort(message), Incoming::Ignore, "{:?}", message.body);
        }
    }
}
