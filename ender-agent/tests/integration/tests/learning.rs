use ender_learning::{Action, Environment, GridWorld, Policy, Step, TabularPolicy, Trainer};

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

#[test]
fn gridworld_apprend_a_recolter_au_bon_endroit() {
    let mut env = GridWorld::new();
    let mut trainer = Trainer::new(100_000, 0.5);
    for _ in 0..800 {
        trainer.run_episode(&mut env, 64);
        trainer.train(64);
    }
    trainer.policy.epsilon = 0.0;
    let mut probe = GridWorld::new();
    probe.x = probe.log.0;
    probe.y = probe.log.1;
    let key = probe.obs_key();
    assert_eq!(
        trainer.policy.best_action(key),
        Action::GatherWood,
        "la politique doit apprendre a recolter sur la case du log"
    );
}
