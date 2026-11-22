use std::time::Duration;

use ender_minecraft::MinecraftAdapter;
use ender_perception::{BlockPos, Value, WorldState};
use ender_planner::{ActionDefinition, Condition, GoapPlanner, StateMap};
use ender_skills::{FleeSkill, MoveToSkill, Skill, SkillContext, SkillStatus};
use ender_world_model::Knowledge;

pub struct BenchmarkResult {
    pub name: &'static str,
    pub score: f64,
    pub details: String,
}

fn ctx<'a>(state: &'a WorldState, adapter: &'a dyn MinecraftAdapter, knowledge: &'a Knowledge) -> SkillContext<'a> {
    SkillContext {
        state,
        adapter,
        knowledge,
    }
}

async fn fresh_state(adapter: &dyn MinecraftAdapter) -> WorldState {
    let mut state = WorldState::empty();
    if let Ok(snap) = adapter.snapshot() {
        state.apply_snapshot(&snap);
    }
    state
}

pub async fn benchmark_navigation(
    adapter: &dyn MinecraftAdapter,
    knowledge: &Knowledge,
    targets: &[[i32; 3]],
) -> BenchmarkResult {
    let mut arrived = 0usize;
    for target in targets {
        let mut state = fresh_state(adapter).await;
        let mut skill = MoveToSkill::new(*target, 1.5).with_max_ticks(20 * 60);
        {
            let ctx = ctx(&state, adapter, knowledge);
            skill.start(&ctx);
        }
        let mut status = SkillStatus::Running;
        for _ in 0..20 * 60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(snap) = adapter.snapshot() {
                state.apply_snapshot(&snap);
            }
            let ctx = ctx(&state, adapter, knowledge);
            status = skill.tick(&ctx);
            if status != SkillStatus::Running {
                break;
            }
        }
        if status == SkillStatus::Success {
            arrived += 1;
        }
    }
    let score = if targets.is_empty() {
        0.0
    } else {
        arrived as f64 / targets.len() as f64
    };
    BenchmarkResult {
        name: "navigation",
        score,
        details: format!("{arrived}/{} destinations atteintes", targets.len()),
    }
}

pub async fn benchmark_planning() -> BenchmarkResult {
    let actions = vec![
        ActionDefinition::new("explore", 1.0).effect("explored", true),
        ActionDefinition::new("gather_log", 2.0)
            .pre(Condition::is_true("block_log_nearby"))
            .effect("has_item:minecraft:oak_log", true),
        ActionDefinition::new("craft_planks", 1.5)
            .pre(Condition::is_true("has_item:minecraft:oak_log"))
            .effect("has_item:minecraft:oak_planks", true)
            .effect("has_item:minecraft:oak_log", false),
    ];
    let planner = GoapPlanner::new(actions);
    let mut start: StateMap = StateMap::new();
    start.insert("block_log_nearby".to_string(), true.into());
    let goals = vec![
        vec![Condition::is_true("has_item:minecraft:oak_planks")],
        vec![Condition::is_true("explored")],
        vec![Condition::is_true("has_item:minecraft:stone_pickaxe")],
    ];
    let mut found = 0usize;
    let mut total_cost = 0.0_f32;
    for g in &goals {
        if let Some(plan) = planner.plan(&start, g) {
            found += 1;
            total_cost += plan.total_cost;
        }
    }
    let score = found as f64 / goals.len() as f64;
    BenchmarkResult {
        name: "planification",
        score,
        details: format!(
            "{found}/{} plans executables, cout total {total_cost:.1}",
            goals.len()
        ),
    }
}

pub async fn benchmark_memory() -> BenchmarkResult {
    let mut memory = ender_world_model::Memory::in_memory();
    for i in 0..100u64 {
        let kind = if i % 2 == 0 { "observation" } else { "action" };
        let detail = format!("event {i} log near {i}");
        memory.record(i, kind, &detail, 0.9);
    }
    for i in 100..120u64 {
        memory.record(i, "action", &format!("craft {i}"), 0.9);
    }
    let recalled = memory.recall("log", 50);
    let relevant = recalled.iter().filter(|e| e.detail.contains("log")).count();
    let precision = if recalled.is_empty() {
        0.0
    } else {
        relevant as f64 / recalled.len() as f64
    };
    BenchmarkResult {
        name: "mémoire",
        score: precision,
        details: format!("{} evenements retournes, precision {precision:.2}", recalled.len()),
    }
}

