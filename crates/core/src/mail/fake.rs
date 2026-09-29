//! A small fake mail server for tests: IMAP (the commands Mimi uses, IDLE included) and
//! SMTP (EHLO, AUTH PLAIN/LOGIN, MAIL, RCPT, DATA), plain text on 127.0.0.1. Tests
//! deliver messages, renumber mailboxes and read what was sent through it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

#[derive(Debug, Clone)]
pub struct Stored {
    pub uid: u32,
    pub flags: BTreeSet<String>,
    /// Milliseconds since the epoch.
    pub internal_date: i64,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Mailbox {
    pub name: String,
    /// `\Sent`, `\Archive`…; empty for the inbox.
    pub special: &'static str,
    pub uid_validity: u32,
    pub uid_next: u32,
    pub messages: Vec<Stored>,
}

/// One message received over SMTP.
#[derive(Debug, Clone)]
pub struct Sent {
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
}

#[derive(Debug)]
pub struct State {
    pub user: String,
    pub password: String,
    pub mailboxes: Vec<Mailbox>,
    pub sent: Vec<Sent>,
    pub logins: u32,
    pub idle: bool,
    /// A UID whose whole message the server refuses to hand over (a failing fetch).
    pub unreadable: Option<u32>,
    /// Whole messages handed over so far.
    pub bodies_sent: u32,
    /// Hang up on the next connection that's idling, as servers and routers do.
    pub drop_idle: bool,
}

pub struct FakeMail {
    pub state: Mutex<State>,
    pub imap_port: u16,
    pub smtp_port: u16,
}

fn mailbox(name: &str, special: &'static str) -> Mailbox {
    Mailbox {
        name: name.to_owned(),
        special,
        uid_validity: 1,
        uid_next: 1,
        messages: Vec::new(),
    }
}

impl FakeMail {
    /// Starts both servers on free ports.
    pub async fn start(user: &str, password: &str) -> Arc<Self> {
        Self::start_on(user, password, 0, 0).await
    }

