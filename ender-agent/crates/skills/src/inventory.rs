use ender_perception::WorldState;

use crate::{Skill, SkillContext, SkillStatus};

pub struct EnsureItemSkill {
    item: String,
    count: u32,
    started: bool,
    ticks: u64,
    max_ticks: u64,
}

impl EnsureItemSkill {
    pub fn new(item: String, count: u32) -> Self {
        Self {
            item,
            count,
            started: false,
            ticks: 0,
            max_ticks: 20 * 60,
        }
    }
}

impl Skill for EnsureItemSkill {
    fn name(&self) -> &'static str {
        "ensure_item"
    }

    fn can_execute(&self, state: &WorldState) -> bool {
        state.inventory.count(&self.item) < self.count
    }

    fn start(&mut self, _ctx: &SkillContext) {
        self.started = true;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        self.ticks += 1;
        if ctx.state.inventory.count(&self.item) >= self.count {
            return SkillStatus::Success;
        }
        if self.ticks > self.max_ticks {
            return SkillStatus::Failed;
        }
        SkillStatus::Running
    }

    fn cancel(&mut self) {
        self.started = false;
    }
}
