use crate::blackboard::Blackboard;
use crate::nodes::{Node, Status};

pub struct BehaviorTree {
    root: Box<dyn Node>,
    pub blackboard: Blackboard,
}

impl BehaviorTree {
    pub fn new(root: Box<dyn Node>) -> Self {
        Self {
            root,
            blackboard: Blackboard::new(),
        }
    }

    pub fn tick(&mut self) -> Status {
        self.root.tick(&mut self.blackboard)
    }

    pub fn reset(&mut self) {
        self.root.reset();
    }
}