    pub async fn start_on(user: &str, password: &str, imap_port: u16, smtp_port: u16) -> Arc<Self> {
        let imap = TcpListener::bind(("127.0.0.1", imap_port)).await.unwrap();
        let smtp = TcpListener::bind(("127.0.0.1", smtp_port)).await.unwrap();
        let fake = Arc::new(Self {
            state: Mutex::new(State {
                user: user.to_owned(),
                password: password.to_owned(),
                mailboxes: vec![
                    mailbox("INBOX", ""),
                    mailbox("Sent", "\\Sent"),
                    mailbox("Archive", "\\Archive"),
                    mailbox("Junk", "\\Junk"),
                    mailbox("Trash", "\\Trash"),
                ],
                sent: Vec::new(),
                logins: 0,
                idle: true,
                unreadable: None,
                bodies_sent: 0,
                drop_idle: false,
            }),
            imap_port: imap.local_addr().unwrap().port(),
            smtp_port: smtp.local_addr().unwrap().port(),
        });
        let f = fake.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = imap.accept().await {
                let f = f.clone();
                tokio::spawn(async move {
                    let (r, w) = stream.into_split();
                    let _ = f.imap_session(BufReader::new(r), w).await;
                });
            }
        });
        let f = fake.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = smtp.accept().await {
                let f = f.clone();
                tokio::spawn(async move {
                    let (r, w) = stream.into_split();
                    let _ = f.smtp_session(BufReader::new(r), w).await;
                });
            }
        });
        fake
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts a message in a mailbox. Returns its UID.
    pub fn deliver(&self, mailbox: &str, raw: &str, internal_date: i64, flags: &[&str]) -> u32 {
        let mut st = self.lock();
        let mb = st.mailboxes.iter_mut().find(|m| m.name == mailbox).unwrap();
        let uid = mb.uid_next;
        mb.uid_next += 1;
        mb.messages.push(Stored {
            uid,
            flags: flags.iter().map(|f| (*f).to_owned()).collect(),
            internal_date,
            raw: raw.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes(),
        });
        uid
    }

    /// Gives every message of a mailbox a new UID and a new UIDVALIDITY, as servers do
    /// after rebuilding a mailbox.
    pub fn renumber(&self, mailbox: &str) {
        let mut st = self.lock();
        let mb = st.mailboxes.iter_mut().find(|m| m.name == mailbox).unwrap();
        mb.uid_validity += 1;
        for m in &mut mb.messages {
            m.uid += 1000;
        }
        mb.uid_next += 1000;
    }

    pub fn remove(&self, mailbox: &str, uid: u32) {
        let mut st = self.lock();
        let mb = st.mailboxes.iter_mut().find(|m| m.name == mailbox).unwrap();
        mb.messages.retain(|m| m.uid != uid);
    }

    pub fn set_flags(&self, mailbox: &str, uid: u32, flags: &[&str]) {
        let mut st = self.lock();
        let mb = st.mailboxes.iter_mut().find(|m| m.name == mailbox).unwrap();
        if let Some(m) = mb.messages.iter_mut().find(|m| m.uid == uid) {
            m.flags = flags.iter().map(|f| (*f).to_owned()).collect();
        }
    }

    pub fn flags(&self, mailbox: &str, uid: u32) -> BTreeSet<String> {
        let st = self.lock();
        let mb = st.mailboxes.iter().find(|m| m.name == mailbox).unwrap();
        mb.messages
            .iter()
            .find(|m| m.uid == uid)
            .map(|m| m.flags.clone())
            .unwrap_or_default()
    }

    pub fn count(&self, mailbox: &str) -> usize {
        let st = self.lock();
        st.mailboxes
            .iter()
            .find(|m| m.name == mailbox)
            .unwrap()
            .messages
            .len()
    }

    pub fn raw_messages(&self, mailbox: &str) -> Vec<String> {
        let st = self.lock();
        st.mailboxes
            .iter()
            .find(|m| m.name == mailbox)
            .unwrap()
            .messages
            .iter()
            .map(|m| String::from_utf8_lossy(&m.raw).into_owned())
            .collect()
    }

    pub fn sent(&self) -> Vec<Sent> {
        self.lock().sent.clone()
    }

    pub fn logins(&self) -> u32 {
        self.lock().logins
    }

    /// Whether the IMAP server advertises IDLE.
    pub fn set_idle(&self, on: bool) {
        self.lock().idle = on;
    }

    /// Hangs up on the next connection that's idling (within 50 ms, or when one starts).
    pub fn drop_idle_connection(&self) {
        self.lock().drop_idle = true;
    }

    /// Makes fetching one message fail (`None`: none), as a flaky server might.
    pub fn set_unreadable(&self, uid: Option<u32>) {
        self.lock().unreadable = uid;
    }

    pub fn bodies_sent(&self) -> u32 {
        self.lock().bodies_sent
    }

    fn exists(&self, mailbox: &str) -> usize {
        self.count(mailbox)
    }

    // --- IMAP -------------------------------------------------------------------------

    async fn imap_session(
        &self,
        mut r: BufReader<OwnedReadHalf>,
        mut w: OwnedWriteHalf,
    ) -> std::io::Result<()> {
        w.write_all(b"* OK Fake IMAP ready\r\n").await?;
        let mut authed = false;
        let mut selected: Option<String> = None;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            let line = line.trim_end_matches(['\r', '\n']).to_owned();
            let (tag, rest) = line.split_once(' ').unwrap_or((&line, ""));
            let tag = tag.to_owned();
            let (cmd, args) = rest.split_once(' ').unwrap_or((rest, ""));
            let cmd = cmd.to_uppercase();
            let args = args.to_owned();
            let mut out = String::new();
            match cmd.as_str() {
                "CAPABILITY" => {
                    let idle = if self.lock().idle { " IDLE" } else { "" };
                    out +=
                        &*format!("* CAPABILITY IMAP4rev1 MOVE UIDPLUS{idle}\r\n{tag} OK done\r\n");
                }
                "NOOP" => out += &*format!("{tag} OK done\r\n"),
                "LOGOUT" => {
                    w.write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes())
                        .await?;
                    return Ok(());
                }
                "LOGIN" => {
                    let parts = strings(&args);
                    let ok = {
                        let mut st = self.lock();
                        let ok = parts.len() == 2 && parts[0] == st.user && parts[1] == st.password;
                        if ok {
                            st.logins += 1;
                        }
                        ok
                    };
                    authed = ok;
                    out += &*if ok {
                        format!("{tag} OK Logged in\r\n")
                    } else {
                        format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n")
                    };
                }
                _ if !authed => out += &*format!("{tag} BAD log in first\r\n"),
                "LIST" => {
                    let st = self.lock();
                    for mb in &st.mailboxes {
                        let attrs = if mb.special.is_empty() {
                            "\\HasNoChildren".to_owned()
                        } else {
                            format!("\\HasNoChildren {}", mb.special)
                        };
                        out += &*format!("* LIST ({attrs}) \"/\" \"{}\"\r\n", mb.name);
                    }
                    out += &*format!("{tag} OK done\r\n");
                }
                "CREATE" => {
                    let name = strings(&args).into_iter().next().unwrap_or_default();
                    self.lock().mailboxes.push(mailbox(&name, ""));
                    out += &*format!("{tag} OK done\r\n");
                }
                "SELECT" | "EXAMINE" => {
                    let name = strings(&args).into_iter().next().unwrap_or_default();
                    let st = self.lock();
                    match st
                        .mailboxes
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(&name))
                    {
                        Some(mb) => {
                            selected = Some(mb.name.clone());
                            out += &*format!(
                                "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n* {} EXISTS\r\n* 0 RECENT\r\n\
                                 * OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] next\r\n{tag} OK [READ-WRITE] done\r\n",
                                mb.messages.len(),
                                mb.uid_validity,
                                mb.uid_next
                            );
                        }
                        None => out += &*format!("{tag} NO no such mailbox\r\n"),
                    }
                }
                "APPEND" => {
                    // APPEND "name" (flags) {n}
                    let name = strings(&args).into_iter().next().unwrap_or_default();
                    let n: usize = args
                        .rsplit('{')
                        .next()
                        .and_then(|s| s.trim_end_matches('}').parse().ok())
                        .unwrap_or(0);
                    w.write_all(b"+ Ready\r\n").await?;
                    let mut buf = vec![0; n];
                    r.read_exact(&mut buf).await?;
                    let mut end = String::new();
                    r.read_line(&mut end).await?;
                    let flags: Vec<&str> = if args.contains("\\Seen") {
                        vec!["\\Seen"]
                    } else {
                        vec![]
                    };
                    let raw = String::from_utf8_lossy(&buf).into_owned();
                    self.deliver(&name, &raw, crate::now_ms(), &flags);
                    out += &*format!("{tag} OK appended\r\n");
                }
                "UID" => {
                    let Some(mb) = selected.clone() else {
                        w.write_all(format!("{tag} BAD select first\r\n").as_bytes())
                            .await?;
                        continue;
                    };
                    let (sub, rest) = args.split_once(' ').unwrap_or((&args, ""));
                    // A message that can't be fetched: the connection drops.
                    let unreadable = self.lock().unreadable;
                    if sub.eq_ignore_ascii_case("FETCH")
                        && rest.to_uppercase().contains("BODY.PEEK[]")
                        && unreadable.is_some_and(|u| {
                            rest.split(' ')
                                .next()
                                .unwrap_or("")
                                .split(',')
                                .any(|s| s == u.to_string())
                        })
                    {
                        return Ok(());
                    }
                    out += &*self.uid_command(&tag, &sub.to_uppercase(), rest, &mb);
                }
                "EXPUNGE" => {
                    if let Some(mb) = &selected {
                        let mut st = self.lock();
                        let m = st.mailboxes.iter_mut().find(|m| &m.name == mb).unwrap();
                        let mut seq = m.messages.len();
                        while seq > 0 {
                            if m.messages[seq - 1].flags.contains("\\Deleted") {
                                m.messages.remove(seq - 1);
                                out += &*format!("* {seq} EXPUNGE\r\n");
                            }
                            seq -= 1;
                        }
                    }
                    out += &*format!("{tag} OK done\r\n");
                }
                "IDLE" => {
                    let mb = selected.clone().unwrap_or_else(|| "INBOX".to_owned());
                    w.write_all(b"+ idling\r\n").await?;
                    let mut known = self.exists(&mb);
                    loop {
                        let mut done = String::new();
                        tokio::select! {
                            n = r.read_line(&mut done) => {
                                if n? == 0 {
                                    return Ok(());
                                }
                                break;
                            }
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                                if std::mem::take(&mut self.lock().drop_idle) {
                                    return Ok(());
                                }
                                let now = self.exists(&mb);
                                if now != known {
                                    known = now;
                                    w.write_all(format!("* {now} EXISTS\r\n").as_bytes()).await?;
                                }
                            }
                        }
                    }
                    out += &*format!("{tag} OK IDLE terminated\r\n");
                }
                _ => out += &*format!("{tag} BAD unknown command\r\n"),
            }
            w.write_all(out.as_bytes()).await?;
        }
    }

    fn uid_command(&self, tag: &str, sub: &str, rest: &str, mailbox: &str) -> String {
        let mut st = self.lock();
        let mb_index = st.mailboxes.iter().position(|m| m.name == mailbox).unwrap();
        let max_uid = st.mailboxes[mb_index]
            .messages
            .iter()
            .map(|m| m.uid)
            .max()
            .unwrap_or(0);
        let mut out = String::new();
        match sub {
            "SEARCH" => {
                let mb = &st.mailboxes[mb_index];
                let criteria = rest.trim();
                let hits: Vec<u32> = mb
                    .messages
                    .iter()
                    .filter(|m| {
                        if let Some(date) = criteria.strip_prefix("SINCE ") {
                            let since = chrono::NaiveDate::parse_from_str(date.trim(), "%d-%b-%Y")
                                .map(|d| {
                                    d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis()
                                })
                                .unwrap_or(0);
                            m.internal_date >= since
                        } else if let Some(set) = criteria.strip_prefix("UID ") {
                            in_set(set, m.uid, max_uid)
                        } else {
                            true
                        }
                    })
                    .map(|m| m.uid)
                    .collect();
                let list: Vec<String> = hits.iter().map(u32::to_string).collect();
                out += &*format!("* SEARCH {}\r\n{tag} OK done\r\n", list.join(" "))
                    .replace("SEARCH \r\n", "SEARCH\r\n");
            }
            "FETCH" => {
                let (set, items) = rest.split_once(' ').unwrap_or((rest, ""));
                let items = items.to_uppercase();
                let mb = &st.mailboxes[mb_index];
                for (i, m) in mb.messages.iter().enumerate() {
                    if !in_set(set, m.uid, max_uid) {
                        continue;
                    }
                    let mut parts = vec![format!("UID {}", m.uid)];
                    if items.contains("FLAGS") {
                        parts.push(format!(
                            "FLAGS ({})",
                            m.flags.iter().cloned().collect::<Vec<_>>().join(" ")
                        ));
                    }
                    if items.contains("INTERNALDATE") {
                        let d = chrono::DateTime::from_timestamp_millis(m.internal_date).unwrap();
                        parts.push(format!(
                            "INTERNALDATE \"{}\"",
                            d.format("%d-%b-%Y %H:%M:%S +0000")
                        ));
                    }
                    if items.contains("RFC822.SIZE") {
                        parts.push(format!("RFC822.SIZE {}", m.raw.len()));
                    }
                    let mut literal = None;
                    if items.contains("BODY.PEEK[HEADER]") {
                        let raw = String::from_utf8_lossy(&m.raw);
                        let header =
                            raw.split("\r\n\r\n").next().unwrap_or("").to_owned() + "\r\n\r\n";
                        literal = Some(("BODY[HEADER]", header));
                    } else if items.contains("BODY.PEEK[]") {
                        literal = Some(("BODY[]", String::from_utf8_lossy(&m.raw).into_owned()));
                    }
                    let mut line = format!("* {} FETCH ({}", i + 1, parts.join(" "));
                    if let Some((name, body)) = literal {
                        line += &*format!(" {name} {{{}}}\r\n{body}", body.len());
                    }
                    line += ")\r\n";
                    out += &*line;
                }
                if items.contains("BODY.PEEK[]") {
                    let sent = mb
                        .messages
                        .iter()
                        .filter(|m| in_set(set, m.uid, max_uid))
                        .count() as u32;
                    st.bodies_sent += sent;
                }
                out += &*format!("{tag} OK done\r\n");
            }
            "STORE" => {
                // UID STORE set +FLAGS.SILENT (\Seen)
                let mut words = rest.splitn(3, ' ');
                let set = words.next().unwrap_or("").to_owned();
                let op = words.next().unwrap_or("").to_uppercase();
                let flags: Vec<String> = words
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '(' || c == ')')
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                let mb = &mut st.mailboxes[mb_index];
                for m in mb
                    .messages
                    .iter_mut()
                    .filter(|m| in_set(&set, m.uid, max_uid))
                {
                    for f in &flags {
                        if op.starts_with('+') {
                            m.flags.insert(f.clone());
                        } else if op.starts_with('-') {
                            m.flags.remove(f);
                        }
                    }
                }
                out += &*format!("{tag} OK done\r\n");
            }
            "MOVE" | "COPY" => {
                let (set, target) = rest.split_once(' ').unwrap_or((rest, ""));
                let target = strings(target).into_iter().next().unwrap_or_default();
                let Some(t_index) = st.mailboxes.iter().position(|m| m.name == target) else {
                    return format!("{tag} NO [TRYCREATE] no such mailbox\r\n");
                };
                let moving: Vec<Stored> = st.mailboxes[mb_index]
                    .messages
                    .iter()
                    .filter(|m| in_set(set, m.uid, max_uid))
                    .cloned()
                    .collect();
                for m in moving {
                    let t = &mut st.mailboxes[t_index];
                    let uid = t.uid_next;
                    t.uid_next += 1;
                    t.messages.push(Stored { uid, ..m });
                }
                if sub == "MOVE" {
                    let mb = &mut st.mailboxes[mb_index];
                    let mut seq = mb.messages.len();
                    while seq > 0 {
                        if in_set(set, mb.messages[seq - 1].uid, max_uid) {
                            mb.messages.remove(seq - 1);
                            out += &*format!("* {seq} EXPUNGE\r\n");
                        }
                        seq -= 1;
                    }
                }
                out += &*format!("{tag} OK done\r\n");
            }
            _ => out += &*format!("{tag} BAD unknown UID command\r\n"),
        }
        out
    }

    // --- SMTP -------------------------------------------------------------------------

    async fn smtp_session(
        &self,
        mut r: BufReader<OwnedReadHalf>,
        mut w: OwnedWriteHalf,
    ) -> std::io::Result<()> {
        w.write_all(b"220 fake ESMTP ready\r\n").await?;
        let mut authed = false;
        let mut from = String::new();
        let mut to = Vec::new();
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            let line = line.trim_end_matches(['\r', '\n']).to_owned();
            let upper = line.to_uppercase();
            let reply: String = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                "250-fake\r\n250-AUTH PLAIN LOGIN\r\n250-8BITMIME\r\n250 SMTPUTF8\r\n".to_owned()
            } else if upper.starts_with("AUTH PLAIN") {
                let b64 = line[10..].trim().to_owned();
                let b64 = if b64.is_empty() {
                    w.write_all(b"334 \r\n").await?;
                    let mut l = String::new();
                    r.read_line(&mut l).await?;
                    l.trim().to_owned()
                } else {
                    b64
                };
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default();
                let parts: Vec<String> = decoded
                    .split(|b| *b == 0)
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect();
                authed = self.check(
                    parts.get(1).map(String::as_str),
                    parts.get(2).map(String::as_str),
                );
                if authed {
                    "235 ok\r\n".to_owned()
                } else {
                    "535 5.7.8 bad credentials\r\n".to_owned()
                }
            } else if upper.starts_with("AUTH LOGIN") {
                let mut read = async |prompt: &[u8]| -> std::io::Result<String> {
                    w.write_all(prompt).await?;
                    let mut l = String::new();
                    r.read_line(&mut l).await?;
                    let d = base64::engine::general_purpose::STANDARD
                        .decode(l.trim())
                        .unwrap_or_default();
                    Ok(String::from_utf8_lossy(&d).into_owned())
                };
                let user = read(b"334 VXNlcm5hbWU6\r\n").await?;
                let pass = read(b"334 UGFzc3dvcmQ6\r\n").await?;
                authed = self.check(Some(&user), Some(&pass));
                if authed {
                    "235 ok\r\n".to_owned()
                } else {
                    "535 5.7.8 bad credentials\r\n".to_owned()
                }
            } else if upper.starts_with("NOOP") || upper.starts_with("RSET") {
                "250 ok\r\n".to_owned()
            } else if upper.starts_with("QUIT") {
                w.write_all(b"221 bye\r\n").await?;
                return Ok(());
            } else if !authed {
                "530 5.7.0 authenticate first\r\n".to_owned()
            } else if upper.starts_with("MAIL FROM:") {
                from = angle(&line[10..]);
                to.clear();
                "250 ok\r\n".to_owned()
            } else if upper.starts_with("RCPT TO:") {
                to.push(angle(&line[8..]));
                "250 ok\r\n".to_owned()
            } else if upper == "DATA" {
                w.write_all(b"354 go ahead\r\n").await?;
                let mut data = String::new();
                loop {
                    let mut l = String::new();
                    if r.read_line(&mut l).await? == 0 {
                        return Ok(());
                    }
                    if l == ".\r\n" || l == ".\n" {
                        break;
                    }
                    data.push_str(
                        l.strip_prefix('.')
                            .filter(|_| l.starts_with(".."))
                            .unwrap_or(&l),
                    );
                }
                self.lock().sent.push(Sent {
                    from: from.clone(),
                    to: std::mem::take(&mut to),
                    data,
                });
                "250 queued\r\n".to_owned()
            } else {
                "502 not implemented\r\n".to_owned()
            };
            w.write_all(reply.as_bytes()).await?;
        }
    }

    fn check(&self, user: Option<&str>, pass: Option<&str>) -> bool {
        let st = self.lock();
        user == Some(st.user.as_str()) && pass == Some(st.password.as_str())
    }
}

fn angle(s: &str) -> String {
    let s = s.trim();
    let s = s.split_whitespace().next().unwrap_or(s);
    s.trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// The quoted (or bare) strings in IMAP arguments.
fn strings(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = args.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => s.extend(chars.next()),
                    '"' => break,
                    c => s.push(c),
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

/// Whether `uid` is in an IMAP UID set like "1:*", "4,7,9" or "3:10".
fn in_set(set: &str, uid: u32, max: u32) -> bool {
    set.split(',').any(|part| {
        let num = |s: &str| {
            if s == "*" {
                max
            } else {
                s.parse().unwrap_or(0)
            }
        };
        match part.split_once(':') {
            Some((a, b)) => {
                let (a, b) = (num(a), num(b));
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                (lo..=hi).contains(&uid)
            }
            None => num(part) == uid,
        }
    })
}

/// A simple message for tests.
/// Dated a day ago, so it's always inside the sync window.
pub fn message(from: &str, to: &str, subject: &str, body: &str, id: &str, extra: &str) -> String {
    let date = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc2822();
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\n\
         Message-ID: <{id}>\r\n{extra}MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    )
}
