use async_trait::async_trait;
use dashmap::DashMap;
use redis::{aio::ConnectionManager, AsyncCommands};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug)]
pub enum EnterOutcome {
    Admitted,
    Waiting(usize),
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Admitted,
    Waiting(usize),
    Unknown,
}

#[derive(Debug)]
pub enum AdmitForcedOutcome {
    AlreadyAdmitted,
    Admitted,
    NotWaiting,
}

#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub token: String,
    pub idle_secs: u64,
    pub position: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Snapshot {
    pub active: Vec<TokenEntry>,
    pub waiting: Vec<TokenEntry>,
    pub total_active: usize,
    pub total_waiting: usize,
}

#[async_trait]
pub trait Backend: Send + Sync {
    async fn try_enter(&self, token: &str, max_active: usize) -> EnterOutcome;
    async fn refresh(&self, token: &str, max_active: usize) -> RefreshOutcome;
    async fn heartbeat(&self, token: &str) -> bool;
    async fn leave(&self, token: &str);
    async fn cleanup(&self, max_active: usize, active_ttl: Duration, waiting_ttl: Duration);
    async fn admit_forced(&self, token: &str) -> AdmitForcedOutcome;
    async fn kick(&self, token: &str) -> bool;
    async fn clear(&self);
    async fn list_state(&self) -> Snapshot;
    async fn list_state_paginated(&self, limit: usize, offset: usize) -> Snapshot;
    async fn stats(&self) -> (usize, usize);
}

// ============================================================
// Memory Backend
// ============================================================

pub struct MemoryBackend {
    queue: RwLock<VecDeque<String>>,
    waiting: DashMap<String, i64>,
    active: DashMap<String, i64>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            queue: RwLock::new(VecDeque::new()),
            waiting: DashMap::new(),
            active: DashMap::new(),
        }
    }

    async fn promote_internal(&self, max_active: usize) {
        let mut queue = self.queue.write().await;
        while self.active.len() < max_active {
            match queue.pop_front() {
                Some(token) => {
                    if self.waiting.remove(&token).is_some() {
                        self.active.insert(token, now_ms());
                    }
                }
                None => break,
            }
        }
    }
}

#[async_trait]
impl Backend for MemoryBackend {
    async fn try_enter(&self, token: &str, max_active: usize) -> EnterOutcome {
        self.promote_internal(max_active).await;
        let now = now_ms();
        if self.active.len() < max_active {
            self.active.insert(token.to_string(), now);
            return EnterOutcome::Admitted;
        }
        let mut queue = self.queue.write().await;
        queue.push_back(token.to_string());
        self.waiting.insert(token.to_string(), now);
        EnterOutcome::Waiting(queue.len())
    }

    async fn refresh(&self, token: &str, max_active: usize) -> RefreshOutcome {
        let now = now_ms();
        if let Some(mut e) = self.active.get_mut(token) {
            *e = now;
            return RefreshOutcome::Admitted;
        }
        if let Some(mut e) = self.waiting.get_mut(token) {
            *e = now;
        } else {
            return RefreshOutcome::Unknown;
        }
        self.promote_internal(max_active).await;
        if self.active.contains_key(token) {
            return RefreshOutcome::Admitted;
        }
        let queue = self.queue.read().await;
        let pos = queue.iter().position(|t| t == token).map(|i| i + 1).unwrap_or(0);
        RefreshOutcome::Waiting(pos)
    }

    async fn heartbeat(&self, token: &str) -> bool {
        let now = now_ms();
        if let Some(mut e) = self.active.get_mut(token) {
            *e = now;
            return true;
        }
        if let Some(mut e) = self.waiting.get_mut(token) {
            *e = now;
            return true;
        }
        false
    }

    async fn leave(&self, token: &str) {
        self.active.remove(token);
        self.waiting.remove(token);
        let mut queue = self.queue.write().await;
        queue.retain(|t| t != token);
    }

