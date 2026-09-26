//! Client for the OpenAI-compatible chat completions API, with streaming.

use std::time::Duration;

use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use hearth_protocol::ModelInfo;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Couldn't reach {0}. Is it running?")]
    Unreachable(String),
    #[error("The provider rejected the API key.")]
    Unauthorized,
    #[error("The provider returned an error: {0}")]
    Status(String),
    #[error("The provider sent a response we couldn't understand: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// A piece of a streamed reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatChunk {
    Content(String),
    /// A reasoning model's thinking, shown separately from the answer.
    Reasoning(String),
}

pub struct OpenAiCompatible {
    http: reqwest::Client,
    base_url: Url,
    api_key: Option<String>,
}

impl OpenAiCompatible {
    pub fn new(http: reqwest::Client, base_url: Url, api_key: Option<String>) -> Self {
        Self {
            http,
            base_url,
            api_key,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url.as_str().trim_end_matches('/'))
    }

    fn request(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        let req = self.http.request(method, url);
        match &self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }

        let res = self
            .request(reqwest::Method::GET, self.url("models"))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| self.send_error(e))?;
        let models: Models = check(res)
            .await?
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        let sizes = self.ollama_sizes().await;
        let mut models: Vec<ModelInfo> = models
            .data
            .into_iter()
            .map(|m| ModelInfo {
                size_bytes: sizes.iter().find(|(n, _)| *n == m.id).map(|(_, s)| *s),
                id: m.id,
            })
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    /// Ollama reports model sizes on its native API, next to the OpenAI one. Anything
    /// else answers 404, and we go without.
    async fn ollama_sizes(&self) -> Vec<(String, u64)> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Tag>,
        }
        #[derive(Deserialize)]
        struct Tag {
            name: String,
            size: u64,
        }

        let Some(root) = self
            .base_url
            .as_str()
            .trim_end_matches('/')
            .strip_suffix("/v1")
        else {
            return Vec::new();
        };
        let res = self
            .request(reqwest::Method::GET, format!("{root}/api/tags"))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
        match res {
            Ok(res) if res.status().is_success() => res
                .json::<Tags>()
                .await
                .map(|t| t.models.into_iter().map(|m| (m.name, m.size)).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Streams a reply to `messages`.
    pub async fn stream_chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: &'a [ChatMessage],
            stream: bool,
        }

        let res = self
            .request(reqwest::Method::POST, self.url("chat/completions"))
            .json(&Request {
                model,
                messages,
                stream: true,
            })
            .send()
            .await
            .map_err(|e| self.send_error(e))?;
        let bytes = check(res)
            .await?
            .bytes_stream()
            .map_err(|e| ProviderError::Decode(e.to_string()));
        Ok(parse_sse(bytes).boxed())
    }

    fn send_error(&self, err: reqwest::Error) -> ProviderError {
        if err.is_connect() || err.is_timeout() {
            ProviderError::Unreachable(
                self.base_url
                    .host_str()
                    .unwrap_or("the provider")
                    .to_owned(),
            )
        } else {
            ProviderError::Status(err.to_string())
        }
    }
}

async fn check(res: reqwest::Response) -> Result<reqwest::Response, ProviderError> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ProviderError::Unauthorized);
    }
    let body = res.text().await.unwrap_or_default();
    Err(ProviderError::Status(error_message(status, &body)))
}

/// Pulls the human-readable message out of the error shapes providers use.
fn error_message(status: reqwest::StatusCode, body: &str) -> String {
    let json: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = json.as_ref().and_then(|j| {
        j.pointer("/error/message")
            .or_else(|| j.get("error"))
            .or_else(|| j.get("message"))
            .and_then(|m| m.as_str())
            .map(str::to_owned)
    });
    match message {
        Some(m) => m,
        None if body.trim().is_empty() => status.to_string(),
        None => body.chars().take(300).collect(),
    }
}

