use std::collections::HashMap;

use crate::{
    cli::display::EventDisplay,
    domain::{model::Event, repo::Repos},
};

const PAGE_SIZE: usize = 200;

/// 数据层：维护一个 seq 升序的滑动窗口，仅含匹配 chat_filter/kind_filter 的事件。
/// 查询错误以 `Result` 上抛（不吞错）；边界（at_start/at_end）以「查询返回 0 条」判定，
/// 瞬时错误不污染边界状态（错误后可重试）。
pub(super) struct EventStore {
    repos: Repos,
    pub chat_filter: Option<i64>,
    pub kind_filter: Option<String>,

    window: Vec<Event>,
    pub at_start: bool,
    pub at_end: bool,

    capacity: usize,
    /// seq → 展示缓存（避免每帧重复反序列化）
    cache: HashMap<i64, EventDisplay>,
}

impl EventStore {
    pub fn new(repos: Repos, chat_filter: Option<i64>, kind_filter: Option<String>) -> Self {
        Self {
            repos,
            chat_filter,
            kind_filter,
            window: Vec::new(),
            at_start: false,
            at_end: false,
            capacity: PAGE_SIZE * 2,
            cache: HashMap::new(),
        }
    }

    pub fn set_viewport(&mut self, lines: usize) {
        self.capacity = (lines * 3).max(PAGE_SIZE * 2);
    }

    pub fn window(&self) -> &[Event] {
        &self.window
    }

    /// 事件展示（解析缓存，每事件只反序列化一次）。
    /// 按 seq 查询，避免调用方同时持有 window 借用时再可变借用 store。
    pub fn display_by_seq(&mut self, seq: i64) -> Option<EventDisplay> {
        if let Some(d) = self.cache.get(&seq) {
            return Some(d.clone());
        }
        let event = self.window.iter().find(|e| e.seq == seq)?;
        let d = EventDisplay::from_event(event).unwrap_or_else(EventDisplay::unparsed);
        self.cache.insert(seq, d.clone());
        Some(d)
    }

    pub fn min_seq(&self) -> Option<i64> {
        self.window.first().map(|e| e.seq)
    }

    pub fn max_seq(&self) -> Option<i64> {
        self.window.last().map(|e| e.seq)
    }

    /// 跳转到最新匹配事件。
    pub async fn load_end(&mut self) -> crate::error::Result<()> {
        let mut events = self
            .repos
            .event
            .query(
                self.chat_filter,
                self.kind_filter.as_deref(),
                None,
                None,
                true,
                self.capacity,
            )
            .await?;
        events.reverse(); // DESC → ASC
        // 重置边界：新窗口是最新 capacity 条——不足则无更旧可加载；
        // at_end 由 append_new 重新判定，避免残留旧值锁死加载路径
        self.at_start = events.len() < self.capacity;
        self.at_end = false;
        self.set_window(events);
        Ok(())
    }

    /// 跳转到最早匹配事件。
    pub async fn load_start(&mut self) -> crate::error::Result<()> {
        let events = self
            .repos
            .event
            .query(
                self.chat_filter,
                self.kind_filter.as_deref(),
                None,
                None,
                false,
                self.capacity,
            )
            .await?;
        // 加载最早一批：无更旧可加载
        self.at_start = true;
        self.at_end = false;
        self.set_window(events);
        Ok(())
    }

    /// 前置加载更旧匹配事件，返回加载条数。
    pub async fn extend_back(&mut self) -> crate::error::Result<usize> {
        let Some(min) = self.min_seq() else {
            self.load_end().await?;
            return Ok(0);
        };
        let mut older = self
            .repos
            .event
            .query(
                self.chat_filter,
                self.kind_filter.as_deref(),
                Some(min),
                None,
                true,
                self.capacity,
            )
            .await?;
        older.reverse();
        if older.is_empty() {
            self.at_start = true;
            return Ok(0);
        }
        self.at_start = false;
        let n = older.len();
        // 前插不裁剪：浏览历史时窗口随加载增长，保留旧窗口与选中（滚动锚定依赖）。
        self.window.splice(0..0, older);
        Ok(n)
    }

    /// 追加最新匹配事件（follow 用），返回新增条数。
    pub async fn append_new(&mut self) -> crate::error::Result<usize> {
        let after = self.max_seq();
        let events = self
            .repos
            .event
            .query(
                self.chat_filter,
                self.kind_filter.as_deref(),
                None,
                after,
                false,
                self.capacity,
            )
            .await?;
        if events.is_empty() {
            self.at_end = true;
            return Ok(0);
        }
        self.at_end = false;
        let n = events.len();
        // 尾部追加不裁剪：窗口随浏览/实时事件纯增长（会话级内存，事件日志量级可接受）。
        // 裁剪会挤出已浏览历史与选中，导致无法继续加载/误触发。
        self.window.extend(events);
        Ok(n)
    }

    /// 统计 seq > max 的匹配事件数（非 follow 提示用，不加载）。
    pub async fn count_new(&mut self) -> crate::error::Result<usize> {
        let Some(after) = self.max_seq() else {
            return Ok(0);
        };
        let count = self
            .repos
            .event
            .count_after(after, self.chat_filter, self.kind_filter.as_deref())
            .await?;
        if count == 0 {
            self.at_end = true;
        }
        Ok(count.max(0) as usize)
    }

    // ── Private ──

    fn set_window(&mut self, events: Vec<Event>) {
        self.window = events;
        self.trim_to_capacity_back();
    }

    fn trim_to_capacity_back(&mut self) {
        if self.window.len() > self.capacity {
            self.window.truncate(self.capacity);
        }
        self.trim_cache();
    }

    /// 缓存膨胀保护：窗口大幅变动时清掉不在窗口内的条目。
    fn trim_cache(&mut self) {
        if self.cache.len() > self.capacity * 4 {
            self.cache
                .retain(|seq, _| self.window.iter().any(|e| e.seq == *seq));
        }
    }
}
