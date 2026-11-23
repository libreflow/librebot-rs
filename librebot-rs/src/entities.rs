//! Suivi des entités du monde (protocole 767) : spawns, suppressions, déplacements.
//! Sert notamment à repérer les drops d'items (type `minecraft:item`) pour les ramasser.

use crate::protocol::Body;
use std::collections::HashMap;

/// Type d'entité `minecraft:item` dans le registre 1.21.1.
pub const ITEM_ENTITY_TYPE: i32 = 58;
/// Types d'entités du registre 1.21.1 (protocole 767) utiles au speedrun.
pub const BLAZE_ENTITY_TYPE: i32 = 8;
pub const GHAST_ENTITY_TYPE: i32 = 45;
pub const PIGLIN_ENTITY_TYPE: i32 = 78;
pub const PIGLIN_BRUTE_ENTITY_TYPE: i32 = 79;
pub const ENDER_DRAGON_ENTITY_TYPE: i32 = 31;
pub const EYE_OF_ENDER_ENTITY_TYPE: i32 = 39;

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: i32,
    pub kind: i32,
    pub pos: [f64; 3],
}

#[derive(Debug, Default)]
pub struct Entities {
    map: HashMap<i32, Entity>,
}

impl Entities {
    pub fn new() -> Self {
        Self::default()
    }

    /// Paquet `spawn_entity` (0x01) : id, uuid (16 o), type, x/y/z, angles, data, vélocité.
    pub fn spawn(&mut self, body: &[u8]) {
        let mut b = Body(body);
        let id = b.varint();
        b.take(16);
        let kind = b.varint();
        let x = b.f64();
        let y = b.f64();
        let z = b.f64();
        self.map.insert(id, Entity { id, kind, pos: [x, y, z] });
    }

