//! A small CalDAV client (RFC 4791): find a user's calendars, read events in a range,
//! create events. Works with iCloud, Fastmail, Nextcloud, Radicale and friends using an
//! app-specific password.

use std::time::Duration;

use chrono::{DateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::Event as Xml;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CalDav {
    http: reqwest::Client,
    username: String,
    password: String,
}

/// A calendar collection on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCalendar {
    pub url: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DavError {
    #[error(
        "The server rejected the username or password. For iCloud and Fastmail, use an app-specific password."
    )]
    Unauthorized,
    #[error("Couldn't reach the calendar server: {0}")]
    Unreachable(String),
    #[error("The calendar server answered unexpectedly: {0}")]
    Protocol(String),
}

/// One response in a WebDAV multistatus body, flattened to what we use.
#[derive(Debug, Default, Clone)]
struct DavResponse {
    href: String,
    is_calendar: bool,
    supports_events: bool,
    display_name: Option<String>,
    color: Option<String>,
    principal: Option<String>,
    calendar_home: Option<String>,
    calendar_data: Option<String>,
}

impl CalDav {
    pub fn new(username: &str, password: &str) -> Self {
        // Redirects are followed by hand: reqwest would turn a redirected PROPFIND into
        // a GET, and CalDAV discovery depends on redirects (/.well-known/caldav).
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("hearth/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("building the CalDAV client");
        Self {
            http,
            username: username.to_owned(),
            password: password.to_owned(),
        }
    }

    /// Finds the calendars of the account behind `server`, which can be the server's
    /// root, its `/.well-known/caldav`, a principal or a calendar-home URL.
    pub async fn discover(&self, server: &Url) -> Result<Vec<RemoteCalendar>, DavError> {
        // 1. Who am I? Try the given URL, then the well-known location.
        let mut principal = None;
        for candidate in [
            server.clone(),
            server.join("/.well-known/caldav").map_err(proto)?,
        ] {
            let (base, responses) = self
                .propfind(&candidate, "0", PROPFIND_PRINCIPAL)
                .await
                .or_else(|e| match e {
                    DavError::Protocol(_) => Ok((candidate.clone(), Vec::new())),
                    other => Err(other),
                })?;
            if let Some(p) = responses.iter().find_map(|r| r.principal.clone()) {
                principal = Some(base.join(&p).map_err(proto)?);
                break;
            }
        }
        let principal = principal.ok_or_else(|| {
            DavError::Protocol("this doesn't look like a CalDAV server address".to_owned())
        })?;

        // 2. Where are my calendars?
        let (base, responses) = self.propfind(&principal, "0", PROPFIND_HOME).await?;
        let home = responses
            .iter()
            .find_map(|r| r.calendar_home.clone())
            .ok_or_else(|| DavError::Protocol("no calendar home for this account".to_owned()))?;
        let home = base.join(&home).map_err(proto)?;

        // 3. Which of those hold events?
        let (base, responses) = self.propfind(&home, "1", PROPFIND_CALENDARS).await?;
        let mut calendars: Vec<RemoteCalendar> = responses
            .into_iter()
            .filter(|r| r.is_calendar && r.supports_events)
            .filter_map(|r| {
                let url = base.join(&r.href).ok()?;
                let fallback = url
                    .path_segments()?
                    .rfind(|s| !s.is_empty())
                    .unwrap_or("Calendar")
                    .to_owned();
                Some(RemoteCalendar {
                    url: url.to_string(),
                    name: r
                        .display_name
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or(fallback),
                    color: r.color.map(|c| c.chars().take(7).collect()),
                })
            })
            .collect();
        calendars.sort_by_key(|c| c.name.to_lowercase());
        Ok(calendars)
    }

    /// The raw iCalendar data of every event object overlapping the range. Recurring
    /// events come back whole; the caller expands them.
    pub async fn event_data(
        &self,
        calendar: &Url,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<String>, DavError> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop><C:calendar-data/></D:prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VEVENT">
        <C:time-range start="{}" end="{}"/>
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>"#,
            from.format("%Y%m%dT%H%M%SZ"),
            to.format("%Y%m%dT%H%M%SZ")
        );
        let (_, responses) = self
            .dav(Method::from_bytes(b"REPORT").unwrap(), calendar, "1", body)
            .await?;
        Ok(responses
            .into_iter()
            .filter_map(|r| r.calendar_data)
            .collect())
    }

