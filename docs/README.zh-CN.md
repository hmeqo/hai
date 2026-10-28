# hai

[![English](https://img.shields.io/badge/English-424242?style=for-the-badge&logo=readme)](../README.md)
[![中文](https://img.shields.io/badge/中文-EE4C2C?style=for-the-badge)](README.zh-CN.md)

一个面向 Telegram 与 TeamSpeak 3 文字聊天的伴侣聊天机器人：记住你的对话、理解你的媒体，并随时间整理所讨论的话题。

## 快速开始

### 前置条件

- 带 [pgvector](https://github.com/pgvector/pgvector) 扩展的 PostgreSQL
- 一个 Telegram 机器人令牌（来自 [@BotFather](https://t.me/BotFather)），或一台 TeamSpeak 3 服务器
- 一个模型 API 密钥（Google、DeepSeek、OpenAI、OpenRouter 或本地 Ollama）

### 构建

构建需要 [Rust](https://rustup.rs/) 工具链（nightly，edition 2024）。

```bash
git clone https://github.com/hmeqo/hai.git
cd hai
cargo build --release
```

二进制文件位于 `target/release/hai`。

### 配置

创建 `.hai/config.toml`：

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

凭据来自环境变量 `OPENROUTER_API_KEY`。

### 运行

```bash
hai db create
hai db migrate
hai
```

完整配置与用法：[docs/usage.md](usage.md)。
