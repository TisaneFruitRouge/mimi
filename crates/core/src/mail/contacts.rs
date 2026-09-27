//! People the user corresponds with by email, as a contact source for the directory.
//! Cards carry only an email handle, so they join an existing person only through a
//! shared address (the directory never merges on names).

use futures::FutureExt;
use futures::future::BoxFuture;
use mimi_protocol::Channel;

use super::store;
use crate::AppState;
use crate::people::{CardHandle, ContactCard, ContactSource, SourceBatch};

pub struct Correspondents;

impl ContactSource for Correspondents {
    fn fetch<'a>(&'a self, state: &'a AppState) -> BoxFuture<'a, Vec<SourceBatch>> {
        async move {
            let accounts = super::accounts(state).await;
            let me = super::my_addresses(state).await;
            let mut out = Vec::new();
            for account in accounts {
                let me = me.clone();
                let id = account.id;
                let cards = state
                    .db
                    .call(move |c| store::correspondents(c, id, &me))
                    .await
                    .map(|people| people.into_iter().map(card).collect())
                    .map_err(|e| e.to_string());
                out.push(SourceBatch {
                    source: account.id.to_string(),
                    cards,
                });
            }
            out
        }
        .boxed()
    }
}

fn card((email, name, _count): (String, Option<String>, u32)) -> ContactCard {
    ContactCard {
        // The address is the card's stable id within the account.
        record: email.clone(),
        name: name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| email.clone()),
        nickname: None,
        handles: vec![CardHandle {
            channel: Channel::Email,
            value: email,
            label: None,
        }],
    }
}
