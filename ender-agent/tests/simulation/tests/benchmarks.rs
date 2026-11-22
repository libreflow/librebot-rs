use ender_evaluation::run_suite;
use ender_minecraft::{MinecraftAdapter, SimAdapter};
use ender_world_model::{Knowledge, WorldMap};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suite_de_benchmarks_sur_sim() {
    let adapter = SimAdapter::new();
    adapter.connect().unwrap();
    let knowledge = Knowledge::vanilla();
    let mut map = WorldMap::new();
    let results = run_suite(&adapter, &knowledge, &mut map).await;
    for r in &results {
        println!("benchmark {}: score={:.2} ({})", r.name, r.score, r.details);
    }
    let score = |name: &str| results.iter().find(|r| r.name == name).unwrap().score;
    assert!(score("navigation") >= 0.99, "navigation: {}", score("navigation"));
    assert!(score("planification") >= 0.66, "planification: {}", score("planification"));
    assert_eq!(score("mémoire"), 1.0);
    assert_eq!(score("adaptation"), 1.0, "la fuite doit reussir apres injection d'un zombie");
    assert!(score("exploration") >= 1.0);
    adapter.disconnect();
}