    /// Stores a new event object. `ics` must be a complete VCALENDAR with one VEVENT.
    pub async fn create(&self, calendar: &Url, uid: &str, ics: String) -> Result<(), DavError> {
        let url = calendar
            .join(&format!("{}.ics", sanitize(uid)))
            .map_err(proto)?;
        let res = self
            .http
            .put(url)
            .basic_auth(&self.username, Some(&self.password))
            .header(CONTENT_TYPE, "text/calendar; charset=utf-8")
            .header("If-None-Match", "*")
            .body(ics)
            .send()
            .await
            .map_err(unreachable)?;
        match res.status() {
            s if s.is_success() => Ok(()),
            StatusCode::UNAUTHORIZED => Err(DavError::Unauthorized),
            s => Err(DavError::Protocol(format!("saving the event failed ({s})"))),
        }
    }

    async fn propfind(
        &self,
        url: &Url,
        depth: &str,
        body: &str,
    ) -> Result<(Url, Vec<DavResponse>), DavError> {
        self.dav(
            Method::from_bytes(b"PROPFIND").unwrap(),
            url,
            depth,
            body.to_owned(),
        )
        .await
    }

    /// Sends a WebDAV request, following redirects with the same method. Returns the
    /// final URL (for resolving relative hrefs) and the parsed multistatus.
    async fn dav(
        &self,
        method: Method,
        url: &Url,
        depth: &str,
        body: String,
    ) -> Result<(Url, Vec<DavResponse>), DavError> {
        let mut url = url.clone();
        for _ in 0..5 {
            let mut headers = HeaderMap::new();
            headers.insert(
                "Depth",
                HeaderValue::from_str(depth).expect("depth is ASCII"),
            );
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/xml; charset=utf-8"),
            );
            let res = self
                .http
                .request(method.clone(), url.clone())
                .basic_auth(&self.username, Some(&self.password))
                .headers(headers)
                .body(body.clone())
                .send()
                .await
                .map_err(unreachable)?;
            let status = res.status();
            if status.is_redirection() {
                let location = res
                    .headers()
                    .get(LOCATION)
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| DavError::Protocol("redirect without a location".to_owned()))?;
                let next = url.join(location).map_err(proto)?;
                // Never let a redirect downgrade to plain HTTP, which would leak the password.
                if url.scheme() == "https" && next.scheme() != "https" {
                    return Err(DavError::Protocol(
                        "the server redirected to an insecure address".to_owned(),
                    ));
                }
                url = next;
                continue;
            }
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err(DavError::Unauthorized);
            }
            if status != StatusCode::MULTI_STATUS {
                return Err(DavError::Protocol(format!("{status}")));
            }
            let text = res.text().await.map_err(unreachable)?;
            return Ok((url, parse_multistatus(&text)?));
        }
        Err(DavError::Protocol("too many redirects".to_owned()))
    }
}

const PROPFIND_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:current-user-principal/></D:prop></D:propfind>"#;

const PROPFIND_HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop><C:calendar-home-set/></D:prop>
</D:propfind>"#;

const PROPFIND_CALENDARS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:A="http://apple.com/ns/ical/">
  <D:prop>
    <D:resourcetype/>
    <D:displayname/>
    <A:calendar-color/>
    <C:supported-calendar-component-set/>
  </D:prop>
</D:propfind>"#;

