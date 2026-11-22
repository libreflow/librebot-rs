use crate::environment::{Action, Environment, Observation};

pub struct GridWorld {
    pub x: usize,
    pub y: usize,
    pub log: (usize, usize),
    pub steps: usize,
    pub max_steps: usize,
    pub got_log: bool,
    rng_state: u64,
}

impl Default for GridWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl GridWorld {
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            log: (6, 3),
            steps: 0,
            max_steps: 64,
            got_log: false,
            rng_state: 0x1234_5678_9abc_def0,
        }
    }

    fn rand(&mut self) -> u64 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng_state >> 33
    }

    pub fn obs_key(&self) -> u64 {
        let dx = (self.x as f64 - self.log.0 as f64).abs();
        let dy = (self.y as f64 - self.log.1 as f64).abs();
        Observation {
            features: vec![
                self.x as f64 / 8.0,
                self.y as f64 / 8.0,
                self.got_log as u8 as f64,
                (dx + dy) / 16.0,
            ],
        }
        .key()
    }
}

impl Environment for GridWorld {
    fn reset(&mut self) -> u64 {
        self.x = 0;
        self.y = 0;
        self.steps = 0;
        self.got_log = false;
        self.obs_key()
    }

    fn step(&mut self, action: Action) -> (u64, f32, bool) {
        self.steps += 1;
        let mut reward = 0.0_f32;
        match action {
            Action::GatherWood => {
                if (self.x, self.y) == self.log && !self.got_log {
                    reward = 1.0;
                    self.got_log = true;
                }
            }
            Action::CraftPlanks => {
                if self.got_log {
                    reward = 0.5;
                }
            }
            Action::Explore => {
                let d = (self.rand() % 4) as i64;
                match d {
                    0 => self.x = (self.x + 1).min(7),
                    1 => self.x = self.x.saturating_sub(1),
                    2 => self.y = (self.y + 1).min(7),
                    _ => self.y = self.y.saturating_sub(1),
                }
            }
            Action::Idle => {
                reward = -0.01;
            }
            _ => {}
        }
        let done = self.steps >= self.max_steps;
        (self.obs_key(), reward, done)
    }
}
