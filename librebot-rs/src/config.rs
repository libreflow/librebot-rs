//! Configuration : config.json (serde), surcharges CLI. Zéro LLM, zéro token :
//! tout le comportement du bot est régi par ce fichier + progress.json.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default = "def_reconnect")]
    pub reconnect_secs: u64,
    #[serde(default = "def_view")]
    pub view_distance: u8,
    /// Dispersion aléatoire du démarrage (ms) pour une flotte de bots.
    #[serde(default)]
    pub start_jitter_secs: u64,
    /// Mode d'authentification : uniquement "offline" (cracked) est supporté.
    #[serde(default = "def_auth")]
    pub auth: String,
    /// Version visée. None/"auto" = détection par ping de statut.
    /// Seul le protocole 767 (1.21.1) est réellement implémenté ; toute autre
    /// version détectée produit un message explicite.
    #[serde(default)]
    pub version: Option<String>,
    /// IP d'un monde LAN ouvert sur un autre PC (ex: "192.168.1.42").
    #[serde(default)]
    pub lan_host: Option<String>,
    /// Chemin du log .minecraft pour la découverte auto du port LAN.
    #[serde(default = "def_minecraft_log")]
    pub minecraft_log: String,
    /// Mode rush : cibles minimales (fer 12, diamants 5, obsidienne 10...).
    #[serde(default)]
    pub rush: bool,
}

fn def_reconnect() -> u64 { 45 }
fn def_view() -> u8 { 8 }
fn def_auth() -> String { "offline".into() }
fn def_minecraft_log() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{}/.minecraft/logs/latest.log", home)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 25565,
            username: "LibreBot".into(),
            reconnect_secs: def_reconnect(),
            view_distance: def_view(),
            start_jitter_secs: 0,
            auth: def_auth(),
            version: None,
            lan_host: None,
            minecraft_log: def_minecraft_log(),
            rush: false,
        }
    }
}

impl Config {
    /// Charge config.json (serde_json), puis applique les surcharges CLI.
    pub fn load(path: &Path, host: Option<&str>, port: Option<u16>, username: Option<&str>) -> Self {
        let mut c = Config::default();
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Ok(lu) = serde_json::from_str::<Config>(&raw) {
                c = lu;
            }
        }
        if let Some(h) = host { c.host = h.into(); }
        if let Some(p) = port { c.port = p; }
        if let Some(u) = username { c.username = u.into(); }
        c
    }

    pub fn save(&self, path: &Path) {
        let _ = std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

/// Extraction naÏve d'un champ pour les configs anciennes « à l'ancienne »
/// (garde-fou de compatibilité, utile avant migration).
#[allow(dead_code)]
fn json_field(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let i = raw.find(&pat)? + pat.len();
    let rest = raw[i..].trim_start().strip_prefix(':')?.trim_start();
    if let Some(rest) = rest.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        let end = rest
            .find([',', '}'])
            .unwrap_or(rest.len());
        let v = rest[..end].trim();
        (!v.is_empty()).then(|| v.to_string())
    }
}
