# hai

[![English](https://img.shields.io/badge/English-424242?style=for-the-badge&logo=readme)](README.md)
[![中文](https://img.shields.io/badge/中文-EE4C2C?style=for-the-badge)](docs/README.zh-CN.md)

A companion chatbot for Telegram and TeamSpeak 3 text chat, which remembers your conversations, understands your media, and organizes the topics you discuss over time.

## Features

- **Remembers what matters** — keeps personal facts, notes, and preferences across conversations
- **Understands your media** — reads the images, videos, and voice messages you send on Telegram
- **Organizes conversations** — tracks topics and summarizes them as they evolve
- **Talks back in text or voice** — replies in a personality shaped to you, in text on Telegram and TeamSpeak and in voice on Telegram
- **Has up-to-date knowledge** — pulls in external information when you need it
- **Does things for you** — runs commands and generates images on request
- **Extends itself** — connects to external tools and follows specialized skills on demand

## Installation

### Prerequisites

- PostgreSQL with the [pgvector](https://github.com/pgvector/pgvector) extension
- A Telegram bot token (from [@BotFather](https://t.me/BotFather)), or a TeamSpeak 3 server for text chat
- An LLM API key (OpenAI, Anthropic, OpenRouter, or local Ollama)

### Build from source

Building requires a [Rust](https://rustup.rs/) toolchain (nightly, edition 2024).

```bash
git clone https://github.com/hmeqo/hai.git
cd hai
cargo build --release
```

The binary is at `target/release/hai` — or run it directly with `cargo run -p hai --`.

### Configure

Create `.hai/config.toml`:

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

# The credential comes from OPENROUTER_API_KEY in the environment, or from a block:
# [providers.openrouter]
# api-key = "sk-or-v1-..."

[model-roles.default]
model = "openrouter/anthropic/claude-3.5-sonnet"

[models]
# fast = "openrouter/deepseek/deepseek-chat"

[conversation]
language = "zh"
# model = "fast"
# allow-persona-change = false
# allow-content-policy-change = false

[conversation.content-policy]
maturity = "safe"
politics = "restricted"

[agent.personality]
name = "hai"

[prompt]
soul = "A warm, witty assistant that answers briefly and clearly."

[model-roles.embedding]
model = "openrouter/openai/text-embedding-3-small"
dimension = 1536
```

### Set up the database

```bash
hai db create      # create the database
hai db migrate     # create tables, extension, indexes (idempotent)
```

## Usage

### Commands

```bash
hai config                   # print the loaded configuration (json/toml)
hai db create                # create the database
hai db migrate               # create tables, extension, indexes (idempotent)
hai db rebuild embeddings    # rebuild vector embeddings
hai log                      # event log TUI (three panels); --id N prints one event
hai kb import <path>         # import documents into the knowledge base (idempotent)
hai kb list                  # list documents (optionally by collection)
hai kb search <query>        # semantic search over the knowledge base
hai kb delete <id>           # delete a document (cascades chunks)
hai kb reindex               # re-chunk/re-embed documents with stale chunker versions
hai conversation list        # list Conversations and their member Chats
hai conversation attach --chat 7 --conversation 3   # move a Chat, carrying pending entries
hai conversation detach --chat 7                    # move a Chat to a fresh Conversation
```

Attach is a cutover: the Chat and its unconsumed pending entries move; historical messages keep the Conversation they were written in. Use `attach` before the first messages when two Chats should share context from the start.

### Configuration reference

`.hai/config.toml` (or `~/.config/hai/config.toml`). Set `HAI_LOCAL_MODE=1` to force `.hai/` only. Keys use kebab-case; all sections are optional with sensible defaults (except `[model-roles.embedding]`, which is required).

#### LLM providers

Supported providers: `openrouter`, `openai`, `anthropic`, `google`, `deepseek`, `groq`, `ollama`, `xai`, `azureopenai`, `minimax`, `phind`, `requesty`. Each kind serves the model kinds it answers (chat, vision, transcription, embedding, image, speech, rerank) at its own base URL.

A provider is available when it has a credential or, for a local service, answers — `OPENROUTER_API_KEY` in the environment (each kind has its own variable, e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`) or `api-key` in a block, with the environment winning. A `[providers.<name>]` block is an optional override, not a declaration:

```toml
[providers.openai]
api-key = "sk-..."                       # instead of OPENAI_API_KEY in the environment

[providers.my-gateway]                   # no credential needed when the block states its endpoint
type = "openai"
base-url = "http://gw.internal/v1"
# headers = { X-Title = "hai" }          # optional; gateways and resellers

[providers.local-rerank]                 # a rerank-only provider: no `type`, no chat transport
base-url = "http://127.0.0.1:8080/v1/rerank"
```

`ollama` needs no block at all: a model such as `"ollama/qwen2.5"` works whenever the service answers at `http://localhost:11434/v1` (point it elsewhere with a provider block's `base-url`).

#### Model roles

`[model-roles.<role>]` names a `provider/model` model; the provider owns the endpoint and the role keeps only its non-endpoint options. `[models]` maps aliases to the same syntax, with chat options inherited from `[model-roles.default]`.

```toml
[model-roles.embedding]           # required; errors when missing (memory/retrieval depend on it)
model = "openrouter/openai/text-embedding-3-small"
dimension = 1536                 # required; the model's output width (the pgvector column width)

[model-roles.vision]              # image/video understanding (defaults to the main model)
model = "openai/gpt-4o"

[model-roles.transcription]       # audio → text (defaults to the main model)
model = "google/gemini-3.1-flash-lite"

[model-roles.speech]                 # speech synthesis (send_message's voice kind when model is set)
model = "openrouter/google/gemini-3.1-flash-tts-preview"
voice = "Zephyr"                # required; voice names are model-scoped (alloy = OpenAI, Zephyr/Kore/Aoede = Gemini, …) and a wrong one is rejected by the provider (HTTP 400)
# speed = 1.0                   # optional (0.25 ~ 4.0, default 1.0)

[model-roles.image]           # image generation (mounts generate_image when model is set)
model = "openrouter/google/gemini-2.5-flash-image"

# [model-roles.rerank]             # retrieval scoring; without the block the fused order stands
# model = "openrouter/cohere/rerank-4-fast"

# A dedicated endpoint is a provider, not a role override:
# [providers.local-rerank]
# base-url = "http://127.0.0.1:8080/v1/rerank"
# [model-roles.rerank]
# model = "local-rerank/bge-reranker-v2-m3"
```

#### Behaviour settings

```toml
[agent.multimodal.vision]    # understanding: the switch and the analysis templates
# enabled = true
# image-prompt = "…"          # optional; replaces the built-in image template
# video-prompt = "…"          # optional; replaces the built-in video template

[agent.multimodal.transcription]
# enabled = true
# prompt = "…"                # optional; replaces the built-in audio template

[agent.retrieval]            # what one rerank request carries
# candidates = 30
# candidate-chars = 768
# timeout-secs = 10
```

#### Attention / scheduling

```toml
[agent.attention]
base-attention = 0.05       # base dispatch probability for Observe events
window-secs = 30            # attention window after an addressed event
```

#### Context

```toml
[agent.context]
context-seed-cap = 10       # messages seeded into the first render of a chapter
pinned-memory-limit = 20    # pinned, still-valid memories in the chapter-opening block
topic-idle-hours = 2        # topic considered stale after this idle time
session-idle-timeout-secs = 600    # no run for this long closes the chapter
steering = true             # turn-interrupting new events resume the turn
compact-token-threshold = 150000   # chapter wrap-up when context tokens exceed this (0 = disabled)
```
