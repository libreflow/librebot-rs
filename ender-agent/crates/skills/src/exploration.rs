use ender_perception::entities::distance;
use ender_perception::WorldState;

use crate::movement::MoveToSkill;
use crate::{Skill, SkillContext, SkillStatus};

pub struct ExploreSkill {
    center: [i32; 3],
    radius: i32,
    inner: Option<MoveToSkill>,
    started: bool,
}

impl ExploreSkill {
    pub fn new(center: [i32; 3], radius: i32) -> Self {
        Self {
            center,
            radius,
            inner: None,
            started: false,
        }
    }

    fn pick_target(&self, tick: u64) -> [i32; 3] {
        let angle = (tick as f64 * 137.508_f64).to_radians();
        let r = self.radius as f64 * 0.8;
        [
            self.center[0] + (angle.cos() * r) as i32,
            self.center[1],
            self.center[2] + (angle.sin() * r) as i32,
        ]
    }
}

impl Skill for ExploreSkill {
    fn name(&self) -> &'static str {
        "explore"
    }

    fn can_execute(&self, _state: &WorldState) -> bool {
        true
    }

    fn start(&mut self, ctx: &SkillContext) {
        let target = self.pick_target(ctx.state.tick);
        let mut mv = MoveToSkill::new(target, 2.0).with_max_ticks(20 * 60);
        mv.start(ctx);
        self.inner = Some(mv);
        self.started = true;
    }

    fn tick(&mut self, ctx: &SkillContext) -> SkillStatus {
        if !self.started {
            return SkillStatus::Failed;
        }
        if self.inner.is_none() {
            let target = self.pick_target(ctx.state.tick);
            let mut mv = MoveToSkill::new(target, 2.0).with_max_ticks(20 * 60);
            mv.start(ctx);
            self.inner = Some(mv);
        }
        let status = self.inner.as_mut().unwrap().tick(ctx);
        match status {
            SkillStatus::Success => {
                let pos = ctx.state.position;
                if distance(pos, [self.center[0] as f64, self.center[1] as f64, self.center[2] as f64]) > self.radius as f64 * 2.0 {
                    self.inner = None;
                    return SkillStatus::Running;
                }
                SkillStatus::Success
            }
            SkillStatus::Failed => SkillStatus::Failed,
            other => other,
        }
    }

    fn cancel(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.cancel();
        }
        self.started = false;
    }
}
