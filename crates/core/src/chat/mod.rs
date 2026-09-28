//! Conversations and the reply loop: persist the user's message, stream the model's
//! answer to clients as it arrives, and save it when done.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use mimi_protocol::{
    Action, ActionStatus, Conversation, Event, Mention, Message, MessageRole, MessageStatus,
    ModelRef, SendMessageResult,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::api::error::AppError;
use crate::providers::{
    self, ChatChunk, ChatMessage, ProviderError, Role, ToolCall, store as provider_store,
};
use crate::tools::{Decision, ToolContext, ToolRegistry};
use crate::{AppState, now_ms, settings};

pub mod store;

pub const DEFAULT_TITLE: &str = "New conversation";

/// Roughly how much history (in characters) goes to the model with each message.
/// About 8k tokens, which every model we suggest can handle.
const HISTORY_BUDGET_CHARS: usize = 32_000;

/// Replies being written right now, by conversation. One at a time per conversation.
#[derive(Default)]
pub struct Generations(Mutex<HashMap<Uuid, CancellationToken>>);

impl Generations {
    /// Whether any reply is being written right now.
    pub fn any(&self) -> bool {
        !self.0.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    fn start(&self, conversation_id: Uuid) -> Option<CancellationToken> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if map.contains_key(&conversation_id) {
            return None;
        }
        let token = CancellationToken::new();
        map.insert(conversation_id, token.clone());
        Some(token)
    }

    fn finish(&self, conversation_id: Uuid) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&conversation_id);
    }

    pub fn is_running(&self, conversation_id: Uuid) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&conversation_id)
    }

    /// Returns whether a reply was in progress.
    pub fn cancel(&self, conversation_id: Uuid) -> bool {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&conversation_id)
            .map(CancellationToken::cancel)
            .is_some()
    }
}

pub async fn send(
    state: Arc<AppState>,
    conversation_id: Uuid,
    content: String,
    model: Option<ModelRef>,
    mentions: Vec<Mention>,
) -> Result<SendMessageResult, AppError> {
    send_with_context(state, conversation_id, content, model, mentions, None).await
}

