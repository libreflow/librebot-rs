//! Cerveau de décision à priorités pondérées — 100 % déterministe, zéro LLM.
//! Évalué en boucle par la mission : la plus haute priorité active gagne.

use crate::bot::Bot;

/// Priorités pondérées. Ordre de déclaration = priorité réelle,
/// les poids documentés viennent de la spécification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgence {
    Aucune,
    Fuite,
    CombatMortel,
    Repas,
    FaimCritique,
    Chute,
    Noyade,
    LaveOuFeu,
    Mourir,
}

impl Urgence {
    #[allow(dead_code)]
    fn poids(self) -> u32 {
        match self {
            Urgence::Aucune => 0,
            Urgence::Fuite => 500,
            Urgence::CombatMortel => 600,
            Urgence::Repas | Urgence::FaimCritique => 700,
            Urgence::Chute => 800,
            Urgence::Noyade => 850,
            Urgence::LaveOuFeu => 900,
            Urgence::Mourir => 1000,
        }
    }
}

/// Décision choisie par le cerveau.
#[derive(Debug, Clone)]
pub struct Decision {
    pub urgence: Urgence,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub enum Action {
    Rien,
    /// PV critique : fuir et mettre de la distance.
    SurvivreFuite,
    /// Bloc de lave/feu proche : s'écarter.
    EviterLave,
    /// Faim : manger.
    Manger,
    /// Hostile menaçant : se défendre.
    Combattre,
}

impl Bot {
    /// Évalue les réflexes par poids décroissant. Pure, déterministe.
    pub fn reflechir(&self) -> Decision {
        // MOURIR 1000 : PV sous le seuil létal.
        if self.health <= 6.0 {
            return Decision { urgence: Urgence::Mourir, action: Action::SurvivreFuite };
        }
        // LAVE/FEU 900 : block de lave dans les 2 blocs sous/autour.
        if self.lave_detectee() {
            return Decision { urgence: Urgence::LaveOuFeu, action: Action::EviterLave };
        }
        // NOYADE 850 : tête dans l'eau.
        if self.tete_dans_eau() {
            return Decision { urgence: Urgence::Noyade, action: Action::EviterLave };
        }
        // FAIM 700 : faim critique (<= 6).
        if self.food <= 6 {
            return Decision { urgence: Urgence::FaimCritique, action: Action::Manger };
        }
        // COMBAT_MORTEL 600 : hostile有意 à portée.
        if let Some(_pos) = self.pos {
            if self.hostile_proche() {
                return Decision { urgence: Urgence::CombatMortel, action: Action::Combattre };
            }
        }
        Decision { urgence: Urgence::Aucune, action: Action::Rien }
    }

    /// Lave à moins de 2 blocs (pieds ou juste sous les pieds).
    pub(crate) fn lave_detectee(&self) -> bool {
        let Some(p) = self.pos else { return false };
        let (x, y, z) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        for (dx, dy, dz) in [
            (0, 0, 0), (0, -1, 0), (1, -1, 0), (-1, -1, 0),
            (0, -1, 1), (0, -1, -1),
        ] {
            if let Some(s) = self.world.block_state(x + dx, y + dy, z + dz) {
                if crate::blocks_gen::block_name(s).contains("lava") {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn tete_dans_eau(&self) -> bool {
        let Some(p) = self.pos else { return false };
        let (x, y, z) = (p[0].floor() as i32, (p[1] + 1.0) as i32, p[2].floor() as i32);
        if let Some(s) = self.world.block_state(x, y, z) {
            return crate::blocks_gen::block_name(s).contains("water");
        }
        false
    }

    pub(crate) fn hostile_proche(&self) -> bool {
        let Some(p) = self.pos else { return false };
        self.entities
            .plus_proche(p[0], p[1], p[2], crate::entities::HOSTILES, 4.5)
            .is_some()
    }
}

/// Exécute la décision : retourne true si une action a été prise (reboucler).
pub async fn executer(bot: &mut Bot, d: &Decision) -> bool {
    match d.action {
        Action::Rien => false,
        Action::Manger => bot.eat_if_needed().await,
        Action::Combattre => bot.self_defend().await,
        Action::SurvivreFuite => bot.fuite_panique().await,
        Action::EviterLave => bot.fuite_panique().await,
    }
}

impl Urgence {
    #[allow(dead_code)]
    pub fn nom(self) -> &'static str {
        match self {
            Urgence::Aucune => "aucune",
            Urgence::Repas => "repas",
            Urgence::FaimCritique => "faim",
            Urgence::Fuite => "fuite",
            Urgence::CombatMortel => "combat",
            Urgence::Chute => "chute",
            Urgence::Noyade => "noyade",
            Urgence::LaveOuFeu => "lave/feu",
            Urgence::Mourir => "mourir",
        }
    }
    #[allow(dead_code)]
    pub fn libre(poids: u32) -> bool {
        poids > Urgence::Fuite.poids()
    }
}
