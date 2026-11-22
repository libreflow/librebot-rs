use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InventorySnapshot {
    items: Vec<(String, u32)>,
}

impl InventorySnapshot {
    pub fn from_items(items: Vec<(String, u32)>) -> Self {
        Self { items }
    }

    pub fn count(&self, item: &str) -> u32 {
        self.items
            .iter()
            .find(|(i, _)| i == item)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    pub fn has(&self, item: &str, n: u32) -> bool {
        self.count(item) >= n
    }

    pub fn items(&self) -> &[(String, u32)] {
        &self.items
    }

    pub fn set(&mut self, items: Vec<(String, u32)>) {
        self.items = items;
    }
}
