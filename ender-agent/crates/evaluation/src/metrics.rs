use std::collections::{HashMap, VecDeque};

pub struct Metrics {
    counters: HashMap<&'static str, u64>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
        }
    }

    pub fn inc(&mut self, key: &'static str) {
        self.add(key, 1);
    }

    pub fn add(&mut self, key: &'static str, n: u64) {
        *self.counters.entry(key).or_insert(0) += n;
    }

    pub fn get(&self, key: &'static str) -> u64 {
        self.counters.get(key).copied().unwrap_or(0)
    }

    pub fn rate(&self, success: &'static str, total: &'static str) -> f64 {
        let t = self.get(total);
        if t == 0 {
            0.0
        } else {
            self.get(success) as f64 / t as f64
        }
    }

    pub fn snapshot(&self) -> HashMap<&'static str, u64> {
        self.counters.clone()
    }
}

pub struct DecisionLog {
    entries: VecDeque<String>,
    capacity: usize,
}

impl DecisionLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, line: String) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(line);
    }

    pub fn observation(&mut self, text: &str) {
        self.push(format!("[OBSERVATION] {text}"));
    }

    pub fn goal(&mut self, text: &str) {
        self.push(format!("[OBJECTIF] {text}"));
    }

    pub fn plan(&mut self, text: &str) {
        self.push(format!("[PLAN] {text}"));
    }

    pub fn execution(&mut self, text: &str) {
        self.push(format!("[EXECUTION] {text}"));
    }

    pub fn learning(&mut self, text: &str) {
        self.push(format!("[APPRENTISSAGE] {text}"));
    }

    pub fn render(&self) -> String {
        self.entries
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
