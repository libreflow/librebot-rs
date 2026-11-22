use ender_perception::entities::{distance, nearest_hostile};
use ender_perception::WorldState;

use crate::movement::MoveToSkill;
use crate::{Skill, SkillContext, SkillStatus};

pub struct FleeSkill {
    inner: Option<MoveToSkill>,
    calm_ticks: u64,
    started: bool,
    max_ticks: u64,
    ticks: u64,
}

impl FleeSkill {
    pub fn new() -> Self {
        Self {
            inner: None,
            calm_ticks: 0,
            started: false,
            max_ticks: 20 * 60,
            ticks: 0,
        }
    }
}

impl Default for FleeSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl Skill for FleeSkill {
    fn name(&self) -> &'static str {
        "flee"
    }

    fn can_execute(&self, state: &WorldState) -> bool {
        nearest_hostile(&state.entities, state.position)
            .map_or(false, |(_, d)| d <= 10.0)
    }

    fn start(&mut self, _ctx: &SkillContext) {
        self.started = true;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        self.ticks += 1;
        match nearest_hostile(&ctx.state.entities, ctx.state.position) {
            Some((hostile, d)) if d <= 10.0 => {
                self.calm_ticks = 0;
                let away = [
                    ctx.state.position[0] + (ctx.state.position[0] - hostile.pos[0]),
                    ctx.state.position[1],
                    ctx.state.position[2] + (ctx.state.position[2] - hostile.pos[2]),
                ];
                let norm = distance(ctx.state.position, away).max(1.0);
                let target = [
                    ctx.state.position[0] + (away[0] - ctx.state.position[0]) / norm * 12.0,
                    ctx.state.position[1],
                    ctx.state.position[2] + (away[2] - ctx.state.position[2]) / norm * 12.0,
                ];
                let target = [
                    target[0].floor() as i32,
                    target[1].floor() as i32,
                    target[2].floor() as i32,
                ];
                let need_new = match &self.inner {
                    Some(mv) => mv.is_finished(),
                    None => true,
                };
                if need_new {
                    let mut mv = MoveToSkill::new(target, 2.0).with_max_ticks(20 * 20);
                    mv.start(ctx);
                    self.inner = Some(mv);
                }
                match self.inner.as_mut().unwrap().tick(ctx) {
                    SkillStatus::Running => SkillStatus::Running,
                    SkillStatus::Success => SkillStatus::Running,
                    SkillStatus::Failed => SkillStatus::Failed,
                    other => other,
                }
            }
            _ => {
                self.calm_ticks += 1;
                if self.calm_ticks >= 20 {
                    return SkillStatus::Success;
                }
                if self.ticks > self.max_ticks {
                    return SkillStatus::Failed;
                }
                SkillStatus::Running
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.cancel();
        }
        self.started = false;
    }
}
