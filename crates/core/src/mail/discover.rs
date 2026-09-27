//! Works out a mailbox's servers from the email address alone, the way mail apps do, so
//! people with their own domain (Migadu, mailbox.org, a company server…) never have to
//! know what IMAP is. Only the user's own mail provider and DNS are contacted; no
//! third-party lookup service.
//!
//! In order: well-known services by domain → the domain's own autoconfig file →
//! RFC 6186 SRV records → the domain's MX host mapped to a known provider → probing the
//! usual host names. The result is still checked by signing in before anything is saved.

use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mimi_protocol::{MailDiscovery, MailSecurity, MailServers};
use quick_xml::Reader;
use quick_xml::events::Event as Xml;

use MailSecurity::{StartTls, Tls};

/// What we know about a mail service.
#[derive(Clone)]
struct Known {
    name: &'static str,
    /// Preset id when the service is one of the connect dialog's presets.
    preset: Option<&'static str>,
    servers: Option<MailServers>,
    app_password: bool,
    help: Option<&'static str>,
    help_url: Option<&'static str>,
    supported: bool,
}

fn servers(
    imap: &str,
    imap_port: u16,
    imap_sec: MailSecurity,
    smtp: &str,
    smtp_port: u16,
    smtp_sec: MailSecurity,
) -> Option<MailServers> {
    Some(MailServers {
        imap_host: imap.to_owned(),
        imap_port,
        imap_security: imap_sec,
        smtp_host: smtp.to_owned(),
        smtp_port,
        smtp_security: smtp_sec,
        username: None,
    })
}

fn known(name: &'static str, servers: Option<MailServers>) -> Known {
    Known {
        name,
        preset: None,
        servers,
        app_password: false,
        help: None,
        help_url: None,
        supported: true,
    }
}

fn from_preset(id: &'static str) -> Known {
    let preset = super::presets()
        .into_iter()
        .find(|p| p.id == id)
        .expect("preset exists");
    let name: &'static str = match id {
        "icloud" => "iCloud Mail",
        "gmail" => "Gmail",
        "fastmail" => "Fastmail",
        "proton" => "Proton Mail",
        _ => "Outlook.com",
    };
    Known {
        name,
        preset: Some(id),
        servers: preset.servers,
        // Every preset service except Proton (Bridge password) wants an app password.
        app_password: matches!(id, "icloud" | "gmail" | "fastmail"),
        help: (!preset.supported).then_some(
            "Microsoft only lets apps in through a Microsoft sign-in, which Mimi doesn't support yet.",
        ),
        help_url: None,
        supported: preset.supported,
    }
}

