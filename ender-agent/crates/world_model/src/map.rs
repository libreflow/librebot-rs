use std::collections::{HashMap, HashSet};

use ender_perception::BlockPos;

#[derive(Debug, Clone)]
pub struct ObservedBlock {
    pub block: String,
    pub tick: u64,
    pub confidence: f32,
}

pub struct WorldMap {
    blocks: HashMap<BlockPos, ObservedBlock>,
    explored_chunks: HashSet<(i32, i32)>,
    pub decay_per_100_ticks: f32,
}

impl Default for WorldMap {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldMap {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            explored_chunks: HashSet::new(),
            decay_per_100_ticks: 0.05,
        }
    }

    pub fn observe(&mut self, pos: BlockPos, block: &str, tick: u64, confidence: f32) {
        self.blocks.insert(
            pos,
            ObservedBlock {
                block: block.to_string(),
                tick,
                confidence: confidence.clamp(0.0, 1.0),
            },
        );
        self.explored_chunks.insert(pos.chunk());
    }

    pub fn get(&self, pos: &BlockPos) -> Option<&ObservedBlock> {
        self.blocks.get(pos)
    }

    pub fn confidence_at(&self, pos: &BlockPos, current_tick: u64) -> f32 {
        match self.blocks.get(pos) {
            Some(obs) => {
                let age = current_tick.saturating_sub(obs.tick);
                let decay = self.decay_per_100_ticks * (age as f32 / 100.0);
                (obs.confidence - decay).clamp(0.0, 1.0)
            }
            None => 0.0,
        }
    }

    pub fn is_stale(&self, pos: &BlockPos, current_tick: u64, max_age_ticks: u64) -> bool {
        match self.blocks.get(pos) {
            Some(obs) => {
                current_tick.saturating_sub(obs.tick) > max_age_ticks
                    || self.confidence_at(pos, current_tick) < 0.05
            }
            None => true,
        }
    }

    pub fn scan_radius(
        &self,
        center: BlockPos,
        radius: i32,
        current_tick: u64,
        max_age_ticks: u64,
    ) -> Vec<(BlockPos, String, f32)> {
        let r2 = (radius * radius) as f64;
        let mut out: Vec<(BlockPos, String, f32)> = self
            .blocks
            .iter()
            .filter(|(p, _)| p.distance_sq(&center) <= r2)
            .filter(|(p, _)| !self.is_stale(p, current_tick, max_age_ticks))
            .map(|(p, o)| (*p, o.block.clone(), self.confidence_at(p, current_tick)))
            .collect();
        out.sort_by(|a, b| a.0.distance_sq(&center).partial_cmp(&b.0.distance_sq(&center)).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    pub fn find_nearest(
        &self,
        center: BlockPos,
        predicate: impl Fn(&str) -> bool,
        current_tick: u64,
        max_age_ticks: u64,
    ) -> Option<(BlockPos, String, f32)> {
        self.blocks
            .iter()
            .filter(|(p, o)| predicate(&o.block) && !self.is_stale(p, current_tick, max_age_ticks))
            .map(|(p, o)| (*p, o.block.clone(), self.confidence_at(p, current_tick)))
            .min_by(|a, b| {
                a.0.distance_sq(&center)
                    .partial_cmp(&b.0.distance_sq(&center))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    pub fn mark_chunk_explored(&mut self, chunk: (i32, i32)) {
        self.explored_chunks.insert(chunk);
    }

    pub fn explored_chunks(&self) -> usize {
        self.explored_chunks.len()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn decay(&mut self, current_tick: u64) {
        let decay = self.decay_per_100_ticks;
        for obs in self.blocks.values_mut() {
            let age = current_tick.saturating_sub(obs.tick);
            if age > 0 {
                obs.confidence = (obs.confidence - decay * (age as f32 / 100.0)).clamp(0.0, 1.0);
            }
        }
    }
}