    async fn cleanup(&self, max_active: usize, active_ttl: Duration, waiting_ttl: Duration) {
        let now = now_ms();
        let active_ttl_ms = active_ttl.as_millis() as i64;
        let waiting_ttl_ms = waiting_ttl.as_millis() as i64;

        self.active.retain(|_, last| now - *last < active_ttl_ms);

        let stale: Vec<String> = self
            .waiting
            .iter()
            .filter_map(|e| {
                if now - *e.value() >= waiting_ttl_ms {
                    Some(e.key().clone())
                } else {
                    None
                }
            })
            .collect();
        if !stale.is_empty() {
            let mut queue = self.queue.write().await;
            for token in &stale {
                self.waiting.remove(token);
                queue.retain(|t| t != token);
            }
        }

        self.promote_internal(max_active).await;
    }

    async fn admit_forced(&self, token: &str) -> AdmitForcedOutcome {
        if self.active.contains_key(token) {
            return AdmitForcedOutcome::AlreadyAdmitted;
        }
        if self.waiting.remove(token).is_none() {
            return AdmitForcedOutcome::NotWaiting;
        }
        {
            let mut queue = self.queue.write().await;
            queue.retain(|t| t != token);
        }
        self.active.insert(token.to_string(), now_ms());
        AdmitForcedOutcome::Admitted
    }

    async fn kick(&self, token: &str) -> bool {
        let was_active = self.active.remove(token).is_some();
        let was_waiting = self.waiting.remove(token).is_some();
        if was_waiting {
            let mut queue = self.queue.write().await;
            queue.retain(|t| t != token);
        }
        was_active || was_waiting
    }

    async fn clear(&self) {
        self.active.clear();
        self.waiting.clear();
        self.queue.write().await.clear();
    }

    async fn list_state(&self) -> Snapshot {
        self.list_state_paginated(usize::MAX, 0).await
    }

    async fn list_state_paginated(&self, limit: usize, offset: usize) -> Snapshot {
        let now = now_ms();
        let total_active = self.active.len();
        let active: Vec<TokenEntry> = self
            .active
            .iter()
            .skip(offset)
            .take(limit)
            .map(|e| TokenEntry {
                token: e.key().clone(),
                idle_secs: ((now - *e.value()).max(0) / 1000) as u64,
                position: None,
            })
            .collect();
        let queue = self.queue.read().await;
        let total_waiting = queue.len();
        let waiting: Vec<TokenEntry> = queue
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit)
            .map(|(i, token)| {
                let idle = self
                    .waiting
                    .get(token)
                    .map(|e| ((now - *e.value()).max(0) / 1000) as u64)
                    .unwrap_or(0);
                TokenEntry {
                    token: token.clone(),
                    idle_secs: idle,
                    position: Some(i + 1),
                }
            })
            .collect();
        Snapshot { active, waiting, total_active, total_waiting }
    }

    async fn stats(&self) -> (usize, usize) {
        (self.active.len(), self.queue.read().await.len())
    }
}

// ============================================================
// Redis Backend
// ============================================================

pub struct RedisBackend {
    conn: ConnectionManager,
    domain: String,
}

impl RedisBackend {
    pub fn new(conn: ConnectionManager, domain: String) -> Self {
        Self { conn, domain }
    }

    fn k_queue(&self) -> String { format!("pcw:queue:{}", self.domain) }
    fn k_waiting(&self) -> String { format!("pcw:waiting:{}", self.domain) }
    fn k_active(&self) -> String { format!("pcw:active:{}", self.domain) }

    async fn active_len(&self) -> redis::RedisResult<usize> {
        let mut c = self.conn.clone();
        c.hlen(self.k_active()).await
    }

    async fn queue_len(&self) -> redis::RedisResult<usize> {
        let mut c = self.conn.clone();
        c.zcard(self.k_queue()).await
    }