/// Consumer services recognised by the address's domain.
fn known_domain(domain: &str) -> Option<Known> {
    if let Some(id) = super::guess_preset(&format!("x@{domain}")) {
        return Some(from_preset(id));
    }
    let is = |names: &[&str]| names.contains(&domain);
    let k = if domain.starts_with("yahoo.") || is(&["ymail.com", "rocketmail.com"]) {
        Known {
            app_password: true,
            help: Some("Yahoo needs an app password: Account security › Generate app password."),
            help_url: Some("https://login.yahoo.com/account/security"),
            ..known(
                "Yahoo Mail",
                servers(
                    "imap.mail.yahoo.com",
                    993,
                    Tls,
                    "smtp.mail.yahoo.com",
                    465,
                    Tls,
                ),
            )
        }
    } else if is(&["aol.com"]) {
        Known {
            app_password: true,
            help: Some("AOL needs an app password: Account security › Generate app password."),
            help_url: Some("https://login.aol.com/account/security"),
            ..known(
                "AOL Mail",
                servers("imap.aol.com", 993, Tls, "smtp.aol.com", 465, Tls),
            )
        }
    } else if domain.starts_with("gmx.") {
        Known {
            help: Some("Turn on IMAP access in GMX's settings first (E-Mail › POP3 & IMAP)."),
            ..known(
                "GMX",
                servers("imap.gmx.net", 993, Tls, "mail.gmx.net", 587, StartTls),
            )
        }
    } else if is(&["web.de"]) {
        Known {
            help: Some("Turn on IMAP access in WEB.DE's settings first (E-Mail › POP3/IMAP)."),
            ..known(
                "WEB.DE",
                servers("imap.web.de", 993, Tls, "smtp.web.de", 587, StartTls),
            )
        }
    } else if is(&["posteo.de", "posteo.net"]) {
        known(
            "Posteo",
            servers("posteo.de", 993, Tls, "posteo.de", 465, Tls),
        )
    } else if is(&["mailbox.org"]) {
        known(
            "mailbox.org",
            servers("imap.mailbox.org", 993, Tls, "smtp.mailbox.org", 465, Tls),
        )
    } else if is(&["orange.fr", "wanadoo.fr"]) {
        known(
            "Orange",
            servers("imap.orange.fr", 993, Tls, "smtp.orange.fr", 465, Tls),
        )
    } else if is(&["free.fr"]) {
        known(
            "Free",
            servers("imap.free.fr", 993, Tls, "smtp.free.fr", 465, Tls),
        )
    } else if is(&["laposte.net"]) {
        known(
            "La Poste",
            servers("imap.laposte.net", 993, Tls, "smtp.laposte.net", 465, Tls),
        )
    } else if is(&["bluewin.ch"]) {
        known(
            "Bluewin",
            servers("imaps.bluewin.ch", 993, Tls, "smtps.bluewin.ch", 465, Tls),
        )
    } else if domain.starts_with("yandex.") || is(&["ya.ru"]) {
        Known {
            app_password: true,
            help: Some("Yandex needs an app password for mail apps."),
            ..known(
                "Yandex Mail",
                servers("imap.yandex.com", 993, Tls, "smtp.yandex.com", 465, Tls),
            )
        }
    } else if is(&[
        "tuta.com",
        "tutanota.com",
        "tutanota.de",
        "tuta.io",
        "keemail.me",
    ]) {
        Known {
            supported: false,
            help: Some(
                "Tuta doesn't let other apps read your mail (it has no IMAP), so Mimi can't connect to it.",
            ),
            ..known("Tuta", None)
        }
    } else if is(&["hey.com"]) {
        Known {
            supported: false,
            help: Some(
                "HEY doesn't let other apps read your mail (it has no IMAP), so Mimi can't connect to it.",
            ),
            ..known("HEY", None)
        }
    } else {
        return None;
    };
    Some(k)
}

/// Hosting providers recognised by the domain's mail exchanger (custom domains).
fn known_mx(mx: &str) -> Option<Known> {
    let ends = |suffix: &str| mx == suffix || mx.ends_with(&format!(".{suffix}"));
    Some(if ends("google.com") || ends("googlemail.com") {
        Known {
            name: "Google Workspace",
            ..from_preset("gmail")
        }
    } else if ends("icloud.com") {
        from_preset("icloud")
    } else if ends("messagingengine.com") {
        from_preset("fastmail")
    } else if ends("protonmail.ch") {
        from_preset("proton")
    } else if ends("outlook.com") {
        Known {
            name: "Microsoft 365",
            ..from_preset("outlook")
        }
    } else if ends("migadu.com") {
        known(
            "Migadu",
            servers("imap.migadu.com", 993, Tls, "smtp.migadu.com", 465, Tls),
        )
    } else if ends("mailbox.org") {
        known(
            "mailbox.org",
            servers("imap.mailbox.org", 993, Tls, "smtp.mailbox.org", 465, Tls),
        )
    } else if ends("posteo.de") {
        known(
            "Posteo",
            servers("posteo.de", 993, Tls, "posteo.de", 465, Tls),
        )
    } else if ends("infomaniak.ch") {
        known(
            "Infomaniak",
            servers(
                "mail.infomaniak.com",
                993,
                Tls,
                "mail.infomaniak.com",
                465,
                Tls,
            ),
        )
    } else if ends("ovh.net") {
        known(
            "OVHcloud",
            servers("ssl0.ovh.net", 993, Tls, "ssl0.ovh.net", 465, Tls),
        )
    } else if ends("gandi.net") {
        known(
            "Gandi",
            servers("mail.gandi.net", 993, Tls, "mail.gandi.net", 465, Tls),
        )
    } else if ends("zoho.eu") {
        known(
            "Zoho Mail",
            servers("imap.zoho.eu", 993, Tls, "smtp.zoho.eu", 465, Tls),
        )
    } else if ends("zoho.com") {
        known(
            "Zoho Mail",
            servers("imap.zoho.com", 993, Tls, "smtp.zoho.com", 465, Tls),
        )
    } else {
        return None;
    })
}