/// Turns a server-sent-events byte stream from `/chat/completions` into chunks.
fn parse_sse(
    bytes: impl futures::Stream<Item = Result<bytes::Bytes, ProviderError>> + Send + 'static,
) -> impl futures::Stream<Item = Result<ChatChunk, ProviderError>> + Send {
    #[derive(Deserialize)]
    struct Frame {
        #[serde(default)]
        choices: Vec<Choice>,
        error: Option<serde_json::Value>,
    }
    #[derive(Deserialize)]
    struct Choice {
        #[serde(default)]
        delta: Delta,
    }
    #[derive(Deserialize, Default)]
    struct Delta {
        content: Option<String>,
        // Ollama uses `reasoning`, DeepSeek and vLLM `reasoning_content`.
        reasoning: Option<String>,
        reasoning_content: Option<String>,
    }

    struct State<S> {
        bytes: std::pin::Pin<Box<S>>,
        buf: Vec<u8>,
        pending: std::collections::VecDeque<Result<ChatChunk, ProviderError>>,
        think: ThinkSplitter,
        done: bool,
    }

    let state = State {
        bytes: Box::pin(bytes),
        buf: Vec::new(),
        pending: Default::default(),
        think: ThinkSplitter::default(),
        done: false,
    };

    futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done {
                return None;
            }
            // Handle every complete line in the buffer.
            while let Some(nl) = st.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = st.buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(data) = line.trim_end().strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim_start();
                if data == "[DONE]" {
                    st.done = true;
                    break;
                }
                match serde_json::from_str::<Frame>(data) {
                    Ok(frame) => {
                        if let Some(err) = frame.error {
                            let msg = err
                                .get("message")
                                .and_then(|m| m.as_str())
                                .map(str::to_owned)
                                .unwrap_or_else(|| err.to_string());
                            st.pending.push_back(Err(ProviderError::Status(msg)));
                            st.done = true;
                            break;
                        }
                        for choice in frame.choices {
                            let d = choice.delta;
                            if let Some(r) = d
                                .reasoning
                                .or(d.reasoning_content)
                                .filter(|r| !r.is_empty())
                            {
                                st.pending.push_back(Ok(ChatChunk::Reasoning(r)));
                            }
                            if let Some(c) = d.content {
                                st.pending.extend(st.think.push(&c).into_iter().map(Ok));
                            }
                        }
                    }
                    Err(e) => {
                        st.pending
                            .push_back(Err(ProviderError::Decode(e.to_string())));
                        st.done = true;
                        break;
                    }
                }
            }
            if st.done {
                st.pending.extend(st.think.finish().into_iter().map(Ok));
                continue;
            }
            if !st.pending.is_empty() {
                continue;
            }
            match st.bytes.next().await {
                Some(Ok(chunk)) => st.buf.extend_from_slice(&chunk),
                Some(Err(e)) => {
                    st.pending.push_back(Err(e));
                    st.done = true;
                }
                // Stream ended without [DONE]: treat what we have as the reply.
                None => st.done = true,
            }
        }
    })
}

/// Separates `<think>…</think>` sections, which some models inline in their content,
/// from the answer. Tags may be split across chunks.
#[derive(Default)]
struct ThinkSplitter {
    in_think: bool,
    buf: String,
}

impl ThinkSplitter {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";

    fn push(&mut self, text: &str) -> Vec<ChatChunk> {
        self.buf.push_str(text);
        let mut out = Vec::new();
        loop {
            let tag = if self.in_think {
                Self::CLOSE
            } else {
                Self::OPEN
            };
            if let Some(pos) = self.buf.find(tag) {
                let before: String = self.buf.drain(..pos).collect();
                self.buf.drain(..tag.len());
                self.emit(&mut out, before);
                self.in_think = !self.in_think;
                continue;
            }
            // Hold back a suffix that could be the start of a tag.
            let keep = (1..tag.len())
                .rev()
                .find(|&n| self.buf.ends_with(&tag[..n]))
                .unwrap_or(0);
            let ready: String = self.buf.drain(..self.buf.len() - keep).collect();
            self.emit(&mut out, ready);
            return out;
        }
    }

    fn finish(&mut self) -> Vec<ChatChunk> {
        let rest = std::mem::take(&mut self.buf);
        let mut out = Vec::new();
        self.emit(&mut out, rest);
        out
    }

    fn emit(&self, out: &mut Vec<ChatChunk>, text: String) {
        if text.is_empty() {
            return;
        }
        out.push(if self.in_think {
            ChatChunk::Reasoning(text)
        } else {
            ChatChunk::Content(text)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: Vec<ChatChunk>) -> (String, String) {
        let (mut content, mut reasoning) = (String::new(), String::new());
        for c in chunks {
            match c {
                ChatChunk::Content(s) => content += &s,
                ChatChunk::Reasoning(s) => reasoning += &s,
            }
        }
        (content, reasoning)
    }

    #[test]
    fn think_tags_split_across_chunks() {
        let mut s = ThinkSplitter::default();
        let mut out = Vec::new();
        for piece in [
            "<thi",
            "nk>pondering",
            " more</th",
            "ink>\n\nHello",
            " <b>world</b>",
        ] {
            out.extend(s.push(piece));
        }
        out.extend(s.finish());
        let (content, reasoning) = collect(out);
        assert_eq!(reasoning, "pondering more");
        assert_eq!(content, "\n\nHello <b>world</b>");
    }

    #[tokio::test]
    async fn parses_sse_stream() {
        let body = concat!(
            ": keep-alive\n\n",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"reasoning\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        // Split mid-line to exercise buffering.
        let (a, b) = body.split_at(70);
        let bytes = futures::stream::iter(vec![
            Ok(bytes::Bytes::from(a.to_owned())),
            Ok(bytes::Bytes::from(b.to_owned())),
        ]);
        let chunks: Vec<_> = parse_sse(bytes).collect().await;
        let chunks: Vec<ChatChunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(collect(chunks), ("Hello".into(), "hmm".into()));
    }

    #[tokio::test]
    async fn surfaces_mid_stream_errors() {
        let body = "data: {\"error\":{\"message\":\"model not found\"}}\n\n";
        let bytes = futures::stream::iter(vec![Ok(bytes::Bytes::from(body))]);
        let chunks: Vec<_> = parse_sse(bytes).collect().await;
        assert!(matches!(&chunks[..], [Err(ProviderError::Status(m))] if m == "model not found"));
    }
}
