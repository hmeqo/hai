# Prompt rendering

> System prompt assembly, context XML rendering, personality, and skills injection. Required reading when changing `agent/context/`, `agent/node/main/`, `agent/personality/`, `agentcore/skills/`. Terminology: docs/CONTEXT.md (Memory≠knowledge base, Topic≠thread).

## Overview

Produces the two inputs for each turn's LLM call in the agent: **system prompt** (assembled by `SystemPromptBuilder`, fixed within a Turn) + **context XML** (first user message, varies by turn). Both are produced by the Node engine of `agentcore/render` (`render_pretty(…, Format::Xml)`).

## Design

### System prompt (SystemPromptBuilder)

Key symbols: `agent/node/main/prompt.rs:SystemPromptBuilder`, `SYSTEM_PROMPT`, `SEPARATOR = "\n----------------\n"`, `Section`.

Section order (empty content dropped; when there is a heading, add `{h}\n\n{content}`; joined by SEPARATOR):

```txt
SYSTEM_PROMPT (hardcoded constant, no config override, no heading)
  → # 人格 (personality_context, heading="# 人格")
  → User system_prompt ([agent.context] system-prompt, skipped if empty, no heading)
  → # 私聊 / # 群聊 (private_prompt / group_prompt; Channel → heading=None; skipped if empty)
  → skills (skill_manager.discovery_prompt(), no heading)
```

`SYSTEM_PROMPT` content highlights (hard-coded; changing behavior requires touching the code):

**Section responsibilities (avoid duplication — the same logic lives in exactly one place; never write behavioral guidance in tool descriptions)**:
- Tool descriptions (`- \`xxx\`：`) — only the tool's function/effect (e.g., `skip` only says "this turn's no external speaking")
- Behavioral guidance (interaction style / basic responsibilities) — only "when/how" (e.g., "nothing to say → tidy up first then `skip`" — the single location for guidance)
- Domain rules (memory & topic maintenance, etc.) — domain behavior; do not repeat what the tool already expresses

- **Interface (scene awareness)**: you operate in a chat app; each wake-up you receive a snapshot of the chat interface; in `<conversation>` the `<separator>新消息</separator>` divider line has old messages above and pending new messages below; `<date>`/`<msg from at>`/`<msg own>`/`<reference>` are message elements
- **Interaction style**: the only way to interact with the world is tools — `send_message`/`send_voice`/`generate_image` send content in the input box (text/voice/image — **users can only see what these three tools emit**); `skip` — this turn's no external speaking; interaction rhythm: have it figured out → send a message; not sure → think again; the other party is still typing → wait a bit; there is information/topic change worth tidying up → tidy up (memory/topic tools) then `skip`; nothing to say and nothing to tidy up → directly `skip`
- **Basic responsibilities** (memory/topic maintenance is a **basic behavior**, not an extra task): before responding, check memory/topics; when you hear something worth remembering, record it immediately; create new topics / update progress summaries; archive topics when they end; if you remembered wrong, update or delete. One memory records one independent fact; one topic revolves around one theme — unrelated content is established separately. Knowledge-base retrieval: when external knowledge is needed, query `search_knowledge_base` first

### Context XML rendering

Key symbols: `agent/context/builder.rs:build_prompt` (single entry point), `sections/context.rs:render_main_context` / `render_context` (first turn/subsequent turns unified root = `context`), `render_context.rs:RenderContext` / `RenderContextData`.

Section order (first turn and subsequent turns unified `<context>` root; empty sections dropped — `Node::is_empty` = no children):

```txt
situation (WakeEvent notification: <trigger reason [count]><text/></trigger>)
→ environment (you_are + current_time + unread count)
→ chat (id/platform/type/created_at/name)
→ accounts (filters out the bot itself; includes identity-related sibling accounts)
→ related_memories (semantic search, limit = related_memory_limit, default 5)
→ knowledge (knowledge-base RAG auto retrieval, limit = [knowledge.inject] limit, default 5; collection whitelist filter)
→ related_topics (semantic search, only closed topics returned, excluding the current active; limit = related_topic_limit, default 3)
→ current_topics (active/stale split by topic_idle_hours; stale adds need-close)
→ perceptions (URL source only; attachment perception goes through in-message inline rendering)
→ conversation (date separator / <separator>新消息</separator> before the first unread (the only new/old divider) / reply <reference>: in-window references truncated to 50 chars (content already visible as an independent message); out-of-window references looked up separately via "reference context", **not truncated** (the only presentation channel) and not re-rendered as an independent message — the conversation flow contains only in-window messages)
```

First turn vs subsequent turns differences (`is_first` passed in by dispatch from `conversation.is_first_render()` — chapter initial-state determination; see docs/topics/session.md):

