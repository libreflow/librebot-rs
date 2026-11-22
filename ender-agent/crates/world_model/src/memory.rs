use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub tick: u64,
    pub wall_ms: u128,
    pub kind: String,
    pub detail: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: u64,
    pub started_ms: u128,
    pub events: Vec<MemoryEvent>,
}

pub trait MemoryStore: Send + Sync {
    fn append(&mut self, event: &MemoryEvent);
    fn recall(&self, query: &str, limit: usize) -> Vec<MemoryEvent>;
    fn all(&self, limit: usize) -> Vec<MemoryEvent>;
    fn save_episode(&mut self, episode: &Episode);
    fn episodes(&self, limit: usize) -> Vec<Episode>;
}

pub struct InMemoryMemory {
    events: Vec<MemoryEvent>,
    episodes: Vec<Episode>,
}

impl Default for InMemoryMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMemory {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            episodes: Vec::new(),
        }
    }
}

impl MemoryStore for InMemoryMemory {
    fn append(&mut self, event: &MemoryEvent) {
        self.events.push(event.clone());
    }

    fn recall(&self, query: &str, limit: usize) -> Vec<MemoryEvent> {
        let q = query.to_lowercase();
        self.events
            .iter()
            .filter(|e| e.confidence >= 0.05)
            .filter(|e| {
                e.kind.to_lowercase().contains(&q) || e.detail.to_lowercase().contains(&q)
            })
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    fn all(&self, limit: usize) -> Vec<MemoryEvent> {
        self.events.iter().rev().take(limit).cloned().collect()
    }

    fn save_episode(&mut self, episode: &Episode) {
        self.episodes.push(episode.clone());
    }

    fn episodes(&self, limit: usize) -> Vec<Episode> {
        self.episodes.iter().rev().take(limit).cloned().collect()
    }
}

#[cfg(feature = "persist")]
pub struct SqliteMemory {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

#[cfg(feature = "persist")]
impl SqliteMemory {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tick INTEGER NOT NULL,
                wall_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                detail TEXT NOT NULL,
                confidence REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_kind ON memory_events(kind);
            CREATE TABLE IF NOT EXISTS episodes (
                id INTEGER PRIMARY KEY,
                started_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS episode_events (
                episode_id INTEGER NOT NULL,
                tick INTEGER NOT NULL,
                wall_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                detail TEXT NOT NULL,
                confidence REAL NOT NULL
            );",
        )?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tick INTEGER NOT NULL,
                wall_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                detail TEXT NOT NULL,
                confidence REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS episodes (
                id INTEGER PRIMARY KEY,
                started_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS episode_events (
                episode_id INTEGER NOT NULL,
                tick INTEGER NOT NULL,
                wall_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                detail TEXT NOT NULL,
                confidence REAL NOT NULL
            );",
        )?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }
}

#[cfg(feature = "persist")]
impl MemoryStore for SqliteMemory {
    fn append(&mut self, event: &MemoryEvent) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO memory_events (tick, wall_ms, kind, detail, confidence) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                event.tick as i64,
                event.wall_ms as i64,
                &event.kind,
                &event.detail,
                event.confidence
            ],
        );
    }

    fn recall(&self, query: &str, limit: usize) -> Vec<MemoryEvent> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT tick, wall_ms, kind, detail, confidence FROM memory_events
                 WHERE confidence >= 0.05 AND (kind LIKE ?1 OR detail LIKE ?1)
                 ORDER BY id DESC LIMIT ?2",
            )
            .unwrap();
        let rows = stmt
            .query_map(rusqlite::params![pattern, limit as i64], |row| {
                Ok(MemoryEvent {
                    tick: row.get::<_, i64>(0)? as u64,
                    wall_ms: row.get::<_, i64>(1)? as u128,
                    kind: row.get(2)?,
                    detail: row.get(3)?,
                    confidence: row.get(4)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    fn all(&self, limit: usize) -> Vec<MemoryEvent> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT tick, wall_ms, kind, detail, confidence FROM memory_events
                 ORDER BY id DESC LIMIT ?1",
            )
            .unwrap();
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(MemoryEvent {
                    tick: row.get::<_, i64>(0)? as u64,
                    wall_ms: row.get::<_, i64>(1)? as u128,
                    kind: row.get(2)?,
                    detail: row.get(3)?,
                    confidence: row.get(4)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    fn save_episode(&mut self, episode: &Episode) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO episodes (id, started_ms) VALUES (?1, ?2)",
            rusqlite::params![episode.id as i64, episode.started_ms as i64],
        );
        for ev in &episode.events {
            let _ = conn.execute(
                "INSERT INTO episode_events (episode_id, tick, wall_ms, kind, detail, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    episode.id as i64,
                    ev.tick as i64,
                    ev.wall_ms as i64,
                    &ev.kind,
                    &ev.detail,
                    ev.confidence
                ],
            );
        }
    }

    fn episodes(&self, limit: usize) -> Vec<Episode> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, started_ms FROM episodes ORDER BY id DESC LIMIT ?1")
            .unwrap();
        let ids = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u128))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        for (id, started_ms) in ids {
            let mut stmt = conn
                .prepare(
                    "SELECT tick, wall_ms, kind, detail, confidence FROM episode_events
                     WHERE episode_id = ?1 ORDER BY rowid",
                )
                .unwrap();
            let events = stmt
                .query_map(rusqlite::params![id as i64], |row| {
                    Ok(MemoryEvent {
                        tick: row.get::<_, i64>(0)? as u64,
                        wall_ms: row.get::<_, i64>(1)? as u128,
                        kind: row.get(2)?,
                        detail: row.get(3)?,
                        confidence: row.get(4)?,
                    })
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            out.push(Episode {
                id,
                started_ms,
                events,
            });
        }
        out
    }
}

pub struct Memory {
    store: Box<dyn MemoryStore>,
    next_episode: u64,
    current: Episode,
}

impl Memory {
    pub fn in_memory() -> Self {
        Self {
            store: Box::new(InMemoryMemory::new()),
            next_episode: 1,
            current: Episode {
                id: 0,
                started_ms: now_ms(),
                events: Vec::new(),
            },
        }
    }

    #[cfg(feature = "persist")]
    pub fn sqlite(path: &str) -> rusqlite::Result<Self> {
        Ok(Self {
            store: Box::new(SqliteMemory::open(path)?),
            next_episode: 1,
            current: Episode {
                id: 0,
                started_ms: now_ms(),
                events: Vec::new(),
            },
        })
    }

    pub fn record(&mut self, tick: u64, kind: &str, detail: &str, confidence: f32) {
        let event = MemoryEvent {
            tick,
            wall_ms: now_ms(),
            kind: kind.to_string(),
            detail: detail.to_string(),
            confidence: confidence.clamp(0.0, 1.0),
        };
        self.store.append(&event);
        self.current.events.push(event);
    }

    pub fn recall(&self, query: &str, limit: usize) -> Vec<MemoryEvent> {
        self.store.recall(query, limit)
    }

    pub fn close_episode(&mut self) {
        let mut ep = Episode {
            id: self.next_episode,
            started_ms: self.current.started_ms,
            events: std::mem::take(&mut self.current.events),
        };
        self.next_episode += 1;
        ep.id = self.next_episode - 1;
        self.store.save_episode(&ep);
        self.current = Episode {
            id: 0,
            started_ms: now_ms(),
            events: Vec::new(),
        };
    }

    pub fn episodes(&self, limit: usize) -> Vec<Episode> {
        self.store.episodes(limit)
    }

    pub fn current_episode(&self) -> &Episode {
        &self.current
    }
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
