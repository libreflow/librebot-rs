#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ender_agent::Config::from_env();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("ender_agent={},info", config.log_level).into()),
        )
        .init();
    if config.mode == ender_agent::Mode::Live && !ender_agent::Config::live_available() {
        anyhow::bail!("ENDER_MODE=live requiert la feature 'live' : cargo run -p ender-agent --features live");
    }
    let report = ender_agent::run(config).await?;
    println!("{}", report.decisions);
    println!("metriques: {:?}", report.metrics);
    println!(
        "etat final: pos=({:.1},{:.1},{:.1}) vie={:.0} inventaire={:?}",
        report.final_state.position[0],
        report.final_state.position[1],
        report.final_state.position[2],
        report.final_state.health,
        report.final_state.inventory.items()
    );
    println!(
        "apprentissage: episodes={} buffer={}",
        report.episodes, report.buffer_len
    );
    Ok(())
}
