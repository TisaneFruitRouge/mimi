# Models

Where the assistant's models come from and how Mimi talks to them. Model sources are
OpenAI-compatible endpoints (Ollama, LM Studio, a server on the network, a cloud
service), Anthropic's Messages API (`ProviderKind::Anthropic`, the user's own API key),
plus one that is part of Mimi: **Built into Mimi**, backed by llama.cpp's `llama-server`,
which ships with the installers, so nobody has to install a model runner (nobody needs
Ollama). Sources live in `crates/core/src/providers/`, the built-in runtime in
`crates/core/src/runtime/`, and the suggested models in
`crates/core/src/hardware/catalog.json`.

## Rules

- **Always get a chat client through `providers::chat_client`** (and list models through
  `providers::list_models`): they route the built-in source to the runtime. Don't call
  `providers::connect` for a record that might be built-in (its address changes).
- **Internal model calls don't think.** Learning, mail triage and anything else the user
  doesn't read as it streams use `ChatClient::complete(model, messages,
  ChatOptions::QUICK)`: thinking off on local sources, a harmless no-op elsewhere. Chats
  keep `stream_chat`.
- **Cloud is always visible.** Every place a model is chosen or used shows its locality
  (`LocalityBadge`; see [Frontend](frontend.md)). An Anthropic source is always `cloud`,
  whatever its address. Local is the default and the recommended choice.
- **The built-in source belongs to the daemon.** It is created at startup whenever the
  daemon finds `llama-server` (`ProviderKind::Builtin`, device locality); it can't be
  added, edited or removed through the API.
- **`llama-server` stays private.** Loopback only, a random port, a random API key per
  start passed via `--api-key-file` (never on the command line), `--no-webui`,
  `--offline`.
- **Downloads are pinned.** Every catalog model pins a single GGUF file on Hugging Face
  (repo, file, size, SHA-256) and is downloaded straight from huggingface.co; a file only
  takes its real name once its SHA-256 matches. A new catalog model needs its `gguf`
  block too (single-file GGUFs only; the sha256 is on the Hugging Face file page, from
  the repo's LFS metadata).
- Model names appear only on the Models page (and in onboarding, which may name models
  like the Models page; see [Onboarding](onboarding.md)).

## Model sources

```
 chat / memory learning
        │ providers::chat_client(source, model) -> ChatClient
        ├── OpenAI-compatible source ──► its URL (Ollama, LM Studio, cloud, …)
        ├── Anthropic source ──► https://api.anthropic.com/v1/messages
        └── built-in source ──► Runtime::ensure(model)
                                  │ start or reuse llama-server:
                                  │ 127.0.0.1:<random port>, per-start API key file,
                                  │ context by hardware tier, GPU layers auto
                                  └► http://127.0.0.1:<port>/v1
