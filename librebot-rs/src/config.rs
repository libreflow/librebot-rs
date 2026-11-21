//! Configuration : config.json à côté de l'exécutable, surcharges CLI.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub reconnect_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 25565,
            username: "LibreBot".into(),
            reconnect_secs: 30,
        }
    }
}

impl Config {
    /// Charge `config.json` si présent (host, port, username, reconnectSecs),
    /// puis applique les surcharges `--host`, `--port`, `--username`.
    pub fn load(path: &Path, host: Option<&str>, port: Option<u16>, username: Option<&str>) -> Self {
        let mut c = Config::default();
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Some(v) = json_field(&raw, "host") {
                c.host = v;
            }
            if let Some(v) = json_field(&raw, "port") {
                if let Ok(p) = v.parse() {
                    c.port = p;
                }
            }
            if let Some(v) = json_field(&raw, "username") {
                c.username = v;
            }
            if let Some(v) = json_field(&raw, "reconnectSecs") {
                if let Ok(p) = v.parse() {
                    c.reconnect_secs = p;
                }
            }
        }
        if let Some(h) = host {
            c.host = h.into();
        }
        if let Some(p) = port {
            c.port = p;
        }
        if let Some(u) = username {
            c.username = u.into();
        }
        c
    }
}

/// Extraction naïve d'un champ string/number de premier niveau (sans dépendance serde).
fn json_field(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let i = raw.find(&pat)? + pat.len();
    let rest = raw[i..].trim_start().strip_prefix(':')?.trim_start();
    if let Some(rest) = rest.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(rest.len());
        let v = rest[..end].trim();
        (!v.is_empty()).then(|| v.to_string())
    }
}
