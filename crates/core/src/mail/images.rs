//! Fetching an email's pictures when the user asks for them ("Load images").
//!
//! Only the addresses found in that message are fetched, straight from the daemon (no
//! cookies, no referrer, no proxy), and only from the internet: an address on this
//! computer or the local network is refused, even behind a redirect or a name that
//! resolves there, so an email can't use the daemon to reach the user's router or
//! other local services. Only pictures come back, capped in size, as `data:` addresses
//! (`render::data_uri`).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::render::{IMAGE_TYPES, data_uri};

/// Largest picture fetched.
pub const MAX_IMAGE: usize = 5 * 1024 * 1024;
/// Most bytes fetched for one message, in all.
pub const MAX_TOTAL: usize = 20 * 1024 * 1024;
/// Most pictures fetched for one message.
const MAX_COUNT: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(15);
/// Longest wait for all of a message's pictures; the slow ones are left out.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const AT_ONCE: usize = 6;

pub struct Fetcher {
    client: reqwest::Client,
    /// Tests only: their picture server runs on this computer.
    allow_loopback: bool,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::build(false)
    }
}

impl Fetcher {
    #[cfg(test)]
    pub fn allowing_loopback() -> Self {
        Self::build(true)
    }

    fn build(allow_loopback: bool) -> Self {
        let client = reqwest::Client::builder()
            // A proxy would resolve names itself, past the checks below.
            .no_proxy()
            .referer(false)
            .user_agent("Mozilla/5.0")
            .dns_resolver(PublicOnly { allow_loopback })
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if allowed_url(attempt.url(), allow_loopback) {
                    attempt.follow()
                } else {
                    attempt.error("redirected to a refused address")
                }
            }))
            .connect_timeout(Duration::from_secs(5))
            .timeout(TIMEOUT)
            .build()
            .expect("the picture client builds");
        Self {
            client,
            allow_loopback,
        }
    }

    /// Fetches pictures (at most `MAX_COUNT`, within `MAX_TOTAL`) and returns them as
    /// `data:` addresses by original address. The ones that can't be fetched are left
    /// out.
    pub async fn fetch_all(&self, urls: &[String]) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let mut total = 0;
        let mut results = futures::stream::iter(urls.iter().take(MAX_COUNT).cloned())
            .map(|url| async move {
                let result = self.fetch(&url).await;
                (url, result)
            })
            .buffer_unordered(AT_ONCE);
        let deadline = tokio::time::Instant::now() + TOTAL_TIMEOUT;
        while let Ok(Some((url, result))) = tokio::time::timeout_at(deadline, results.next()).await
        {
            match result {
                Ok((ctype, data)) if total + data.len() <= MAX_TOTAL => {
                    total += data.len();
                    out.insert(url, data_uri(&ctype, &data));
                }
                Ok(_) => {}
                // Never the address itself: it may identify the user.
                Err(e) => tracing::debug!("an email picture wasn't loaded: {e}"),
            }
        }
        out
    }

    /// One picture: its type and bytes.
    pub async fn fetch(&self, url: &str) -> Result<(String, Vec<u8>), &'static str> {
        let url = Url::parse(url).map_err(|_| "not a web address")?;
        if !allowed_url(&url, self.allow_loopback) {
            return Err("refused address");
        }
        let mut res = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "image/*")
            .send()
            .await
            .map_err(|_| "unreachable")?;
        if !res.status().is_success() {
            return Err("not found");
        }
        let ctype = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !IMAGE_TYPES.contains(&ctype.as_str()) {
            return Err("not a picture");
        }
        if res.content_length().is_some_and(|n| n > MAX_IMAGE as u64) {
            return Err("too large");
        }
        let mut data = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(|_| "unreachable")? {
            data.extend_from_slice(&chunk);
            if data.len() > MAX_IMAGE {
                return Err("too large");
            }
        }
        Ok((ctype, data))
    }
}

/// A web address that may be fetched: http(s), no password in it, and not an address
/// on this computer or the local network when written as a number.
fn allowed_url(url: &Url, allow_loopback: bool) -> bool {
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() {
        return false;
    }
    if url.password().is_some() {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(_)) => true,
        Some(url::Host::Ipv4(ip)) => allowed_ip(IpAddr::V4(ip), allow_loopback),
        Some(url::Host::Ipv6(ip)) => allowed_ip(IpAddr::V6(ip), allow_loopback),
        None => false,
    }
}

fn allowed_ip(ip: IpAddr, allow_loopback: bool) -> bool {
    is_public(ip) || (allow_loopback && ip.is_loopback())
}