pub async fn benchmark_adaptation(
    adapter: &dyn MinecraftAdapter,
    knowledge: &Knowledge,
) -> BenchmarkResult {
    let mut state = fresh_state(adapter).await;
    let mut skill = MoveToSkill::new([30, 64, 30], 2.0).with_max_ticks(20 * 30);
    {
        let ctx = ctx(&state, adapter, knowledge);
        skill.start(&ctx);
    }
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(snap) = adapter.snapshot() {
            state.apply_snapshot(&snap);
        }
    }
    let zombie_pos = [
        state.position[0] + 3.0,
        state.position[1],
        state.position[2],
    ];
    adapter
        .debug_spawn_hostile("zombie", zombie_pos)
        .expect("le sim doit pouvoir spawner un hostile");
    tokio::time::sleep(Duration::from_millis(150)).await;
    if let Ok(snap) = adapter.snapshot() {
        state.apply_snapshot(&snap);
    }
    assert!(
        state
            .to_state_map()
            .get("hostile_nearby")
            .and_then(ender_perception::Value::as_bool)
            .unwrap_or(false),
        "l'injection d'hostile doit rendre hostile_nearby=true"
    );
    let mut flee = FleeSkill::new();
    {
        let ctx = ctx(&state, adapter, knowledge);
        flee.start(&ctx);
    }
    let mut recovered = false;
    for _ in 0..20 * 30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(snap) = adapter.snapshot() {
            state.apply_snapshot(&snap);
        }
        let ctx = ctx(&state, adapter, knowledge);
        match flee.tick(&ctx) {
            SkillStatus::Success => {
                recovered = true;
                break;
            }
            SkillStatus::Failed => break,
            _ => {}
        }
    }
    BenchmarkResult {
        name: "adaptation",
        score: if recovered { 1.0 } else { 0.0 },
        details: if recovered {
            "fuite reussie apres interruption".to_string()
        } else {
            "echec de la fuite".to_string()
        },
    }
}

pub async fn benchmark_exploration(
    adapter: &dyn MinecraftAdapter,
    knowledge: &Knowledge,
    map: &mut ender_world_model::WorldMap,
    forays: usize,
) -> BenchmarkResult {
    let before = map.explored_chunks();
    for _ in 0..forays {
        let mut state = fresh_state(adapter).await;
        let center = [
            state.position[0].floor() as i32,
            state.position[1].floor() as i32,
            state.position[2].floor() as i32,
        ];
        let mut skill = ender_skills::ExploreSkill::new(center, 12);
        {
            let ctx = ctx(&state, adapter, knowledge);
            skill.start(&ctx);
        }
        for _ in 0..20 * 40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(snap) = adapter.snapshot() {
                state.apply_snapshot(&snap);
                for (pos, block) in &snap.nearby_blocks {
                    map.observe(BlockPos::from_array(*pos), block, snap.tick, 0.8);
                }
            }
            let ctx = ctx(&state, adapter, knowledge);
            if skill.tick(&ctx) != SkillStatus::Running {
                break;
            }
        }
    }
    let after = map.explored_chunks();
    BenchmarkResult {
        name: "exploration",
        score: (after.saturating_sub(before)) as f64,
        details: format!("chunks explores: {before} -> {after}"),
    }
}

pub async fn run_suite(
    adapter: &dyn MinecraftAdapter,
    knowledge: &Knowledge,
    map: &mut ender_world_model::WorldMap,
) -> Vec<BenchmarkResult> {
    let nav = benchmark_navigation(adapter, knowledge, &[[10, 64, 0], [0, 64, 10], [-8, 64, -8]]).await;
    let plan = benchmark_planning().await;
    let mem = benchmark_memory().await;
    let adapt = benchmark_adaptation(adapter, knowledge).await;
    let explore = benchmark_exploration(adapter, knowledge, map, 2).await;
    vec![nav, plan, mem, adapt, explore]
}
