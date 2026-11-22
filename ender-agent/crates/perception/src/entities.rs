use ender_minecraft::EntityInfo;
use serde::{Deserialize, Serialize};

pub const HOSTILE_KINDS: &[&str] = &[
    "zombie",
    "skeleton",
    "creeper",
    "spider",
    "enderman",
    "witch",
    "pillager",
    "vex",
    "hoglin",
    "blaze",
    "ghast",
    "magma_cube",
    "slime",
    "phantom",
    "warden",
    "stray",
    "drowned",
    "husk",
    "zombified_piglin",
    "ravager",
    "evoker",
    "vindicator",
];

pub fn is_hostile(kind: &str) -> bool {
    let k = kind.to_lowercase();
    HOSTILE_KINDS.iter().any(|h| k.contains(h))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub id: i64,
    pub kind: String,
    pub pos: [f64; 3],
    pub hostile: bool,
}

impl From<EntityInfo> for EntityView {
    fn from(e: EntityInfo) -> Self {
        Self {
            id: e.id,
            hostile: e.hostile || is_hostile(&e.kind),
            kind: e.kind,
            pos: e.pos,
        }
    }
}

pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

pub fn nearest_hostile(entities: &[EntityView], from: [f64; 3]) -> Option<(EntityView, f64)> {
    entities
        .iter()
        .filter(|e| e.hostile)
        .map(|e| (e.clone(), distance(from, e.pos)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

pub fn nearest_matching(
    entities: &[EntityView],
    predicate: impl Fn(&str) -> bool,
    from: [f64; 3],
) -> Option<(EntityView, f64)> {
    entities
        .iter()
        .filter(|e| predicate(&e.kind))
        .map(|e| (e.clone(), distance(from, e.pos)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}
