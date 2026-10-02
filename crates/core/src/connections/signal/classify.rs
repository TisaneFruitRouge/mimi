//! Which incoming Signal messages are for the assistant.
//!
//! As a linked device Mimi receives everything on the account: other people's messages,
//! groups, calls, receipts, typing, stories. Only what the user writes in Note to Self
//! counts. Those arrive as "sent" transcripts from another of the user's own devices,
//! addressed to the account itself. Everything else is dropped here, unread and unlogged.

use presage::libsignal_service::content::ContentBody;
use presage::libsignal_service::prelude::{Content, Uuid};
use presage::libsignal_service::proto::{
    AttachmentPointer, DataMessage, SyncMessage, sync_message,
};
use presage::libsignal_service::protocol::ServiceId;

/// What an incoming message is, for Mimi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// Something the user wrote or said in Note to Self. `quote` is the timestamp of the
    /// message it replies to, if any. `photos` and `voice` are filled in by the worker,
    /// which downloads the message's [`pictures`] and [`recording`]; `text` may be empty
    /// when there are some.
    Note {
        text: String,
        quote: Option<u64>,
        timestamp: u64,
        photos: Vec<crate::attachments::Upload>,
        voice: Option<Vec<u8>>,
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
    let Some((message, timestamp)) = note_message(content, account) else {
        return Incoming::Ignore;
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
    if text.is_empty() && pictures_in(message).is_empty() && recording_in(message).is_none() {
        // Files, stickers and control messages: not for the assistant.
        return Incoming::Ignore;
    }
    Incoming::Note {
        text: text.to_owned(),
        quote: message.quote.as_ref().and_then(|q| q.id),
        timestamp,
        photos: Vec::new(),
        voice: None,
    }
}

/// The voice note (or other recording) of a message [`classify`] found to be a note, to
/// download, if it isn't larger than may be downloaded.
pub fn recording(content: &Content, account: Uuid) -> Option<AttachmentPointer> {
    note_message(content, account).and_then(|(message, _)| recording_in(message))
}

fn recording_in(message: &DataMessage) -> Option<AttachmentPointer> {
    message
        .attachments
        .iter()
        .find(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|t| t.starts_with("audio/"))
                && a.size
                    .is_some_and(|n| n as usize <= crate::attachments::MAX_UPLOAD_BYTES)
        })
        .cloned()
}

/// The pictures of a message [`classify`] found to be a note, to download: at most as
/// many as a message may have, none larger than may be downloaded.
pub fn pictures(content: &Content, account: Uuid) -> Vec<AttachmentPointer> {
    match note_message(content, account) {
        Some((message, _)) => pictures_in(message),
        None => Vec::new(),
    }
}

fn pictures_in(message: &DataMessage) -> Vec<AttachmentPointer> {
    message
        .attachments
        .iter()
        .filter(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|t| t.starts_with("image/"))
                && a.size
                    .is_some_and(|n| n as usize <= crate::attachments::MAX_UPLOAD_BYTES)
        })
        .take(crate::attachments::MAX_PER_MESSAGE)
        .cloned()
        .collect()
}

