pub mod combat;
pub mod crafting;
pub mod exploration;
pub mod gathering;
pub mod inventory;
pub mod movement;

use std::collections::HashMap;

pub use combat::FleeSkill;
pub use crafting::CraftSkill;
pub use exploration::ExploreSkill;
pub use gathering::GatherSkill;
pub use inventory::EnsureItemSkill;
pub use movement::MoveToSkill;

use ender_minecraft::MinecraftAdapter;
use ender_perception::WorldState;
use ender_world_model::Knowledge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillStatus {
    Running,
    Success,
    Failed,
    Interrupted,
}

pub struct SkillContext<'a> {
    pub state: &'a WorldState,
    pub adapter: &'a dyn MinecraftAdapter,
    pub knowledge: &'a Knowledge,
}

pub trait Skill: Send {
    fn name(&self) -> &'static str;
    fn can_execute(&self, state: &WorldState) -> bool;
    fn start(&mut self, ctx: &SkillContext);
    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus;
    fn cancel(&mut self);
}

#[derive(Debug, Clone)]
pub enum SkillParams {
    MoveTo { target: [i32; 3], radius: f64 },
    Gather { pos: [i32; 3], drop: String },
    EnsureItem { item: String, count: u32 },
    Craft { item: String, count: u32 },
    Explore { center: [i32; 3], radius: i32 },
    Flee,
}

type Factory = Box<dyn Fn(SkillParams) -> Box<dyn Skill> + Send + Sync>;

pub struct SkillLibrary {
    factories: HashMap<&'static str, Factory>,
}

impl Default for SkillLibrary {
    fn default() -> Self {
        Self::standard()
    }
}

impl SkillLibrary {
    pub fn standard() -> Self {
        let mut factories: HashMap<&'static str, Factory> = HashMap::new();
        factories.insert(
            "move_to",
            Box::new(|p| match p {
                SkillParams::MoveTo { target, radius } => Box::new(MoveToSkill::new(target, radius)),
                _ => Box::new(MoveToSkill::new([0, 64, 0], 1.5)),
            }),
        );
        factories.insert(
            "gather",
            Box::new(|p| match p {
                SkillParams::Gather { pos, drop } => Box::new(GatherSkill::new(pos, drop)),
                _ => Box::new(GatherSkill::new([0, 64, 0], "minecraft:oak_log".to_string())),
            }),
        );
        factories.insert(
            "ensure_item",
            Box::new(|p| match p {
                SkillParams::EnsureItem { item, count } => {
                    Box::new(EnsureItemSkill::new(item, count))
                }
                _ => Box::new(EnsureItemSkill::new("minecraft:oak_log".to_string(), 1)),
            }),
        );
        factories.insert(
            "craft",
            Box::new(|p| match p {
                SkillParams::Craft { item, count } => Box::new(CraftSkill::new(item, count)),
                _ => Box::new(CraftSkill::new("minecraft:oak_planks".to_string(), 4)),
            }),
        );
        factories.insert(
            "explore",
            Box::new(|p| match p {
                SkillParams::Explore { center, radius } => {
                    Box::new(ExploreSkill::new(center, radius))
                }
                _ => Box::new(ExploreSkill::new([0, 64, 0], 16)),
            }),
        );
        factories.insert("flee", Box::new(|_| Box::new(FleeSkill::new())));
        Self { factories }
    }

    pub fn instantiate(&self, name: &str, params: SkillParams) -> Option<Box<dyn Skill>> {
        self.factories.get(name).map(|f| f(params))
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.factories.keys().copied().collect();
        names.sort_unstable();
        names
    }
}
