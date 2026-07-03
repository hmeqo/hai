# hai

Telegram chatbot with personality system, long-term memory, MCP tool integration, and skill-based prompting.

## Features

- **Personality system** — configurable sociability, verbosity, humor, and mood traits shape response style
- **Long-term memory** — vector search across user facts, notes, and knowledge via pgvector
- **Topic tracking** — automatic conversation topic detection, assignment, and summarization
- **ReAct loop** — thinking-acting-observing cycle with full tool use, preemption, and turn management
- **MCP support** — integrate any [Model Context Protocol](https://modelcontextprotocol.io/) server for additional tools
- **Multi-platform** — Telegram (extensible via `PlatformHandler` trait)
- **Sandbox execution** — optional Docker sandbox for shell tool execution
- **Skills system** — loadable markdown skills with frontmatter for structured agent instructions

## Prerequisites

- [Rust](https://rustup.rs/) nightly (edition 2024)
- PostgreSQL 15+ with [pgvector](https://github.com/pgvector/pgvector) extension
- A Telegram bot token (from [@BotFather](https://t.me/BotFather))
- An LLM API key (OpenAI, Anthropic, OpenRouter, or local Ollama)

## Quick Start

### 1. Configure PostgreSQL

Create a database with pgvector:

```bash
createdb hai
psql hai -c "CREATE EXTENSION vector;"
```

### 2. Configuration

Create `.hai/config.toml`:

```toml
[database]
url = "postgres://user:password@localhost:5432/hai"

[bot.main]
type = "telegram"
bot-token = "your-bot-token"
allowed-chat-ids = [123456789]

[providers.openrouter]
api_key = "sk-or-v1-..."

[agent]
provider = "openrouter"
model = "anthropic/claude-3.5-sonnet"

[agent.personality]
name = "hai"
sociability = 0.05
verbosity = 0.35
honesty = 0.60
humor = 0.70
rationality = 0.35
mood = 0.1

[multimodal.embedding]
provider = "openrouter"
model = "openai/text-embedding-3-small"
dimension = 1536
```

### 3. Setup database

```bash
cargo run -- db migrate           # apply ORM + embedding schema migrations
```

### 4. Start

```bash
cargo run --bin hai
```

## Commands

```bash
cargo run --bin hai               # start the bot
cargo run --bin hai -- config     # print current config
cargo run -- db create            # create database
cargo run -- db migrate           # apply migrations + vector column
cargo run -- db rebuild embeddings   # re-embed all memories
cargo run --bin toasty-cli -- migration generate    # generate ORM migration
cargo run --bin toasty-cli -- migration apply       # apply ORM migration
```

## Configuration

`.hai/config.toml` (or `~/.config/hai/config.toml`). Set `HAI_LOCAL_MODE=1` to force `.hai/` only.

### LLM Providers

Supported providers: `openai`, `anthropic`, `openrouter`, `ollama`, `gemini`, `deepseek`, `groq`.

```toml
[providers.openai]
api_key = "sk-..."

[providers.ollama]
# api_key is optional for local providers
```

### Multimodal

```toml
[multimodal.embedding]
provider = "openrouter"
model = "openai/text-embedding-3-small"
dimension = 1536

[multimodal.input]
audio = { model = "whisper-1" }
image = { model = "gpt-4o" }
video = { model = "gpt-4o" }

[multimodal.tts]
model = "tts-1"
voice = "alloy"
speed = 1.0
```

### Attention / Scheduling

```toml
[agent.attention]
base_heat = 0.02          # base dispatch probability for Observe events
window_secs = 120          # attention window after addressed event

[agent.context]
history_cap = 25           # max messages loaded per turn
session_idle_timeout_secs = 7200
preempt = true              # inject mid-processing events as turn interruptions
conversation_mode = "persistent"   # or "ephemeral"
```

## Development

```bash
cargo check
cargo clippy --all-targets
cargo test
```

## Project Structure

```
hai/src/
├── agent/          agent logic (nodes, runtime, context, tools)
├── agentcore/      infrastructure (tool trait, MCP, embedding, rendering)
├── domain/         domain model + services (toasty ORM + sqlx)
├── platform/       platform integrations (Telegram)
├── config/         configuration system
├── util/           shared utilities (pgvector)
└── app/            application context + startup
```

## License

MIT
