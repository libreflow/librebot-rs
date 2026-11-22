use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Sim,
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub mode: Mode,
    pub server: String,
    pub username: String,
    pub db_path: String,
    pub log_level: String,
    pub decision_interval_ms: u64,
    pub skill_tick_ms: u64,
    pub sample_interval_ms: u64,
    pub max_runtime_secs: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Config {
    pub fn from_env() -> Self {
        let mode = match std::env::var("ENDER_MODE").as_deref() {
            Ok("live") => Mode::Live,
            _ => Mode::Sim,
        };
        Self {
            mode,
            server: std::env::var("ENDER_SERVER").unwrap_or_else(|_| "127.0.0.1:25565".to_string()),
            username: std::env::var("ENDER_USERNAME").unwrap_or_else(|_| "ender".to_string()),
            db_path: std::env::var("ENDER_DB").unwrap_or_else(|_| "ender-memory.db".to_string()),
            log_level: std::env::var("ENDER_LOG").unwrap_or_else(|_| "info".to_string()),
            decision_interval_ms: std::env::var("ENDER_DECISION_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
            skill_tick_ms: std::env::var("ENDER_TICK_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            sample_interval_ms: std::env::var("ENDER_SAMPLE_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            max_runtime_secs: std::env::var("ENDER_MAX_SECS")
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }

    pub fn live_available() -> bool {
        cfg!(feature = "live")
    }
}
