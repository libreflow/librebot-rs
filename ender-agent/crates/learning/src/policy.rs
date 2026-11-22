use std::collections::HashMap;

use crate::environment::{Action, Step, ALL_ACTIONS};

pub trait Policy: Send {
    fn select(&mut self, obs_key: u64) -> Action;
    fn update(&mut self, step: &Step);
}

pub struct TabularPolicy {
    pub q: HashMap<(u64, Action), f64>,
    pub alpha: f64,
    pub gamma: f64,
    pub epsilon: f64,
    rng_state: u64,
}

impl TabularPolicy {
    pub fn new(epsilon: f64) -> Self {
        Self {
            q: HashMap::new(),
            alpha: 0.1,
            gamma: 0.9,
            epsilon,
            rng_state: 0x9e3779b97f4a7c15,
        }
    }

    pub fn rand_f64(&mut self) -> f64 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng_state >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn q_value(&self, obs_key: u64, action: Action) -> f64 {
        self.q.get(&(obs_key, action)).copied().unwrap_or(0.0)
    }

    pub fn best_action(&self, obs_key: u64) -> Action {
        let mut best = ALL_ACTIONS[0];
        let mut best_v = f64::NEG_INFINITY;
        for a in ALL_ACTIONS {
            let v = self.q_value(obs_key, *a);
            if v > best_v {
                best_v = v;
                best = *a;
            }
        }
        best
    }
}

impl Policy for TabularPolicy {
    fn select(&mut self, obs_key: u64) -> Action {
        if self.rand_f64() < self.epsilon {
            let idx = (self.rand_f64() * ALL_ACTIONS.len() as f64) as usize;
            return ALL_ACTIONS[idx.min(ALL_ACTIONS.len() - 1)];
        }
        let best = self.best_action(obs_key);
        best
    }

    fn update(&mut self, step: &Step) {
        let q_sa = self.q_value(step.obs_key, step.action);
        let max_next = ALL_ACTIONS
            .iter()
            .map(|a| self.q_value(step.next_obs_key, *a))
            .fold(0.0_f64, f64::max);
        let target = if step.done {
            step.reward as f64
        } else {
            step.reward as f64 + self.gamma * max_next
        };
        self.q.insert(
            (step.obs_key, step.action),
            q_sa + self.alpha * (target - q_sa),
        );
    }
}
