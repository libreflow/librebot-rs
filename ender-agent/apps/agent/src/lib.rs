pub mod config;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::Context as _;
use ender_behavior::{Action as BtAction, BehaviorTree, Check, Selector, Sequence, Status as BtStatus};
use ender_evaluation::{DecisionLog, Metrics};
use ender_learning::{observe, Action as RlAction, Policy, Reward, Step, Trainer};
use ender_minecraft::{Client, GameEvent, MinecraftAdapter, SimAdapter};
#[cfg(feature = "live")]
use ender_minecraft::AzaleaAdapter;
use ender_perception::{BlockPos, Perception, Value, WorldState};
use ender_planner::{
    select_goal, ActionDefinition, Condition, Goal, GoapPlanner, ProgressionGraph, StateMap,
};
use ender_skills::{Skill, SkillContext, SkillLibrary, SkillParams, SkillStatus};
use ender_world_model::WorldModel;

pub use config::{Config, Mode};

pub fn default_actions() -> Vec<ActionDefinition> {
    vec![
        ActionDefinition::new("explore", 1.0).effect("explored", true),
        ActionDefinition::new("gather_log", 2.0)
            .pre(Condition::is_true("block_log_nearby"))
            .effect("has_item:minecraft:oak_log", true),
        ActionDefinition::new("craft_planks", 1.5)
            .pre(Condition::is_true("has_item:minecraft:oak_log"))
            .effect("has_item:minecraft:oak_planks", true)
            .effect("has_item:minecraft:oak_log", false),
    ]
}

pub fn default_goals() -> Vec<Goal> {
    vec![
        Goal::new("survivre", 10.0, vec![Condition::at_least("health", 15.0)]),
        Goal::new(
            "obtenir_du_bois",
            5.0,
            vec![Condition::is_true("has_item:minecraft:oak_log")],
        ),
        Goal::new(
            "fabriquer_des_planks",
            4.0,
            vec![Condition::is_true("has_item:minecraft:oak_planks")],
        ),
        Goal::new("explorer", 1.0, vec![Condition::is_true("explored")]),
    ]
}

fn parse_pos(s: &str) -> Option<[i32; 3]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

pub fn skill_params_for(action: &str, smap: &StateMap, center: [i32; 3]) -> Option<SkillParams> {
    match action {
        "gather_log" => {
            let pos = smap
                .get("nearest_log_pos")
                .and_then(Value::as_str)
                .and_then(parse_pos)?;
            Some(SkillParams::Gather {
                pos,
                drop: "minecraft:oak_log".to_string(),
            })
        }
        "craft_planks" => Some(SkillParams::Craft {
            item: "minecraft:oak_planks".to_string(),
            count: 4,
        }),
        "explore" => Some(SkillParams::Explore { center, radius: 16 }),
        _ => None,
    }
}

fn enrich_state_map(smap: &mut StateMap, model: &WorldModel) {
    let chunks = model.map.explored_chunks();
    smap.insert("explored_chunks".to_string(), Value::from(chunks));
    smap.insert("explored".to_string(), Value::Bool(chunks > 0));
}

fn rl_action_to_params(action: RlAction, smap: &StateMap, center: [i32; 3]) -> SkillParams {
    match action {
        RlAction::GatherWood => match smap
            .get("nearest_log_pos")
            .and_then(Value::as_str)
            .and_then(parse_pos)
        {
            Some(pos) => SkillParams::Gather {
                pos,
                drop: "minecraft:oak_log".to_string(),
            },
            None => SkillParams::Explore { center, radius: 16 },
        },
        RlAction::CraftPlanks => {
            if smap
                .get("has_item:minecraft:oak_log")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                SkillParams::Craft {
                    item: "minecraft:oak_planks".to_string(),
                    count: 4,
                }
            } else {
                SkillParams::Explore { center, radius: 16 }
            }
        }
        RlAction::Flee => SkillParams::Flee,
        _ => SkillParams::Explore { center, radius: 16 },
    }
}

fn skill_name(params: &SkillParams) -> &'static str {
    match params {
        SkillParams::MoveTo { .. } => "move_to",
        SkillParams::Gather { .. } => "gather",
        SkillParams::EnsureItem { .. } => "ensure_item",
        SkillParams::Craft { .. } => "craft",
        SkillParams::Explore { .. } => "explore",
        SkillParams::Flee => "flee",
    }
}

pub struct AgentReport {
    pub metrics: HashMap<&'static str, u64>,
    pub decisions: String,
    pub final_state: WorldState,
    pub episodes: u64,
    pub buffer_len: usize,
}

