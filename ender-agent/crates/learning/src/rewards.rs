#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reward {
    pub objective_progress: f32,
    pub survival: f32,
    pub resource_progress: f32,
    pub exploration: f32,
    pub task_failure: f32,
}

pub const WEIGHTS: Reward = Reward {
    objective_progress: 1.0,
    survival: 0.5,
    resource_progress: 0.3,
    exploration: 0.1,
    task_failure: -1.0,
};

impl Reward {
    pub fn zero() -> Self {
        Reward {
            objective_progress: 0.0,
            survival: 0.0,
            resource_progress: 0.0,
            exploration: 0.0,
            task_failure: 0.0,
        }
    }

    pub fn total(&self) -> f32 {
        WEIGHTS.objective_progress * self.objective_progress
            + WEIGHTS.survival * self.survival
            + WEIGHTS.resource_progress * self.resource_progress
            + WEIGHTS.exploration * self.exploration
            + WEIGHTS.task_failure * self.task_failure
    }
}
