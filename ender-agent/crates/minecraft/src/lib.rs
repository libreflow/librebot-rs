pub mod adapter;
pub mod client;
pub mod events;

pub use adapter::{AdapterError, MinecraftAdapter, PlayerSnapshot, SimAdapter};
#[cfg(feature = "live")]
pub use adapter::AzaleaAdapter;
pub use client::Client;
pub use events::{EntityInfo, GameEvent};

pub const VANILLA_RECIPES: &[(&str, u32, &[(&str, u32)])] = &[
    ("minecraft:oak_planks", 4, &[("minecraft:oak_log", 1)]),
    ("minecraft:stick", 4, &[("minecraft:oak_planks", 2)]),
    ("minecraft:crafting_table", 1, &[("minecraft:oak_planks", 4)]),
    (
        "minecraft:wooden_pickaxe",
        1,
        &[("minecraft:oak_planks", 3), ("minecraft:stick", 2)],
    ),
];

pub fn normalize_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find("minecraft:") {
        let rest = &trimmed[start..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '.' || c == '/'))
            .unwrap_or(rest.len());
        return rest[..end].to_string();
    }
    if let Some(start) = trimmed.find('"') {
        if let Some(end) = trimmed[start + 1..].find('"') {
            return trimmed[start + 1..start + 1 + end].to_string();
        }
    }
    trimmed.to_string()
}
