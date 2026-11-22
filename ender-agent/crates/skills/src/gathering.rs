use ender_perception::WorldState;

use crate::movement::MoveToSkill;
use crate::{Skill, SkillContext, SkillStatus};

pub struct GatherSkill {
    pos: [i32; 3],
    drop: String,
    started: bool,
    /// Phase 1 : se rapprocher du bloc à portée de minage (~4.5 blocs max côté serveur).
    approach: Option<MoveToSkill>,
    /// Phase 2 : minage lancé.
    digging: bool,
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
            approach: None,
            digging: false,
            count_at_start: 0,
            ticks: 0,
            max_ticks: 20 * 60,
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
        // D'abord se rapprocher : un minage hors portée est refusé par le serveur.
        let mut mv = MoveToSkill::new(self.pos, 2.0).with_max_ticks(20 * 30);
        mv.start(ctx);
        self.approach = Some(mv);
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
        if !self.digging {
            let approach = self.approach.as_mut().unwrap();
            match approach.tick(ctx) {
                SkillStatus::Success => match ctx.adapter.break_block(self.pos) {
                    Ok(()) => self.digging = true,
                    Err(_) => return SkillStatus::Failed,
                },
                SkillStatus::Failed => return SkillStatus::Failed,
                _ => {}
            }
        }
        SkillStatus::Running
    }

    fn cancel(&mut self) {
        if let Some(mv) = self.approach.as_mut() {
            mv.cancel();
        }
        self.started = false;
    }
}
