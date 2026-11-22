use crate::goals::Goal;
use crate::goap::StateMap;

pub fn goal_utility(goal: &Goal, state: &StateMap) -> f32 {
    goal.priority * (1.0 - goal.satisfied_ratio(state))
}

pub fn select_goal<'a>(goals: &'a [Goal], state: &StateMap) -> Option<&'a Goal> {
    goals
        .iter()
        .filter(|g| !g.is_satisfied(state))
        .max_by(|a, b| {
            goal_utility(a, state)
                .partial_cmp(&goal_utility(b, state))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}