/// The message the user wrote to themselves, with its timestamp, if that's what it is.
fn note_message(content: &Content, account: Uuid) -> Option<(&DataMessage, Option<u64>)> {
    Some(match &content.body {
        ContentBody::SynchronizeMessage(SyncMessage {
            content: Some(sync_message::Content::Sent(sent)),
            ..
        }) => {
            // A message the user sent from another device: only the ones to themselves.
            if !sent
                .parse_destination_service_id()
                .is_some_and(|to| is_account(&to, account))
            {
                return None;
            }
            // Edits and anything without a plain message have none.
            let message = sent.message.as_ref()?;
            (message, sent.timestamp.or(message.timestamp))
        }
        // Some apps may write to themselves directly rather than as a transcript.
        ContentBody::DataMessage(message) if is_account(&content.metadata.destination, account) => {
            (message, message.timestamp)
        }
        _ => return None,
    })
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
                timestamp: 42,
                photos: Vec::new(),
                voice: None,
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
            // A file saved to Note to Self, with no text.
            content(ME, PHONE, sent_to(ME, DataMessage::default())),
            content(
                ME,
                PHONE,
                sent_to(
                    ME,
                    DataMessage {
                        attachments: vec![attachment("application/pdf", 1000)],
                        ..DataMessage::default()
                    },
                ),
            ),
            // Someone else's voice note.
            content(
                FRIEND,
                PHONE,
                DataMessage {
                    attachments: vec![attachment("audio/aac", 1000)],
                    ..DataMessage::default()
                },
            ),
            // Someone else's photo.
            content(
                FRIEND,
                PHONE,
                DataMessage {
                    attachments: vec![attachment("image/jpeg", 1000)],
                    ..DataMessage::default()
                },
            ),
        ];
        for message in &ignored {
            assert_eq!(sort(message), Incoming::Ignore, "{:?}", message.body);
        }
    }

    fn attachment(content_type: &str, size: u32) -> AttachmentPointer {
        AttachmentPointer {
            content_type: Some(content_type.to_owned()),
            size: Some(size),
            ..Default::default()
        }
    }

    #[test]
    fn photos_in_note_to_self_are_for_the_assistant() {
        let photos = DataMessage {
            attachments: vec![
                attachment("image/jpeg", 2_000_000),
                attachment("application/pdf", 1000),
                attachment("image/png", 300_000),
                // Too large to download.
                attachment("image/jpeg", 50_000_000),
            ],
            timestamp: Some(44),
            ..Default::default()
        };
        let note = content(ME, PHONE, sent_to(ME, photos.clone()));
        assert_eq!(
            sort(&note),
            Incoming::Note {
                text: String::new(),
                quote: None,
                timestamp: 44,
                photos: Vec::new(),
                voice: None,
            }
        );
        let found = pictures(&note, ME);
        assert_eq!(
            found
                .iter()
                .map(|a| a.content_type.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["image/jpeg", "image/png"]
        );
        // With a caption, the words come along.
        let captioned = DataMessage {
            body: Some("Add these to the calendar".into()),
            ..photos
        };
        assert!(matches!(
            sort(&content(ME, PHONE, sent_to(ME, captioned))),
            Incoming::Note { text, .. } if text == "Add these to the calendar"
        ));
        // Nothing to download from anyone else's message.
        let theirs = content(
            ME,
            PHONE,
            sent_to(
                FRIEND,
                DataMessage {
                    attachments: vec![attachment("image/jpeg", 1000)],
                    ..Default::default()
                },
            ),
        );
        assert!(pictures(&theirs, ME).is_empty());
    }

    #[test]
    fn voice_notes_in_note_to_self_are_for_the_assistant() {
        let said = DataMessage {
            attachments: vec![attachment("audio/aac", 40_000)],
            timestamp: Some(45),
            ..Default::default()
        };
        let note = content(ME, PHONE, sent_to(ME, said));
        assert!(matches!(sort(&note), Incoming::Note { text, .. } if text.is_empty()));
        assert_eq!(
            recording(&note, ME).unwrap().content_type.as_deref(),
            Some("audio/aac")
        );
        // Too long to download, or someone else's: nothing.
        let huge = content(
            ME,
            PHONE,
            sent_to(
                ME,
                DataMessage {
                    attachments: vec![attachment("audio/aac", 90_000_000)],
                    ..Default::default()
                },
            ),
        );
        assert_eq!(sort(&huge), Incoming::Ignore);
        let theirs = content(
            ME,
            PHONE,
            sent_to(
                FRIEND,
                DataMessage {
                    attachments: vec![attachment("audio/aac", 1000)],
                    ..Default::default()
                },
            ),
        );
        assert!(recording(&theirs, ME).is_none());
    }
}
