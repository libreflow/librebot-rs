//! Machine à états du speedrun RSG 1.21.1 (contrat : `route.md`).
//! Transitions explicites (`Phase` + `match`), RunLogger central, aucun panic :
//! une phase en incident rend un `Step` (Retry / PlanB / Abort), jamais un unwrap.

use crate::bot::Bot;
use crate::runlog::{RunEvent, RunLogger};
use std::time::Duration;

/// Timeout global d'une phase avant `PhaseTimeout` + retry (route.md §2).
pub const PHASE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Nombre max de retries d'une même phase avant abandon propre.
pub const MAX_RETRIES: u32 = 3;

/// Les 6 phases du run, dans l'ordre de route.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Spawn,
    NetherEntry,
    Bastion,
    Fortress,
    Stronghold,
    End,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Spawn => "spawn",
            Phase::NetherEntry => "nether_entry",
            Phase::Bastion => "bastion",
            Phase::Fortress => "fortress",
            Phase::Stronghold => "stronghold",
            Phase::End => "end",
        }
    }

    /// Phase suivante selon route.md. `None` après `End`.
    pub fn next(self) -> Option<Phase> {
        match self {
            Phase::Spawn => Some(Phase::NetherEntry),
            Phase::NetherEntry => Some(Phase::Bastion),
            Phase::Bastion => Some(Phase::Fortress),
            Phase::Fortress => Some(Phase::Stronghold),
            Phase::Stronghold => Some(Phase::End),
            Phase::End => None,
        }
    }
}

/// Résultat d'une étape de phase : transition explicite, jamais de panic.
#[derive(Debug)]
pub enum Step {
    /// Aller vers cette phase (transition explicite, ex. plan B).
    Go(Phase),
    /// Phase terminée : avancer à la suivante de route.md.
    Next,
    /// Réessayer la phase courante (incident récupérable).
    Retry,
    /// Run terminé avec succès (dragon tué).
    Done,
    /// Abandon propre du run.
    Abort(String),
}

pub struct Speedrun {
    pub phase: Phase,
    /// Phase de repli après une mort (route.md §2).
    pub checkpoint: Phase,
    pub logger: RunLogger,
    retries: u32,
}

impl Default for Speedrun {
    fn default() -> Self {
        Self::new()
    }
}

impl Speedrun {
    pub fn new() -> Self {
        Self {
            phase: Phase::Spawn,
            checkpoint: Phase::Spawn,
            logger: RunLogger::new(),
            retries: 0,
        }
    }

    /// Applique une transition. Retourne false quand le run s'arrête.
    pub fn transition(&mut self, step: Step) -> bool {
        match step {
            Step::Go(p) => {
                self.logger.event(RunEvent::PhaseEnter { phase: p.name() });
                self.phase = p;
                self.checkpoint = p;
                self.retries = 0;
                true
            }
            Step::Next => match self.phase.next() {
                Some(p) => {
                    self.phase = p;
                    self.retries = 0;
                    true
                }
                None => false,
            },
            Step::Retry => {
                self.retries += 1;
                if self.retries > MAX_RETRIES {
                    self.logger.event(RunEvent::Custom {
                        kind: "abort".into(),
                        detail: format!(
                            "phase {} : {} retries épuisés",
                            self.phase.name(),
                            MAX_RETRIES
                        ),
                    });
                    false
                } else {
                    true
                }
            }
            Step::Done => false,
            Step::Abort(reason) => {
                self.logger.event(RunEvent::Custom {
                    kind: "abort".into(),
                    detail: reason,
                });
                false
            }
        }
    }
}

/// Pompe centrale : lit les paquets (timeout interne 50 ms) + physique.
/// S'arrête si le bot meurt ; l'appelant gère le respawn + checkpoint.
async fn pump(bot: &mut Bot, ticks: usize) {
    for _ in 0..ticks {
        bot.drain_packets().await;
        if bot.dead {
            return;
        }
        if bot.tick(0.05).await.is_err() {
            return;
        }
    }
}

