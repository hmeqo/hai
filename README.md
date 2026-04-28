# 👋 hai

Telegram 群聊机器人，拥有独立性格和长期记忆的数字生命。

## 核心特性

- **人格系统**：6 维性格参数（社恐/话痨、话量、坦诚度、幽默感、共情、情绪稳定性），可灵活配置
- **智能话题管理**：自动识别、归类、总结群聊话题
- **长期记忆**：记住群友特征、爱好、群规，支持向量检索
- **多模态**：图片生成、语音分析

## 快速开始

### 1. 环境准备

- Rust 1.75+
- PostgreSQL + pgvector 扩展
- Telegram Bot Token
- LLM API Key（OpenRouter / OpenAI / Anthropic 等）

### 2. 配置

创建 `.hai/config.toml`：

```toml
[database]
url = "postgres://user:password@localhost:5433/hai"

[telegram]
bot_token = "your-bot-token"
allowed_chat_ids = [123456789]  # 可留空允许所有

[agent]
provider = "openrouter"
default_model = "anthropic/claude-3.5-sonnet"

[agent.personality]
name = "hai"
sociability = 0.05
verbosity = 0.35
honesty = 0.65
humor = 0.70
empathy = 0.75
mood = 0.30

[providers.openrouter]
api_key = "your-api-key"
```

### 3. 运行

```bash
# 初始化 SQLx 查询缓存
cargo sqlx prepare --workspace

# 启动
cargo run --bin hai -- run
```

### 4. 查看配置

```bash
# 查看当前配置（支持 toml/json 格式）
cargo run --bin hai -- config --format toml
```

## 开发

```bash
# 编译检查（离线模式）
SQLX_OFFLINE=true cargo check

# 运行
cargo run --bin hai -- run

# 查看配置
cargo run --bin hai -- config
```

## TODO

- [ ] 基础能力
  - [x] 记事板
  - [x] 记忆
  - [x] 智能话题管理
  - [ ] 计划任务
- [x] 人格系统
  - [x] 基础人格系统
- [x] 接收消息
  - [x] 多模态分析
    - [x] 图片
    - [x] 视频
    - [x] 语音
- [ ] 发送消息
  - [x] 发送文本
  - [ ] 发送和管理 sticky
  - [ ] 多模态
    - [x] 语音
    - [ ] 图片
    - [ ] 视频
- [x] 增强功能
  - [x] MCP
  - [x] Skills
- [ ] 多平台支持
  - [x] Telegram
  - [ ] Qq
