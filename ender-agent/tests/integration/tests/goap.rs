use ender_perception::Value;
use ender_planner::{ActionDefinition, Condition, GoapPlanner, StateMap};

fn sample_planner() -> GoapPlanner {
    GoapPlanner::new(vec![
        ActionDefinition::new("explore", 1.0).effect("explored", true),
        ActionDefinition::new("gather_log", 2.0)
            .pre(Condition::is_true("block_log_nearby"))
            .effect("has_item:minecraft:oak_log", true),
        ActionDefinition::new("craft_planks", 1.5)
            .pre(Condition::is_true("has_item:minecraft:oak_log"))
            .effect("has_item:minecraft:oak_planks", true)
            .effect("has_item:minecraft:oak_log", false),
    ])
}

#[test]
fn goap_trouve_la_sequence_gather_puis_craft() {
    let planner = sample_planner();
    let mut start: StateMap = StateMap::new();
    start.insert("block_log_nearby".to_string(), Value::Bool(true));
    let goal = vec![Condition::is_true("has_item:minecraft:oak_planks")];
    let plan = planner.plan(&start, &goal).expect("un plan doit exister");
    assert_eq!(plan.actions, vec!["gather_log", "craft_planks"]);
    assert!(plan.total_cost > 0.0);
}

#[test]
fn goap_plan_vide_si_objectif_deja_satisfait() {
    let planner = sample_planner();
    let mut start: StateMap = StateMap::new();
    start.insert(
        "has_item:minecraft:oak_planks".to_string(),
        Value::Bool(true),
    );
    let goal = vec![Condition::is_true("has_item:minecraft:oak_planks")];
    let plan = planner.plan(&start, &goal).unwrap();
    assert!(plan.is_empty());
}

#[test]
fn goap_retourne_none_si_plan_impossible() {
    let planner = sample_planner();
    let start: StateMap = StateMap::new();
    let goal = vec![Condition::is_true("has_item:minecraft:netherite_sword")];
    assert!(planner.plan(&start, &goal).is_none());
}

#[test]
fn goap_respecte_les_preconditions_numeriques() {
    let planner = GoapPlanner::new(vec![ActionDefinition::new("heal", 1.0)
        .pre(Condition::at_least("has_item:minecraft:cooked_beef", 1.0))
        .effect("health", 20.0)]);
    let mut start: StateMap = StateMap::new();
    start.insert("health".to_string(), Value::from(5.0_f32));
    let goal = vec![Condition::at_least("health", 18.0)];
    assert!(planner.plan(&start, &goal).is_none());
    start.insert("has_item:minecraft:cooked_beef".to_string(), Value::from(1_i64));
    let plan = planner.plan(&start, &goal).unwrap();
    assert_eq!(plan.actions, vec!["heal"]);
}
