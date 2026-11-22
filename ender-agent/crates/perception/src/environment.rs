use ender_minecraft::{GameEvent, PlayerSnapshot};

use crate::entities::EntityView;
use crate::inventory::InventorySnapshot;
use crate::world::{BlockPos, WorldState};

pub struct Perception {
    state: WorldState,
}

impl Default for Perception {
    fn default() -> Self {
        Self::new()
    }
}

impl Perception {
    pub fn new() -> Self {
        Self {
            state: WorldState::empty(),
        }
    }

    pub fn state(&self) -> &WorldState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut WorldState {
        &mut self.state
    }

    pub fn apply_snapshot(&mut self, snap: &PlayerSnapshot) {
        self.state.apply_snapshot(snap);
    }

    pub fn apply_event(&mut self, event: &GameEvent) {
        match event {
            GameEvent::Tick => {}
            GameEvent::Login => {}
            GameEvent::Chat { .. } => {}
            GameEvent::BlockUpdate { pos, block } => {
                self.state
                    .nearby_blocks
                    .insert(BlockPos::from_array(*pos), block.clone());
            }
            GameEvent::EntitySpawned(e) => {
                let view = EntityView::from(e.clone());
                self.state.entities.retain(|x| x.id != view.id);
                self.state.entities.push(view);
            }
            GameEvent::EntityDespawned { id } => {
                self.state.entities.retain(|x| x.id != *id);
            }
            GameEvent::HealthUpdate { health, hunger } => {
                self.state.health = *health;
                self.state.hunger = *hunger;
            }
            GameEvent::InventoryUpdate { items } => {
                self.state.inventory = InventorySnapshot::from_items(items.clone());
            }
            GameEvent::Damage { amount, source } => {
                self.state.last_damage = Some((*amount, source.clone()));
            }
            GameEvent::Death { .. } => {
                self.state.health = 0.0;
            }
            GameEvent::Disconnect { .. } => {}
        }
    }

    pub fn observe_environment(&self, radius: i32) -> Vec<(BlockPos, String)> {
        let center = BlockPos::from_array([
            self.state.position[0].floor() as i32,
            self.state.position[1].floor() as i32,
            self.state.position[2].floor() as i32,
        ]);
        let r2 = (radius * radius) as f64;
        self.state
            .nearby_blocks
            .iter()
            .filter(|(p, _)| p.distance_sq(&center) <= r2)
            .map(|(p, b)| (*p, b.clone()))
            .collect()
    }
}
