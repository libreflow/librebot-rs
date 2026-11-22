use ender_behavior::{Action, BehaviorTree, Check, Selector, Sequence, Status};

#[test]
fn selecteur_interrompt_la_branche_en_cours_quand_une_garde_passe() {
    let mut tree = BehaviorTree::new(Box::new(Selector::new(vec![
        Box::new(Sequence::new(vec![
            Box::new(Check::new(|bb| bb.get_str("danger") == Some("oui"))),
            Box::new(Action::new(|bb| {
                bb.set("a_fui", true);
                Status::Running
            })),
        ])),
        Box::new(Sequence::new(vec![
            Box::new(Check::new(|bb| bb.get_str("danger") == Some("non"))),
            Box::new(Action::new(|bb| {
                bb.set("a_avance", true);
                Status::Success
            })),
        ])),
    ])));

    tree.blackboard.set("danger", "non");
    let status = tree.tick();
    assert_eq!(status, Status::Success);
    assert!(tree.blackboard.get_bool("a_avance"));
    assert!(!tree.blackboard.get_bool("a_fui"));

    tree.blackboard.set("danger", "oui");
    let status = tree.tick();
    assert_eq!(status, Status::Running);
    assert!(tree.blackboard.get_bool("a_fui"));
}

#[test]
fn sequence_echoue_si_un_enfant_echoue() {
    let mut tree = BehaviorTree::new(Box::new(Sequence::new(vec![
        Box::new(Action::new(|_| Status::Success)),
        Box::new(Action::new(|_| Status::Failure)),
        Box::new(Action::new(|_| Status::Success)),
    ])));
    assert_eq!(tree.tick(), Status::Failure);
    assert_eq!(tree.tick(), Status::Failure);
}

#[test]
fn sequence_reprend_la_ou_elle_en_est() {
    let mut tree = BehaviorTree::new(Box::new(Sequence::new(vec![
        Box::new(Action::new(|bb| {
            if bb.get_bool("pret") {
                Status::Success
            } else {
                Status::Running
            }
        })),
        Box::new(Action::new(|bb| {
            bb.set("fait", true);
            Status::Success
        })),
    ])));
    assert_eq!(tree.tick(), Status::Running);
    assert!(!tree.blackboard.get_bool("fait"));
    tree.blackboard.set("pret", true);
    assert_eq!(tree.tick(), Status::Success);
    assert!(tree.blackboard.get_bool("fait"));
}
