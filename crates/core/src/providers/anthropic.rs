//! Client for Anthropic's Messages API (Claude models), with streaming and tools.
//!
//! Mimi's prompts are OpenAI-shaped ([`ChatMessage`]); this module translates them:
//! system messages become the top-level `system`, tool calls become `tool_use` blocks,
//! and tool results are gathered into the next user message.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use mimi_protocol::ModelInfo;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};

use super::openai::{ChatChunk, ChatMessage, ChatOptions, ProviderError, Role, ToolCall, ToolSpec};

const API_VERSION: &str = "2023-06-01";

/// Replies are capped here even when a model could write more: a chat answer never
/// needs it, and older models reject anything above their own limit.
const MAX_OUTPUT: u64 = 32_000;
/// When the model's own limit can't be looked up.
const FALLBACK_OUTPUT: u64 = 8_192;

/// Each model's output limit, as `/models` reported it, shared by every client.
static OUTPUT_LIMITS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

pub struct Anthropic {
    http: reqwest::Client,
    base_url: Url,
    api_key: Option<String>,
}

impl Anthropic {
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
        let req = self
            .http
            .request(method, url)
            .header("anthropic-version", API_VERSION);
        match &self.api_key {
            Some(key) => req.header("x-api-key", key),
            None => req,
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        #[derive(Deserialize)]
        struct Page {
            data: Vec<Model>,
            #[serde(default)]
            has_more: bool,
            last_id: Option<String>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
            display_name: Option<String>,
            max_tokens: Option<u64>,
        }

        let mut models = Vec::new();
        let mut after: Option<String> = None;
        // A few pages at most; the list is short.
        for _ in 0..5 {
            let mut url = format!("{}?limit=100", self.url("models"));
            if let Some(id) = &after {
                url.push_str(&format!("&after_id={id}"));
            }
            let res = self
                .request(reqwest::Method::GET, url)
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| self.send_error(e))?;
            let page: Page = check(res)
                .await?
                .json()
                .await
                .map_err(|e| ProviderError::Decode(e.to_string()))?;
            {
                let mut limits = OUTPUT_LIMITS.lock().expect("output limits lock");
                for m in &page.data {
                    if let Some(max) = m.max_tokens {
                        limits.insert(m.id.clone(), max);
                    }
                }
            }
            models.extend(page.data.into_iter().map(|m| ModelInfo {
                id: m.id,
                name: m.display_name,
                size_bytes: None,
                supports_tools: Some(true),
                price: None,
            }));
            match (page.has_more, page.last_id) {
                (true, Some(last)) => after = Some(last),
                _ => break,
            }
        }
        // Newest first, as Anthropic lists them.
        Ok(models)
    }

    /// How long a reply may be, from the model's own limit.
    async fn output_limit(&self, model: &str) -> u64 {
        #[derive(Deserialize)]
        struct Model {
            max_tokens: Option<u64>,
        }

        let known = OUTPUT_LIMITS
            .lock()
            .expect("output limits lock")
            .get(model)
            .copied();
        let max = match known {
            Some(max) => Some(max),
            None => {
                let res = self
                    .request(
                        reqwest::Method::GET,
                        self.url(&format!("models/{}", urlencode(model))),
                    )
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await;
                let max = match res {
                    Ok(res) if res.status().is_success() => {
                        res.json::<Model>().await.ok().and_then(|m| m.max_tokens)
                    }
                    _ => None,
                };
                if let Some(max) = max {
                    OUTPUT_LIMITS
                        .lock()
                        .expect("output limits lock")
                        .insert(model.to_owned(), max);
                }
                max
            }
        };
        max.map_or(FALLBACK_OUTPUT, |m| m.min(MAX_OUTPUT))
    }

    /// Streams a reply to `messages`, offering `tools` when there are any. Thinking is
    /// left to each model's default: the switches differ between Claude models, and
    /// sending the wrong one is refused, so [`ChatOptions`] has no effect here.
    pub async fn stream_chat_with(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        _options: ChatOptions,
    ) -> Result<BoxStream<'static, Result<ChatChunk, ProviderError>>, ProviderError> {
        let body = request_body(model, messages, tools, self.output_limit(model).await);
        let res = self
            .request(reqwest::Method::POST, self.url("messages"))
            .json(&body)
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
            ProviderError::Status(err.without_url().to_string())
        }
    }
}

