pub mod knowledge;
pub mod map;
pub mod memory;

pub use knowledge::{BlockInfo, Knowledge, Recipe};
pub use map::{ObservedBlock, WorldMap};
pub use memory::{now_ms, Episode, InMemoryMemory, Memory, MemoryEvent, MemoryStore};
#[cfg(feature = "persist")]
pub use memory::SqliteMemory;

use ender_perception::BlockPos;

pub struct WorldModel {
    pub map: WorldMap,
    pub memory: Memory,
    pub knowledge: Knowledge,
}

impl Default for WorldModel {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldModel {
    pub fn new() -> Self {
        Self {
            map: WorldMap::new(),
            memory: Memory::in_memory(),
            knowledge: Knowledge::vanilla(),
        }
    }

    #[cfg(feature = "persist")]
    pub fn with_sqlite(path: &str) -> rusqlite::Result<Self> {
        Ok(Self {
            map: WorldMap::new(),
            memory: Memory::sqlite(path)?,
            knowledge: Knowledge::vanilla(),
        })
    }

    pub fn observe_blocks(&mut self, blocks: &[(BlockPos, String)], tick: u64, confidence: f32) {
        for (pos, block) in blocks {
            self.map.observe(*pos, block, tick, confidence);
        }
    }

    pub fn record(&mut self, tick: u64, kind: &str, detail: &str, confidence: f32) {
        self.memory.record(tick, kind, detail, confidence);
    }
}
