# Config and Error Handling

> Config override chain, structure semantics, Paths, Provider resolution, and the error type system. A must-read when modifying the `config/`, `error.rs`, or `app/context.rs` assembly.

## Overview

Config is a pure data layer (+ ProviderRegistry resolution), and app is the assembly root. This document covers "where the config comes from, its structure semantics, and who consumes it" and "how errors are expressed" (field authority is in config/schema.rs). See the "Error shape" section for error handling.

## Design

### Config override chain

```txt
.hai/config.toml → HAI_ env vars (prefix "HAI", separator "__" — config-rs prefixes include the separator, so **the actual prefix is the double underscore `HAI__`**: `HAI__AGENT__MAX_TOKENS` takes effect, whereas `HAI_AGENT__MAX_TOKENS` is silently skipped)
```

- **"Runtime hot reload" is not wired up**: `Config::reload()/update()/save()` have zero callers and no file watcher; what actually takes effect is a one-time file→env merge at startup (`config/manager.rs:rebuild_and_store`: `T::default().apply(file_intent).apply(env_intent)`, env overrides file)
- The `HAI_LOCAL_MODE` environment variable takes effect **by existence alone** (value ignored, `config/env.rs:local_mode`), forcing `.hai/`; otherwise it falls back to `$XDG_CONFIG_HOME/hai/`
- Patch boundary: sections that can be hot-updated (agent/auxiliary/mcp/database/sandbox/knowledge) get `#[derive(Patch)]`; the non-Patch ones (ProviderConfig/BotConfigRaw/SkillsConfig/LoggingConfig) are startup-only, one-time config

Key symbols: `config/manager.rs:Configurable` + `Config<T>{file_path, intent, env_intent, current}` (ArcSwap), `config/env.rs:ENV_PREFIX = "HAI"`, `config/meta.rs:AGENT_NAME`.

### Paths singleton

`config/paths.rs:Paths::inferred() -> &'static Self` (OnceLock, resolved once at process startup). Fields:

- `config_dir` / `data_dir`: `.hai/` or `$XDG_CONFIG_HOME/hai/` / `$XDG_DATA_HOME/hai/`
- `config_file` / `config_file_str`: config_dir + "config.toml" (pre-cached UTF-8, expects valid UTF-8)
- `file_cache_dir`: data_dir + "files"
- `skill_dirs`: `[config_dir/skills, .hai/skills (deduped), .agents/skills]` retain(exists)

`resolve()` decides: `.hai/` exists or `HAI_LOCAL_MODE` is set → local mode; `dirs::config_dir/data_dir` as fallback.

### Config structure essentials (field authority = `config/schema.rs`)

> The complete field list, default values, and inline comments are authoritative in `config/schema.rs` (grep to find them) — this section only records the structure semantics that are not readable from the code.

- **Patch boundary**: sections that can be hot-updated (agent/auxiliary/mcp/database/sandbox/knowledge) get `#[derive(Patch)]`; the non-Patch ProviderConfig/BotConfigRaw/SkillsConfig/LoggingConfig are startup-only, one-time config
- **Default inheritance and clamp**: `[auxiliary.embedding]` dimension defaults to 1024; `[auxiliary.tts]` voice defaults to "alloy" and speed is clamped to (0.25, 4.0); `[auxiliary.vision]`/`[auxiliary.audio]` enabled = feature switch (default true); `[bot.*]` rich-message defaults to true
- **Model roles (auxiliary)**: one block per capability = model + capability parameters (`auxiliary.{vision,audio,tts,embedding,image-gen}` — each block's `provider` defaults to `agent.provider`). Comprehension classes (vision/audio): model defaults to delegating to the main model (agent) + enabled switch (default true) + analysis prompt override (image-prompt/video-prompt/prompt — empty = built-in template). Dedicated classes (tts/embedding/image-gen): model defaults to capability unavailable (embedding is a required capability — defaulting to it raises an error, contract-style failure). image-gen is a general capability: the tool accepts a list of reference images (image-to-image / multi-image composition, e.g. "replace the person in image 1 with the person in image 2"; the API goes through chat/completions + modalities image form — mainstream image models such as nano banana all support reference image input).
- **Cross-document semantics**: the scheduling semantics for wrap-up-token-threshold / base-attention / window-secs are in docs/topics/session.md; the three chunk parameters and `[knowledge.inject]` are in docs/topics/domain.md + docs/topics/prompting.md; the participation rules for image-prompt/prompt are in docs/topics/tools.md

### Provider/Bot resolution

**Provider**:

- `config/schema.rs:ProviderConfig::infer_kind(name)`: the `type` field takes priority, otherwise `ProviderKind::from_str(name)` (invalid → Config error)
- `config/provider_manager.rs:ProviderRegistry::new(&AppConfig)` resolves them one by one; `get_checked` (NotFound → "Provider 'x' not found"); `get_endpoint(provider, model) -> Endpoint{base_url, api_key, model}`
- `agentcore/provider.rs:ProviderKind` (12 variants) + `create_genai_client(&ProviderEntry)` (adapters: AzureOpenAI/Phind/Requesty → OpenAI-compatible; Google → Gemini; XAI → Xai)
- Consumers: `app/context.rs` (AppContext assembly + MultimodalService::from_config), `rebuild.rs` (get_endpoint), `agent/runtime/engine.rs` (create_genai_client), `cli/kb.rs` (build_services)

**Bot**:

- `config/schema.rs:BotConfig::resolve(key, raw)`: type takes priority, else the key name → `BotConfig{key, platform, bot_token, allowed_chat_ids, rich_message}`; `BotPlatform` is Telegram only
- Startup: `platform/manager.rs:spawn_bots` iterates `cfg.bot`, each bot gets its own SessionManager + dispatcher; the JoinHandle is dropped (a crash is only logged)

### Error handling (error.rs)

- **Error shape**: `error.rs:AppError{kind, message, source}` single type + `register_errors!` centralized From registrations
- Conventions: `?` takes priority; `let _` only for confirming something is irrelevant; `if let Err(e) = ... { tracing::warn!(?e, ...) }` (the same hard rule is in AGENTS.md §1)
- Known tension: several domain services' `get_*` swallow errors as `Ok(None)`, so callers cannot distinguish them — see the "swallow-error mode" pitfall in docs/topics/domain.md; do not spread it

## Boundaries

- The field-level config list is not repeated here (config/schema.rs is the field authority)
- No config hot reload (currently not wired up; see evolution direction below)
- Provider/bot resolution is startup-only, one-time (non-Patch)

## Pitfalls / common mistakes

- `AgentConfig` derives Default (empty provider/model strings) — `ProviderRegistry::new` will fail with a Config error when inference from the key name fails; the normal path relies on config.toml, so do not assume `default()` is directly usable
- `config/schema.rs:ContainerRuntime::detect` runs `which` on every Default call (including the serde default path); watch the overhead when taking defaults frequently
- `[sandbox] runtime` has no "auto" enum value — to probe, **omit the field** (go through Default::detect)
- Leftovers: the hot-reload APIs have no callers (`Config::save` only supports json, the "toml" branch is commented out); `AgentContext.current_model` is not kept in sync with config hot updates ([INFERENCE])

## Evolution direction

- Wire up config hot reload (enable reload/update/save + file watching) — the API is ready now but has no callers