    async fn promote_internal(&self, max_active: usize) -> redis::RedisResult<()> {
        let mut c = self.conn.clone();
        loop {
            let active_len: usize = c.hlen(self.k_active()).await?;
            if active_len >= max_active {
                break;
            }
            let popped: Vec<String> = c.zpopmin(self.k_queue(), 1).await.unwrap_or_default();
            if popped.is_empty() {
                break;
            }
            let token = &popped[0];
            let removed: i64 = c.hdel(self.k_waiting(), token).await.unwrap_or(0);
            if removed > 0 {
                let _: () = c.hset(self.k_active(), token, now_ms()).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Backend for RedisBackend {
    async fn try_enter(&self, token: &str, max_active: usize) -> EnterOutcome {
        let _ = self.promote_internal(max_active).await;
        let mut c = self.conn.clone();
        let now = now_ms();
        let active_len: usize = c.hlen(self.k_active()).await.unwrap_or(0);
        if active_len < max_active {
            let _: redis::RedisResult<()> = c.hset(self.k_active(), token, now).await;
            return EnterOutcome::Admitted;
        }
        let _: redis::RedisResult<()> = c.zadd(self.k_queue(), token, now).await;
        let _: redis::RedisResult<()> = c.hset(self.k_waiting(), token, now).await;
        let rank: i64 = c.zrank(self.k_queue(), token).await.unwrap_or(0);
        EnterOutcome::Waiting((rank + 1) as usize)
    }

    async fn refresh(&self, token: &str, max_active: usize) -> RefreshOutcome {
        let mut c = self.conn.clone();
        let now = now_ms();
        let in_active: bool = c.hexists(self.k_active(), token).await.unwrap_or(false);
        if in_active {
            let _: redis::RedisResult<()> = c.hset(self.k_active(), token, now).await;
            return RefreshOutcome::Admitted;
        }
        let in_waiting: bool = c.hexists(self.k_waiting(), token).await.unwrap_or(false);
        if !in_waiting {
            return RefreshOutcome::Unknown;
        }
        let _: redis::RedisResult<()> = c.hset(self.k_waiting(), token, now).await;
        let _ = self.promote_internal(max_active).await;
        let now_in_active: bool = c.hexists(self.k_active(), token).await.unwrap_or(false);
        if now_in_active {
            return RefreshOutcome::Admitted;
        }
        let rank: Option<i64> = c.zrank(self.k_queue(), token).await.ok().flatten();
        RefreshOutcome::Waiting(rank.map(|r| (r + 1) as usize).unwrap_or(0))
    }

    async fn heartbeat(&self, token: &str) -> bool {
        let mut c = self.conn.clone();
        let now = now_ms();
        let in_active: bool = c.hexists(self.k_active(), token).await.unwrap_or(false);
        if in_active {
            let _: redis::RedisResult<()> = c.hset(self.k_active(), token, now).await;
            return true;
        }
        let in_waiting: bool = c.hexists(self.k_waiting(), token).await.unwrap_or(false);
        if in_waiting {
            let _: redis::RedisResult<()> = c.hset(self.k_waiting(), token, now).await;
            return true;
        }
        false
    }

    async fn leave(&self, token: &str) {
        let mut c = self.conn.clone();
        let _: redis::RedisResult<i64> = c.hdel(self.k_active(), token).await;
        let _: redis::RedisResult<i64> = c.hdel(self.k_waiting(), token).await;
        let _: redis::RedisResult<i64> = c.zrem(self.k_queue(), token).await;
    }

    async fn cleanup(&self, max_active: usize, active_ttl: Duration, waiting_ttl: Duration) {
        let mut c = self.conn.clone();
        let now = now_ms();
        let active_cutoff = now - active_ttl.as_millis() as i64;
        let waiting_cutoff = now - waiting_ttl.as_millis() as i64;

        let active_all: std::collections::HashMap<String, i64> =
            c.hgetall(self.k_active()).await.unwrap_or_default();
        let stale_active: Vec<&String> = active_all
            .iter()
            .filter_map(|(k, v)| if *v < active_cutoff { Some(k) } else { None })
            .collect();
        for token in &stale_active {
            let _: redis::RedisResult<i64> = c.hdel(self.k_active(), token.as_str()).await;
        }

        let waiting_all: std::collections::HashMap<String, i64> =
            c.hgetall(self.k_waiting()).await.unwrap_or_default();
        let stale_waiting: Vec<&String> = waiting_all
            .iter()
            .filter_map(|(k, v)| if *v < waiting_cutoff { Some(k) } else { None })
            .collect();
        for token in &stale_waiting {
            let _: redis::RedisResult<i64> = c.hdel(self.k_waiting(), token.as_str()).await;
            let _: redis::RedisResult<i64> = c.zrem(self.k_queue(), token.as_str()).await;
        }

        let _ = self.promote_internal(max_active).await;
    }

    async fn admit_forced(&self, token: &str) -> AdmitForcedOutcome {
        let mut c = self.conn.clone();
        let in_active: bool = c.hexists(self.k_active(), token).await.unwrap_or(false);
        if in_active {
            return AdmitForcedOutcome::AlreadyAdmitted;
        }
        let removed: i64 = c.hdel(self.k_waiting(), token).await.unwrap_or(0);
        if removed == 0 {
            return AdmitForcedOutcome::NotWaiting;
        }
        let _: redis::RedisResult<i64> = c.zrem(self.k_queue(), token).await;
        let _: redis::RedisResult<()> = c.hset(self.k_active(), token, now_ms()).await;
        AdmitForcedOutcome::Admitted
    }

    async fn kick(&self, token: &str) -> bool {
        let mut c = self.conn.clone();
        let a: i64 = c.hdel(self.k_active(), token).await.unwrap_or(0);
        let w: i64 = c.hdel(self.k_waiting(), token).await.unwrap_or(0);
        let _: redis::RedisResult<i64> = c.zrem(self.k_queue(), token).await;
        a > 0 || w > 0
    }

    async fn clear(&self) {
        let mut c = self.conn.clone();
        let _: redis::RedisResult<i64> = c.del(self.k_active()).await;
        let _: redis::RedisResult<i64> = c.del(self.k_waiting()).await;
        let _: redis::RedisResult<i64> = c.del(self.k_queue()).await;
    }

    async fn list_state(&self) -> Snapshot {
        self.list_state_paginated(usize::MAX, 0).await
    }

    async fn list_state_paginated(&self, limit: usize, offset: usize) -> Snapshot {
        let mut c = self.conn.clone();
        let now = now_ms();

        let total_active: usize = c.hlen(self.k_active()).await.unwrap_or(0);
        let total_waiting: usize = c.zcard(self.k_queue()).await.unwrap_or(0);

        // Active: HGETALL 후 slice (Redis hash는 순서 보장 X, 그래도 안정성 위해 일관성 유지)
        let active_all: std::collections::HashMap<String, i64> =
            c.hgetall(self.k_active()).await.unwrap_or_default();
        let active: Vec<TokenEntry> = active_all
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(token, last)| TokenEntry {
                token,
                idle_secs: ((now - last).max(0) / 1000) as u64,
                position: None,
            })
            .collect();

        // Waiting: ZRANGE에 LIMIT 사용 (효율적인 페이지네이션)
        let start = offset as isize;
        let end = if limit == usize::MAX {
            -1
        } else {
            (offset + limit).saturating_sub(1) as isize
        };
        let queue: Vec<String> = c.zrange(self.k_queue(), start, end).await.unwrap_or_default();
        let waiting_all: std::collections::HashMap<String, i64> =
            c.hgetall(self.k_waiting()).await.unwrap_or_default();
        let waiting: Vec<TokenEntry> = queue
            .into_iter()
            .enumerate()
            .map(|(i, token)| {
                let idle = waiting_all
                    .get(&token)
                    .map(|v| ((now - *v).max(0) / 1000) as u64)
                    .unwrap_or(0);
                TokenEntry {
                    token,
                    idle_secs: idle,
                    position: Some(offset + i + 1),
                }
            })
            .collect();

        Snapshot { active, waiting, total_active, total_waiting }
    }

    async fn stats(&self) -> (usize, usize) {
        let active = self.active_len().await.unwrap_or(0);
        let waiting = self.queue_len().await.unwrap_or(0);
        (active, waiting)
    }
}

// ============================================================
// Factory
// ============================================================

#[derive(Clone)]
pub enum BackendFactory {
    Memory,
    Redis(ConnectionManager),
}

impl BackendFactory {
    pub fn make(&self, domain: &str) -> Arc<dyn Backend> {
        match self {
            BackendFactory::Memory => Arc::new(MemoryBackend::new()),
            BackendFactory::Redis(conn) => {
                Arc::new(RedisBackend::new(conn.clone(), domain.to_string()))
            }
        }
    }
}