/// Anthropic's busy signals get plain words; everything else as the API says it.
async fn check(res: reqwest::Response) -> Result<reqwest::Response, ProviderError> {
    match res.status().as_u16() {
        429 => Err(ProviderError::Status(
            "Anthropic is limiting requests from your account right now. Try again in a minute."
                .to_owned(),
        )),
        529 => Err(ProviderError::Status(
            "Anthropic is overloaded right now. Try again in a moment.".to_owned(),
        )),
        _ => super::openai::check(res).await,
    }
}

fn urlencode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Tool ids and names may only use these characters. Ids from other model sources
/// (a conversation that switched models) are made to fit.
fn clean_id(id: &str) -> String {
    let id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if id.is_empty() { "call".to_owned() } else { id }
}

/// The Messages API request for Mimi's prompt.
///
/// Caching: the tool list is marked for caching (it's long and identical from one
/// request to the next), and with tools the conversation is too, so each round of a
/// tool-using reply reads the rounds before it from the cache. Without tools (sorting
/// mail, learning) only the system prompt is marked: the rest differs every time.
fn request_body(
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
    max_tokens: u64,
) -> Value {
    let with_tools = !tools.is_empty();
    let system: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == Role::System && !m.content.trim().is_empty())
        .map(|m| m.content.as_str())
        .collect();

    let mut out: Vec<(Role, Vec<Value>)> = Vec::new();
    let mut push = |role: Role, blocks: Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, existing)) if *last == role => existing.extend(blocks),
            _ => out.push((role, blocks)),
        }
    };
    // Which tool each call id was, for writing results out as text.
    let mut names: HashMap<&str, &str> = HashMap::new();

    for m in messages {
        match m.role {
            Role::System => {}
            Role::User => {
                if !m.content.trim().is_empty() {
                    push(Role::User, vec![json!({"type": "text", "text": m.content})]);
                }
            }
            Role::Assistant => {
                for call in &m.tool_calls {
                    names.insert(&call.id, &call.function.name);
                }
                if with_tools && let Some(Value::Array(blocks)) = &m.replay {
                    push(Role::Assistant, blocks.clone());
                    continue;
                }
                let mut blocks = Vec::new();
                if !m.content.trim().is_empty() {
                    blocks.push(json!({"type": "text", "text": m.content}));
                }
                for call in &m.tool_calls {
                    if with_tools {
                        let input = serde_json::from_str::<Value>(&call.function.arguments)
                            .ok()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": clean_id(&call.id),
                            "name": clean_id(&call.function.name),
                            "input": input,
                        }));
                    } else {
                        blocks.push(json!({
                            "type": "text",
                            "text": format!("(Used {} with {})", call.function.name, call.function.arguments),
                        }));
                    }
                }
                push(Role::Assistant, blocks);
            }
            Role::Tool => {
                let id = m.tool_call_id.as_deref().unwrap_or("call");
                let content = if m.content.is_empty() {
                    "(no output)"
                } else {
                    m.content.as_str()
                };
                if with_tools {
                    push(
                        Role::User,
                        vec![json!({
                            "type": "tool_result",
                            "tool_use_id": clean_id(id),
                            "content": content,
                        })],
                    );
                } else {
                    let name = names.get(id).copied().unwrap_or("a tool");
                    push(
                        Role::User,
                        vec![
                            json!({"type": "text", "text": format!("(Result of {name}:)\n{content}")}),
                        ],
                    );
                }
            }
        }
    }
    // The conversation must open with the user.
    if out.first().is_none_or(|(role, _)| *role != Role::User) {
        out.insert(
            0,
            (
                Role::User,
                vec![json!({"type": "text", "text": "(The conversation so far.)"})],
            ),
        );
    }

    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": out
            .into_iter()
            .map(|(role, content)| json!({
                "role": if role == Role::Assistant { "assistant" } else { "user" },
                "content": content,
            }))
            .collect::<Vec<_>>(),
    });
    if !system.is_empty() {
        let mut block = json!({"type": "text", "text": system.join("\n\n")});
        if !with_tools {
            block["cache_control"] = json!({"type": "ephemeral"});
        }
        body["system"] = json!([block]);
    }
    if with_tools {
        let mut specs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                })
            })
            .collect();
        if let Some(last) = specs.last_mut() {
            last["cache_control"] = json!({"type": "ephemeral"});
        }
        body["tools"] = json!(specs);
        body["cache_control"] = json!({"type": "ephemeral"});
    }
    body
}