fn into_discovery(k: Known) -> MailDiscovery {
    MailDiscovery {
        provider: Some(k.name.to_owned()),
        preset: k.preset.map(str::to_owned),
        servers: k.servers,
        needs_app_password: k.app_password,
        help: k.help.map(str::to_owned),
        help_url: k.help_url.map(str::to_owned),
        supported: k.supported,
    }
}

fn found(servers: MailServers) -> MailDiscovery {
    MailDiscovery {
        provider: None,
        preset: None,
        servers: Some(servers),
        needs_app_password: false,
        help: None,
        help_url: None,
        supported: true,
    }
}

/// Everything discovery can learn from an address. Never fails: when nothing is found,
/// `servers` is `None` and the dialog asks for the settings.
pub async fn discover(email: &str, http: &reqwest::Client) -> MailDiscovery {
    let Some(domain) = domain_of(email) else {
        return not_found();
    };
    if let Some(k) = known_domain(&domain) {
        return into_discovery(k);
    }
    // Everything below talks to the network; cap the whole search.
    let search = async {
        // A known host behind the domain's mail exchanger is the quickest sure answer.
        let resolver = TokioResolver::builder_tokio()
            .ok()
            .and_then(|b| b.build().ok());
        if let Some(resolver) = &resolver {
            for mx in mx_hosts(resolver, &domain).await {
                if let Some(k) = known_mx(&mx) {
                    return Some(into_discovery(k));
                }
            }
        }
        if let Some(s) = autoconfig(http, &domain, email).await {
            return Some(found(s));
        }
        if let Some(resolver) = &resolver
            && let Some(s) = srv(resolver, &domain).await
        {
            return Some(found(s));
        }
        probe(&domain).await.map(found)
    };
    tokio::time::timeout(Duration::from_secs(12), search)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(not_found)
}

/// Nothing found: the user can still enter the servers themselves.
fn not_found() -> MailDiscovery {
    MailDiscovery {
        supported: true,
        ..MailDiscovery::default()
    }
}

fn domain_of(email: &str) -> Option<String> {
    let (local, domain) = email.trim().rsplit_once('@')?;
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    (!local.is_empty() && domain.contains('.') && !domain.contains('/')).then_some(domain)
}

/// The domain's own Thunderbird-style autoconfig file, over HTTPS only.
async fn autoconfig(http: &reqwest::Client, domain: &str, email: &str) -> Option<MailServers> {
    let fetch = |url: String| async move {
        let res = http
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        parse_autoconfig(&res.text().await.ok()?, email)
    };
    // Both places at once: a missing one often just times out.
    let (a, b) = tokio::join!(
        fetch(format!("https://autoconfig.{domain}/mail/config-v1.1.xml")),
        fetch(format!(
            "https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"
        )),
    );
    a.or(b)
}

