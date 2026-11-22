use ender_perception::WorldState;

use crate::{Skill, SkillContext, SkillStatus};

pub struct CraftSkill {
    item: String,
    count: u32,
    started: bool,
    count_at_start: u32,
    command_error: Option<String>,
    ticks: u64,
    max_ticks: u64,
}

impl CraftSkill {
    pub fn new(item: String, count: u32) -> Self {
        Self {
            item,
            count,
            started: false,
            count_at_start: 0,
            command_error: None,
            ticks: 0,
            max_ticks: 20 * 30,
        }
    }
}

impl Skill for CraftSkill {
    fn name(&self) -> &'static str {
        "craft"
    }

    fn can_execute(&self, state: &WorldState) -> bool {
        state.inventory.count(&self.item) < self.count
    }

    fn start(&mut self, ctx: &SkillContext) {
        self.count_at_start = ctx.state.inventory.count(&self.item);
        match ctx.adapter.craft(&self.item, self.count) {
            Ok(()) => {}
            Err(e) => self.command_error = Some(e.to_string()),
        }
        self.started = true;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        self.ticks += 1;
        if let Some(err) = &self.command_error {
            let _ = err;
            return SkillStatus::Failed;
        }
        if ctx.state.inventory.count(&self.item) >= self.count_at_start + self.count {
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
