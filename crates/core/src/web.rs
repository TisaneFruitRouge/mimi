//! Browser access: one-time login links and cookie sessions.
//!
//! Native clients (desktop app, CLI) authenticate with the bearer token from the
//! discovery file. A browser can't read that file, so a native client asks the daemon
//! for a single-use login link; opening it trades the code for a session cookie.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use crate::db::{Db, DbError};
use crate::fsutil::random_hex;
use crate::now_ms;

pub const SESSION_COOKIE: &str = "hearth_session";
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const LOGIN_CODE_TTL: Duration = Duration::from_secs(120);

/// Unredeemed login codes, in memory only: a restart invalidates them.
#[derive(Default)]
pub struct LoginCodes(Mutex<HashMap<String, Instant>>);

impl LoginCodes {
    pub fn issue(&self) -> anyhow::Result<String> {
        let code = random_hex(16)?;
        let mut codes = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        codes.retain(|_, expires| *expires > now);
        codes.insert(code.clone(), now + LOGIN_CODE_TTL);
        Ok(code)
    }

    /// Consumes the code. True if it existed and hadn't expired.
    pub fn redeem(&self, code: &str) -> bool {
        let mut codes = self.0.lock().unwrap_or_else(|e| e.into_inner());
        codes
            .remove(code)
            .is_some_and(|expires| Instant::now() < expires)
    }

    #[cfg(test)]
    pub fn insert_expired(&self, code: &str) {
        let mut codes = self.0.lock().unwrap();
        codes.insert(code.to_owned(), Instant::now());
    }
}

pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Creates a session and returns its token (to be sent to the browser, never stored).
pub async fn create_session(db: &Db) -> anyhow::Result<String> {
    let token = random_hex(32)?;
    let hash = hash_token(&token);
    let now = now_ms();
    let expires = now + SESSION_TTL.as_millis() as i64;
    db.call(move |c| {
        c.execute("DELETE FROM web_sessions WHERE expires_at <= ?1", [now])?;
        c.execute(
            "INSERT INTO web_sessions (token_hash, created_at, expires_at) VALUES (?1, ?2, ?3)",
            (hash, now, expires),
        )
    })
    .await?;
    Ok(token)
}

pub async fn session_valid(db: &Db, token: &str) -> Result<bool, DbError> {
    let hash = hash_token(token);
    let now = now_ms();
    db.call(move |c| {
        c.query_row(
            "SELECT 1 FROM web_sessions WHERE token_hash = ?1 AND expires_at > ?2",
            (hash, now),
            |_| Ok(()),
        )
        .optional()
    })
    .await
    .map(|found| found.is_some())
}

pub async fn delete_session(db: &Db, token: &str) -> Result<(), DbError> {
    let hash = hash_token(token);
    db.call(move |c| c.execute("DELETE FROM web_sessions WHERE token_hash = ?1", [hash]))
        .await
        .map(|_| ())
}

/// The value of cookie `name` in a `Cookie` header.
pub fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then_some(v)
    })
}

/// Hosts a browser may use to reach the daemon. Anything else is refused for cookie
/// auth, which stops DNS-rebinding pages from riding on the session.
pub fn allowed_host(host: &str, port: u16) -> bool {
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

pub fn session_cookie(token: &str) -> String {
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

pub fn cleared_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_single_use_and_expire() {
        let codes = LoginCodes::default();
        let code = codes.issue().unwrap();
        assert!(codes.redeem(&code));
        assert!(!codes.redeem(&code));
        codes.insert_expired("old");
        assert!(!codes.redeem("old"));
        assert!(!codes.redeem("never-issued"));
    }

    #[test]
    fn parses_cookies_and_hosts() {
        assert_eq!(
            cookie("a=1; hearth_session=xyz; b=2", SESSION_COOKIE),
            Some("xyz")
        );
        assert_eq!(cookie("a=1", SESSION_COOKIE), None);
        assert!(allowed_host("127.0.0.1:7437", 7437));
        assert!(allowed_host("localhost:7437", 7437));
        assert!(!allowed_host("evil.example:7437", 7437));
        assert!(!allowed_host("127.0.0.1:8000", 7437));
    }
}
