//! Address books (CardDAV, RFC 6352) on the same accounts as connected calendars:
//! iCloud, Fastmail and Nextcloud serve contacts with the same app password.

use futures::FutureExt;
use futures::future::BoxFuture;
use reqwest::{Method, Url};

use super::{ContactCard, ContactSource, SourceBatch, vcard};
use crate::AppState;
use crate::connections::calendar::caldav::{CalDav, DavError};
use crate::connections::calendar::{CALDAV, CalDavConfig};

/// Contacts from every connected CalDAV account that also has address books.
pub struct AddressBooks;

impl ContactSource for AddressBooks {
    fn fetch<'a>(&'a self, state: &'a AppState) -> BoxFuture<'a, Vec<SourceBatch>> {
        async move {
            let Ok(rows) = crate::connections::store::list(&state.db).await else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for row in rows.into_iter().filter(|r| r.integration == CALDAV) {
                let Ok(config) = serde_json::from_value::<CalDavConfig>(row.config) else {
                    continue;
                };
                let cards = read_account(&config).await.map_err(|e| e.to_string());
                out.push(SourceBatch {
                    source: row.id.to_string(),
                    cards,
                });
            }
            out
        }
        .boxed()
    }
}

/// Where an account's contacts live when it's not the calendar server itself.
fn contact_server(calendar_server: &Url) -> Url {
    let host = calendar_server.host_str().unwrap_or_default();
    let known = if host.ends_with("icloud.com") {
        Some("https://contacts.icloud.com/")
    } else if host.ends_with("fastmail.com") {
        Some("https://carddav.fastmail.com/")
    } else {
        None
    };
    known
        .and_then(|u| Url::parse(u).ok())
        .unwrap_or_else(|| calendar_server.clone())
}

/// Every card in every address book of the account. An account without address
/// books simply has no contacts.
pub async fn read_account(config: &CalDavConfig) -> Result<Vec<ContactCard>, DavError> {
    let client = CalDav::new(&config.username, &config.password);
    let server = Url::parse(&config.server_url).map_err(|e| DavError::Protocol(e.to_string()))?;
    let Some(books) = address_books(&client, &contact_server(&server)).await? else {
        return Ok(Vec::new());
    };
    let mut cards = Vec::new();
    for book in books {
        cards.extend(read_book(&client, &book).await?);
    }
    Ok(cards)
}

async fn address_books(client: &CalDav, server: &Url) -> Result<Option<Vec<Url>>, DavError> {
    // 1. Who am I?
    let mut principal = None;
    for candidate in [
        server.clone(),
        server.join("/.well-known/carddav").map_err(proto)?,
    ] {
        match client.propfind(&candidate, "0", PROPFIND_PRINCIPAL).await {
            Ok((base, responses)) => {
                if let Some(p) = responses.iter().find_map(|r| r.principal.clone()) {
                    principal = Some(base.join(&p).map_err(proto)?);
                    break;
                }
            }
            Err(DavError::Protocol(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let Some(principal) = principal else {
        return Ok(None);
    };

    // 2. Where are my address books?
    let (base, responses) = client.propfind(&principal, "0", PROPFIND_HOME).await?;
    let Some(home) = responses.iter().find_map(|r| r.addressbook_home.clone()) else {
        return Ok(None);
    };
    let home = base.join(&home).map_err(proto)?;

    // 3. Which collections are address books?
    let (base, responses) = client.propfind(&home, "1", PROPFIND_BOOKS).await?;
    Ok(Some(
        responses
            .into_iter()
            .filter(|r| r.is_addressbook)
            .filter_map(|r| base.join(&r.href).ok())
            .collect(),
    ))
}

/// Lists the book's cards, then fetches them in batches.
async fn read_book(client: &CalDav, book: &Url) -> Result<Vec<ContactCard>, DavError> {
    let (base, listing) = client.propfind(book, "1", PROPFIND_ETAGS).await?;
    let hrefs: Vec<String> = listing
        .into_iter()
        .filter(|r| !r.is_addressbook)
        .filter_map(|r| base.join(&r.href).ok())
        .filter(|u| u.path() != book.path())
        .map(|u| u.path().to_owned())
        .collect();
    let mut cards = Vec::new();
    for chunk in hrefs.chunks(100) {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<C:addressbook-multiget xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
  <D:prop><D:getetag/><C:address-data/></D:prop>
  {}
</C:addressbook-multiget>"#,
            chunk
                .iter()
                .map(|h| format!("<D:href>{}</D:href>", xml_escape(h)))
                .collect::<String>()
        );
        let (_, responses) = client
            .dav(
                Method::from_bytes(b"REPORT").expect("valid method"),
                book,
                "1",
                body,
            )
            .await?;
        for r in responses {
            if let Some(data) = r.address_data {
                cards.extend(vcard::parse(&data, &r.href));
            }
        }
    }
    Ok(cards)
}

const PROPFIND_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:current-user-principal/></D:prop></D:propfind>"#;

const PROPFIND_HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
  <D:prop><C:addressbook-home-set/></D:prop>
</D:propfind>"#;

const PROPFIND_BOOKS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/><D:displayname/></D:prop></D:propfind>"#;

const PROPFIND_ETAGS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/><D:getetag/></D:prop></D:propfind>"#;

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn proto(e: impl std::fmt::Display) -> DavError {
    DavError::Protocol(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_servers() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            contact_server(&u("https://caldav.icloud.com")).as_str(),
            "https://contacts.icloud.com/"
        );
        assert_eq!(
            contact_server(&u("https://caldav.fastmail.com/dav/")).as_str(),
            "https://carddav.fastmail.com/"
        );
        assert_eq!(
            contact_server(&u("https://cloud.example.com/remote.php/dav")).as_str(),
            "https://cloud.example.com/remote.php/dav"
        );
    }
}