/// Reads the first IMAP and SMTP servers from a `clientConfig` document.
fn parse_autoconfig(xml: &str, email: &str) -> Option<MailServers> {
    #[derive(Default, Clone)]
    struct Server {
        host: String,
        port: u16,
        socket: String,
        username: String,
    }
    let mut reader = Reader::from_str(xml);
    let (mut imap, mut smtp): (Option<Server>, Option<Server>) = (None, None);
    let mut current: Option<(String, Server)> = None;
    let mut field = String::new();
    loop {
        match reader.read_event().ok()? {
            Xml::Start(e) => {
                let name = e.name().as_ref().to_owned();
                if name == "incomingServer" || name == "outgoingServer" {
                    let kind = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "type")
                        .map(|a| a.value.to_string())
                        .unwrap_or_default();
                    current = Some((kind, Server::default()));
                } else {
                    field = name;
                }
            }
            Xml::Text(t) => {
                if let Some((_, s)) = current.as_mut() {
                    let text = t.into_inner().trim().to_owned();
                    match field.as_str() {
                        "hostname" => s.host = text,
                        "port" => s.port = text.parse().unwrap_or(0),
                        "socketType" => s.socket = text,
                        "username" => s.username = text,
                        _ => {}
                    }
                }
            }
            Xml::End(e) => {
                let name = e.name().as_ref().to_owned();
                if (name == "incomingServer" || name == "outgoingServer")
                    && let Some((kind, s)) = current.take()
                {
                    if kind == "imap" && imap.is_none() {
                        imap = Some(s);
                    } else if kind == "smtp" && smtp.is_none() {
                        smtp = Some(s);
                    }
                }
                field.clear();
            }
            Xml::Eof => break,
            _ => {}
        }
    }
    let (imap, smtp) = (imap?, smtp?);
    let security = |socket: &str| match socket.to_ascii_uppercase().as_str() {
        "SSL" | "TLS" => Some(Tls),
        "STARTTLS" => Some(StartTls),
        _ => None, // plain text: never
    };
    if imap.host.is_empty() || smtp.host.is_empty() || imap.port == 0 || smtp.port == 0 {
        return None;
    }
    let local = email.split('@').next().unwrap_or_default();
    let username = match imap.username.as_str() {
        "%EMAILLOCALPART%" => Some(local.to_owned()),
        "" | "%EMAILADDRESS%" => None,
        other if !other.contains('%') => Some(other.to_owned()),
        _ => None,
    };
    Some(MailServers {
        imap_host: imap.host,
        imap_port: imap.port,
        imap_security: security(&imap.socket)?,
        smtp_host: smtp.host,
        smtp_port: smtp.port,
        smtp_security: security(&smtp.socket)?,
        username,
    })
}

/// RFC 6186 service records: `_imaps._tcp`, `_submissions._tcp`, `_submission._tcp`.
async fn srv(resolver: &TokioResolver, domain: &str) -> Option<MailServers> {
    let first = |name: String| async move {
        let lookup = resolver.srv_lookup(name).await.ok()?;
        let mut records: Vec<(u16, u16, String)> = lookup
            .answers()
            .iter()
            .filter_map(|r| match &r.data {
                RData::SRV(s) => Some((s.priority, s.port, s.target.to_ascii())),
                _ => None,
            })
            .filter(|(_, _, target)| target != "." && !target.is_empty())
            .collect();
        records.sort();
        records
            .into_iter()
            .next()
            .map(|(_, port, host)| (host.trim_end_matches('.').to_owned(), port))
    };
    let (imap_host, imap_port) = first(format!("_imaps._tcp.{domain}.")).await?;
    let (smtp_host, smtp_port, smtp_security) =
        match first(format!("_submissions._tcp.{domain}.")).await {
            Some((h, p)) => (h, p, Tls),
            None => {
                let (h, p) = first(format!("_submission._tcp.{domain}.")).await?;
                (h, p, StartTls)
            }
        };
    Some(MailServers {
        imap_host,
        imap_port,
        imap_security: Tls,
        smtp_host,
        smtp_port,
        smtp_security,
        username: None,
    })
}

/// The domain's mail exchangers, most preferred first, lowercase without the final dot.
async fn mx_hosts(resolver: &TokioResolver, domain: &str) -> Vec<String> {
    let Ok(lookup) = resolver.mx_lookup(format!("{domain}.")).await else {
        return Vec::new();
    };
    let mut hosts: Vec<(u16, String)> = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::MX(mx) => Some((
                mx.preference,
                mx.exchange
                    .to_ascii()
                    .trim_end_matches('.')
                    .to_ascii_lowercase(),
            )),
            _ => None,
        })
        .collect();
    hosts.sort();
    hosts.into_iter().map(|(_, h)| h).collect()
}

