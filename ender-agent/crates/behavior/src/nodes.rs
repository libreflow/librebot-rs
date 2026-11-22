use crate::blackboard::Blackboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Success,
    Failure,
}

pub trait Node: Send {
    fn tick(&mut self, bb: &mut Blackboard) -> Status;
    fn reset(&mut self) {}
}

pub struct Selector {
    children: Vec<Box<dyn Node>>,
}

impl Selector {
    pub fn new(children: Vec<Box<dyn Node>>) -> Self {
        Self { children }
    }
}

impl Node for Selector {
    fn tick(&mut self, bb: &mut Blackboard) -> Status {
        for child in &mut self.children {
            match child.tick(bb) {
                Status::Failure => continue,
                status => return status,
            }
        }
        Status::Failure
    }

    fn reset(&mut self) {
        for child in &mut self.children {
            child.reset();
        }
    }
}

pub struct Sequence {
    children: Vec<Box<dyn Node>>,
    index: usize,
}

impl Sequence {
    pub fn new(children: Vec<Box<dyn Node>>) -> Self {
        Self {
            children,
            index: 0,
        }
    }
}

impl Node for Sequence {
    fn tick(&mut self, bb: &mut Blackboard) -> Status {
        while self.index < self.children.len() {
            match self.children[self.index].tick(bb) {
                Status::Success => {
                    self.children[self.index].reset();
                    self.index += 1;
                }
                Status::Running => return Status::Running,
                Status::Failure => {
                    self.reset();
                    return Status::Failure;
                }
            }
        }
        Status::Success
    }

    fn reset(&mut self) {
        self.index = 0;
        for child in &mut self.children {
            child.reset();
        }
    }
}

pub struct Inverter {
    child: Box<dyn Node>,
}

impl Inverter {
    pub fn new(child: Box<dyn Node>) -> Self {
        Self { child }
    }
}

impl Node for Inverter {
    fn tick(&mut self, bb: &mut Blackboard) -> Status {
        match self.child.tick(bb) {
            Status::Success => Status::Failure,
            Status::Failure => Status::Success,
            Status::Running => Status::Running,
        }
    }

    fn reset(&mut self) {
        self.child.reset();
    }
}

pub struct Check<F>
where
    F: Fn(&Blackboard) -> bool + Send,
{
    pred: F,
}

impl<F> Check<F>
where
    F: Fn(&Blackboard) -> bool + Send,
{
    pub fn new(pred: F) -> Self {
        Self { pred }
    }
}

impl<F> Node for Check<F>
where
    F: Fn(&Blackboard) -> bool + Send,
{
    fn tick(&mut self, bb: &mut Blackboard) -> Status {
        if (self.pred)(bb) {
            Status::Success
        } else {
            Status::Failure
        }
    }
}

pub struct Action<F>
where
    F: FnMut(&mut Blackboard) -> Status + Send,
{
    f: F,
}

impl<F> Action<F>
where
    F: FnMut(&mut Blackboard) -> Status + Send,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F> Node for Action<F>
where
    F: FnMut(&mut Blackboard) -> Status + Send,
{
    fn tick(&mut self, bb: &mut Blackboard) -> Status {
        (self.f)(bb)
    }
}
