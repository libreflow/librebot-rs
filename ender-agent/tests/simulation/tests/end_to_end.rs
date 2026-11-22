#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_recolte_du_bois_puis_craft_des_planks_en_autonomie() {
    let mut config = ender_agent::Config::from_env();
    config.mode = ender_agent::Mode::Sim;
    config.db_path = String::new();
    config.decision_interval_ms = 200;
    config.skill_tick_ms = 50;
    config.sample_interval_ms = 50;
    config.max_runtime_secs = Some(30);
    let report = ender_agent::run(config).await.expect("run sim");
    println!("{}", report.decisions);
    assert!(
        report.final_state.inventory.count("minecraft:oak_planks") >= 4,
        "l'agent devait fabriquer 4 planks, inventaire: {:?}",
        report.final_state.inventory.items()
    );
    assert!(report.metrics.get("plans_generated").copied().unwrap_or(0) >= 1);
    assert!(report.metrics.get("skills_success").copied().unwrap_or(0) >= 2);
    assert_eq!(
        report.metrics.get("verify_mismatch").copied().unwrap_or(0),
        0,
        "chaque SUCCESS doit verifier l'objectif dans l'etat reel"
    );
    assert!(report.buffer_len >= 1, "des experiences doivent etre enregistrees");
}
