use ender_learning::{Action, Policy, Step, TabularPolicy};

#[test]
fn politique_tabulaires_apprend_l_action_recompensee() {
    let mut policy = TabularPolicy::new(0.0);
    for _ in 0..20 {
        policy.update(&Step {
            obs_key: 42,
            action: Action::CraftPlanks,
            reward: 1.0,
            next_obs_key: 42,
            done: false,
        });
        policy.update(&Step {
            obs_key: 42,
            action: Action::Idle,
            reward: 0.0,
            next_obs_key: 42,
            done: false,
        });
    }
    assert_eq!(policy.select(42), Action::CraftPlanks);
    assert!(policy.q_value(42, Action::CraftPlanks) > policy.q_value(42, Action::Idle));
}

#[test]
fn politique_epsilon_explore() {
    let mut policy = TabularPolicy::new(1.0);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..50 {
        seen.insert(policy.select(7));
    }
    assert!(seen.len() > 1, "epsilon=1 doit explorer plusieurs actions");
}