/// Like [`send`], with extra context for the model that the user doesn't see in the
/// conversation (e.g. that a message was sent by a scheduled routine). It's kept with the
/// message, like mention context, so later turns see it too.
pub async fn send_with_context(
    state: Arc<AppState>,
    conversation_id: Uuid,
    content: String,
    model: Option<ModelRef>,
    mentions: Vec<Mention>,
    hidden_context: Option<String>,
) -> Result<SendMessageResult, AppError> {
    let content = content.trim().to_owned();
    if content.is_empty() {
        return Err(AppError::bad_request("The message is empty."));
    }
    // Only mentions that are still in the text count; a deleted "@Sam" means no Sam.
    let mut mentions: Vec<Mention> = mentions
        .into_iter()
        .filter(|m| {
            !m.label.trim().is_empty()
                && content.contains(&format!("{}{}", m.kind.sigil(), m.label))
        })
        .take(20)
        .collect();
    mentions.dedup_by(|a, b| a.kind == b.kind && a.id == b.id);
    let mut conversation = store::get_conversation(&state.db, conversation_id)
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?;
    let settings = settings::load(&state.db).await?;
    let model = model.or(settings.default_model).ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "no_model",
            "Choose a model before sending messages.",
        )
    })?;
    let provider = provider_store::get(&state.db, model.provider_id)
        .await?
        .ok_or_else(|| {
            AppError::new(
                axum::http::StatusCode::BAD_REQUEST,
                "no_model",
                "The chosen model's provider was removed. Choose another model.",
            )
        })?;
    // Check the source up front so the user hears about a broken setup right away. The
    // client itself is made in the reply task: the built-in runtime may need a while to
    // load the model, and the message should appear immediately.
    if provider.provider.kind == mimi_protocol::ProviderKind::Builtin {
        if !state.runtime.available() {
            return Err(AppError::bad_request(
                "Mimi's built-in model runtime isn't installed on this computer. Choose another model in Models.",
            ));
        }
        if crate::runtime::download::installed_path(&state.paths, &model.model).is_none() {
            return Err(AppError::new(
                axum::http::StatusCode::BAD_REQUEST,
                "no_model",
                "The chosen model isn't on this computer anymore. Download it again or choose another in Models.",
            ));
        }
    } else {
        providers::connect(&state.http, &provider).map_err(AppError::bad_request)?;
    }
    let mention_context = match (
        crate::people::mentions::resolve(&state, &mentions).await,
        hidden_context,
    ) {
        (Some(a), Some(b)) => Some(format!("{b}\n\n{a}")),
        (a, b) => a.or(b),
    };

    let cancel = state.generations.start(conversation_id).ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::CONFLICT,
            "busy",
            "The assistant is still replying in this conversation.",
        )
    })?;

    // Build the model's view of the conversation before adding the new messages.
    let history = store::messages(&state.db, conversation_id).await;
    let history = match history {
        Ok(h) => h,
        Err(e) => {
            state.generations.finish(conversation_id);
            return Err(e.into());
        }
    };
    // What's remembered about the user that matters for this message; the previous
    // user message helps with follow-ups ("and what does she like?").
    let previous = history
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::User)
        .map(|m| m.content.as_str());
    let query = crate::memory::recall::Query {
        message: &content,
        context: previous,
        people: crate::memory::link::people_in(&state, &content, &mentions).await,
        meaning: None,
    };
    let recall = crate::memory::recall::recall(&state, query)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("recalling memories failed: {e}");
            Default::default()
        });
    let memory = PromptMemory {
        block: crate::memory::recall::prompt_block(&recall),
        learning: settings.memory_learning,
    };
    let contexts = store::mention_contexts(&state.db, conversation_id)
        .await
        .unwrap_or_default();
    let prompt = build_prompt(
        &settings.assistant_name,
        &history,
        &contexts,
        &with_context(&content, mention_context.as_deref()),
        &memory,
    );
    tracing::debug!(
        %conversation_id,
        prompt_messages = prompt.len(),
        model = %model.model,
        "sending to model"
    );

    let now = now_ms();
    let user_message = Message {
        id: Uuid::now_v7(),
        conversation_id,
        role: MessageRole::User,
        content: content.clone(),
        reasoning: String::new(),
        status: MessageStatus::Complete,
        model: None,
        locality: None,
        error: None,
        created_at: now,
        actions: Vec::new(),
        mentions,
    };
    let assistant_message = Message {
        id: Uuid::now_v7(),
        role: MessageRole::Assistant,
        content: String::new(),
        status: MessageStatus::Streaming,
        model: Some(model.clone()),
        locality: Some(provider.provider.locality),
        mentions: Vec::new(),
        ..user_message.clone()
    };
    if conversation.title == DEFAULT_TITLE && history.is_empty() {
        conversation.title = title_from(&content);
    }
    conversation.updated_at = now;

    let saved = async {
        store::upsert_message(&state.db, user_message.clone()).await?;
        if let Some(context) = mention_context {
            store::set_mention_context(&state.db, user_message.id, context).await?;
        }
        store::upsert_message(&state.db, assistant_message.clone()).await?;
        store::upsert_conversation(&state.db, conversation.clone()).await
    }
    .await;
    if let Err(e) = saved {
        state.generations.finish(conversation_id);
        return Err(e.into());
    }
    state.events.publish(Event::MessageUpdated {
        message: user_message.clone(),
    });
    state.events.publish(Event::MessageUpdated {
        message: assistant_message.clone(),
    });
    state
        .events
        .publish(Event::ConversationUpdated { conversation });

    let tools = state.tool_sources.registry(&state).await;
    tokio::spawn(generate(
        state.clone(),
        provider,
        model.model,
        prompt,
        assistant_message.clone(),
        cancel,
        tools,
    ));

    Ok(SendMessageResult {
        user_message,
        assistant_message,
    })
}

/// Most model rounds that may use tools in one reply. After that the model is asked
/// once more without tools, so it has to answer in words.
pub(crate) const MAX_TOOL_ROUNDS: u32 = 8;

/// Tool output handed back to the model is cut at this many characters.
const TOOL_OUTPUT_LIMIT: usize = 16_000;

