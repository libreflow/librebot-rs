use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityInfo {
    pub id: i64,
    pub kind: String,
    pub pos: [f64; 3],
    pub hostile: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    Tick,
    Login,
    Chat { sender: Option<String>, content: String },
    BlockUpdate { pos: [i32; 3], block: String },
    EntitySpawned(EntityInfo),
    EntityDespawned { id: i64 },
    HealthUpdate { health: f32, hunger: f32 },
    InventoryUpdate { items: Vec<(String, u32)> },
    Damage { amount: f32, source: String },
    Death { reason: Option<String> },
    Disconnect { reason: Option<String> },
}
