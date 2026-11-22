use ender_perception::WorldState;

#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub features: Vec<f64>,
}

impl Observation {
    pub fn key(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for f in &self.features {
            let q = (f * 10.0).round() as i64 as u64;
            h ^= q;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

pub fn observe(state: &WorldState) -> Observation {
    let features = vec![
        state.health as f64 / 20.0,
        state.hunger as f64 / 20.0,
        (state.position[0] / 16.0).floor() / 64.0,
        (state.position[2] / 16.0).floor() / 64.0,
        state.entities.iter().filter(|e| e.hostile).count() as f64,
        state.inventory.count("minecraft:oak_log") as f64 / 8.0,
        state.inventory.count("minecraft:oak_planks") as f64 / 16.0,
        state.inventory.items().iter().map(|(_, c)| *c as f64).sum::<f64>() / 64.0,
    ];
    Observation { features }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Idle,
    Explore,
    GatherWood,
    CraftPlanks,
    CraftPickaxe,
    Flee,
    Eat,
}

pub const ALL_ACTIONS: &[Action] = &[
    Action::Idle,
    Action::Explore,
    Action::GatherWood,
    Action::CraftPlanks,
    Action::CraftPickaxe,
    Action::Flee,
    Action::Eat,
];

#[derive(Debug, Clone)]
pub struct Step {
    pub obs_key: u64,
    pub action: Action,
    pub reward: f32,
    pub next_obs_key: u64,
    pub done: bool,
}

pub trait Environment: Send {
    fn reset(&mut self) -> u64;
    fn step(&mut self, action: Action) -> (u64, f32, bool);
}
