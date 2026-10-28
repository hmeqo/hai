# hai

[![English](https://img.shields.io/badge/English-424242?style=for-the-badge&logo=readme)](README.md)
[![中文](https://img.shields.io/badge/中文-EE4C2C?style=for-the-badge)](docs/README.zh-CN.md)

A companion chatbot for Telegram and TeamSpeak 3 text chat: it remembers conversations, reads media, and keeps track of topics over time.

## Quick start

### Prerequisites

- PostgreSQL with the [pgvector](https://github.com/pgvector/pgvector) extension
- A Telegram bot token (from [@BotFather](https://t.me/BotFather)), or a TeamSpeak 3 server
- A model API key (Google, DeepSeek, OpenAI, OpenRouter, or local Ollama)

### Build

Building requires a [Rust](https://rustup.rs/) toolchain (nightly, edition 2024).

```bash
git clone https://github.com/hmeqo/hai.git
cd hai
cargo build --release
```

The binary is `target/release/hai`.

### Configure

Create `.hai/config.toml`:

```toml
[database]
url = "postgres://user:password@localhost:5432/hai"

[channel.telegram]
bot-token = "your-bot-token"
allowed-chat-ids = [123456789]

[models.chat]
model = "openrouter/google/gemini-3.1-pro-preview"

[models.embedding]
model = "openrouter/openai/text-embedding-3-small"
roles = ["embedding"]

[model-roles.default]
model = "chat"

[model-roles.embedding]
model = "embedding"
dimension = 1536
```

The credential comes from `OPENROUTER_API_KEY` in the environment.

### Run

```bash
hai db create
hai db migrate
hai
```

Full configuration and usage: [docs/usage.md](docs/usage.md).