| Dimension | First turn (is_first) | Subsequent turns |
|---|---|---|
| root tag | `context` | `context` (unified) |
| topics / perceptions | full injection | `vec![]` (empty) |
| related retrieval | `search_related_context` no shown limit | `search_related_dedup` excludes shown + reduced by limit×2/3 (5→3, 3→2) |
| total_unread | real-time count | always 0 |
| **knowledge (RAG injection)** | retrieved when `[knowledge.inject] enable` (query same source as memory/topics) | empty (deep query goes through the `search_knowledge_base` tool) |

> `[knowledge.inject] enable` controls passive injection (**default false**); when enabled, injection happens only on the first turn, subsequent turns are empty.
> First-turn determination = **chapter initial state** (`conversation.rs:Conversation::is_first_render()` = `turn_count == 0`): **new chapters from any source** (brand-new conversation / successful wrap-up — summary placed at the start / failed wrap-up opens a clean chapter) fully build the first turn — context messages fetch all unread messages (≥ seed cap 10 items; when insufficient, pull from history messages to make up the count); a restored-loaded chapter with `turn_count > 0` → not first turn (seamless continuation). See topics/session.md "First-turn determination".

Search assembly (`helper/search.rs`):

- `build_search_query`: active topics title/summary + parsed text + perception content join — memory/topic/knowledge base share the same query
- `search_related_context`: `generate_embedding` → **parallel** memory.search_related + topic.search_related_topics (`try_join!`); knowledge.search is an **independent sequential call** in builder.rs (not parallel), collection whitelist comes from `[knowledge] collections`

Helpers: `helper/chat.rs:load_chat` / `load_reply_context` / `collect_accounts` (includes identity siblings); `helper/perception.rs:PerceptionLoader` (attachment batch query + same-resource dedup, URL perception lookup).

### personality

Key symbol: `agent/personality/render.rs:personality_context`.

```txt
"你是 {name}。\n{description}"
```

Personality is **one-piece free-form text** (`config/schema.rs:PersonalityConfig.description`: persona foundation + behavioral etiquette + emotional aptness) — the system does no structured parsing; judgment and expression all live in the text. Struct name `PersonalityConfig` (stable trait config) and field name `description` (narrative block) each have their place — `persona` (relational mask) belongs to a future dynamic layer concept and does not occupy the current config.

### Skills injection

- `agentcore/skills/manager.rs:SkillManager::discovery_prompt()`: when non-empty, outputs a "## Skills" section (`- name: description` list), instructing the model to activate with the `load_skill` tool
- skills with `disable_model_invocation` don't enter the discovery list (`SkillManager::discoverable_skills`)
- `Skill::body()` replaces the `{baseDir}` placeholder with the skill directory (the instruction the model receives after `load_skill`)

## Boundaries

- Section order/rendering structure is "code is the fact"; only semantics and differences are recorded here (for specific rendering, read the `sections/` files)
- No structured parsing of personality (one-piece free-form text)
- knowledge RAG injects only on the first turn; deep queries go through active tools

## Pitfalls / common mistakes

- The `<environment>` unread node is output only when the in-window unread count < total (format `"{shown} in window ({total} total unread)"`) — don't assume this node always exists (`sections/context.rs:ContextBuilder::env`)
- Two sets of topic rendering (`sections/topic.rs:topic_element` vs `topic_element_static`): context and tool-response formats differ (`need-close`/`last_active` only exist in context), intentional but increases maintenance surface — changing one side must sync the other
- **When subsequent-turn messages is empty, `build_prompt` early-returns** (`builder.rs:build_prompt` start `!is_first && messages.is_empty()` → empty BuiltContext) — an empty turn emits a TurnStarted with an empty full_prompt and spawns the turn as usual; be careful when changing the early-return condition or relying on rendered_prompt for display
- First-turn determination = **chapter initial state** (`conversation.rs:Conversation::is_first_render()` = `turn_count == 0`), not "whether the session is fresh" nor "whether context messages are empty" — **a new chapter from successful wrap-up (summary at the start, context non-empty) is still first turn** (turn_count reset to zero), the first turn fully built; a restored conversation with `turn_count > 0` is not first turn (seamless continuation); a failed turn doesn't advance turn_count → first-turn eligibility is naturally preserved
- `Node::is_empty` only recognizes children (Text nodes are always non-empty) — "empty sections dropped" relies on the no-children check; adding fixed text/attributes to a section won't trigger dropping
- Legacy: unread semantics rely only on `interaction_status` and `<separator>` position; when changing rendering, keep the "single divider" convention. Already cleaned: first-turn topics injection cap (`related_topic_limit` truncation)
