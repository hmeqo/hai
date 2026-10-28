# Usage

> Setup, configuration, and commands for running hai. Architecture and implementation notes live under `docs/topics/`.

## What it does

- **Remembers what matters** — keeps personal facts, notes, and preferences across conversations
- **Understands your media** — reads images, videos, and voice messages on Telegram
- **Organizes conversations** — tracks topics and summarizes them as they evolve
- **Talks back in text or voice** — replies in a personality shaped to you; text on Telegram and TeamSpeak, voice on Telegram
- **Has up-to-date knowledge** — pulls in external information when needed
- **Does things for you** — runs commands and generates images on request
- **Extends itself** — connects to external tools and follows specialized skills on demand

## Install

### Prerequisites

- PostgreSQL with the [pgvector](https://github.com/pgvector/pgvector) extension
- A Telegram bot token (from [@BotFather](https://t.me/BotFather)), or a TeamSpeak 3 server for text chat
- A model API key (Google, DeepSeek, OpenAI, OpenRouter, Anthropic, or local Ollama)

### Build

Building requires a [Rust](https://rustup.rs/) toolchain (nightly, edition 2024).

```bash
git clone https://github.com/hmeqo/hai.git
cd hai
cargo build --release
```

The binary is at `target/release/hai`; run it with `cargo run -p hai --`.

### Configure

Create `.hai/config.toml` (or `~/.config/hai/config.toml`). `HAI_LOCAL_MODE=1` forces `.hai/` only. Keys are kebab-case; all sections are optional except `[model-roles.default]` and `[model-roles.embedding]`.

```toml
[database]
url = "postgres://user:password@localhost:5432/hai"

[channel.telegram]
bot-token = "your-bot-token"
allowed-chat-ids = [123456789]

# TeamSpeak 3 text chat; no allowed-chat-ids admits every chat.
# [channel.ts3]
# type = "teamspeak"
# address = "127.0.0.1:9987"
# name = "hai"
# channel = "Lobby"
# allowed-chat-ids = ["channel:1"]
# default-conversation-scope = "shared"   # shared | isolated; default isolated

[models.gemini]
model = "openrouter/google/gemini-3.1-pro-preview"
name = "Gemini 3.1 Pro"
roles = ["chat", "vision"]
input = ["text", "image"]
max-tokens = 65536
context-window = 1048576
thinking = { efforts = ["low", "medium", "high"], default = "high" }

[models.embed-small]
model = "openrouter/openai/text-embedding-3-small"
roles = ["embedding"]

[model-roles.default]
model = "gemini"

[model-roles.embedding]
model = "embed-small"
dimension = 1536

[model-roles.vision]
model = "gemini"

[conversation]
language = "zh"
content-policy = "safe"
# allow-persona-change = false
# allow-content-policy-change = false

[agent.personality]
name = "hai"

[prompt]
soul = "A warm, witty assistant that answers briefly and clearly."
```

### Set up the database

```bash
hai db create
hai db migrate
```

`db migrate` creates tables, the vector extension, indexes and constraints; it is idempotent. `hai db rebuild embeddings` rebuilds every vector and index when the embedding model or dimension changes.

## Configuration

### Providers

Supported providers: `openrouter`, `openai`, `anthropic`, `google`, `deepseek`, `groq`, `ollama`, `xai`, `azureopenai`, `minimax`, `phind`, `requesty`. Each kind serves the model kinds it answers at its own base URL.

A provider is available when it has a credential or, for a local service, answers. The environment variable is read from the provider's own name first (`[providers.my-gateway]` reads `MY_GATEWAY_API_KEY`), then the kind's shared variable (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, …), then the block's `api-key`; the environment wins. A `[providers.<name>]` block is an optional override, not a declaration:

```toml
[providers.openai]
api-key = "sk-..."                       # instead of OPENAI_API_KEY

[providers.my-gateway]
type = "openai"
base-url = "http://gw.internal/v1"
# headers = { X-Title = "hai" }

[providers.local-rerank]                 # rerank-only: no type, no chat transport
base-url = "http://127.0.0.1:8080/v1/rerank"
```

`ollama` needs no block: `model = "ollama/qwen2.5"` works while the service answers at `http://localhost:11434/v1`.

### Models and roles

`[models.<alias>]` is an optional preset: `model` as `provider/model`, optional display `name`, `roles` (default `["chat"]`), `input` (default `["text"]`), `max-tokens`, `context-window`, `temperature`, `thinking` and `pricing`.

`[model-roles.<role>].model` accepts a preset alias or a direct `provider/model`. An absent block turns that role off; an empty alias selects the only declared candidate, and more than one candidate fails startup. A direct selector uses safe defaults: text-only, no token cap, no thinking control, no pricing, and it is not a selectable preset. Define a preset when those matter. `[model-roles.default].model` is required.

Optional roles include `vision`, `transcription`, `speech`, `image` and `rerank`:

```toml
[models.gemini-lite]
model = "openrouter/google/gemini-3.1-flash-lite"
roles = ["transcription"]
input = ["text", "image", "audio", "video"]

[models.gemini-tts]
model = "openrouter/google/gemini-3.1-flash-tts-preview"
roles = ["speech"]

[models.gemini-image]
model = "openrouter/google/gemini-2.5-flash-image"
roles = ["image"]

[model-roles.transcription]
model = "gemini-lite"

[model-roles.speech]
model = "gemini-tts"
voice = "Zephyr"

[model-roles.image]
model = "gemini-image"

# Retrieval scoring is optional; without it the fused order stands.
# [models.reranker]
# model = "openrouter/cohere/rerank-4-fast"
# roles = ["rerank"]
# [model-roles.rerank]
# model = "reranker"
```

A cheaper chat model can be selected directly: point `[model-roles.default].model` at `deepseek/deepseek-flash` (1,000,000 context, 384,000 output, thinking `low`/`high`/`max`).

### Conversation defaults

`[conversation]` holds `language`, an optional default `persona`, `content-policy` (`safe`, `standard` or `unrestricted`), and the `allow-persona-change` / `allow-content-policy-change` gates. A Conversation stores its own overrides; the configured values apply when none is stored.

### Agent behaviour

```toml
[agent.multimodal.vision]    # understanding: switch and templates
# enabled = true
# image-prompt = "…"
# video-prompt = "…"

[agent.multimodal.transcription]
# enabled = true
# prompt = "…"

[agent.retrieval]            # what one rerank request carries
# candidates = 30
# candidate-chars = 768
# timeout-secs = 10
```

Attention and context:

```toml
[agent.attention]
base-attention = 0.05
window-secs = 30

[agent.context]
context-seed-cap = 10
pinned-memory-limit = 20
topic-idle-hours = 2
session-idle-timeout-secs = 600
steering = true
compact-token-threshold = 150000
```

## Commands

### Management CLI

```bash
hai config                   # print the loaded configuration (json/toml)
hai db create                # create the database
hai db migrate               # create tables, extension, indexes (idempotent)
hai db rebuild embeddings    # rebuild vector embeddings
hai log                      # event log TUI; --id N prints one event
hai kb import <path>         # import documents into the knowledge base
hai kb list                  # list documents (optionally by collection)
hai kb search <query>        # semantic search over the knowledge base
hai kb delete <id>           # delete a document (cascades chunks)
hai kb reindex               # re-chunk/re-embed stale documents
hai conversation list        # list Conversations and their member Chats
hai conversation 7           # show a Conversation's Chats and effective settings
hai conversation 7 lang en   # read or write one setting
hai conversation attach --chat 7 --conversation 3   # move a Chat, carrying pending entries
hai conversation detach --chat 7                    # move a Chat to a fresh Conversation
```

`conversation <CONVERSATION> lang|model|content-policy|persona [VALUE]` reads a setting when no value is given, writes it when one is, and restores the configured default with `--reset`. `model` names a `[models]` key whose entry declares `chat`; `content-policy` takes `safe`, `standard` or `unrestricted`.

Attach is a cutover: the Chat and its unconsumed pending entries move; historical messages keep the Conversation they were written in. Use `attach` before the first messages when two Chats should share context from the start.

### Chat commands

Telegram uses `/`; TeamSpeak uses `!`. Each command accepts `--help` for the exact form.

- `/status` — the agent's status block
- `/lang [CODE] [--reset]` — read or set the Conversation language
- `/contentpolicy [LEVEL] [--reset]` — read or set the content level
- `/persona [TEXT...] [--reset]` — read or set the persona
- `/purge [RANGE] [--surfaces LIST] [--yes]` — purge a range
- `/digest [RANGE] [--lang CODE] [--to here|me]` — summarize a range

`RANGE` is `all`, hours or days back (`24h`, `7d`), or two local dates (`2026-10-01..2026-10-07`).

## See also

- [Configuration details](topics/config.md)
- [CLI and event log](topics/cli.md)
- [Chat commands](topics/commands.md)
- [Telegram and TeamSpeak](topics/platform.md)
- [Conversation behavior](topics/chat-agent.md)
- [Tools](topics/tools.md)
