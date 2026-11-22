use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{Graph, NodeIndex};

use crate::goap::{Condition, StateMap};

#[derive(Debug, Clone)]
pub struct Goal {
    pub name: String,
    pub conditions: Vec<Condition>,
    pub priority: f32,
}

impl Goal {
    pub fn new(name: &str, priority: f32, conditions: Vec<Condition>) -> Self {
        Self {
            name: name.to_string(),
            conditions,
            priority,
        }
    }

    pub fn is_satisfied(&self, state: &StateMap) -> bool {
        self.conditions.iter().all(|c| c.check(state))
    }

    pub fn satisfied_ratio(&self, state: &StateMap) -> f32 {
        if self.conditions.is_empty() {
            return 1.0;
        }
        let ok = self.conditions.iter().filter(|c| c.check(state)).count() as f32;
        ok / self.conditions.len() as f32
    }
}

pub struct ProgressionGraph {
    graph: Graph<String, ()>,
    index: HashMap<String, NodeIndex>,
    priorities: HashMap<String, f32>,
}

impl Default for ProgressionGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressionGraph {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            index: HashMap::new(),
            priorities: HashMap::new(),
        }
    }

    pub fn add_goal(&mut self, name: &str, priority: f32) {
        if self.index.contains_key(name) {
            return;
        }
        let i = self.graph.add_node(name.to_string());
        self.index.insert(name.to_string(), i);
        self.priorities.insert(name.to_string(), priority);
    }

    pub fn add_dependency(&mut self, from: &str, to: &str) {
        self.add_goal(from, self.priorities.get(from).copied().unwrap_or(1.0));
        self.add_goal(to, self.priorities.get(to).copied().unwrap_or(1.0));
        let a = self.index[from];
        let b = self.index[to];
        self.graph.add_edge(a, b, ());
    }

    pub fn priority(&self, name: &str) -> f32 {
        self.priorities.get(name).copied().unwrap_or(1.0)
    }

    pub fn order(&self) -> Option<Vec<String>> {
        toposort(&self.graph, None)
            .ok()
            .map(|nodes| nodes.into_iter().map(|n| self.graph[n].clone()).collect())
    }

    pub fn dependencies_of(&self, name: &str) -> Vec<String> {
        match self.index.get(name) {
            Some(&i) => self
                .graph
                .neighbors_directed(i, petgraph::Direction::Incoming)
                .map(|n| self.graph[n].clone())
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn available<'a>(&self, goals: &'a [Goal], state: &StateMap) -> Vec<&'a Goal> {
        goals
            .iter()
            .filter(|g| !g.is_satisfied(state))
            .filter(|g| {
                self.dependencies_of(&g.name)
                    .iter()
                    .all(|dep| goals.iter().find(|x| &x.name == dep).map_or(true, |d| d.is_satisfied(state)))
            })
            .collect()
    }
}
