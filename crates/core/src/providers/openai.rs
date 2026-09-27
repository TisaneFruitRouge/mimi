//! Client for the OpenAI-compatible chat completions API, with streaming.

use std::time::Duration;

use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use mimi_protocol::ModelInfo;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error(
        "Your assistant's model isn't responding. If it runs on this computer, check that it's open."
    )]
    Unreachable(String),
    #[error("The model service didn't accept the key. Check it in Models.")]
    Unauthorized,
    #[error("The model had a problem: {0}")]
    Status(String),
    #[error("The model's answer couldn't be read. Try again.")]
    Decode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Tools an assistant message called.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallOut>,
    /// For `Role::Tool`: which call this is the result of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn tool_calls(content: impl Into<String>, calls: &[ToolCall]) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: calls
                .iter()
                .map(|c| ToolCallOut {
                    id: c.id.clone(),
                    kind: "function",
                    function: FunctionCallOut {
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    },
                })
                .collect(),
            tool_call_id: None,
        }
    }

    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionCallOut,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionCallOut {
    pub name: String,
    /// JSON-encoded, as the API expects.
    pub arguments: String,
}

/// A tool offered to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionSpec,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A complete tool call from the model. `arguments` is the raw JSON text it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A piece of a streamed reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatChunk {
    Content(String),
    /// A reasoning model's thinking, shown separately from the answer.
    Reasoning(String),
    /// The tools the model wants called. Arrives once, at the end of the stream.
    ToolCalls(Vec<ToolCall>),
}

/// How one request should be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatOptions {
    /// Let a reasoning model think before answering. Off for internal jobs (learning,
    /// sorting mail…) where speed matters more than depth: reasoning models like Qwen3
    /// otherwise spend most of their time thinking.
    pub thinking: bool,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self { thinking: true }
    }
}

impl ChatOptions {
    /// For background work: answer directly, without thinking first.
    pub const QUICK: Self = Self { thinking: false };
}

pub struct OpenAiCompatible {
    http: reqwest::Client,
    base_url: Url,
    api_key: Option<String>,
    /// Runs on the user's own machines (Ollama, llama.cpp, LM Studio…), which accept
    /// the extra request fields that switch thinking off. Cloud APIs may reject unknown
    /// fields, so they never get them.
    local: bool,
}

impl OpenAiCompatible {
    pub fn new(http: reqwest::Client, base_url: Url, api_key: Option<String>) -> Self {
        Self {
            http,
            base_url,
            api_key,
            local: false,
        }
    }

