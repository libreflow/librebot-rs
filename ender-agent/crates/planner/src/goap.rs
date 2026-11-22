use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use ender_perception::Value;

pub type StateMap = HashMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Equals { key: String, value: Value },
    AtLeast { key: String, value: f64 },
    AtMost { key: String, value: f64 },
    IsTrue { key: String },
    IsFalse { key: String },
}

impl Condition {
    pub fn equals(key: &str, value: impl Into<Value>) -> Self {
        Condition::Equals {
            key: key.to_string(),
            value: value.into(),
        }
    }

    pub fn at_least(key: &str, value: f64) -> Self {
        Condition::AtLeast {
            key: key.to_string(),
            value,
        }
    }

    pub fn at_most(key: &str, value: f64) -> Self {
        Condition::AtMost {
            key: key.to_string(),
            value,
        }
    }

    pub fn is_true(key: &str) -> Self {
        Condition::IsTrue {
            key: key.to_string(),
        }
    }

    pub fn is_false(key: &str) -> Self {
        Condition::IsFalse {
            key: key.to_string(),
        }
    }

    pub fn check(&self, state: &StateMap) -> bool {
        match self {
            Condition::Equals { key, value } => state.get(key) == Some(value),
            Condition::AtLeast { key, value } => state
                .get(key)
                .and_then(Value::as_f64)
                .map_or(false, |v| v >= *value),
            Condition::AtMost { key, value } => state
                .get(key)
                .and_then(Value::as_f64)
                .map_or(false, |v| v <= *value),
            Condition::IsTrue { key } => state.get(key).and_then(Value::as_bool).unwrap_or(false),
            Condition::IsFalse { key } => !state.get(key).and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub key: String,
    pub value: Value,
}

impl Effect {
    pub fn new(key: &str, value: impl Into<Value>) -> Self {
        Effect {
            key: key.to_string(),
            value: value.into(),
        }
    }

    pub fn apply(&self, state: &mut StateMap) {
        state.insert(self.key.clone(), self.value.clone());
    }
}

#[derive(Debug, Clone)]
pub struct ActionDefinition {
    pub name: String,
    pub preconditions: Vec<Condition>,
    pub effects: Vec<Effect>,
    pub cost: f32,
}

impl ActionDefinition {
    pub fn new(name: &str, cost: f32) -> Self {
        Self {
            name: name.to_string(),
            preconditions: Vec::new(),
            effects: Vec::new(),
            cost,
        }
    }

    pub fn pre(mut self, condition: Condition) -> Self {
        self.preconditions.push(condition);
        self
    }

    pub fn effect(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.effects.push(Effect::new(key, value));
        self
    }

    pub fn satisfied_by(&self, state: &StateMap) -> bool {
        self.preconditions.iter().all(|c| c.check(state))
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub actions: Vec<String>,
    pub total_cost: f32,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

pub fn state_key(state: &StateMap) -> String {
    let mut entries: Vec<(&String, &Value)> = state.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .iter()
        .map(|(k, v)| format!("{k}={}", v.key()))
        .collect::<Vec<_>>()
        .join(";")
}

struct Node {
    state: StateMap,
    g: f32,
    h: f32,
    actions: Vec<String>,
}

impl Node {
    fn f(&self) -> f32 {
        self.g + self.h
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.f().to_bits() == other.f().to_bits()
    }
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f()
            .to_bits()
            .cmp(&self.f().to_bits())
            .then_with(|| other.g.to_bits().cmp(&self.g.to_bits()))
    }
}

pub struct GoapPlanner {
    pub actions: Vec<ActionDefinition>,
    pub max_iterations: usize,
}

impl GoapPlanner {
    pub fn new(actions: Vec<ActionDefinition>) -> Self {
        Self {
            actions,
            max_iterations: 10_000,
        }
    }

    pub fn plan(&self, start: &StateMap, goal: &[Condition]) -> Option<Plan> {
        if goal.iter().all(|c| c.check(start)) {
            return Some(Plan {
                actions: Vec::new(),
                total_cost: 0.0,
            });
        }
        let heuristic = |state: &StateMap| {
            goal.iter().filter(|c| !c.check(state)).count() as f32
        };
        let mut open = BinaryHeap::new();
        let mut closed: HashSet<String> = HashSet::new();
        open.push(Node {
            state: start.clone(),
            g: 0.0,
            h: heuristic(start),
            actions: Vec::new(),
        });
        let mut iterations = 0usize;
        while let Some(node) = open.pop() {
            iterations += 1;
            if iterations > self.max_iterations {
                return None;
            }
            let key = state_key(&node.state);
            if !closed.insert(key) {
                continue;
            }
            if goal.iter().all(|c| c.check(&node.state)) {
                return Some(Plan {
                    actions: node.actions,
                    total_cost: node.g,
                });
            }
            for action in &self.actions {
                if !action.satisfied_by(&node.state) {
                    continue;
                }
                let mut next = node.state.clone();
                for effect in &action.effects {
                    effect.apply(&mut next);
                }
                let next_key = state_key(&next);
                if closed.contains(&next_key) {
                    continue;
                }
                let mut actions = node.actions.clone();
                actions.push(action.name.clone());
                open.push(Node {
                    g: node.g + action.cost,
                    h: heuristic(&next),
                    state: next,
                    actions,
                });
            }
        }
        None
    }
}