/// A content block being streamed.
enum Block {
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
    Redacted(Value),
    Tool {
        id: String,
        name: String,
        input: String,
    },
    Other,
}

impl Block {
    /// The block as it must be sent back, or `None` for what isn't kept.
    fn replay(&self) -> Option<Value> {
        match self {
            Block::Text(text) if !text.is_empty() => Some(json!({"type": "text", "text": text})),
            Block::Thinking { text, signature } => {
                Some(json!({"type": "thinking", "thinking": text, "signature": signature}))
            }
            Block::Redacted(block) => Some(block.clone()),
            Block::Tool { id, name, input } => Some(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": parse_input(input),
            })),
            _ => None,
        }
    }
}

fn parse_input(input: &str) -> Value {
    if input.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(input)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// Turns the Messages API's server-sent events into chunks. When the model calls
/// tools, the whole reply (thinking included) comes out as [`ChatChunk::Replay`] just
/// before [`ChatChunk::ToolCalls`], since it must be sent back unchanged.
fn parse_sse(
    bytes: impl futures::Stream<Item = Result<bytes::Bytes, ProviderError>> + Send + 'static,
) -> impl futures::Stream<Item = Result<ChatChunk, ProviderError>> + Send {
    struct State<S> {
        bytes: std::pin::Pin<Box<S>>,
        buf: Vec<u8>,
        pending: std::collections::VecDeque<Result<ChatChunk, ProviderError>>,
        blocks: std::collections::BTreeMap<usize, Block>,
        refused: bool,
        done: bool,
        flushed: bool,
    }

    let state = State {
        bytes: Box::pin(bytes),
        buf: Vec::new(),
        pending: Default::default(),
        blocks: Default::default(),
        refused: false,
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
            while let Some(nl) = st.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = st.buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                // Every data line carries its own `type`, so `event:` lines add nothing.
                let Some(data) = line.trim_end().strip_prefix("data:") else {
                    continue;
                };
                let event: Value = match serde_json::from_str(data.trim_start()) {
                    Ok(v) => v,
                    Err(e) => {
                        st.pending
                            .push_back(Err(ProviderError::Decode(e.to_string())));
                        st.done = true;
                        st.blocks.clear();
                        break;
                    }
                };
                match event["type"].as_str().unwrap_or_default() {
                    "content_block_start" => {
                        let index = event["index"].as_u64().unwrap_or(0) as usize;
                        let block = &event["content_block"];
                        let started = match block["type"].as_str().unwrap_or_default() {
                            "text" => {
                                let text = block["text"].as_str().unwrap_or_default().to_owned();
                                if !text.is_empty() {
                                    st.pending.push_back(Ok(ChatChunk::Content(text.clone())));
                                }
                                Block::Text(text)
                            }
                            "thinking" => Block::Thinking {
                                text: block["thinking"].as_str().unwrap_or_default().to_owned(),
                                signature: block["signature"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_owned(),
                            },
                            "redacted_thinking" => Block::Redacted(block.clone()),
                            "tool_use" => Block::Tool {
                                id: block["id"].as_str().unwrap_or_default().to_owned(),
                                name: block["name"].as_str().unwrap_or_default().to_owned(),
                                input: String::new(),
                            },
                            _ => Block::Other,
                        };
                        st.blocks.insert(index, started);
                    }
                    "content_block_delta" => {
                        let index = event["index"].as_u64().unwrap_or(0) as usize;
                        let delta = &event["delta"];
                        let Some(block) = st.blocks.get_mut(&index) else {
                            continue;
                        };
                        match (block, delta["type"].as_str().unwrap_or_default()) {
                            (Block::Text(text), "text_delta") => {
                                let piece = delta["text"].as_str().unwrap_or_default();
                                if !piece.is_empty() {
                                    text.push_str(piece);
                                    st.pending
                                        .push_back(Ok(ChatChunk::Content(piece.to_owned())));
                                }
                            }
                            (Block::Thinking { text, .. }, "thinking_delta") => {
                                let piece = delta["thinking"].as_str().unwrap_or_default();
                                if !piece.is_empty() {
                                    text.push_str(piece);
                                    st.pending
                                        .push_back(Ok(ChatChunk::Reasoning(piece.to_owned())));
                                }
                            }
                            (Block::Thinking { signature, .. }, "signature_delta") => {
                                signature.push_str(delta["signature"].as_str().unwrap_or_default());
                            }
                            (Block::Tool { input, .. }, "input_json_delta") => {
                                input.push_str(delta["partial_json"].as_str().unwrap_or_default());
                            }
                            _ => {}
                        }
                    }
                    "message_delta" => {
                        if event["delta"]["stop_reason"] == "refusal" {
                            st.refused = true;
                        }
                    }
                    "message_stop" => {
                        st.done = true;
                        break;
                    }
                    "error" => {
                        let message = event["error"]["message"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| event["error"].to_string());
                        let message = match event["error"]["type"].as_str() {
                            Some("overloaded_error") => {
                                "Anthropic is overloaded right now. Try again in a moment."
                                    .to_owned()
                            }
                            _ => message,
                        };
                        st.pending.push_back(Err(ProviderError::Status(message)));
                        st.done = true;
                        // Don't act on half-received tool calls after a failed reply.
                        st.blocks.clear();
                        break;
                    }
                    _ => {}
                }
            }
            if st.done && !st.flushed {
                st.flushed = true;
                if st.refused {
                    st.pending.push_back(Err(ProviderError::Status(
                        "The model declined to answer this request.".to_owned(),
                    )));
                    continue;
                }
                let blocks = std::mem::take(&mut st.blocks);
                let calls: Vec<ToolCall> = blocks
                    .values()
                    .filter_map(|b| match b {
                        Block::Tool { id, name, input } if !name.is_empty() => Some(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: if input.trim().is_empty() {
                                "{}".to_owned()
                            } else {
                                input.clone()
                            },
                        }),
                        _ => None,
                    })
                    .collect();
                if !calls.is_empty() {
                    let replay: Vec<Value> = blocks.values().filter_map(Block::replay).collect();
                    st.pending
                        .push_back(Ok(ChatChunk::Replay(Value::Array(replay))));
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
                    st.blocks.clear();
                }
                // Cut off before `message_stop`: keep the text, but not tool calls that
                // may be incomplete.
                None => {
                    st.done = true;
                    st.blocks.clear();
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::openai::FunctionSpec;

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            kind: "function",
            function: FunctionSpec {
                name: name.to_owned(),
                description: format!("Does {name}."),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
        }
    }

    fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    #[test]
    fn prompt_becomes_a_messages_request() {
        let messages = vec![
            ChatMessage::text(Role::System, "You are Mimi."),
            ChatMessage::text(Role::User, "What's on today?"),
            ChatMessage::tool_calls(
                "Let me look.",
                &[
                    call("toolu_1", "calendar_list", "{\"day\":\"today\"}"),
                    call("call:2", "memory_search", "not json"),
                ],
            ),
            ChatMessage::tool_result("toolu_1", "Dentist at 3"),
            ChatMessage::tool_result("call:2", ""),
            ChatMessage::text(Role::Assistant, "You have the dentist at 3."),
            ChatMessage::text(Role::User, "Thanks"),
        ];
        let body = request_body(
            "claude-sonnet-5",
            &messages,
            &[tool("calendar_list"), tool("memory_search")],
            32_000,
        );

        assert_eq!(body["model"], "claude-sonnet-5");
        assert_eq!(body["max_tokens"], 32_000);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"][0]["text"], "You are Mimi.");
        // With tools, the tool list and the conversation are cached, not the system.
        assert!(body["system"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["cache_control"]["type"], "ephemeral");

        let msgs = body["messages"].as_array().unwrap();
        let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);
        let calls = msgs[1]["content"].as_array().unwrap();
        assert_eq!(calls[0], json!({"type": "text", "text": "Let me look."}));
        assert_eq!(calls[1]["input"], json!({"day": "today"}));
        // Ids are made to fit, and unreadable arguments become an empty object.
        assert_eq!(calls[2]["id"], "call_2");
        assert_eq!(calls[2]["input"], json!({}));
        // Both results go back in one user message, matched to their calls.
        let results = msgs[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[1]["tool_use_id"], "call_2");
        assert_eq!(results[1]["content"], "(no output)");
    }

    #[test]
    fn replayed_replies_are_sent_back_unchanged() {
        let replay = json!([
            {"type": "thinking", "thinking": "", "signature": "sig=="},
            {"type": "tool_use", "id": "toolu_9", "name": "lookup", "input": {"q": "cats"}},
        ]);
        let mut assistant =
            ChatMessage::tool_calls("", &[call("toolu_9", "lookup", "{\"q\":\"cats\"}")]);
        assistant.replay = Some(replay.clone());
        let messages = vec![
            ChatMessage::text(Role::User, "cats?"),
            assistant,
            ChatMessage::tool_result("toolu_9", "many"),
        ];
        let body = request_body("m", &messages, &[tool("lookup")], 1000);
        assert_eq!(body["messages"][1]["content"], replay);
    }

    #[test]
    fn without_tools_calls_are_written_as_text() {
        let messages = vec![
            ChatMessage::text(Role::System, "Sort this."),
            ChatMessage::tool_calls("", &[call("c1", "lookup", "{}")]),
            ChatMessage::tool_result("c1", "found it"),
        ];
        let body = request_body("m", &messages, &[], 1000);
        assert!(body.get("tools").is_none());
        assert!(body.get("cache_control").is_none());
        // A stable system prompt is worth caching for jobs like sorting mail.
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        let msgs = body["messages"].as_array().unwrap();
        // Opens with the user, as the API requires.
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["content"][0]["text"], "(Used lookup with {})");
        assert_eq!(
            msgs[2]["content"][0]["text"],
            "(Result of lookup:)\nfound it"
        );
        assert!(
            serde_json::to_string(&body)
                .unwrap()
                .find("tool_use")
                .is_none()
        );
    }

    fn sse(events: &[Value]) -> Vec<Result<bytes::Bytes, ProviderError>> {
        let mut body = String::new();
        for e in events {
            body.push_str(&format!(
                "event: {}\ndata: {e}\n\n",
                e["type"].as_str().unwrap()
            ));
        }
        // Split mid-line to exercise buffering.
        let (a, b) = body.split_at(body.len() / 2);
        vec![
            Ok(bytes::Bytes::from(a.to_owned())),
            Ok(bytes::Bytes::from(b.to_owned())),
        ]
    }

    async fn run(events: &[Value]) -> Vec<Result<ChatChunk, ProviderError>> {
        parse_sse(futures::stream::iter(sse(events)))
            .collect()
            .await
    }

    #[tokio::test]
    async fn streams_text_and_thinking() {
        let chunks = run(&[
            json!({"type": "message_start", "message": {"id": "msg_1"}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "hmm"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "abc"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hel"}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "lo"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
            json!({"type": "message_stop"}),
        ])
        .await;
        let chunks: Vec<ChatChunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [
                ChatChunk::Reasoning("hmm".into()),
                ChatChunk::Content("Hel".into()),
                ChatChunk::Content("lo".into()),
            ]
        );
    }

    #[tokio::test]
    async fn tool_calls_come_with_the_reply_to_send_back() {
        let chunks = run(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig=="}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Checking."}}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_a", "name": "lookup", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"q\":"}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"cats\"}"}}),
            json!({"type": "content_block_start", "index": 3, "content_block": {"type": "tool_use", "id": "toolu_b", "name": "today", "input": {}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
            json!({"type": "message_stop"}),
        ])
        .await;
        let chunks: Vec<ChatChunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [
                ChatChunk::Content("Checking.".into()),
                ChatChunk::Replay(json!([
                    {"type": "thinking", "thinking": "", "signature": "sig=="},
                    {"type": "text", "text": "Checking."},
                    {"type": "tool_use", "id": "toolu_a", "name": "lookup", "input": {"q": "cats"}},
                    {"type": "tool_use", "id": "toolu_b", "name": "today", "input": {}},
                ])),
                ChatChunk::ToolCalls(vec![
                    call("toolu_a", "lookup", "{\"q\":\"cats\"}"),
                    call("toolu_b", "today", "{}")
                ]),
            ]
        );
    }

    #[tokio::test]
    async fn errors_refusals_and_cut_offs() {
        let chunks = run(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": "Par"}}),
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
        ])
        .await;
        assert!(matches!(&chunks[0], Ok(ChatChunk::Content(t)) if t == "Par"));
        assert!(matches!(&chunks[1], Err(ProviderError::Status(m)) if m.contains("overloaded")));

        let chunks = run(&[
            json!({"type": "message_delta", "delta": {"stop_reason": "refusal"}}),
            json!({"type": "message_stop"}),
        ])
        .await;
        assert!(matches!(&chunks[..], [Err(ProviderError::Status(m))] if m.contains("declined")));

        // A stream that ends early keeps its text but drops half-received tool calls.
        let chunks = run(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": "Hi"}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "t", "name": "send", "input": {}}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"to\":"}}),
        ])
        .await;
        let chunks: Vec<ChatChunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(chunks, [ChatChunk::Content("Hi".into())]);
    }

    /// A fake Messages API: checks the headers, lists two pages of models and answers
    /// every message with "OK", recording the requests.
    async fn fake_api() -> (Url, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        use axum::Json;
        use axum::extract::Query;
        use axum::http::{HeaderMap, StatusCode};
        use axum::response::IntoResponse;
        use axum::routing::{get, post};

        fn authorized(headers: &HeaderMap) -> bool {
            headers.get("x-api-key").is_some_and(|k| k == "sk-test")
                && headers
                    .get("anthropic-version")
                    .is_some_and(|v| v == API_VERSION)
        }
        let seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = Default::default();
        let recorded = seen.clone();
        let app = axum::Router::new()
            .route(
                "/v1/models",
                get(|headers: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                    if !authorized(&headers) {
                        return (StatusCode::UNAUTHORIZED, "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}").into_response();
                    }
                    let page = if q.get("after_id").map(String::as_str) == Some("claude-sonnet-5") {
                        json!({"data": [{"id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5", "max_tokens": 64000}], "has_more": false, "last_id": "claude-haiku-4-5"})
                    } else {
                        json!({"data": [{"id": "claude-sonnet-5", "display_name": "Claude Sonnet 5", "max_tokens": 128000}], "has_more": true, "last_id": "claude-sonnet-5"})
                    };
                    Json(page).into_response()
                }),
            )
            .route(
                "/v1/models/{id}",
                get(|| async { Json(json!({"id": "claude-old", "max_tokens": 4096})) }),
            )
            .route(
                "/v1/messages",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let recorded = recorded.clone();
                    async move {
                        if !authorized(&headers) {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        recorded.lock().unwrap().push(body);
                        concat!(
                            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n",
                            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                        )
                        .into_response()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/v1", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    #[tokio::test]
    async fn lists_models_and_chats_against_the_api() {
        let (url, seen) = fake_api().await;
        let client = Anthropic::new(reqwest::Client::new(), url.clone(), Some("sk-test".into()));
        let models = client.list_models().await.unwrap();
        assert_eq!(
            models,
            [
                ModelInfo {
                    id: "claude-sonnet-5".into(),
                    name: Some("Claude Sonnet 5".into()),
                    size_bytes: None,
                    supports_tools: Some(true),
                    price: None,
                },
                ModelInfo {
                    id: "claude-haiku-4-5".into(),
                    name: Some("Claude Haiku 4.5".into()),
                    size_bytes: None,
                    supports_tools: Some(true),
                    price: None,
                },
            ]
        );

        let messages = [ChatMessage::text(Role::User, "hi")];
        let mut stream = client
            .stream_chat_with("claude-sonnet-5", &messages, &[], ChatOptions::QUICK)
            .await
            .unwrap();
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            ChatChunk::Content("OK".into())
        );
        // Capped for chat, from the limit the model list reported.
        assert_eq!(seen.lock().unwrap()[0]["max_tokens"], MAX_OUTPUT);
        // A model that wasn't listed is looked up once.
        let _reply = client
            .stream_chat_with("claude-old", &messages, &[], ChatOptions::QUICK)
            .await
            .unwrap();
        assert_eq!(seen.lock().unwrap()[1]["max_tokens"], 4096);

        let wrong = Anthropic::new(reqwest::Client::new(), url, Some("sk-wrong".into()));
        assert!(matches!(
            wrong.list_models().await,
            Err(ProviderError::Unauthorized)
        ));
    }
}
