# 👋 hai

Telegram 聊天机器人，可配置性格和长期记忆。

## 功能

- **人格系统**：6 维性格参数（社交活跃度、话量、坦诚度、幽默感、理性/感性、情绪稳定性）
- **话题管理**：自动归类、总结话题
- **长期记忆**：记忆用户特征、知识、笔记、规则，向量检索
- **多模态**：图片分析、语音合成

## 快速开始

### 1. 环境准备

- Rust nightly (edition 2024)
- PostgreSQL + pgvector 扩展
- Telegram Bot Token
- LLM API Key（OpenRouter / OpenAI / Anthropic 等）

### 2. 配置

创建 `.hai/config.toml`：

```toml
[logging]
level = "info"

[database]
url = "postgres://user:password@localhost:5433/hai"

[bot.main]
type = "telegram"
bot-token = "your-bot-token"
allowed-chat-ids = [123456789]

[providers.openrouter]
api_key = "your-api-key"

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

### 3. 初始化 embedding（首次或换模型时执行）

```bash
cargo run -- db rebuild embeddings
```

### 4. 运行

```bash
cargo run --bin hai
```

### 5. 查看配置

```bash
cargo run --bin hai -- config --format toml
```

## 开发

```bash
cargo check                # 编译检查
cargo clippy --all-targets # lint
cargo run --bin hai        # 运行
cargo run --bin hai -- config  # 查看配置
```

## TODO

- [x] 基础能力
  - [x] 记事板
  - [x] 记忆
  - [x] 智能话题管理
  - [ ] 计划任务
- [x] 人格系统
  - [x] 基础人格系统
- [x] 接收消息
  - [x] 接收并存入数据库
  - [x] 多模态分析
    - [x] 图片
    - [x] 视频
    - [x] 语音
- [x] 发送消息
  - [x] 发送文本
  - [ ] 发送和管理 sticky
  - [x] 多模态
    - [ ] 图片
    - [ ] 视频
    - [x] 语音
- [x] 增强功能
  - [x] MCP
  - [x] Skills
- [x] 多平台支持
  - [x] Telegram
  - [ ] Cli
  - [ ] Qq
