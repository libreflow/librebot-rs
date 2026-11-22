use ender_perception::WorldState;

use crate::{Skill, SkillContext, SkillStatus};

pub struct GatherSkill {
    pos: [i32; 3],
    drop: String,
    started: bool,
    count_at_start: u32,
    ticks: u64,
    max_ticks: u64,
}

impl GatherSkill {
    pub fn new(pos: [i32; 3], drop: String) -> Self {
        Self {
            pos,
            drop,
            started: false,
            count_at_start: 0,
            ticks: 0,
            max_ticks: 20 * 30,
        }
    }
}

impl Skill for GatherSkill {
    fn name(&self) -> &'static str {
        "gather"
    }

    fn can_execute(&self, state: &WorldState) -> bool {
        state
            .block_at(ender_perception::BlockPos::from_array(self.pos))
            .map_or(true, |b| b != "minecraft:air")
    }

    fn start(&mut self, ctx: &SkillContext) {
        self.count_at_start = ctx.state.inventory.count(&self.drop);
        let _ = ctx.adapter.break_block(self.pos);
        self.started = true;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        self.ticks += 1;
        if ctx.state.inventory.count(&self.drop) > self.count_at_start {
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
