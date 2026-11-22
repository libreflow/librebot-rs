use std::time::Duration;

use ender_minecraft::{MinecraftAdapter, SimAdapter};
use ender_perception::WorldState;
use ender_skills::{CraftSkill, MoveToSkill, Skill, SkillContext, SkillStatus};
use ender_world_model::Knowledge;

fn ctx<'a>(state: &'a WorldState, adapter: &'a dyn MinecraftAdapter, knowledge: &'a Knowledge) -> SkillContext<'a> {
    SkillContext {
        state,
        adapter,
        knowledge,
    }
}

fn state_of(adapter: &dyn MinecraftAdapter) -> WorldState {
    let mut s = WorldState::empty();
    if let Ok(snap) = adapter.snapshot() {
        s.apply_snapshot(&snap);
    }
    s
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_to_atteint_la_cible() {
    let adapter = SimAdapter::new();
    adapter.connect().unwrap();
    let knowledge = Knowledge::vanilla();
    let mut skill = MoveToSkill::new([6, 64, 0], 1.5).with_max_ticks(20 * 30);
    {
        let state = state_of(&adapter);
        let ctx = ctx(&state, &adapter, &knowledge);
        skill.start(&ctx);
    }
    let mut status = SkillStatus::Running;
    for _ in 0..20 * 30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = state_of(&adapter);
        let ctx = ctx(&state, &adapter, &knowledge);
        status = skill.tick(&ctx);
        if status != SkillStatus::Running {
            break;
        }
    }
    assert_eq!(status, SkillStatus::Success);
    let state = state_of(&adapter);
    let dx = state.position[0] - 6.5;
    let dz = state.position[2] - 0.5;
    assert!(dx * dx + dz * dz <= 1.5 * 1.5 + 0.5);
    adapter.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn craft_verifie_le_resultat_dans_lventaire_reel() {
    let adapter = SimAdapter::new();
    adapter.connect().unwrap();
    adapter.give("minecraft:oak_log", 1);
    let knowledge = Knowledge::vanilla();
    let mut skill = CraftSkill::new("minecraft:oak_planks".to_string(), 4);
    {
        let state = state_of(&adapter);
        let ctx = ctx(&state, &adapter, &knowledge);
        assert!(skill.can_execute(&state));
        skill.start(&ctx);
    }
    let mut status = SkillStatus::Running;
    for _ in 0..20 * 30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = state_of(&adapter);
        let ctx = ctx(&state, &adapter, &knowledge);
        status = skill.tick(&ctx);
        if status != SkillStatus::Running {
            break;
        }
    }
    assert_eq!(status, SkillStatus::Success);
    let state = state_of(&adapter);
    assert!(state.inventory.count("minecraft:oak_planks") >= 4);
    assert_eq!(state.inventory.count("minecraft:oak_log"), 0);
    adapter.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn craft_echoue_sans_materiaux() {
    let adapter = SimAdapter::new();
    adapter.connect().unwrap();
    let knowledge = Knowledge::vanilla();
    let mut skill = CraftSkill::new("minecraft:stone_pickaxe".to_string(), 1);
    let state = state_of(&adapter);
    assert!(skill.can_execute(&state));
    let ctx = ctx(&state, &adapter, &knowledge);
    skill.start(&ctx);
    let status = skill.tick(&ctx);
    assert_eq!(status, SkillStatus::Failed);
    adapter.disconnect();
}