/// Boucle principale du run : split à l'entrée de chaque phase, dispatch explicite,
/// timeout global par phase, mort → respawn + retour au checkpoint.
pub async fn run(bot: &mut Bot) {
    let mut sr = Speedrun::new();
    sr.logger.event(RunEvent::PhaseEnter {
        phase: sr.phase.name(),
    });
    loop {
        sr.logger.split(sr.phase.name());
        let step = match tokio::time::timeout(PHASE_TIMEOUT, dispatch(bot, &mut sr)).await {
            Ok(s) => s,
            Err(_) => {
                sr.logger.event(RunEvent::PhaseTimeout {
                    phase: sr.phase.name(),
                });
                Step::Retry
            }
        };
        if bot.dead {
            sr.logger.event(RunEvent::Death);
            if bot.send_respawn().await.is_ok() {
                sr.logger.event(RunEvent::Respawn);
            }
            sr.phase = sr.checkpoint;
            sr.retries = 0;
            continue;
        }
        if !sr.transition(step) {
            break;
        }
    }
    let summary = sr.logger.summary();
    tracing::info!(target: "run", "\n{}", summary);
    println!("{summary}");
}

async fn dispatch(bot: &mut Bot, sr: &mut Speedrun) -> Step {
    match sr.phase {
        Phase::Spawn => phase_spawn(bot, &mut sr.logger).await,
        Phase::NetherEntry | Phase::Bastion | Phase::Fortress | Phase::Stronghold | Phase::End => {
            phase_stub(sr).await
        }
    }
}

/// Phase 1 `Spawn` (route.md) : chunks chargés + atterri, puis bois → pierre
/// (réutilise la mission survie existante). Split `spawn_ready`.
async fn phase_spawn(bot: &mut Bot, logger: &mut RunLogger) -> Step {
    for _ in 0..150 {
        bot.drain_packets().await;
        if bot.world.chunk_count() >= 25 {
            break;
        }
    }
    for _ in 0..200 {
        if bot.on_ground {
            break;
        }
        pump(bot, 1).await;
    }
    let wood = bot.phase_wood().await;
    logger.event(RunEvent::Custom {
        kind: "spawn_wood".into(),
        detail: format!("ok={wood}"),
    });
    if !wood || bot.dead {
        return Step::Retry;
    }
    let stone = bot.phase_stone().await;
    logger.event(RunEvent::Custom {
        kind: "spawn_stone".into(),
        detail: format!("ok={stone}"),
    });
    if !stone || bot.dead {
        return Step::Retry;
    }
    logger.split("spawn_ready");
    Step::Next
}

/// Phases pas encore implémentées (une par itération) : abandon propre, loggé.
async fn phase_stub(sr: &mut Speedrun) -> Step {
    Step::Abort(format!(
        "phase '{}' non implémentée (itération suivante selon route.md)",
        sr.phase.name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordre_des_phases_conforme_a_route_md() {
        let order = [
            Phase::Spawn,
            Phase::NetherEntry,
            Phase::Bastion,
            Phase::Fortress,
            Phase::Stronghold,
            Phase::End,
        ];
        for w in order.windows(2) {
            assert_eq!(w[0].next(), Some(w[1]));
        }
        assert_eq!(Phase::End.next(), None);
        assert_eq!(Phase::Spawn.name(), "spawn");
        assert_eq!(Phase::End.name(), "end");
    }

    #[test]
    fn transitions_explicites() {
        let mut sr = Speedrun::new();
        assert_eq!(sr.phase, Phase::Spawn);
        assert!(sr.transition(Step::Next));
        assert_eq!(sr.phase, Phase::NetherEntry);
        assert!(sr.transition(Step::Go(Phase::Fortress)));
        assert_eq!(sr.phase, Phase::Fortress);
        assert_eq!(sr.checkpoint, Phase::Fortress);
        assert!(sr.transition(Step::Retry));
        assert!(sr.transition(Step::Retry));
        assert!(sr.transition(Step::Retry));
        assert!(!sr.transition(Step::Retry)); // MAX_RETRIES épuisés
    }

    #[test]
    fn done_et_abort_arretent_proprement() {
        let mut sr = Speedrun::new();
        assert!(!sr.transition(Step::Done));
        let mut sr2 = Speedrun::new();
        assert!(!sr2.transition(Step::Abort("test".into())));
        assert_eq!(sr2.logger.event_count(), 1);
    }
}