/// Whether an address is on the internet, rather than this computer, the local network
/// or a reserved range.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b)) // shared (carrier-grade NAT)
                || (a == 192 && b == 0 && c == 0) // protocol assignments
                || (a == 198 && (b == 18 || b == 19)) // benchmarking
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            let embedded =
                || Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
            if s[0] == 0x64 && s[1] == 0xff9b {
                // NAT64: an IPv4 address in disguise.
                return is_public(IpAddr::V4(embedded()));
            }
            if s[0] == 0x2002 {
                // 6to4: the IPv4 address is in the next 32 bits.
                let v4 =
                    Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8);
                return is_public(IpAddr::V4(v4));
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || s[..6].iter().all(|x| *x == 0) // IPv4-compatible
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
                || (s[0] & 0xffc0) == 0xfec0 // site-local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x2001 && s[1] == 0)) // Teredo
        }
    }
}

/// Resolves names, keeping only internet addresses (so a name pointing at this computer
/// or the local network can't be used, whatever it's called).
struct PublicOnly {
    allow_loopback: bool,
}

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let allow_loopback = self.allow_loopback;
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| allowed_ip(a.ip(), allow_loopback))
                .collect();
            if addrs.is_empty() {
                return Err("that name doesn't lead to the internet".into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::http::header;
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::get;

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

    /// A picture server on this computer, counting its requests.
    async fn server() -> (String, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        let app = Router::new()
            .route(
                "/pic.png",
                get(|| async { ([(header::CONTENT_TYPE, "image/png")], PNG) }),
            )
            .route(
                "/page",
                get(|| async { ([(header::CONTENT_TYPE, "text/html")], "<script>x</script>") }),
            )
            .route(
                "/drawing.svg",
                get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], "<svg/>") }),
            )
            .route(
                "/huge.png",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "image/png")],
                        vec![0u8; MAX_IMAGE + 1],
                    )
                }),
            )
            .route(
                "/to-router",
                get(|| async { Redirect::temporary("http://192.168.1.1/admin.png") }),
            )
            .route(
                "/to-metadata",
                get(|| async { Redirect::temporary("http://169.254.169.254/latest") }),
            )
            .route(
                "/to-pic",
                get(|| async { Redirect::temporary("/pic.png").into_response() }),
            )
            .layer(axum::middleware::from_fn(
                move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let counted = counted.clone();
                    async move {
                        counted.fetch_add(1, Ordering::SeqCst);
                        next.run(req).await
                    }
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://127.0.0.1:{}", addr.port()), hits)
    }

    #[tokio::test]
    async fn pictures_are_fetched_only_as_pictures() {
        let (base, _) = server().await;
        let f = Fetcher::allowing_loopback();
        let (ctype, data) = f.fetch(&format!("{base}/pic.png")).await.unwrap();
        assert_eq!((ctype.as_str(), data.as_slice()), ("image/png", PNG));
        assert!(f.fetch(&format!("{base}/to-pic")).await.is_ok());
        assert_eq!(f.fetch(&format!("{base}/page")).await, Err("not a picture"));
        assert_eq!(
            f.fetch(&format!("{base}/drawing.svg")).await,
            Err("not a picture")
        );
        assert_eq!(f.fetch(&format!("{base}/huge.png")).await, Err("too large"));
        assert!(f.fetch(&format!("{base}/missing.png")).await.is_err());
        // Redirects into the local network are refused before anything is sent there.
        assert!(f.fetch(&format!("{base}/to-router")).await.is_err());
        assert!(f.fetch(&format!("{base}/to-metadata")).await.is_err());

        let all = f
            .fetch_all(&[format!("{base}/pic.png"), format!("{base}/page")])
            .await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[&format!("{base}/pic.png")], data_uri("image/png", PNG));
    }

    #[tokio::test]
    async fn local_addresses_are_refused() {
        let (base, hits) = server().await;
        let port = base.rsplit(':').next().unwrap();
        let f = Fetcher::default();
        for url in [
            format!("{base}/pic.png"),
            format!("http://localhost:{port}/pic.png"),
            format!("http://[::1]:{port}/pic.png"),
            format!("http://[::ffff:127.0.0.1]:{port}/pic.png"),
            format!("http://0.0.0.0:{port}/pic.png"),
            format!("http://2130706433:{port}/pic.png"),
            format!("http://0x7f.1:{port}/pic.png"),
            "http://10.0.0.1/a.png".to_owned(),
            "http://192.168.1.1/a.png".to_owned(),
            "http://172.16.5.4/a.png".to_owned(),
            "http://169.254.169.254/latest".to_owned(),
            "http://100.64.0.1/a.png".to_owned(),
            "http://[fd00::1]/a.png".to_owned(),
            "http://[fe80::1]/a.png".to_owned(),
            "http://[64:ff9b::7f00:1]/a.png".to_owned(),
            "http://[2002:7f00:1::]/a.png".to_owned(),
            "http://user:pass@example.com/a.png".to_owned(),
            "ftp://example.com/a.png".to_owned(),
            "file:///etc/passwd".to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
        ] {
            assert!(f.fetch(&url).await.is_err(), "{url} was fetched");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn public_addresses_are_told_apart() {
        for ip in [
            "93.184.216.34",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.31.0.1",
            "192.168.0.10",
            "169.254.1.1",
            "100.100.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:192.168.0.1",
            "::127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }
}
