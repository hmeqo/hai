use crate::{cli::display, domain::model::Event};

const PAGE_SIZE: usize = 200;

/// 数据层：维护一个 seq 升序的滑动窗口。
/// buffer 中仅包含匹配当前 chat_filter/kind_filter 的事件。
/// 渲染层直接读 `buffer()`，无中间索引。
pub(super) struct EventStore {
    db: toasty::Db,
    pub chat_filter: Option<i64>,
    pub kind_filter: Option<String>,

    /// 窗口中的事件，seq 升序，仅包含匹配 filter 的事件
    window: Vec<Event>,

    /// 窗口中最小/最大 seq
    pub min_seq: i64,
    pub max_seq: i64,

    /// 当前 filter 下是否已到边界
    pub at_start: bool,
    pub at_end: bool,

    capacity: usize,
}

impl EventStore {
    pub fn new(db: toasty::Db, chat_filter: Option<i64>, kind_filter: Option<String>) -> Self {
        Self {
            db,
            chat_filter,
            kind_filter,
            window: Vec::new(),
            min_seq: i64::MAX,
            max_seq: 0,
            at_start: false,
            at_end: true,
            capacity: PAGE_SIZE * 2,
        }
    }

    pub fn set_viewport(&mut self, lines: usize) {
        self.capacity = (lines * 3).max(PAGE_SIZE * 2);
    }

    pub fn window(&self) -> &[Event] {
        &self.window
    }

    /// 跳转到最新匹配事件。
    pub async fn load_end(&mut self) {
        let events = display::raw_query(
            &mut self.db.clone(),
            self.chat_filter,
            self.kind_filter.as_deref(),
            None,
            None,
            true,
            self.capacity,
        )
        .await
        .unwrap_or_default();
        self.set_window(events);
        self.at_end = true;
        self.at_start = false;
    }

    /// 跳转到最早匹配事件。
    pub async fn load_start(&mut self) {
        let events = display::raw_query(
            &mut self.db.clone(),
            self.chat_filter,
            self.kind_filter.as_deref(),
            None,
            None,
            false,
            self.capacity,
        )
        .await
        .unwrap_or_default();
        self.set_window(events);
        self.at_start = self.window.first().map(|e| e.seq == 1).unwrap_or(true);
        self.at_end = false;
    }

    /// 前置加载更旧匹配事件。返回前置数量。
    pub async fn extend_back(&mut self) -> usize {
        if self.at_start || self.window.is_empty() {
            return 0;
        }

        let older = display::raw_query(
            &mut self.db.clone(),
            self.chat_filter,
            self.kind_filter.as_deref(),
            Some(self.min_seq),
            None,
            true,
            PAGE_SIZE,
        )
        .await
        .unwrap_or_default();

        if older.is_empty() {
            self.at_start = true;
            return 0;
        }

        self.at_start = older.len() < PAGE_SIZE;
        let n = older.len();

        // older is DESC (newest-first within the older range), reverse to ASC
        let mut older = older;
        older.reverse();

        // Update min_seq from the oldest in the batch (first after reverse)
        if let Some(oldest) = older.first() {
            self.min_seq = oldest.seq;
        }

        self.window.splice(0..0, older);
        self.trim_to_capacity_back();
        n
    }

    /// 轮询最新匹配事件。返回新增数量。
    pub async fn poll(&mut self) -> usize {
        let before = self.window.len();
        let events = display::raw_query(
            &mut self.db.clone(),
            self.chat_filter,
            self.kind_filter.as_deref(),
            None,
            Some(self.max_seq),
            false,
            PAGE_SIZE,
        )
        .await
        .unwrap_or_default();

        if events.is_empty() {
            self.at_end = true;
            return 0;
        }

        self.at_end = false;

        if let Some(last) = events.last()
            && last.seq > self.max_seq
        {
            self.max_seq = last.seq;
        }
        if let Some(first) = events.first()
            && first.seq < self.min_seq
        {
            self.min_seq = first.seq;
        }

        self.window.extend(events);
        self.trim_to_capacity_front();
        self.window.len() - before
    }

    // ── Private ──

    fn set_window(&mut self, mut events: Vec<Event>) {
        if events.is_empty() {
            self.window.clear();
            self.min_seq = i64::MAX;
            self.max_seq = 0;
            return;
        }

        if events.first().map(|e| e.seq).unwrap_or(0) > events.last().map(|e| e.seq).unwrap_or(0) {
            events.reverse();
        }

        self.min_seq = events.first().map(|e| e.seq).unwrap_or(i64::MAX);
        self.max_seq = events.last().map(|e| e.seq).unwrap_or(0);
        self.window = events;
        self.trim_to_capacity();
    }

    fn trim_to_capacity(&mut self) {
        if self.window.len() > self.capacity {
            self.window.truncate(self.capacity);
            self.max_seq = self.window.last().map(|e| e.seq).unwrap_or(0);
        }
    }

    fn trim_to_capacity_front(&mut self) {
        let overflow = self.window.len().saturating_sub(self.capacity);
        if overflow > 0 {
            self.window.drain(..overflow);
            self.min_seq = self.window.first().map(|e| e.seq).unwrap_or(i64::MAX);
        }
    }

    fn trim_to_capacity_back(&mut self) {
        let overflow = self.window.len().saturating_sub(self.capacity);
        if overflow > 0 {
            self.window.truncate(self.capacity);
            self.max_seq = self.window.last().map(|e| e.seq).unwrap_or(0);
        }
    }
}
