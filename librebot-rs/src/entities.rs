//! Registre minimal des types d'entités (minecraft-data 1.20.5/1.21.1) et suivi de position.

use std::collections::HashMap;

pub const ITEM: i32 = 58;
pub const COW: i32 = 22;
pub const PIG: i32 = 77;
pub const SHEEP: i32 = 87;
pub const CHICKEN: i32 = 19;
pub const RABBIT: i32 = 84;

pub const HOSTILES: &[i32] = &[
    124, // zombie
    54,  // husk
    27,  // drowned
    126, // zombie_villager
    91,  // skeleton
    102, // stray
    100, // spider (neutre le jour)
    16,  // cave_spider
    93,  // slime
    118, // witch
    33,  // enderman
];

pub const CREEPER: i32 = 23;

pub const GIBIERS: &[i32] = &[COW, PIG, SHEEP, CHICKEN, RABBIT];

pub fn entity_name(etype: i32) -> &'static str {
    match etype {
        ITEM => "item",
        COW => "cow",
        PIG => "pig",
        SHEEP => "sheep",
        CHICKEN => "chicken",
        RABBIT => "rabbit",
        CREEPER => "creeper",
        _ => "entity",
    }
}

pub fn is_gibier(etype: i32) -> bool {
    GIBIERS.contains(&etype)
}

pub fn is_hostile(etype: i32) -> bool {
    HOSTILES.contains(&etype)
}

#[derive(Debug, Clone)]
pub struct Entity {
    pub etype: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Default)]
pub struct Entities {
    pub alive: HashMap<i32, Entity>,
}

impl Entities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn(&mut self, id: i32, etype: i32, x: f64, y: f64, z: f64) {
        self.alive.insert(id, Entity { etype, x, y, z });
    }

    pub fn remove(&mut self, ids: &[i32]) {
        for id in ids {
            self.alive.remove(id);
        }
    }

    pub fn translate(&mut self, id: i32, dx: f64, dy: f64, dz: f64) {
        if let Some(e) = self.alive.get_mut(&id) {
            e.x += dx;
            e.y += dy;
            e.z += dz;
        }
    }

    pub fn teleport(&mut self, id: i32, x: f64, y: f64, z: f64) {
        if let Some(e) = self.alive.get_mut(&id) {
            e.x = x;
            e.y = y;
            e.z = z;
        }
    }

    /// Entité la plus proche du point (x, y, z) parmi les types donnés, à moins de `max` blocs.
    pub fn plus_proche(&self, x: f64, y: f64, z: f64, types: &[i32], max: f64) -> Option<(i32, Entity)> {
        let mut best: Option<(f64, i32, Entity)> = None;
        for (id, e) in &self.alive {
            if !types.contains(&e.etype) {
                continue;
            }
            let d = (e.x - x).powi(2) + (e.y - y).powi(2) + (e.z - z).powi(2);
            if d > max * max {
                continue;
            }
            if best.as_ref().map(|(bd, ..)| d < *bd).unwrap_or(true) {
                best = Some((d, *id, e.clone()));
            }
        }
        best.map(|(_, id, e)| (id, e))
    }
}