/// One assistant reply in progress: model rounds, tool calls and approvals.
struct Turn {
    state: Arc<AppState>,
    client: providers::ChatClient,
    model: String,
    prompt: Vec<ChatMessage>,
    message: Message,
    cancel: CancellationToken,
    tools: ToolRegistry,
    /// Put a paragraph break before the next text, because tools ran since the last.
    separate: bool,
}

async fn generate(
    state: Arc<AppState>,
    provider: providers::store::ProviderRecord,
    model: String,
    prompt: Vec<ChatMessage>,
    message: Message,
    cancel: CancellationToken,
    tools: ToolRegistry,
) {
    let client = tokio::select! {
        c = providers::chat_client(&state, &provider, &model) => c,
        _ = cancel.cancelled() => Err(String::new()),
    };
    let (state, mut message, cancel, outcome) = match client {
        Ok(client) => {
            let mut turn = Turn {
                state,
                client,
                model,
                prompt,
                message,
                cancel,
                tools,
                separate: false,
            };
            let outcome = turn.run().await;
            let Turn {
                state,
                message,
                cancel,
                ..
            } = turn;
            (state, message, cancel, outcome)
        }
        // Stopped while the model was still loading: nothing to report.
        Err(_) if cancel.is_cancelled() => (state, message, cancel, Ok(())),
        Err(e) => (state, message, cancel, Err(e)),
    };
    let conversation_id = message.conversation_id;

    message.status = match &outcome {
        Err(_) => MessageStatus::Error,
        Ok(()) if cancel.is_cancelled() => MessageStatus::Cancelled,
        Ok(()) => MessageStatus::Complete,
    };
    message.error = outcome.err();
    // Trimming shifts the text, so shift the actions' positions in it too.
    let leading = message
        .content
        .chars()
        .take_while(|c| c.is_whitespace())
        .count() as u32;
    message.content = message.content.trim().to_owned();
    let len = message.content.chars().count() as u32;
    for a in &mut message.actions {
        a.content_offset = a.content_offset.saturating_sub(leading).min(len);
    }
    message.reasoning = message.reasoning.trim().to_owned();
    for a in &mut message.actions {
        if matches!(
            a.status,
            ActionStatus::PendingApproval | ActionStatus::Approved | ActionStatus::Running
        ) {
            state.approvals.forget(a.id);
            a.status = ActionStatus::Failed;
            a.error = Some("Stopped before it ran.".to_owned());
        }
    }

    if let Err(e) = store::upsert_message(&state.db, message.clone()).await {
        tracing::error!("saving the assistant's reply failed: {e}");
    }
    if let Ok(Some(mut conversation)) = store::get_conversation(&state.db, conversation_id).await {
        conversation.updated_at = now_ms();
        if store::upsert_conversation(&state.db, conversation.clone())
            .await
            .is_ok()
        {
            state
                .events
                .publish(Event::ConversationUpdated { conversation });
        }
    }
    state.generations.finish(conversation_id);
    state.events.publish(Event::MessageUpdated { message });
    // Once the conversation goes quiet, learn from it.
    state.learner.schedule(conversation_id);
}

