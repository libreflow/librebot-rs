use ender_perception::entities::distance;
use ender_perception::WorldState;

use crate::{Skill, SkillContext, SkillStatus};

pub struct MoveToSkill {
    target: [i32; 3],
    radius: f64,
    started: bool,
    finished: bool,
    ticks: u64,
    max_ticks: u64,
    last_pos: [f64; 3],
    stuck_ticks: u64,
    repaths: u32,
    max_repaths: u32,
}

impl MoveToSkill {
    pub fn new(target: [i32; 3], radius: f64) -> Self {
        Self {
            target,
            radius,
            started: false,
            finished: false,
            ticks: 0,
            max_ticks: 20 * 120,
            last_pos: [0.0; 3],
            stuck_ticks: 0,
            repaths: 0,
            max_repaths: 3,
        }
    }

    pub fn with_max_ticks(mut self, max_ticks: u64) -> Self {
        self.max_ticks = max_ticks;
        self
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn target_f64(&self) -> [f64; 3] {
        [
            self.target[0] as f64 + 0.5,
            self.target[1] as f64,
            self.target[2] as f64 + 0.5,
        ]
    }
}

impl Skill for MoveToSkill {
    fn name(&self) -> &'static str {
        "move_to"
    }

    fn can_execute(&self, _state: &WorldState) -> bool {
        true
    }

    fn start(&mut self, ctx: &SkillContext) {
        ctx.adapter.goto(self.target);
        self.started = true;
        self.last_pos = ctx.state.position;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        self.ticks += 1;
        let target = self.target_f64();
        if distance(ctx.state.position, target) <= self.radius {
            self.finished = true;
            return SkillStatus::Success;
        }
        let moved = distance(ctx.state.position, self.last_pos);
        self.last_pos = ctx.state.position;
        if moved < 0.05 {
            self.stuck_ticks += 1;
        } else {
            self.stuck_ticks = 0;
        }
        if self.stuck_ticks > 40 {
            if self.repaths < self.max_repaths {
                ctx.adapter.goto(self.target);
                self.repaths += 1;
                self.stuck_ticks = 0;
            } else {
                self.finished = true;
                return SkillStatus::Failed;
            }
        }
        if self.ticks > self.max_ticks {
            self.finished = true;
            return SkillStatus::Failed;
        }
        SkillStatus::Running
    }

    fn cancel(&mut self) {
        self.started = false;
        self.finished = true;
    }
}
