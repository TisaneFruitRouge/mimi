//! Model providers: where they are, how they're stored, and how to talk to them.

use std::net::IpAddr;

use futures::stream::BoxStream;
use mimi_protocol::{Locality, ProviderKind, ProviderPreset};
use reqwest::Url;

pub mod anthropic;
pub mod openai;
pub mod pull;
pub mod store;

pub use anthropic::Anthropic;
pub use openai::{
    ChatChunk, ChatMessage, ChatOptions, FunctionSpec, OpenAiCompatible, ProviderError, Role,
    ToolCall, ToolSpec,
};

/// A chat client for any kind of source: the OpenAI-compatible API most of them speak,
/// or Anthropic's own.
pub enum ChatClient {
    OpenAi(OpenAiCompatible),
    Anthropic(Anthropic),
}

impl ChatClient {
    pub async fn list_models(&self) -> Result<Vec<mimi_protocol::ModelInfo>, ProviderError> {
        match self {
            Self::OpenAi(c) => c.list_models().await,
            Self::Anthropic(c) => c.list_models().await,
        }
    }

    /// Streams a reply to `messages`, offering `tools` when there are any.
    pub async fn stream_chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        self.stream_chat_with(model, messages, tools, ChatOptions::default())
            .await
    }

    pub async fn stream_chat_with(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        options: ChatOptions,
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        match self {
            Self::OpenAi(c) => c.stream_chat_with(model, messages, tools, options).await,
            Self::Anthropic(c) => c.stream_chat_with(model, messages, tools, options).await,
        }
    }

    /// The whole answer to `messages`, without tools; reasoning is dropped. For
    /// internal jobs such as learning or sorting mail, usually with [`ChatOptions::QUICK`].
    pub async fn complete(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: ChatOptions,
    ) -> Result<String, ProviderError> {
        use futures::StreamExt;
        let mut stream = self.stream_chat_with(model, messages, &[], options).await?;
        let mut out = String::new();
        while let Some(chunk) = stream.next().await {
            if let ChatChunk::Content(text) = chunk? {
                out.push_str(&text);
            }
        }
        Ok(out)
    }
}

/// A client for chatting with `model` from a stored source. The built-in runtime starts
/// (or switches to) that model first.
pub async fn chat_client(
    state: &crate::AppState,
    record: &store::ProviderRecord,
    model: &str,
) -> Result<ChatClient, String> {
    if record.provider.kind == ProviderKind::Builtin {
        let endpoint = state.runtime.ensure(state, model).await?;
        return Ok(ChatClient::OpenAi(
            OpenAiCompatible::new(state.http.clone(), endpoint.url, Some(endpoint.key)).local(true),
        ));
    }
    connect(&state.http, record)
}

/// The models a source offers. The built-in source lists what's been downloaded.
pub async fn list_models(
    state: &crate::AppState,
    record: &store::ProviderRecord,
) -> Result<Vec<mimi_protocol::ModelInfo>, ProviderError> {
    if record.provider.kind == ProviderKind::Builtin {
        return Ok(crate::runtime::download::installed(&state.paths));
    }
    let client = connect(&state.http, record).map_err(ProviderError::Status)?;
    client.list_models().await
}

/// A chat client for a stored provider's HTTP API. Not for the built-in source, whose
/// address changes; use [`chat_client`] or [`list_models`].
pub fn connect(
    http: &reqwest::Client,
    record: &store::ProviderRecord,
) -> Result<ChatClient, String> {
    let url = parse_base_url(&record.provider.base_url)?;
    Ok(match record.provider.kind {
        ProviderKind::Anthropic => {
            ChatClient::Anthropic(Anthropic::new(http.clone(), url, record.api_key.clone()))
        }
        ProviderKind::OpenaiCompatible | ProviderKind::Builtin => ChatClient::OpenAi(
            OpenAiCompatible::new(http.clone(), url, record.api_key.clone())
                .local(record.provider.locality != Locality::Cloud),
        ),
    })
}

/// The OpenAI-compatible client for a stored provider, for what only those offer
/// (downloads through Ollama, embeddings).
pub fn connect_openai(
    http: &reqwest::Client,
    record: &store::ProviderRecord,
) -> Result<OpenAiCompatible, String> {
    match connect(http, record)? {
        ChatClient::OpenAi(client) => Ok(client),
        ChatClient::Anthropic(_) => Err("This model source can't do that.".to_owned()),
    }
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
    struct Preset {
        id: &'static str,
        name: &'static str,
        description: &'static str,
        kind: ProviderKind,
        base_url: &'static str,
        key_url: Option<&'static str>,
        recommended_model: Option<&'static str>,
    }
    let local = |id, name, base_url| Preset {
        id,
        name,
        description: "Runs models on this computer.",
        kind: ProviderKind::OpenaiCompatible,
        base_url,
        key_url: None,
        recommended_model: None,
    };
    [
        local("ollama", "Ollama", "http://localhost:11434/v1"),
        local("lmstudio", "LM Studio", "http://localhost:1234/v1"),
        local("llamacpp", "llama.cpp server", "http://localhost:8080/v1"),
        Preset {
            id: "anthropic",
            name: "Anthropic",
            description: "Claude models. Thoughtful, good at writing and at using tools.",
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com/v1",
            key_url: Some("https://platform.claude.com/settings/keys"),
            recommended_model: Some("claude-sonnet-5"),
        },
        Preset {
            id: "openai",
            name: "OpenAI",
            description: "GPT models, from the makers of ChatGPT.",
            kind: ProviderKind::OpenaiCompatible,
            base_url: "https://api.openai.com/v1",
            key_url: Some("https://platform.openai.com/api-keys"),
            recommended_model: None,
        },
        Preset {
            id: "mistral",
            name: "Mistral",
            description: "Models from Mistral AI, hosted in the EU.",
            kind: ProviderKind::OpenaiCompatible,
            base_url: "https://api.mistral.ai/v1",
            key_url: Some("https://console.mistral.ai/api-keys"),
            recommended_model: Some("mistral-medium-latest"),
        },
        Preset {
            id: "openrouter",
            name: "OpenRouter",
            description: "Many companies' models behind one account.",
            kind: ProviderKind::OpenaiCompatible,
            base_url: "https://openrouter.ai/api/v1",
            key_url: Some("https://openrouter.ai/settings/keys"),
            recommended_model: Some("deepseek/deepseek-v4-flash"),
        },
    ]
    .into_iter()
    .map(|p| {
        let locality = locality_of(&Url::parse(p.base_url).expect("preset URLs are valid"));
        ProviderPreset {
            id: p.id.to_owned(),
            name: p.name.to_owned(),
            description: p.description.to_owned(),
            kind: p.kind,
            base_url: p.base_url.to_owned(),
            locality,
            needs_api_key: locality == Locality::Cloud,
            key_url: p.key_url.map(str::to_owned),
            recommended_model: p.recommended_model.map(str::to_owned),
        }
    })
    .collect()
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