impl Turn {
    /// Runs model rounds until the model answers without calling tools. `Ok` also
    /// covers being cancelled; the caller tells the two apart.
    async fn run(&mut self) -> Result<(), String> {
        let mut offer_tools = !self.tools.is_empty();
        let mut retried_without_tools = false;
        let mut round: u32 = 0;
        loop {
            let specs = if offer_tools && round < MAX_TOOL_ROUNDS {
                self.tools.specs()
            } else {
                Vec::new()
            };
            let started = tokio::select! {
                s = self.client.stream_chat(&self.model, &self.prompt, &specs) => s,
                _ = self.cancel.cancelled() => return Ok(()),
            };
            let mut stream = match started {
                Ok(stream) => stream,
                // Some models and servers don't support tools; carry on as plain chat.
                Err(ProviderError::Status(msg))
                    if !specs.is_empty()
                        && !retried_without_tools
                        && msg.to_lowercase().contains("tool") =>
                {
                    tracing::info!(model = %self.model, "model can't use tools ({msg}); continuing without");
                    offer_tools = false;
                    retried_without_tools = true;
                    strip_tool_messages(&mut self.prompt);
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };

            let mut calls = Vec::new();
            let mut replay = None;
            let mut text = String::new();
            loop {
                let chunk = tokio::select! {
                    c = stream.next() => c,
                    _ = self.cancel.cancelled() => return Ok(()),
                };
                match chunk {
                    None => break,
                    Some(Err(e)) => return Err(e.to_string()),
                    Some(Ok(ChatChunk::Content(c))) => {
                        text.push_str(&c);
                        self.append(c, String::new());
                    }
                    Some(Ok(ChatChunk::Reasoning(r))) => self.append(String::new(), r),
                    Some(Ok(ChatChunk::ToolCalls(c))) => calls = c,
                    Some(Ok(ChatChunk::Replay(r))) => replay = Some(r),
                }
            }
            // Calls when no tools were offered (after the round cap) are ignored.
            if calls.is_empty() || specs.is_empty() {
                return Ok(());
            }

            self.prompt
                .push(ChatMessage::tool_calls(text, &calls).with_replay(replay));
            self.separate = true;
            for call in calls {
                if !self.act(call, round).await {
                    return Ok(());
                }
            }
            round += 1;
        }
    }

    fn append(&mut self, mut content: String, reasoning: String) {
        if self.separate && !content.trim().is_empty() {
            self.separate = false;
            if !self.message.content.trim().is_empty() {
                content = format!("\n\n{}", content.trim_start());
            }
        }
        self.message.content.push_str(&content);
        self.message.reasoning.push_str(&reasoning);
        self.state.events.publish(Event::MessageDelta {
            conversation_id: self.message.conversation_id,
            message_id: self.message.id,
            content,
            reasoning,
        });
    }

    /// Handles one tool call: asks for approval when needed, runs it, and gives the
    /// result back to the model. Returns false when the reply was cancelled meanwhile.
    async fn act(&mut self, call: ToolCall, round: u32) -> bool {
        let tool = self.tools.get(&call.name).cloned();
        let parsed: Result<Value, String> = if call.arguments.trim().is_empty() {
            Ok(serde_json::json!({}))
        } else {
            serde_json::from_str(&call.arguments)
                .map_err(|e| format!("The arguments weren't valid JSON: {e}"))
        };
        // What the card shows is what runs: the arguments in the shape the tool reads.
        let parsed = match (&tool, parsed) {
            (Some(t), Ok(args)) if t.needs_approval(&args) => t.prepare(args),
            (_, parsed) => parsed,
        };
        let shown_args = parsed
            .clone()
            .unwrap_or(Value::String(call.arguments.clone()));
        let (summary, requires_approval) = match (&tool, &parsed) {
            (Some(t), Ok(args)) => (t.summary(args), t.needs_approval(args)),
            // Can't tell what it would do, and it won't run anyway.
            _ => (call.name.clone(), false),
        };
        self.message.actions.push(Action {
            id: Uuid::now_v7(),
            tool: call.name.clone(),
            summary,
            arguments: shown_args,
            requires_approval,
            status: ActionStatus::Running,
            result: None,
            error: None,
            output: None,
            call_id: call.id.clone(),
            round,
            content_offset: self.message.content.chars().count() as u32,
        });
        let idx = self.message.actions.len() - 1;
        let action_id = self.message.actions[idx].id;

        let (tool, mut args) = match (tool, parsed) {
            (None, _) => {
                self.finish_action(idx, Err(format!("There is no tool called {}.", call.name)))
                    .await;
                return true;
            }
            (_, Err(e)) => {
                self.finish_action(idx, Err(e)).await;
                return true;
            }
            (Some(tool), Ok(args)) => (tool, args),
        };

        if requires_approval {
            self.message.actions[idx].status = ActionStatus::PendingApproval;
            self.save().await;
            let decision = self.state.approvals.wait(action_id);
            let decision = tokio::select! {
                d = decision => d.ok(),
                _ = self.cancel.cancelled() => {
                    self.state.approvals.forget(action_id);
                    self.fail_unrun(idx, "Stopped before it ran.");
                    return false;
                }
            };
            match decision {
                Some(Decision::Approve(edited)) => {
                    if let Some(edited) = edited {
                        args = match tool.prepare(edited) {
                            Ok(args) => args,
                            Err(e) => {
                                self.finish_action(idx, Err(e)).await;
                                return true;
                            }
                        };
                        let a = &mut self.message.actions[idx];
                        a.summary = tool.summary(&args);
                        a.arguments = args.clone();
                    }
                    self.message.actions[idx].status = ActionStatus::Approved;
                }
                Some(Decision::Reject) | None => {
                    self.message.actions[idx].status = ActionStatus::Rejected;
                    self.save().await;
                    self.prompt
                        .push(ChatMessage::tool_result(&call.id, declined_note()));
                    return true;
                }
            }
        }

        self.message.actions[idx].status = ActionStatus::Running;
        self.save().await;
        let ctx = ToolContext {
            state: self.state.clone(),
            conversation_id: self.message.conversation_id,
        };
        let result = tokio::select! {
            r = tool.run(&ctx, args.clone()) => r,
            _ = self.cancel.cancelled() => {
                self.fail_unrun(idx, "Stopped while it was running. It may or may not have finished.");
                return false;
            }
        };
        if let Ok(output) = &result {
            self.message.actions[idx].result = Some(tool.result_label(&args, output));
        }
        self.finish_action(idx, result).await;
        true
    }

    /// Records a tool's outcome and hands it to the model.
    async fn finish_action(&mut self, idx: usize, result: Result<Value, String>) {
        let a = &mut self.message.actions[idx];
        match result {
            Ok(output) => {
                a.status = ActionStatus::Done;
                a.output = Some(output);
            }
            Err(e) => {
                a.status = ActionStatus::Failed;
                a.error = Some(e);
            }
        }
        let reply = tool_output_text(a);
        let call_id = a.call_id.clone();
        self.save().await;
        self.prompt.push(ChatMessage::tool_result(call_id, reply));
    }

    fn fail_unrun(&mut self, idx: usize, why: &str) {
        let a = &mut self.message.actions[idx];
        a.status = ActionStatus::Failed;
        a.error = Some(why.to_owned());
    }

    /// Persists the message and tells clients, so action states show up live.
    async fn save(&self) {
        if let Err(e) = store::upsert_message(&self.state.db, self.message.clone()).await {
            tracing::error!("saving the assistant's reply failed: {e}");
        }
        self.state.events.publish(Event::MessageUpdated {
            message: self.message.clone(),
        });
    }
}

fn declined_note() -> String {
    serde_json::json!({
        "declined": true,
        "note": "The user declined this action, so it did not happen. Don't try it again unless they ask."
    })
    .to_string()
}

/// What the model is told about an action's outcome.
fn tool_output_text(a: &Action) -> String {
    match a.status {
        ActionStatus::Done => {
            let text = a
                .output
                .as_ref()
                .map_or_else(|| "null".to_owned(), Value::to_string);
            if text.len() > TOOL_OUTPUT_LIMIT {
                let cut = (0..=TOOL_OUTPUT_LIMIT)
                    .rev()
                    .find(|&i| text.is_char_boundary(i))
                    .unwrap_or(0);
                format!("{}… (truncated)", &text[..cut])
            } else {
                text
            }
        }
        ActionStatus::Rejected => declined_note(),
        _ => serde_json::json!({
            "error": a.error.clone().unwrap_or_else(|| "It did not run.".to_owned())
        })
        .to_string(),
    }
}

/// Drops tool calls and results, for models that don't accept them.
fn strip_tool_messages(prompt: &mut Vec<ChatMessage>) {
    prompt.retain(|m| m.role != Role::Tool && m.tool_calls.is_empty());
}

/// A past message as the model saw it: tool calls and results by round, then the text.
fn replay(m: &Message, contexts: &HashMap<Uuid, String>) -> Vec<ChatMessage> {
    if m.role == MessageRole::User {
        let context = contexts.get(&m.id).map(String::as_str);
        return vec![ChatMessage::text(
            Role::User,
            with_context(&m.content, context),
        )];
    }
    let mut rounds: std::collections::BTreeMap<u32, Vec<&Action>> = Default::default();
    for a in &m.actions {
        rounds.entry(a.round).or_default().push(a);
    }
    let mut out = Vec::new();
    for actions in rounds.values() {
        let calls: Vec<ToolCall> = actions
            .iter()
            .map(|a| ToolCall {
                id: a.call_id.clone(),
                name: a.tool.clone(),
                arguments: a.arguments.to_string(),
            })
            .collect();
        out.push(ChatMessage::tool_calls(String::new(), &calls));
        for a in actions {
            out.push(ChatMessage::tool_result(
                a.call_id.clone(),
                tool_output_text(a),
            ));
        }
    }
    if !m.content.is_empty() {
        out.push(ChatMessage::text(Role::Assistant, m.content.clone()));
    }
    out
}

/// Memory for one prompt: what's recalled, and whether new things may be remembered.
#[derive(Default)]
struct PromptMemory {
    block: Option<String>,
    learning: bool,
}

/// A user message as the model reads it: the text, then what its @ and # mentions
/// refer to.
fn with_context(content: &str, mention_context: Option<&str>) -> String {
    match mention_context {
        Some(context) => format!("{content}\n\n{context}"),
        None => content.to_owned(),
    }
}

fn build_prompt(
    assistant_name: &str,
    history: &[Message],
    contexts: &HashMap<Uuid, String>,
    new_message: &str,
    memory: &PromptMemory,
) -> Vec<ChatMessage> {
    let now = jiff::Zoned::now();
    let mut system = format!(
        "You are {assistant_name}, a personal assistant. You run on the user's own \
         computer, and their conversations stay private. Be helpful, direct and warm. \
         Answer in the user's language. Use Markdown when it helps readability. When a \
         tool can answer or do what the user asks, use it; actions that change something \
         are shown to the user for approval before they happen.\n\n\
         Current date and time: {}.",
        now.strftime("%A, %B %-d, %Y, %H:%M (%Z)")
    );
    // Short on purpose: small local models follow a few clear lines best.
    system.push_str(
        "\n\nYou have a private long-term memory about the user. Use what you remember \
         naturally, without saying where it comes from. If something may have been \
         mentioned before but isn't below, look it up with memory_search.",
    );
    if memory.learning {
        system.push_str(
            " When the user tells you something lasting about themselves or people in \
             their life, save it with memory_write, then carry on with your answer.",
        );
    }
    if let Some(block) = &memory.block {
        system.push_str("\n\n");
        system.push_str(block);
    }

    // Newest history first until the budget runs out, then back in order.
    let mut budget = HISTORY_BUDGET_CHARS.saturating_sub(new_message.len());
    let mut kept: Vec<Vec<ChatMessage>> = Vec::new();
    for m in history.iter().rev() {
        if m.status == MessageStatus::Streaming || (m.content.is_empty() && m.actions.is_empty()) {
            continue;
        }
        let block = replay(m, contexts);
        let size: usize = block.iter().map(|c| c.content.len()).sum();
        if size > budget {
            break;
        }
        budget -= size;
        kept.push(block);
    }
    kept.reverse();

    let mut prompt = vec![ChatMessage::text(Role::System, system)];
    prompt.extend(kept.into_iter().flatten());
    prompt.push(ChatMessage::text(Role::User, new_message));
    prompt
}

/// A short title from the first message: its first line, cut at a word boundary.
pub fn title_from(content: &str) -> String {
    const MAX: usize = 48;
    let line = content.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut title = String::new();
    for word in words {
        let next_len =
            title.chars().count() + word.chars().count() + usize::from(!title.is_empty());
        if next_len > MAX {
            if title.is_empty() {
                title = word.chars().take(MAX).collect();
            }
            title.push('…');
            return title;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if title.is_empty() {
        DEFAULT_TITLE.to_owned()
    } else {
        title
    }
}

pub fn new_conversation(title: Option<String>) -> Conversation {
    let now = now_ms();
    Conversation {
        id: Uuid::now_v7(),
        title: title
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| DEFAULT_TITLE.to_owned()),
        created_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            title_from("What's the weather like?"),
            "What's the weather like?"
        );
        assert_eq!(
            title_from(
                "\n  Can you help me plan a trip to Lisbon next month with my family and our dog?"
            ),
            "Can you help me plan a trip to Lisbon next month…"
        );
        assert_eq!(title_from("   "), DEFAULT_TITLE);
    }
}
