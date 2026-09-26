//! Model providers: where they are, how they're stored, and how to talk to them.

use std::net::IpAddr;

use hearth_protocol::{Locality, ProviderPreset};
use reqwest::Url;

pub mod openai;
pub mod store;

pub use openai::{ChatChunk, ChatMessage, OpenAiCompatible, ProviderError, Role};

/// A client for a stored provider.
pub fn connect(
    http: &reqwest::Client,
    record: &store::ProviderRecord,
) -> Result<OpenAiCompatible, String> {
    let url = parse_base_url(&record.provider.base_url)?;
    Ok(OpenAiCompatible::new(
        http.clone(),
        url,
        record.api_key.clone(),
    ))
}

/// Normalized form for storage: no trailing slash.
pub fn base_url_string(url: &Url) -> String {
    url.as_str().trim_end_matches('/').to_owned()
}

/// Parses and checks a provider base URL: http(s) only, no credentials embedded.
pub fn parse_base_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| format!("\"{raw}\" is not a valid URL."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("The address must start with http:// or https://.".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Put credentials in the API key field, not in the address.".to_owned());
    }
    if url.host().is_none() {
        return Err("The address has no host.".to_owned());
    }
    Ok(url)
}

/// Best guess of where a provider runs, from its URL. Users can override it, e.g. for
/// their own server behind a public domain name.
pub fn locality_of(url: &Url) -> Locality {
    let Some(host) = url.host_str() else {
        return Locality::Cloud;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return locality_of_ip(ip);
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Locality::Device;
    }
    const PRIVATE_SUFFIXES: &[&str] = &[".local", ".lan", ".home.arpa", ".internal", ".ts.net"];
    if !host.contains('.') || PRIVATE_SUFFIXES.iter().any(|s| host.ends_with(s)) {
        return Locality::Network;
    }
    Locality::Cloud
}

fn locality_of_ip(ip: IpAddr) -> Locality {
    if ip.is_loopback() {
        return Locality::Device;
    }
    let private = match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            // 100.64.0.0/10 is carrier-grade NAT, which Tailscale uses.
            v4.is_private() || v4.is_link_local() || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    };
    if private {
        Locality::Network
    } else {
        Locality::Cloud
    }
}

pub fn presets() -> Vec<ProviderPreset> {
    let preset = |id: &str, name: &str, description: &str, base_url: &str, needs_api_key| {
        let locality = locality_of(&Url::parse(base_url).expect("preset URLs are valid"));
        ProviderPreset {
            id: id.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            base_url: base_url.to_owned(),
            locality,
            needs_api_key,
        }
    };
    vec![
        preset(
            "ollama",
            "Ollama",
            "Runs models on this computer.",
            "http://localhost:11434/v1",
            false,
        ),
        preset(
            "lmstudio",
            "LM Studio",
            "Runs models on this computer.",
            "http://localhost:1234/v1",
            false,
        ),
        preset(
            "llamacpp",
            "llama.cpp server",
            "Runs models on this computer.",
            "http://localhost:8080/v1",
            false,
        ),
        preset(
            "mistral",
            "Mistral",
            "Cloud models from Mistral AI, hosted in the EU.",
            "https://api.mistral.ai/v1",
            true,
        ),
        preset(
            "openai",
            "OpenAI",
            "Cloud models from OpenAI.",
            "https://api.openai.com/v1",
            true,
        ),
        preset(
            "openrouter",
            "OpenRouter",
            "Many cloud models behind one account.",
            "https://openrouter.ai/api/v1",
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(url: &str) -> Locality {
        locality_of(&Url::parse(url).unwrap())
    }

    #[test]
    fn locality_from_url() {
        assert_eq!(loc("http://localhost:11434/v1"), Locality::Device);
        assert_eq!(loc("http://127.0.0.1:8080"), Locality::Device);
        assert_eq!(loc("http://[::1]:8080"), Locality::Device);
        assert_eq!(loc("http://192.168.1.20:11434"), Locality::Network);
        assert_eq!(loc("http://10.0.0.5"), Locality::Network);
        assert_eq!(loc("http://100.101.102.103"), Locality::Network);
        assert_eq!(loc("http://[fd12:3456::1]"), Locality::Network);
        assert_eq!(loc("http://gpubox:11434"), Locality::Network);
        assert_eq!(loc("http://gpubox.local:11434"), Locality::Network);
        assert_eq!(loc("http://gpubox.tail1234.ts.net"), Locality::Network);
        assert_eq!(loc("https://api.openai.com/v1"), Locality::Cloud);
        assert_eq!(loc("http://8.8.8.8"), Locality::Cloud);
        assert_eq!(loc("http://100.128.0.1"), Locality::Cloud);
    }

    #[test]
    fn base_url_validation() {
        assert!(parse_base_url("http://localhost:11434/v1").is_ok());
        assert!(parse_base_url("ftp://example.com").is_err());
        assert!(parse_base_url("https://user:pw@example.com").is_err());
        assert!(parse_base_url("not a url").is_err());
    }
}