/// Parses a multistatus body by local element names; servers disagree on prefixes.
fn parse_multistatus(xml: &str) -> Result<Vec<DavResponse>, DavError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<DavResponse> = None;
    let mut path: Vec<String> = Vec::new();
    // Text of the innermost open element; entity references arrive as separate events.
    let mut text = String::new();
    let mut saw_comp_list = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| DavError::Protocol(format!("unreadable XML: {e}")))?;
        match event {
            Xml::Start(e) => {
                let name = local(e.name().as_ref());
                if name == "response" {
                    current = Some(DavResponse::default());
                    saw_comp_list = false;
                }
                path.push(name);
                text.clear();
            }
            Xml::Empty(e) => {
                let name = local(e.name().as_ref());
                if let Some(r) = current.as_mut() {
                    let parent = path.last().map(String::as_str);
                    if name == "calendar" && parent == Some("resourcetype") {
                        r.is_calendar = true;
                    }
                    if name == "comp" && parent == Some("supported-calendar-component-set") {
                        saw_comp_list = true;
                        r.supports_events |= e.attributes().flatten().any(|a| {
                            local(a.key.as_ref()) == "name"
                                && a.value.eq_ignore_ascii_case("VEVENT")
                        });
                    }
                }
            }
            Xml::Text(t) => text.push_str(&t.into_inner()),
            Xml::CData(t) => text.push_str(&t.into_inner()),
            Xml::GeneralRef(r) => {
                let reference = format!("&{};", r.into_inner());
                match quick_xml::escape::unescape(&reference) {
                    Ok(resolved) => text.push_str(&resolved),
                    Err(_) => text.push_str(&reference),
                }
            }
            Xml::End(e) => {
                let name = local(e.name().as_ref());
                record_text(current.as_mut(), &path, std::mem::take(&mut text));
                path.pop();
                if name == "response"
                    && let Some(mut r) = current.take()
                {
                    // No component list means the server allows every component.
                    if !saw_comp_list {
                        r.supports_events = true;
                    }
                    out.push(r);
                }
            }
            Xml::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn record_text(current: Option<&mut DavResponse>, path: &[String], text: String) {
    let Some(r) = current else { return };
    let text = if path.last().is_some_and(|p| p == "calendar-data") {
        text
    } else {
        text.trim().to_owned()
    };
    if text.is_empty() {
        return;
    }
    let here = path.last().map(String::as_str);
    let parent = path
        .len()
        .checked_sub(2)
        .and_then(|i| path.get(i))
        .map(String::as_str);
    match (parent, here) {
        (Some("response"), Some("href")) => r.href = text,
        (Some("current-user-principal"), Some("href")) => r.principal = Some(text),
        (Some("calendar-home-set"), Some("href")) => r.calendar_home = Some(text),
        (_, Some("displayname")) => r.display_name = Some(text),
        (_, Some("calendar-color")) => r.color = Some(text),
        (_, Some("calendar-data")) => r.calendar_data = Some(text),
        _ => {}
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

fn sanitize(uid: &str) -> String {
    uid.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn proto(e: impl std::fmt::Display) -> DavError {
    DavError::Protocol(e.to_string())
}

fn unreachable(e: reqwest::Error) -> DavError {
    DavError::Unreachable(
        e.url()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "the server".to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calendar_listing_with_mixed_prefixes() {
        let xml = r##"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:x1="http://apple.com/ns/ical/">
  <d:response><d:href>/dav/calendars/me/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/calendars/me/personal/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      <d:displayname>Personal</d:displayname>
      <x1:calendar-color>#E8A33DFF</x1:calendar-color>
      <cal:supported-calendar-component-set><cal:comp name="VEVENT"/></cal:supported-calendar-component-set>
    </d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/calendars/me/tasks/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      <d:displayname>Tasks</d:displayname>
      <cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
    </d:prop></d:propstat></d:response>
</d:multistatus>"##;
        let r = parse_multistatus(xml).unwrap();
        let calendars: Vec<_> = r
            .iter()
            .filter(|r| r.is_calendar && r.supports_events)
            .collect();
        assert_eq!(calendars.len(), 1);
        assert_eq!(calendars[0].display_name.as_deref(), Some("Personal"));
        assert_eq!(calendars[0].color.as_deref(), Some("#E8A33DFF"));
    }

    #[test]
    fn parses_principal_and_calendar_data() {
        let xml = r#"<multistatus xmlns="DAV:"><response><href>/</href><propstat><prop>
          <current-user-principal><href>/principals/me/</href></current-user-principal>
          </prop></propstat></response></multistatus>"#;
        assert_eq!(
            parse_multistatus(xml).unwrap()[0].principal.as_deref(),
            Some("/principals/me/")
        );

        let xml = r#"<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav"><D:response>
          <D:href>/cal/a.ics</D:href><D:propstat><D:prop><C:calendar-data>BEGIN:VCALENDAR&#13;
END:VCALENDAR</C:calendar-data></D:prop></D:propstat></D:response></D:multistatus>"#;
        let data = parse_multistatus(xml).unwrap()[0]
            .calendar_data
            .clone()
            .unwrap();
        assert!(data.starts_with("BEGIN:VCALENDAR"));
    }
}
