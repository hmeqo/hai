# hai

A Telegram chatbot that remembers your conversations, understands your media, and organizes the topics you discuss over time.

## Features

- **Remembers what matters** — keeps personal facts, notes, and preferences across conversations
- **Understands your media** — reads the images, videos, and voice messages you send
- **Organizes conversations** — tracks topics and summarizes them as they evolve
- **Talks back in text or voice** — replies in a personality shaped to you
- **Has up-to-date knowledge** — pulls in external information when you need it
- **Does things for you** — runs commands and generates images on request
- **Extends itself** — connects to external tools and follows specialized skills on demand

## Installation

### Prerequisites

- PostgreSQL with the [pgvector](https://github.com/pgvector/pgvector) extension
- A Telegram bot token (from [@BotFather](https://t.me/BotFather))
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

[bot.telegram]
bot-token = "your-bot-token"
allowed-chat-ids = [123456789]

[providers.openrouter]
api-key = "sk-or-v1-..."

[agent]
provider = "openrouter"
model = "anthropic/claude-3.5-sonnet"

[agent.personality]
name = "hai"
description = "A warm, witty assistant that answers briefly and clearly."

[auxiliary.embedding]           # required (memory/retrieval depend on it)
provider = "openrouter"
model = "openai/text-embedding-3-small"
dimension = 1536
```

### Set up the database

```bash
hai db create      # create the database if missing
hai db migrate     # create tables, extension, indexes (idempotent)
```

## Usage

### Commands

```bash
hai config                   # print the loaded configuration (json/toml)
hai db create                # create the database
hai db migrate               # apply sqlx schema migrations (idempotent)
hai db rebuild embeddings    # rebuild vector embeddings
hai log                      # event log TUI (three panels); --id N prints one event
hai kb import <path>         # import documents into the knowledge base (idempotent)
hai kb list                  # list documents (optionally by collection)
hai kb search <query>        # semantic search over the knowledge base
hai kb delete <id>           # delete a document (cascades chunks)
hai kb reindex               # re-chunk/re-embed documents with stale chunker versions
```

### Configuration reference

`.hai/config.toml` (or `~/.config/hai/config.toml`). Set `HAI_LOCAL_MODE=1` to force `.hai/` only. Keys use kebab-case; all sections are optional with sensible defaults (except `[auxiliary.embedding]`, which is required).

#### LLM providers

Supported providers: `openrouter`, `openai`, `anthropic`, `google`, `deepseek`, `groq`, `ollama`, `xai`, `azureopenai`, `minimax`, `phind`, `requesty`.

```toml
[providers.openai]
api-key = "sk-..."

[providers.ollama]
# api-key is optional for local providers
base_url = "http://localhost:11434/v1"   # optional override
```

#### Auxiliary capabilities

```toml
[auxiliary.embedding]           # required; errors when missing (memory/retrieval depend on it)
provider = "openrouter"
model = "openai/text-embedding-3-small"
# dimension = 1024              # optional; pgvector column dimension (default 1024)

[auxiliary.vision]              # image/video understanding (defaults to the main model)
model = "gpt-4o"
# image-prompt = "…"            # optional; override the built-in image analysis prompt
# video-prompt = "…"            # optional; override the built-in video analysis prompt

[auxiliary.audio]               # speech understanding (defaults to the main model)
# model = "whisper-1"
# prompt = "…"                  # optional; override the built-in audio analysis prompt

[auxiliary.tts]                 # speech synthesis (enabled when model is set)
provider = "openrouter"
model = "openai/gpt-4o-mini-tts"
# voice = "alloy"               # optional (default alloy)
# speed = 1.0                   # optional (0.25 ~ 4.0, default 1.0)

[auxiliary.image-gen]           # image generation (mounts generate_image when model is set)
provider = "openrouter"
model = "google/gemini-2.5-flash-image"
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
related-memory-limit = 5    # related memories injected per turn
related-topic-limit = 3     # related topics injected per turn
topic-idle-hours = 3        # topic considered stale after this idle time
session-idle-timeout-secs = 300    # idle timeout triggering chapter wrap-up
steering = true             # turn-interrupting new events resume the turn
compact-token-threshold = 150000   # chapter wrap-up when context tokens exceed this (0 = disabled)
```
