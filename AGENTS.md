# AGENTS.md

## 开发命令

```bash
cargo check                          # 日常编译
cargo run --bin hai                  # 启动 bot
cargo run --bin hai -- config        # 查看当前配置
cargo run --bin hai -- config --format toml  # TOML 格式输出
cargo sqlx migrate run               # 跑迁移（无 down migration）
cargo sqlx prepare --workspace       # 修改 sqlx::query! 后必须重跑
```

## 编码规范

- `rustfmt.toml`: `imports_granularity = "Crate"`
- LSP 报 `error communicating with database` 是离线模式的正常现象，忽略即可
- 无 CI / 无 pre-commit
- 层次依赖：`entity → vo → repo → service → agent → app`，`infra` 不依赖上层
- 多 chat 并行，单 chat 串行

## 注意事项

- 改 `sqlx::query!` 后必须先 `cargo sqlx prepare --workspace`，否则编译报错
- `TIMESTAMPTZ` 字段需显式标注：`updated_at as "updated_at!: jiff_sqlx::Timestamp"`
- `Scratchpad` 与 chat 一对一（`PRIMARY KEY (chat_id)`），每次 agent 运行结束覆盖
- `PerceptionService::upsert()` 内部自动生成并保存 embedding，调用方只需调一次
- `AgentEvents` trait（`agent/event/cause.rs`）聚合语义查询，**不要**在调用处手写 `iter().any(...)`
- Config 覆盖链：`.hai/config.toml` → 环境变量 `HAI_` 前缀覆盖 → 运行时热加载
- `HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`