#[cfg(feature = "live")]
fn build_live(config: &Config) -> anyhow::Result<Arc<dyn MinecraftAdapter>> {
    Ok(Arc::new(AzaleaAdapter::new(&config.username, &config.server)))
}

#[cfg(not(feature = "live"))]
fn build_live(_config: &Config) -> anyhow::Result<Arc<dyn MinecraftAdapter>> {
    anyhow::bail!("mode live : compiler avec --features live (azalea =0.10.3+mc1.21.1, toolchain nightly)")
}

pub async fn run(config: Config) -> anyhow::Result<AgentReport> {
    let adapter: Arc<dyn MinecraftAdapter> = match config.mode {
        Mode::Sim => Arc::new(SimAdapter::new()),
        Mode::Live => build_live(&config)?,
    };
    let client = Client::new(Arc::clone(&adapter));
    client.connect().context("connexion au monde")?;
    tracing::info!(mode = ?config.mode, "agent demarre");

    let world_model = if config.db_path.is_empty() {
        WorldModel::new()
    } else {
        WorldModel::with_sqlite(&config.db_path).context("ouverture sqlite")?
    };
    let perception = Arc::new(RwLock::new(Perception::new()));
    let model = Arc::new(Mutex::new(world_model));
    let active_skill: Arc<Mutex<Option<Box<dyn Skill>>>> = Arc::new(Mutex::new(None));
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    let log = Arc::new(Mutex::new(DecisionLog::new(500)));
    let trainer = Arc::new(Mutex::new(Trainer::new(10_000, 0.1)));
    let last_goal: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    {
        let mut rx = adapter.events();
        let perception = Arc::clone(&perception);
        let model = Arc::clone(&model);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let tick = perception.read().unwrap().state().tick;
                        match &ev {
                            GameEvent::BlockUpdate { pos, block } => {
                                perception.write().unwrap().apply_event(&ev);
                                model
                                    .lock()
                                    .unwrap()
                                    .map
                                    .observe(BlockPos::from_array(*pos), block, tick, 0.9);
                            }
                            GameEvent::Damage { .. } | GameEvent::Death { .. } => {
                                perception.write().unwrap().apply_event(&ev);
                                model
                                    .lock()
                                    .unwrap()
                                    .record(tick, "danger", &format!("{ev:?}"), 1.0);
                            }
                            GameEvent::Disconnect { .. } => {
                                perception.write().unwrap().apply_event(&ev);
                                model.lock().unwrap().record(tick, "session", "deconnexion", 1.0);
                                break;
                            }
                            _ => perception.write().unwrap().apply_event(&ev),
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    {
        let perception = Arc::clone(&perception);
        let model = Arc::clone(&model);
        let adapter = Arc::clone(&adapter);
        let interval = Duration::from_millis(config.sample_interval_ms);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                if let Ok(snap) = adapter.snapshot() {
                    let tick = snap.tick;
                    let blocks: Vec<(BlockPos, String)> = snap
                        .nearby_blocks
                        .iter()
                        .map(|(p, b)| (BlockPos::from_array(*p), b.clone()))
                        .collect();
                    perception.write().unwrap().apply_snapshot(&snap);
                    model.lock().unwrap().observe_blocks(&blocks, tick, 0.9);
                }
            }
        });
    }

    {
        let perception = Arc::clone(&perception);
        let model = Arc::clone(&model);
        let adapter = Arc::clone(&adapter);
        let active_skill = Arc::clone(&active_skill);
        let metrics = Arc::clone(&metrics);
        let log = Arc::clone(&log);
        let trainer = Arc::clone(&trainer);
        let last_goal = Arc::clone(&last_goal);
        let planner = GoapPlanner::new(default_actions());
        let goals = default_goals();
        let library = SkillLibrary::standard();
        let interval = Duration::from_millis(config.decision_interval_ms);
        tokio::spawn(async move {
            let mut graph = ProgressionGraph::new();
            graph.add_goal("survivre", 10.0);
            graph.add_goal("obtenir_du_bois", 5.0);
            graph.add_goal("fabriquer_des_planks", 4.0);
            graph.add_dependency("obtenir_du_bois", "fabriquer_des_planks");
            graph.add_goal("explorer", 1.0);
            let mut reactive = BehaviorTree::new(Box::new(Selector::new(vec![
                Box::new(Sequence::new(vec![
                    Box::new(Check::new(|bb| bb.get_bool("hostile_nearby"))),
                    Box::new(BtAction::new(|bb| {
                        bb.set("flee_requested", true);
                        BtStatus::Running
                    })),
                ])),
                Box::new(Sequence::new(vec![
                    Box::new(Check::new(|bb| {
                        bb.get_f64("health").map_or(false, |h| h < 8.0)
                    })),
                    Box::new(BtAction::new(|bb| {
                        bb.set("eat_requested", true);
                        BtStatus::Running
                    })),
                ])),
            ])));
            loop {
                tokio::time::sleep(interval).await;
                let state = perception.read().unwrap().state().clone();
                let mut smap = state.to_state_map();
                enrich_state_map(&mut smap, &model.lock().unwrap());
                log.lock().unwrap().observation(&format!(
                    "pos=({:.1},{:.1},{:.1}) vie={:.0} faim={:.0} blocs_vus={} entites={}",
                    state.position[0],
                    state.position[1],
                    state.position[2],
                    state.health,
                    state.hunger,
                    state.nearby_blocks.len(),
                    state.entities.len()
                ));

                reactive.blackboard.set("hostile_nearby", smap.get("hostile_nearby").cloned().unwrap_or(Value::Bool(false)));
                reactive.blackboard.set("health", Value::from(state.health));
                reactive.tick();
                if reactive.blackboard.get_bool("flee_requested") {
                    reactive.blackboard.set("flee_requested", false);
                    let mut guard = active_skill.lock().unwrap();
                    if let Some(skill) = guard.as_mut() {
                        skill.cancel();
                        log.lock().unwrap().execution(&format!("{} interrompu (hostile proche)", skill.name()));
                        metrics.lock().unwrap().inc("interruptions");
                    }
                    drop(guard);
                    let model_guard = model.lock().unwrap();
                    let ctx = SkillContext { state: &state, adapter: &*adapter, knowledge: &model_guard.knowledge };
                    if let Some(mut flee) = library.instantiate("flee", SkillParams::Flee) {
                        if flee.can_execute(&state) {
                            flee.start(&ctx);
                            log.lock().unwrap().goal("fuir un hostile");
                            *active_skill.lock().unwrap() = Some(flee);
                        }
                    }
                    drop(model_guard);
                    continue;
                }
                if reactive.blackboard.get_bool("eat_requested") {
                    reactive.blackboard.set("eat_requested", false);
                    log.lock().unwrap().goal("manger (vie faible)");
                }

                if active_skill.lock().unwrap().is_some() {
                    continue;
                }
                let avail: Vec<Goal> = graph.available(&goals, &smap).into_iter().cloned().collect();
                match select_goal(&avail, &smap) {
                    Some(goal) => {
                        log.lock().unwrap().goal(&format!("{} (priorite {:.1})", goal.name, goal.priority));
                        *last_goal.lock().unwrap() = Some(goal.name.clone());
                        match planner.plan(&smap, &goal.conditions) {
                            Some(plan) if !plan.is_empty() => {
                                metrics.lock().unwrap().inc("plans_generated");
                                log.lock().unwrap().plan(&format!(
                                    "{} (cout {:.1})",
                                    plan.actions.join(" -> "),
                                    plan.total_cost
                                ));
                                let first = plan.actions[0].clone();
                                let center = [
                                    state.position[0].floor() as i32,
                                    state.position[1].floor() as i32,
                                    state.position[2].floor() as i32,
                                ];
                                if let Some(params) = skill_params_for(&first, &smap, center) {
                                    let name = skill_name(&params);
                                    let model_guard = model.lock().unwrap();
                                    let ctx = SkillContext { state: &state, adapter: &*adapter, knowledge: &model_guard.knowledge };
                                    if let Some(mut skill) = library.instantiate(name, params) {
                                        if skill.can_execute(&state) {
                                            skill.start(&ctx);
                                            log.lock().unwrap().execution(&format!("skill {} demarre", skill.name()));
                                            *active_skill.lock().unwrap() = Some(skill);
                                        }
                                    }
                                    drop(model_guard);
                                }
                            }
                            Some(_) => {
                                log.lock().unwrap().plan("objectif deja satisfait");
                            }
                            None => {
                                metrics.lock().unwrap().inc("plans_impossible");
                                let rl_action = {
                                    let mut t = trainer.lock().unwrap();
                                    t.policy.select(observe(&state).key())
                                };
                                log.lock().unwrap().plan(&format!(
                                    "aucun plan executable, fallback politique RL ({rl_action:?})"
                                ));
                                let center = [
                                    state.position[0].floor() as i32,
                                    state.position[1].floor() as i32,
                                    state.position[2].floor() as i32,
                                ];
                                let params = rl_action_to_params(rl_action, &smap, center);
                                let name = skill_name(&params);
                                let model_guard = model.lock().unwrap();
                                let ctx = SkillContext { state: &state, adapter: &*adapter, knowledge: &model_guard.knowledge };
                                if let Some(mut skill) = library.instantiate(name, params) {
                                    skill.start(&ctx);
                                    *active_skill.lock().unwrap() = Some(skill);
                                }
                                drop(model_guard);
                            }
                        }
                    }
                    None => {
                        log.lock().unwrap().goal("tous les objectifs satisfaits");
                    }
                }
            }
        });
    }

    {
        let perception = Arc::clone(&perception);
        let model = Arc::clone(&model);
        let adapter = Arc::clone(&adapter);
        let active_skill = Arc::clone(&active_skill);
        let metrics = Arc::clone(&metrics);
        let log = Arc::clone(&log);
        let trainer = Arc::clone(&trainer);
        let last_goal = Arc::clone(&last_goal);
        let goals = default_goals();
        let interval = Duration::from_millis(config.skill_tick_ms);
        tokio::spawn(async move {
            let mut completions = 0u64;
            loop {
                tokio::time::sleep(interval).await;
                let state = perception.read().unwrap().state().clone();
                let mut guard = active_skill.lock().unwrap();
                if guard.is_none() {
                    continue;
                }
                let model_guard = model.lock().unwrap();
                let ctx = SkillContext {
                    state: &state,
                    adapter: &*adapter,
                    knowledge: &model_guard.knowledge,
                };
                let (status, name) = {
                    let skill = guard.as_mut().unwrap();
                    let status = skill.tick(&ctx);
                    (status, skill.name())
                };
                drop(model_guard);
                match status {
                    SkillStatus::Running => {}
                    SkillStatus::Success => {
                        log.lock().unwrap().execution(&format!("{name} -> SUCCESS (verifie dans l'etat reel)"));
                        metrics.lock().unwrap().inc("skills_success");
                        let goal_name = last_goal.lock().unwrap().clone();
                        if let Some(gn) = goal_name {
                            let mut smap2 = state.to_state_map();
                            enrich_state_map(&mut smap2, &model.lock().unwrap());
                            if let Some(g) = goals.iter().find(|g| g.name == gn) {
                                if !g.is_satisfied(&smap2) {
                                    metrics.lock().unwrap().inc("verify_mismatch");
                                    log.lock().unwrap().execution(&format!(
                                        "VERIFY: objectif '{gn}' non satisfait apres SUCCESS, replan necessaire"
                                    ));
                                }
                            }
                        }
                        let obs = observe(&state);
                        trainer.lock().unwrap().record(Step {
                            obs_key: obs.key(),
                            action: ender_learning::Action::Idle,
                            reward: Reward {
                                objective_progress: 1.0,
                                ..Reward::zero()
                            }
                            .total(),
                            next_obs_key: obs.key(),
                            done: false,
                        });
                        model.lock().unwrap().record(state.tick, "skill", &format!("{name} success"), 1.0);
                        *guard = None;
                        completions += 1;
                        if completions % 10 == 0 {
                            let loss = trainer.lock().unwrap().train(32);
                            log.lock().unwrap().learning(&format!("batch entraine, perte {loss:.4}"));
                        }
                    }
                    SkillStatus::Failed => {
                        log.lock().unwrap().execution(&format!("{name} -> FAILED"));
                        metrics.lock().unwrap().inc("skills_failed");
                        let obs = observe(&state);
                        trainer.lock().unwrap().record(Step {
                            obs_key: obs.key(),
                            action: ender_learning::Action::Idle,
                            reward: Reward {
                                task_failure: 1.0,
                                ..Reward::zero()
                            }
                            .total(),
                            next_obs_key: obs.key(),
                            done: false,
                        });
                        model.lock().unwrap().record(state.tick, "skill", &format!("{name} echec"), 0.8);
                        *guard = None;
                    }
                    SkillStatus::Interrupted => {
                        log.lock().unwrap().execution(&format!("{name} -> INTERRUPTED"));
                        *guard = None;
                    }
                }
            }
        });
    }

    if let Some(secs) = config.max_runtime_secs {
        tokio::time::sleep(Duration::from_secs(secs)).await;
    } else {
        tokio::signal::ctrl_c().await.context("ctrl-c")?;
    }
    adapter.disconnect();
    model.lock().unwrap().memory.close_episode();
    let final_state = perception.read().unwrap().state().clone();
    let report_metrics = metrics.lock().unwrap().snapshot();
    let decisions = log.lock().unwrap().render();
    let (episodes, buffer_len) = {
        let t = trainer.lock().unwrap();
        (t.episodes, t.buffer.len())
    };
    Ok(AgentReport {
        metrics: report_metrics,
        decisions,
        final_state,
        episodes,
        buffer_len,
    })
}
