use ender_perception::BlockPos;
use ender_world_model::{InMemoryMemory, MemoryStore, SqliteMemory, WorldMap};

#[test]
fn confiance_decroit_avec_le_temps() {
    let mut map = WorldMap::new();
    let pos = BlockPos::new(1, 64, 1);
    map.observe(pos, "minecraft:oak_log", 0, 1.0);
    assert!((map.confidence_at(&pos, 0) - 1.0).abs() < 1e-6);
    let c = map.confidence_at(&pos, 1000);
    assert!((c - 0.5).abs() < 1e-6, "confiance attendue 0.5, obtenu {c}");
    assert!(map.is_stale(&pos, 1000, 500));
    assert!(!map.is_stale(&pos, 100, 500));
}

#[test]
fn find_nearest_ignore_les_observations_perimees() {
    let mut map = WorldMap::new();
    let center = BlockPos::new(0, 64, 0);
    map.observe(BlockPos::new(2, 64, 0), "minecraft:oak_log", 0, 1.0);
    map.observe(BlockPos::new(50, 64, 0), "minecraft:oak_log", 5000, 0.01);
    let found = map.find_nearest(center, |b| b.contains("log"), 500, 1000);
    assert_eq!(found.map(|(p, _, _)| p), Some(BlockPos::new(2, 64, 0)));
}

#[test]
fn memoire_in_memory_filtre_par_requete_et_confiance() {
    let mut mem = InMemoryMemory::new();
    mem.append(&ender_world_model::MemoryEvent {
        tick: 1,
        wall_ms: 0,
        kind: "observation".to_string(),
        detail: "log vu en 2,64,0".to_string(),
        confidence: 0.9,
    });
    mem.append(&ender_world_model::MemoryEvent {
        tick: 2,
        wall_ms: 0,
        kind: "action".to_string(),
        detail: "craft planks".to_string(),
        confidence: 0.9,
    });
    mem.append(&ender_world_model::MemoryEvent {
        tick: 3,
        wall_ms: 0,
        kind: "observation".to_string(),
        detail: "log ancien oublie".to_string(),
        confidence: 0.0,
    });
    let res = mem.recall("log", 10);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].tick, 1);
}

#[test]
fn sqlite_roundtrip() {
    let mut mem = SqliteMemory::in_memory().unwrap();
    for i in 0..10u64 {
        mem.append(&ender_world_model::MemoryEvent {
            tick: i,
            wall_ms: i as u128 * 1000,
            kind: "observation".to_string(),
            detail: format!("bloc log numero {i}"),
            confidence: 0.95,
        });
    }
    let res = mem.recall("log", 5);
    assert_eq!(res.len(), 5);
    assert_eq!(res[0].tick, 9);
    assert_eq!(mem.all(100).len(), 10);
}
