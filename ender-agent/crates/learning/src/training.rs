use std::collections::VecDeque;

use crate::environment::{Environment, Step, ALL_ACTIONS};
use crate::policy::{Policy, TabularPolicy};

pub struct ReplayBuffer {
    buf: VecDeque<Step>,
    pub capacity: usize,
    rng_state: u64,
}

impl ReplayBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity.min(1_000_000)),
            capacity,
            rng_state: 0xdeadbeefcafe1234,
        }
    }

    pub fn push(&mut self, step: Step) {
        if self.buf.len() >= self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(step);
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn sample(&mut self, n: usize) -> Vec<Step> {
        let len = self.buf.len();
        if len == 0 {
            return Vec::new();
        }
        let n = n.min(len);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            self.rng_state = self
                .rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let idx = (self.rng_state as usize) % len;
            if let Some(s) = self.buf.get(idx) {
                out.push(s.clone());
            }
        }
        out
    }
}

pub struct EpisodeStats {
    pub steps: usize,
    pub total_reward: f32,
}

pub struct Trainer {
    pub policy: TabularPolicy,
    pub buffer: ReplayBuffer,
    pub episodes: u64,
    pub steps: u64,
    pub last_loss: f32,
}

impl Trainer {
    pub fn new(buffer_capacity: usize, epsilon: f64) -> Self {
        Self {
            policy: TabularPolicy::new(epsilon),
            buffer: ReplayBuffer::new(buffer_capacity),
            episodes: 0,
            steps: 0,
            last_loss: 0.0,
        }
    }

    pub fn record(&mut self, step: Step) {
        self.buffer.push(step);
        self.steps += 1;
    }

    pub fn train(&mut self, batch_size: usize) -> f32 {
        let batch = self.buffer.sample(batch_size);
        if batch.is_empty() {
            return 0.0;
        }
        let mut loss = 0.0_f64;
        for step in &batch {
            let q_before = self.policy.q_value(step.obs_key, step.action);
            let max_next = ALL_ACTIONS
                .iter()
                .map(|a| self.policy.q_value(step.next_obs_key, *a))
                .fold(0.0_f64, f64::max);
            let target = if step.done {
                step.reward as f64
            } else {
                step.reward as f64 + self.policy.gamma * max_next
            };
            loss += (target - q_before).abs();
            self.policy.update(step);
        }
        let mean = (loss / batch.len() as f64) as f32;
        self.last_loss = mean;
        mean
    }

    pub fn run_episode<E: Environment>(&mut self, env: &mut E, max_steps: usize) -> EpisodeStats {
        let mut obs = env.reset();
        let mut total = 0.0_f32;
        let mut steps = 0_usize;
        for _ in 0..max_steps {
            let action = self.policy.select(obs);
            let (next, reward, done) = env.step(action);
            total += reward;
            steps += 1;
            self.record(Step {
                obs_key: obs,
                action,
                reward,
                next_obs_key: next,
                done,
            });
            obs = next;
            if done {
                break;
            }
        }
        self.episodes += 1;
        EpisodeStats {
            steps,
            total_reward: total,
        }
    }
}
