use std::collections::HashMap;

use ender_minecraft::PlayerSnapshot;
use serde::{Deserialize, Serialize};

use crate::entities::{nearest_hostile, EntityView};
use crate::inventory::InventorySnapshot;
use crate::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_array(a: [i32; 3]) -> Self {
        Self {
            x: a[0],
            y: a[1],
            z: a[2],
        }
    }

    pub fn to_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }

    pub fn distance_sq(&self, other: &BlockPos) -> f64 {
        let dx = (self.x - other.x) as f64;
        let dy = (self.y - other.y) as f64;
        let dz = (self.z - other.z) as f64;
        dx * dx + dy * dy + dz * dz
    }

    pub fn chunk(&self) -> (i32, i32) {
        (self.x >> 4, self.z >> 4)
    }
}

impl From<[i32; 3]> for BlockPos {
    fn from(a: [i32; 3]) -> Self {
        Self::from_array(a)
    }
}

#[derive(Debug, Clone)]
pub struct WorldState {
    pub tick: u64,
    pub position: [f64; 3],
    pub dimension: String,
    pub health: f32,
    pub hunger: f32,
    pub inventory: InventorySnapshot,
    pub entities: Vec<EntityView>,
    pub nearby_blocks: HashMap<BlockPos, String>,
    pub last_damage: Option<(f32, String)>,
    pub updated_ms: u128,
}

impl Default for WorldState {
    fn default() -> Self {
        Self::empty()
    }
}

impl WorldState {
    pub fn empty() -> Self {
        Self {
            tick: 0,
            position: [0.0, 64.0, 0.0],
            dimension: "minecraft:overworld".to_string(),
            health: 20.0,
            hunger: 20.0,
            inventory: InventorySnapshot::default(),
            entities: Vec::new(),
            nearby_blocks: HashMap::new(),
            last_damage: None,
            updated_ms: 0,
        }
    }

    pub fn apply_snapshot(&mut self, snap: &PlayerSnapshot) {
        self.tick = snap.tick;
        self.position = snap.position;
        self.dimension = snap.dimension.clone();
        self.health = snap.health;
        self.hunger = snap.hunger;
        self.inventory = InventorySnapshot::from_items(snap.inventory.clone());
        self.entities = snap.entities.iter().cloned().map(EntityView::from).collect();
        self.nearby_blocks = snap
            .nearby_blocks
            .iter()
            .map(|(p, b)| (BlockPos::from_array(*p), b.clone()))
            .collect();
        self.updated_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
    }

    pub fn block_at(&self, pos: BlockPos) -> Option<&str> {
        self.nearby_blocks.get(&pos).map(|s| s.as_str())
    }

    pub fn nearest_block(
        &self,
        predicate: impl Fn(&str) -> bool,
    ) -> Option<(BlockPos, f64)> {
        let from = [
            self.position[0].floor() as i32,
            self.position[1].floor() as i32,
            self.position[2].floor() as i32,
        ];
        let from = BlockPos::from_array(from);
        self.nearby_blocks
            .iter()
            .filter(|(_, b)| predicate(b))
            .map(|(p, _)| (*p, p.distance_sq(&from)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn to_state_map(&self) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        map.insert("tick".to_string(), Value::from(self.tick));
        map.insert("pos_x".to_string(), Value::from(self.position[0].floor() as i64));
        map.insert("pos_y".to_string(), Value::from(self.position[1].floor() as i64));
        map.insert("pos_z".to_string(), Value::from(self.position[2].floor() as i64));
        map.insert("dimension".to_string(), Value::from(self.dimension.as_str()));
        map.insert("health".to_string(), Value::from(self.health));
        map.insert("hunger".to_string(), Value::from(self.hunger));
        for (item, count) in self.inventory.items() {
            map.insert(format!("count_item:{item}"), Value::from(*count));
            map.insert(format!("has_item:{item}"), Value::from(*count > 0));
        }
        let log = self.nearest_block(|b| b.contains("log"));
        match log {
            Some((pos, d)) => {
                map.insert("nearest_log_dist".to_string(), Value::from(d.sqrt() as i64));
                map.insert("block_log_nearby".to_string(), Value::Bool(d.sqrt() <= 6.0));
                map.insert(
                    "nearest_log_pos".to_string(),
                    Value::from(format!("{},{},{}", pos.x, pos.y, pos.z)),
                );
            }
            None => {
                map.insert("nearest_log_dist".to_string(), Value::from(i64::MAX));
                map.insert("block_log_nearby".to_string(), Value::Bool(false));
            }
        }
        let hostile = nearest_hostile(&self.entities, self.position);
        match hostile {
            Some((_, d)) => {
                map.insert("hostile_nearby".to_string(), Value::Bool(d <= 12.0));
                map.insert("nearest_hostile_dist".to_string(), Value::from(d));
            }
            None => {
                map.insert("hostile_nearby".to_string(), Value::Bool(false));
                map.insert("nearest_hostile_dist".to_string(), Value::from(f32::MAX));
            }
        }
        map.insert(
            "damaged_recently".to_string(),
            Value::Bool(self.last_damage.is_some()),
        );
        map
    }
}