    /// Paquet `remove_entities` (0x42) : count, puis les ids. Retourne les ids supprimés.
    pub fn remove(&mut self, body: &[u8]) -> Vec<i32> {
        let mut b = Body(body);
        let n = b.varint().max(0) as usize;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let id = b.varint();
            self.map.remove(&id);
            out.push(id);
        }
        out
    }

    /// Paquets `update_entity_position` (0x2e) / `update_entity_position_and_rotation`
    /// (0x2f) : id, deltas x/y/z en i16 (1/4096 de bloc), puis angles si `with_rot`.
    pub fn move_rel(&mut self, body: &[u8], with_rot: bool) {
        let mut b = Body(body);
        let id = b.varint();
        let dx = b.i16() as f64 / 4096.0;
        let dy = b.i16() as f64 / 4096.0;
        let dz = b.i16() as f64 / 4096.0;
        if with_rot {
            let _yaw = b.u8();
            let _pitch = b.u8();
        }
        if let Some(e) = self.map.get_mut(&id) {
            e.pos[0] += dx;
            e.pos[1] += dy;
            e.pos[2] += dz;
        }
    }

    /// Paquet `teleport_entity` (0x70) : id, x/y/z absolus, puis angles.
    pub fn teleport(&mut self, body: &[u8]) {
        let mut b = Body(body);
        let id = b.varint();
        let x = b.f64();
        let y = b.f64();
        let z = b.f64();
        match self.map.get_mut(&id) {
            Some(e) => e.pos = [x, y, z],
            None => {
                self.map.insert(id, Entity { id, kind: -1, pos: [x, y, z] });
            }
        }
    }

    pub fn contains(&self, id: i32) -> bool {
        self.map.contains_key(&id)
    }

    pub fn count(&self) -> usize {
        self.map.len()
    }

    /// Nombre d'entités de type `item` (drops) actuellement connues.
    pub fn count_items(&self) -> usize {
        self.map.values().filter(|e| e.kind == ITEM_ENTITY_TYPE).count()
    }

    /// Positions de toutes les entités `item` connues.
    pub fn item_positions(&self) -> Vec<(i32, [f64; 3])> {
        self.map
            .values()
            .filter(|e| e.kind == ITEM_ENTITY_TYPE)
            .map(|e| (e.id, e.pos))
            .collect()
    }

    /// Drop d'item le plus proche de `from`, dans un rayon donné.
    pub fn get(&self, id: i32) -> Option<&Entity> {
        self.map.get(&id)
    }

    /// Entité d'un type donné la plus proche de `from`, dans `radius`.
    pub fn nearest_kind(&self, kind: i32, from: [f64; 3], radius: f64) -> Option<(i32, [f64; 3])> {
        self.map
            .values()
            .filter(|e| e.kind == kind)
            .map(|e| {
                let d = ((e.pos[0] - from[0]).powi(2)
                    + (e.pos[1] - from[1]).powi(2)
                    + (e.pos[2] - from[2]).powi(2))
                .sqrt();
                (d, e)
            })
            .filter(|(d, _)| *d <= radius)
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, e)| (e.id, e.pos))
    }

    pub fn nearest_item(&self, from: [f64; 3], radius: f64) -> Option<(i32, [f64; 3])> {
        let mut best: Option<(i32, [f64; 3], f64)> = None;
        for e in self.map.values() {
            if e.kind != ITEM_ENTITY_TYPE {
                continue;
            }
            let d = ((e.pos[0] - from[0]).powi(2)
                + (e.pos[1] - from[1]).powi(2)
                + (e.pos[2] - from[2]).powi(2))
            .sqrt();
            if d <= radius && best.map(|(_, _, bd)| d < bd).unwrap_or(true) {
                best = Some((e.id, e.pos, d));
            }
        }
        best.map(|(id, pos, _)| (id, pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::write_varint;

    fn spawn_body(id: i32, kind: i32, x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut b = Vec::new();
        write_varint(id, &mut b);
        b.extend_from_slice(&[0u8; 16]);
        write_varint(kind, &mut b);
        b.extend_from_slice(&x.to_be_bytes());
        b.extend_from_slice(&y.to_be_bytes());
        b.extend_from_slice(&z.to_be_bytes());
        b
    }

    #[test]
    fn spawn_move_remove() {
        let mut e = Entities::new();
        e.spawn(&spawn_body(10, ITEM_ENTITY_TYPE, 1.0, 2.0, 3.0));
        assert_eq!(e.count_items(), 1);
        let mut mv = Vec::new();
        write_varint(10, &mut mv);
        mv.extend_from_slice(&4096i16.to_be_bytes()); // +1 bloc en x
        mv.extend_from_slice(&0i16.to_be_bytes());
        mv.extend_from_slice(&0i16.to_be_bytes());
        mv.push(1);
        e.move_rel(&mv, false);
        let (_, pos) = e.nearest_item([0.0, 0.0, 0.0], 10.0).unwrap();
        assert!((pos[0] - 2.0).abs() < 1e-9);
        let mut rm = Vec::new();
        write_varint(1, &mut rm);
        write_varint(10, &mut rm);
        assert_eq!(e.remove(&rm), vec![10]);
        assert_eq!(e.count_items(), 0);
    }

    #[test]
    fn nearest_kind_trouve_piglin() {
        let mut e = Entities::new();
        e.spawn(&spawn_body(20, PIGLIN_ENTITY_TYPE, 5.0, 1.0, 0.0));
        e.spawn(&spawn_body(21, BLAZE_ENTITY_TYPE, 2.0, 1.0, 0.0));
        let (id, pos) = e.nearest_kind(PIGLIN_ENTITY_TYPE, [0.0, 0.0, 0.0], 24.0).unwrap();
        assert_eq!(id, 20);
        assert!((pos[0] - 5.0).abs() < 1e-9);
        // hors rayon -> None
        assert!(e.nearest_kind(PIGLIN_ENTITY_TYPE, [0.0, 0.0, 0.0], 3.0).is_none());
        // blaze trouvé aussi
        assert_eq!(e.nearest_kind(BLAZE_ENTITY_TYPE, [0.0, 0.0, 0.0], 24.0).map(|(i, _)| i), Some(21));
        // get
        assert!(e.get(20).is_some());
        assert!(e.get(99).is_none());
    }
}
