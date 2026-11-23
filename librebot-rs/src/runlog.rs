//! RunLogger central du speedrun : splits par phase et événements (morts, resets
//! de path, barters, timeouts), émis via `tracing` sur la cible `run`.

use std::time::{Duration, Instant};

/// Événement notable d'un run, loggé avec son timestamp relatif à T0.
#[derive(Debug, Clone)]
pub enum RunEvent {
    PhaseEnter { phase: &'static str },
    Death,
    Respawn,
    PathReset { goal: (i32, i32, i32) },
    Barter { pearls: u32 },
    IoTimeout,
    PhaseTimeout { phase: &'static str },
    PlanB { phase: &'static str, reason: String },
    Custom { kind: String, detail: String },
}

pub struct RunLogger {
    start: Instant,
    splits: Vec<(&'static str, Duration)>,
    events: Vec<(Duration, RunEvent)>,
}

impl Default for RunLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl RunLogger {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            splits: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Enregistre le split d'entrée dans une phase (T0 = login jeu).
    pub fn split(&mut self, phase: &'static str) {
        let t = self.start.elapsed();
        tracing::info!(target: "run", t_ms = t.as_millis() as u64, phase, "SPLIT");
        self.splits.push((phase, t));
    }

    pub fn event(&mut self, ev: RunEvent) {
        let t = self.start.elapsed();
        tracing::info!(target: "run", t_ms = t.as_millis() as u64, event = ?ev, "EVENT");
        self.events.push((t, ev));
    }

    pub fn split_count(&self) -> usize {
        self.splits.len()
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Tableau des splits : cumul depuis T0 + delta depuis le split précédent.
    pub fn summary(&self) -> String {
        let mut out =
            String::from("=== Résumé du run ===\nphase                cumul      delta\n");
        let mut prev = Duration::ZERO;
        for (phase, t) in &self.splits {
            out.push_str(&format!(
                "{:<20} {:>8.1?} {:>8.1?}\n",
                phase,
                t,
                t.saturating_sub(prev)
            ));
            prev = *t;
        }
        out.push_str(&format!(
            "{:>20} {:>8.1?}\n{} événements\n",
            "TOTAL",
            self.start.elapsed(),
            self.events.len()
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_ordonnes_et_summary() {
        let mut log = RunLogger::new();
        log.split("spawn");
        std::thread::sleep(Duration::from_millis(5));
        log.split("nether_entry");
        assert_eq!(log.split_count(), 2);
        assert!(log.splits[1].1 >= log.splits[0].1);
        let s = log.summary();
        assert!(s.contains("spawn"));
        assert!(s.contains("nether_entry"));
        assert!(s.contains("TOTAL"));
    }

    #[test]
    fn events_enregistres() {
        let mut log = RunLogger::new();
        log.event(RunEvent::Death);
        log.event(RunEvent::Barter { pearls: 4 });
        log.event(RunEvent::PlanB {
            phase: "fortress",
            reason: "barter vide".into(),
        });
        assert_eq!(log.event_count(), 3);
        assert!(matches!(log.events[0].1, RunEvent::Death));
        assert!(matches!(log.events[1].1, RunEvent::Barter { pearls: 4 }));
    }
}
