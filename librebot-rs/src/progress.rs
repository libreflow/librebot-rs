//! Persistance de progression : progress.json (serde).
//! Idempotence : chaque phase/succès déjà réussi n'est JAMAIS retenté ; les
//! tâches échouées sont marquées « tentées » et la mission continue ; à la
//! mort, la mission repart de la phase 1 mais CONSERVE les découvertes.

use serde::{Deserialize, Serialize};

const CHEMIN: &str = "progress.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Progress {
    /// Dernière phase atteinte (index 1..=19).
    pub phase: u32,
    /// Identifiants de succès déjà RÉUSSIS (jamais retentés).
    #[serde(default)]
    pub reussis: Vec<String>,
    /// Identifiants de tâches tentées sans succès (isolées, on passe).
    #[serde(default)]
    pub tente: Vec<String>,
    /// Découvertes de coordonnées — conservées à travers les morts.
    #[serde(default)]
    pub decouvertes: Decouvertes,
    /// Statistiques de runs (baseline speedrun).
    #[serde(default)]
    pub runs: Vec<Run>,
    /// Nombre de morts de la session courante.
    #[serde(default)]
    pub morts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Decouvertes {
    pub forteresse: Option<[i32; 3]>,
    pub stronghold: Option<[i32; 3]>,
    pub portail_nether: Option<[i32; 3]>,
    pub base: Option<[i32; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub debut: String,
    pub fin: String,
    pub phase_atteinte: u32,
    pub cause: String,
}

impl Progress {
    pub fn charger() -> Self {
        std::fs::read_to_string(CHEMIN)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn sauver(&self) {
        let _ = std::fs::write(CHEMIN, serde_json::to_string_pretty(self).unwrap_or_default());
    }

    pub fn est_reussi(&self, id: &str) -> bool {
        self.reussis.iter().any(|s| s == id)
    }

    pub fn marquer_reussi(&mut self, id: &str) {
        if !self.est_reussi(id) {
            self.reussis.push(id.to_string());
        }
        self.sauver();
    }

    pub fn a_tente(&self, id: &str) -> bool {
        self.tente.iter().any(|s| s == id)
    }

    pub fn marquer_tente(&mut self, id: &str) {
        if !self.est_reussi(id) && !self.a_tente(id) {
            self.tente.push(id.to_string());
        }
        self.sauver();
    }

    /// Enregistrer une fin de run (split « baseline »).
    pub fn enregistrer_run(&mut self, phase: u32, cause: &str) {
        self.runs.truncate(29); // garde les 30 derniers
        self.runs.push(Run {
            debut: chrono_maintenant(),
            fin: chrono_maintenant(),
            phase_atteinte: phase,
            cause: cause.to_string(),
        });
        self.sauver();
    }

    /// Les phases les plus lentes/abaandonnées en premier (commande baseline).
    pub fn baseline(&self) -> Vec<String> {
        let mut compte: std::collections::HashMap<u32, usize> = Default::default();
        for r in &self.runs {
            *compte.entry(r.phase_atteinte).or_insert(0) += 1;
        }
        let mut v: Vec<(u32, usize)> = compte.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        v.iter()
            .map(|(p, n)| format!("phase {} : stoppée {} fois", p, n))
            .collect()
    }
}

#[allow(dead_code)]
fn chrono_maintenant() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "?".into())
}
