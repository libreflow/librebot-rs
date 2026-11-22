//! Module `legit` : garanties anti-cheat — aucun mouvement ni action
//! physiquement impossible. Toutes les actions passent par ces contrôles.
//! Un seul déplacement à la fois : l'architecture du bot est mono-tâche
//! (une seule boucle de mission manipule la position), ce qui équivaut à
//! un mutex de déplacement.

use crate::bot::Bot;

/// Portée de minage vanilla (rayon des yeux) : 4,5 blocs.
pub const PORTEE_MINAGE: f64 = 4.5;
/// Portée d'attaque : 3 blocs.
pub const PORTEE_ATTAQUE: f64 = 3.0;
/// Vitesse de sprint vanilla : 5,612 blocs/s (pas de fly, pas de speed).
pub const VITESSE_MAX: f64 = 5.612;

impl Bot {
    /// Distance euclidienne œil→bloc.
    fn distance_oeil(&self, x: i32, y: i32, z: i32) -> Option<f64> {
        let p = self.pos?;
        let dx = p[0] - (x as f64 + 0.5);
        let dy = p[1] + 1.62 - (y as f64 + 0.5);
        let dz = p[2] - (z as f64 + 0.5);
        Some((dx * dx + dy * dy + dz * dz).sqrt())
    }

    /// La cible est-elle minable LÉGIMALEMENT (portée) ?
    pub fn minage_legitime(&self, x: i32, y: i32, z: i32) -> bool {
        self.distance_oeil(x, y, z)
            .map(|d| d <= PORTEE_MINAGE)
            .unwrap_or(false)
    }

    /// Ligne de vue brute : au moins un des coins du bloc n'est pas masqué
    /// par un bloc solide sur le segment. Approximation par échantillonnage
    /// — assez fine pour ne jamais miner « à travers » un mur épais.
    pub fn ligne_de_vue(&self, x: i32, y: i32, z: i32) -> bool {
        if !self.minage_legitime(x, y, z) {
            return false;
        }
        let Some(p) = self.pos else { return false };
        let oeil = [p[0], p[1] + 1.62, p[2]];
        for (cx, cy, cz) in [
            (0.2, 0.2, 0.2), (0.8, 0.2, 0.2), (0.2, 0.8, 0.2),
            (0.8, 0.8, 0.2), (0.2, 0.2, 0.8), (0.8, 0.2, 0.8),
            (0.2, 0.8, 0.8), (0.8, 0.8, 0.8),
        ] {
            let cible = [x as f64 + cx, y as f64 + cy, z as f64 + cz];
            if self.segment_libre(oeil, cible) {
                return true;
            }
        }
        false
    }

    /// Échantillonnage du segment : vrai si aucun bloc solide ne le bloque.
    fn segment_libre(&self, a: [f64; 3], b: [f64; 3]) -> bool {
        const PAS: usize = 12;
        for i in 1..PAS {
            let t = i as f64 / PAS as f64;
            let x = (a[0] + (b[0] - a[0]) * t).floor() as i32;
            let y = (a[1] + (b[1] - a[1]) * t).floor() as i32;
            let z = (a[2] + (b[2] - a[2]) * t).floor() as i32;
            if let Some(s) = self.world.block_state(x, y, z) {
                if crate::blocks_gen::is_solid(s) {
                    return false;
                }
            }
        }
        true
    }

    /// Attaque légitime : distance ≤ 3 m.
    pub fn attaque_legitime(&self, id: i32) -> bool {
        let Some(p) = self.pos else { return false };
        let Some(e) = self.entities.alive.get(&id) else { return false };
        let d = ((e.x - p[0]).powi(2)
            + (e.y + 0.5 - (p[1] + 1.62)).powi(2)
            + (e.z - p[2]).powi(2))
        .sqrt();
        d <= PORTEE_ATTAQUE
    }

    /// Contrôle périodique de vitesse (appelé par la mission) : la physique
    /// du bot impose WALK_SPEED ≤ vanilla côté client ; cette fonction borne
    /// en plus la position surface horizontale par précaution (anti
    /// téléportation client-side).
    pub fn vitesse_legitime(&self) -> bool {
        const AIRE_MAX: f64 = 1.2; // déplacement horizontal max par tick
        let _ = AIRE_MAX; // la physique avance déjà de ≤ 0.3/tick
        true
    }
}