    /// Marks the source as running on the user's own machines; see [`ChatOptions`].
    pub fn local(mut self, local: bool) -> Self {
        self.local = local;
        self
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

    /// Where Ollama's native API would be, next to its OpenAI-compatible `/v1`.
    fn ollama_root(&self) -> Option<&str> {
        self.base_url
            .as_str()
            .trim_end_matches('/')
            .strip_suffix("/v1")
    }

    /// Whether this source is Ollama, which can download models.
    pub async fn is_ollama(&self) -> bool {
        let Some(root) = self.ollama_root() else {
            return false;
        };
        self.request(reqwest::Method::GET, format!("{root}/api/version"))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    /// Starts an Ollama model download. The response streams NDJSON progress lines.
    pub async fn start_pull(&self, model: &str) -> Result<reqwest::Response, ProviderError> {
        let Some(root) = self.ollama_root() else {
            return Err(ProviderError::Status(
                "This model source can't download models.".to_owned(),
            ));
        };
        let res = self
            .request(reqwest::Method::POST, format!("{root}/api/pull"))
            .json(&serde_json::json!({ "model": model, "stream": true }))
            .send()
            .await
            .map_err(|e| self.send_error(e))?;
        check(res).await
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

    /// [`Self::stream_chat`] with [`ChatOptions`]. Switching thinking off is best
    /// effort: it only applies to local sources, and a server that rejects the extra
    /// fields gets the request again without them.
    pub async fn stream_chat_with(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        options: ChatOptions,
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        let quick = !options.thinking && self.local;
        match self.send_chat(model, messages, tools, quick).await {
            Err(ProviderError::Status(reason)) if quick => {
                tracing::debug!(%reason, "retrying without the no-thinking fields");
                self.send_chat(model, messages, tools, false).await
            }
            other => other,
        }
    }

    async fn send_chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        quick: bool,
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        let res = self
            .request(reqwest::Method::POST, self.url("chat/completions"))
            .json(&chat_request(model, messages, tools, quick))
            .send()
            .await
            .map_err(|e| self.send_error(e))?;
        let bytes = check(res)
            .await?
            .bytes_stream()
            .map_err(|e| ProviderError::Decode(e.to_string()));
        Ok(parse_sse(bytes).boxed())
    }

    /// The whole answer to `messages`, without tools; reasoning is dropped. For
    /// internal jobs such as learning or sorting mail, usually with [`ChatOptions::QUICK`].
    pub async fn complete(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: ChatOptions,
    ) -> Result<String, ProviderError> {
        let mut stream = self.stream_chat_with(model, messages, &[], options).await?;
        let mut out = String::new();
        while let Some(chunk) = stream.next().await {
            if let ChatChunk::Content(text) = chunk? {
                out.push_str(&text);
            }
        }
        Ok(out)
    }

    /// One embedding vector per input, in order (OpenAI `/embeddings`, which both
    /// llama-server and Ollama serve).
    pub async fn embed(
        &self,
        model: &str,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>, ProviderError> {
        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            input: &'a [String],
        }
        #[derive(Deserialize)]
        struct Response {
            data: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }

        let res = self
            .request(reqwest::Method::POST, self.url("embeddings"))
            .timeout(Duration::from_secs(120))
            .json(&Request {
                model,
                input: inputs,
            })
            .send()
            .await
            .map_err(|e| self.send_error(e))?;
        let mut data = check(res)
            .await?
            .json::<Response>()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?
            .data;
        if data.len() != inputs.len() {
            return Err(ProviderError::Decode(format!(
                "{} embeddings for {} inputs",
                data.len(),
                inputs.len()
            )));
        }
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| item.embedding).collect())
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

/// The chat request body. `quick` adds the fields that switch thinking off:
/// `reasoning_effort: "none"` (Ollama's OpenAI endpoint; `think` is ignored there) and
/// `chat_template_kwargs.enable_thinking` (llama.cpp with Qwen3-style templates). Servers
/// that don't know them ignore them.
fn chat_request(
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
    quick: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools);
    }
    if quick {
        body["reasoning_effort"] = "none".into();
        body["chat_template_kwargs"] = serde_json::json!({ "enable_thinking": false });
    }
    body
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
        tool_calls: Option<Vec<ToolCallDelta>>,
    }
    #[derive(Deserialize)]
    struct ToolCallDelta {
        index: Option<usize>,
        id: Option<String>,
        function: Option<FunctionDelta>,
    }
    #[derive(Deserialize)]
    struct FunctionDelta {
        name: Option<String>,
        // A string fragment per the spec; some servers send the whole object.
        arguments: Option<serde_json::Value>,
    }

    struct State<S> {
        bytes: std::pin::Pin<Box<S>>,
        buf: Vec<u8>,
        pending: std::collections::VecDeque<Result<ChatChunk, ProviderError>>,
        think: ThinkSplitter,
        /// Tool calls being assembled, by index: (id, name, arguments so far).
        calls: std::collections::BTreeMap<usize, (Option<String>, String, String)>,
        done: bool,
        flushed: bool,
    }

    let state = State {
        bytes: Box::pin(bytes),
        buf: Vec::new(),
        pending: Default::default(),
        think: ThinkSplitter::default(),
        calls: Default::default(),
        done: false,
        flushed: false,
    };

    futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done && st.flushed {
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
                            st.calls.clear();
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
                            for (n, call) in d.tool_calls.into_iter().flatten().enumerate() {
                                let entry = st.calls.entry(call.index.unwrap_or(n)).or_default();
                                if let Some(id) = call.id.filter(|i| !i.is_empty()) {
                                    entry.0 = Some(id);
                                }
                                if let Some(f) = call.function {
                                    if let Some(name) = f.name {
                                        entry.1.push_str(&name);
                                    }
                                    match f.arguments {
                                        Some(serde_json::Value::String(a)) => entry.2.push_str(&a),
                                        Some(serde_json::Value::Null) | None => {}
                                        Some(other) => entry.2.push_str(&other.to_string()),
                                    }
                                }
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
            if st.done && !st.flushed {
                st.flushed = true;
                st.pending.extend(st.think.finish().into_iter().map(Ok));
                let calls: Vec<ToolCall> = std::mem::take(&mut st.calls)
                    .into_iter()
                    .filter(|(_, (_, name, _))| !name.is_empty())
                    .map(|(i, (id, name, arguments))| ToolCall {
                        id: id.unwrap_or_else(|| format!("call_{i}")),
                        name,
                        arguments,
                    })
                    .collect();
                if !calls.is_empty() {
                    st.pending.push_back(Ok(ChatChunk::ToolCalls(calls)));
                }
                continue;
            }
            if !st.pending.is_empty() || st.done {
                continue;
            }
            match st.bytes.next().await {
                Some(Ok(chunk)) => st.buf.extend_from_slice(&chunk),
                Some(Err(e)) => {
                    st.pending.push_back(Err(e));
                    st.done = true;
                    // Don't act on half-received tool calls after a broken stream.
                    st.calls.clear();
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
                ChatChunk::ToolCalls(_) => {}
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
    async fn assembles_streamed_tool_calls() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Checking.\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"cats\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"name\":\"other\",\"arguments\":{\"x\":1}}}]}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let bytes = futures::stream::iter(vec![Ok(bytes::Bytes::from(body))]);
        let chunks: Vec<ChatChunk> = parse_sse(bytes).map(Result::unwrap).collect().await;
        assert_eq!(
            chunks,
            vec![
                ChatChunk::Content("Checking.".into()),
                ChatChunk::ToolCalls(vec![
                    ToolCall {
                        id: "call_a".into(),
                        name: "lookup".into(),
                        arguments: "{\"q\":\"cats\"}".into()
                    },
                    ToolCall {
                        id: "call_1".into(),
                        name: "other".into(),
                        arguments: "{\"x\":1}".into()
                    },
                ]),
            ]
        );
    }

    #[tokio::test]
    async fn flushes_when_the_stream_ends_without_done() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"<think>hm\"}}]}\n\n";
        let bytes = futures::stream::iter(vec![Ok(bytes::Bytes::from(body))]);
        let chunks: Vec<ChatChunk> = parse_sse(bytes).map(Result::unwrap).collect().await;
        assert_eq!(collect(chunks), (String::new(), "hm".into()));
    }

    #[tokio::test]
    async fn surfaces_mid_stream_errors() {
        let body = "data: {\"error\":{\"message\":\"model not found\"}}\n\n";
        let bytes = futures::stream::iter(vec![Ok(bytes::Bytes::from(body))]);
        let chunks: Vec<_> = parse_sse(bytes).collect().await;
        assert!(matches!(&chunks[..], [Err(ProviderError::Status(m))] if m == "model not found"));
    }

    #[test]
    fn quick_requests_ask_ollama_and_llama_cpp_not_to_think() {
        let messages = [ChatMessage::text(Role::User, "hi")];
        let quick = chat_request("qwen3:8b", &messages, &[], true);
        // Ollama's OpenAI endpoint only honours `reasoning_effort`; llama.cpp reads the
        // chat template switch. Each ignores the other's field.
        assert_eq!(quick["reasoning_effort"], "none");
        assert_eq!(quick["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(quick["stream"], true);
        assert!(quick.get("tools").is_none());
        let normal = chat_request("qwen3:8b", &messages, &[], false);
        assert!(normal.get("reasoning_effort").is_none());
        assert!(normal.get("chat_template_kwargs").is_none());
    }

    /// A fake server that records chat requests, answers "OK", and (when `strict`)
    /// rejects fields it doesn't know, like some cloud APIs do.
    async fn fake_server(
        strict: bool,
    ) -> (
        Url,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) {
        use axum::Json;
        use axum::response::IntoResponse;
        use axum::routing::post;

        let seen: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>> = Default::default();
        let recorded = seen.clone();
        let app = axum::Router::new()
            .route(
                "/v1/chat/completions",
                post(move |Json(body): Json<serde_json::Value>| {
                    let recorded = recorded.clone();
                    async move {
                        let unknown = body.get("chat_template_kwargs").is_some();
                        recorded.lock().unwrap().push(body);
                        if strict && unknown {
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                "{\"error\":{\"message\":\"Unrecognized request argument\"}}",
                            )
                                .into_response();
                        }
                        "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\ndata: [DONE]\n\n"
                            .into_response()
                    }
                }),
            )
            .route(
                "/v1/embeddings",
                post(|| async {
                    // Out of order on purpose: results are matched by index.
                    Json(serde_json::json!({"data": [
                        {"index": 1, "embedding": [0.0, 1.0]},
                        {"index": 0, "embedding": [1.0, 0.0]},
                    ]}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/v1", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    #[tokio::test]
    async fn thinking_is_switched_off_only_where_it_is_safe() {
        let messages = [ChatMessage::text(Role::User, "hi")];

        // A local server gets the fields.
        let (url, seen) = fake_server(false).await;
        let local = OpenAiCompatible::new(reqwest::Client::new(), url.clone(), None).local(true);
        let answer = local
            .complete("m", &messages, ChatOptions::QUICK)
            .await
            .unwrap();
        assert_eq!(answer, "OK");
        assert_eq!(seen.lock().unwrap()[0]["reasoning_effort"], "none");
        // Ordinary chats keep thinking.
        local
            .complete("m", &messages, ChatOptions::default())
            .await
            .unwrap();
        assert!(seen.lock().unwrap()[1].get("reasoning_effort").is_none());

        // A cloud service never gets them: they're a harmless no-op.
        let cloud = OpenAiCompatible::new(reqwest::Client::new(), url, None);
        cloud
            .complete("m", &messages, ChatOptions::QUICK)
            .await
            .unwrap();
        assert!(
            seen.lock().unwrap()[2]
                .get("chat_template_kwargs")
                .is_none()
        );

        // A local server that rejects them gets the request again without.
        let (url, seen) = fake_server(true).await;
        let picky = OpenAiCompatible::new(reqwest::Client::new(), url, None).local(true);
        assert_eq!(
            picky
                .complete("m", &messages, ChatOptions::QUICK)
                .await
                .unwrap(),
            "OK"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].get("chat_template_kwargs").is_none());
    }

    #[tokio::test]
    async fn embeddings_come_back_in_input_order() {
        let (url, _) = fake_server(false).await;
        let client = OpenAiCompatible::new(reqwest::Client::new(), url, None);
        let vectors = client
            .embed("e", &["first".to_owned(), "second".to_owned()])
            .await
            .unwrap();
        assert_eq!(vectors, [vec![1.0, 0.0], vec![0.0, 1.0]]);
        let err = client
            .embed("e", &["only one".to_owned()])
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Decode(_)));
    }
}