```

`ChatClient` is either the OpenAI-compatible client (`OpenAiCompatible`) or `Anthropic`.
`providers::list_models` lists what the built-in source has downloaded
(`runtime::download::installed`) and asks the other sources. Hardware detection drives
the recommendations (cloud models on weak machines, large local models on big GPUs, and
models the user already has installed); see [Onboarding](onboarding.md).

Whether a model can see photos (`providers/vision.rs`), and the optional model for
photos, are in [Photos](photos.md). The embedding model for recall by meaning is in
[Memory](memory.md).

## Anthropic

`providers/anthropic.rs` translates Mimi's OpenAI-shaped prompt: system messages become
`system`, tool calls `tool_use` blocks, tool results one user message.

- When Claude calls tools, its reply (signed thinking blocks included) comes back as
  `ChatChunk::Replay` and rides on the in-memory prompt (`ChatMessage::replay`), so the
  next round sends it unchanged, as the API requires. Saved history is replayed without
  thinking.
- The tool list and, in tool loops, the conversation are marked for prompt caching.
- Replies are capped at the model's own output limit (from `/models`) or 32k.
- Thinking is left to each model's default: `ChatOptions` has no effect, since the
  switches differ between Claude models.
- An Anthropic source is always `cloud`, whatever its address.

## Thinking and internal jobs

`ChatClient::complete(model, messages, ChatOptions::QUICK)` returns a whole answer
without tools and without the model's reasoning, for background work (memory learning,
mail triage). `stream_chat_with` takes the same `ChatOptions { thinking }` for streaming.

With thinking off, local sources (built-in, device or network) get
`reasoning_effort: "none"` (what Ollama's OpenAI endpoint honours; `think` is ignored
there) and `chat_template_kwargs.enable_thinking: false` (llama.cpp with Qwen3-style
templates); a server that rejects them gets the request again without. Cloud sources
never get the fields. Chats keep thinking.

## The built-in runtime

`Runtime::ensure` starts `llama-server` on demand for the requested model.

- **One model at a time.** `ensure` reuses the running server when it serves the same
  model and is alive, else stops it and starts a new one, then waits for `/health`
  (large models can take a while to load; the chat shows the reply as pending).
- **Lifetime.** Started lazily on the first message; stopped after 20 idle minutes (never
  during a reply), when its model is deleted, and when the daemon exits. A crashed server
  is restarted by the next message.
- **Security.** It listens on loopback only, on a random port, and requires a random key
  generated at every start, written to an owner-only file (`--api-key-file`, so the key
  isn't visible in the process list). The web UI is disabled (`--no-webui`) and it never
  fetches anything itself (`--offline`).
- **Context size** follows the hardware tier: 4k (minimal), 8k (light), 16k (standard),
  32k (strong, workstation).
- **GPU.** GPU layers use llama.cpp's automatic fit. macOS builds use Metal. Linux builds
  use Vulkan, which covers AMD, Intel and NVIDIA; ggml loads the Vulkan backend as a
  plugin and falls back to its CPU backends (picked for the CPU's instruction set) when
  there's no Vulkan driver.
- **Log:** `<data>/logs/model-runtime.log`.
- **Status:** `GET /v1/runtime` and `RuntimeChanged` events.

Memory's "Understands meaning" runs a second `llama-server --embedding` on the CPU; see
[Memory](memory.md).

### Finding `llama-server`

`runtime::find_binary`:

1. `MIMI_LLAMA_SERVER`;
2. beside `mimid` (`llama/llama-server`, `llama-server`);
3. the bundle's resource folder: `../Resources/llama/` on macOS
   (`Mimi.app/Contents/Resources/llama/`), `../lib/<app>/llama/` on Linux
   (`/usr/lib/Mimi/llama/`, `$APPDIR/usr/lib/Mimi/llama/` in an AppImage);
4. `PATH`.

Without one there's no built-in source and Mimi recommends Ollama instead.

For development: `scripts/fetch-llama-server.sh`, then
`MIMI_LLAMA_SERVER=$PWD/apps/desktop/src-tauri/resources/llama/llama-server`. How the
binary gets into installers, and how to bump llama.cpp, is in [Packaging](packaging.md).

## Downloads

`runtime/download.rs`.

- **The catalog.** `hardware/catalog.json` lists the suggested open models, keyed by
  their Ollama tag, with the Q4 download size (`download_gb`), `moe` for
  mixture-of-experts models (which stay fast on CPUs despite their size), and a `gguf`
  block (`repo`, `file`, `bytes`, `sha256`, `quant`) for the built-in runtime. The
  embedding model is its `embeddings` entry.
- **Downloading.** The daemon downloads the file directly from huggingface.co into
  `<data>/models/`, as `<file>.part` until complete. There must be room for the rest plus
  a margin before a download starts. Stopping or a failure keeps the part; starting again
  resumes with an HTTP Range request and re-hashes what's there. The file only takes its
  real name once its SHA-256 matches.
- **Progress** goes through the same `ModelPull` events as Ollama pulls (progress, stop,
  resume). `POST /v1/providers/{id}/pull/cancel` stops one.
- **Deleting.** `DELETE /v1/providers/{id}/models/{model}` removes the file; it's refused
  for the model in use (the default model).
- A model picked for download during onboarding becomes the default when its download
  finishes (`settings::adopt_pending`); see [Onboarding](onboarding.md).