/// Last resort: the host names most providers use, if they answer on the mail ports.
async fn probe(domain: &str) -> Option<MailServers> {
    let open = |host: String, port: u16| async move {
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect((host.as_str(), port)),
        )
        .await
        .is_ok_and(|r| r.is_ok())
    };
    let mut imap = None;
    for host in [
        format!("imap.{domain}"),
        format!("mail.{domain}"),
        domain.to_owned(),
    ] {
        if open(host.clone(), 993).await {
            imap = Some(host);
            break;
        }
    }
    let imap = imap?;
    for host in [
        format!("smtp.{domain}"),
        format!("mail.{domain}"),
        imap.clone(),
        domain.to_owned(),
    ] {
        for (port, security) in [(465, Tls), (587, StartTls)] {
            if open(host.clone(), port).await {
                return Some(MailServers {
                    imap_host: imap,
                    imap_port: 993,
                    imap_security: Tls,
                    smtp_host: host,
                    smtp_port: port,
                    smtp_security: security,
                    username: None,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains() {
        assert_eq!(
            domain_of(" Me@Example.ORG ").as_deref(),
            Some("example.org")
        );
        assert_eq!(domain_of("nope"), None);
        assert_eq!(domain_of("@example.org"), None);
        assert_eq!(domain_of("me@localhost"), None);
    }

    #[test]
    fn known_services() {
        let gmx = known_domain("gmx.de").unwrap();
        assert_eq!(gmx.name, "GMX");
        assert!(!gmx.app_password);
        let icloud = known_domain("icloud.com").unwrap();
        assert!(icloud.app_password);
        assert_eq!(icloud.preset, Some("icloud"));
        assert!(!known_domain("tuta.com").unwrap().supported);
        assert!(known_domain("example.org").is_none());
    }

    #[test]
    fn custom_domains_by_mail_exchanger() {
        let migadu = known_mx("aspmx1.migadu.com").unwrap();
        assert_eq!(migadu.name, "Migadu");
        let s = migadu.servers.unwrap();
        assert_eq!(
            (s.imap_host.as_str(), s.imap_port),
            ("imap.migadu.com", 993)
        );
        assert_eq!(
            (s.smtp_host.as_str(), s.smtp_port),
            ("smtp.migadu.com", 465)
        );
        assert!(!migadu.app_password);
        assert_eq!(
            known_mx("aspmx.l.google.com").unwrap().name,
            "Google Workspace"
        );
        assert!(
            !known_mx("example-org.mail.protection.outlook.com")
                .unwrap()
                .supported
        );
        assert!(known_mx("mx.example.org").is_none());
        // A look-alike must not match.
        assert!(known_mx("evilmigadu.com").is_none());
    }

    #[test]
    fn autoconfig_documents() {
        let xml = r#"<?xml version="1.0"?>
<clientConfig version="1.1"><emailProvider id="example.org">
  <incomingServer type="pop3"><hostname>pop.example.org</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
  <incomingServer type="imap"><hostname>mail.example.org</hostname><port>993</port><socketType>SSL</socketType><username>%EMAILLOCALPART%</username></incomingServer>
  <outgoingServer type="smtp"><hostname>mail.example.org</hostname><port>587</port><socketType>STARTTLS</socketType><username>%EMAILADDRESS%</username></outgoingServer>
</emailProvider></clientConfig>"#;
        let s = parse_autoconfig(xml, "me@example.org").unwrap();
        assert_eq!(
            (s.imap_host.as_str(), s.imap_port, s.imap_security),
            ("mail.example.org", 993, Tls)
        );
        assert_eq!((s.smtp_port, s.smtp_security), (587, StartTls));
        assert_eq!(s.username.as_deref(), Some("me"));

        let plain = xml.replace(
            "<socketType>SSL</socketType><username>",
            "<socketType>plain</socketType><username>",
        );
        assert!(
            parse_autoconfig(&plain, "me@example.org").is_none(),
            "never plain text"
        );
    }
}

#[cfg(test)]
mod live {
    /// Real-network check, run by hand: `cargo test -p mimi-core live_discovery -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_discovery() {
        let http = reqwest::Client::new();
        for email in [
            "someone@migadu.com",
            "someone@stripe.com",
            "someone@microsoft.com",
            "someone@kernel.org",
            "someone@gmx.ch",
        ] {
            let d = super::discover(email, &http).await;
            println!(
                "{email:<28} provider={:?} supported={} app_password={} imap={:?} smtp={:?}",
                d.provider,
                d.supported,
                d.needs_app_password,
                d.servers
                    .as_ref()
                    .map(|s| format!("{}:{}", s.imap_host, s.imap_port)),
                d.servers
                    .as_ref()
                    .map(|s| format!("{}:{}", s.smtp_host, s.smtp_port)),
            );
        }
    }
}
