//! Machine à états du speedrun RSG 1.21.1 (contrat : `route.md`).
//! Transitions explicites (`Phase` + `match`), RunLogger central, aucun panic :
//! une phase en incident rend un `Step` (Retry / PlanB / Abort), jamais un unwrap.

use crate::bot::Bot;
use crate::entities;
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
        Phase::NetherEntry => phase_nether_entry(bot, &mut sr.logger).await,
        Phase::Bastion => phase_bastion(bot, &mut sr.logger).await,
        Phase::Fortress => phase_fortress(bot, &mut sr.logger).await,
        Phase::Stronghold | Phase::End => phase_stub(sr).await,
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

/// Phase 2 `NetherEntry` (route.md §1) : trouver un portail ruiné (`nether_portal`)
/// dans les chunks chargés, s'y rendre, attendre le changement de dimension
/// (`minecraft:the_nether`, lu via le paquet respawn). Split `nether_entry`.
/// Exploration déterministe (aucun aléatoire). Plan B (portail à construire) :
/// itération suivante, ici Abort propre si aucun portail à portée.
async fn phase_nether_entry(bot: &mut Bot, logger: &mut RunLogger) -> Step {
    let mut portal = None;
    let mut radius = 32;
    for tries in 0..4u32 {
        if let Some(p) = bot.find_block_any(&["nether_portal"], radius) {
            portal = Some(p);
            break;
        }
        explore(bot, tries).await;
        if bot.dead {
            return Step::Retry;
        }
        radius += 32;
    }
    let Some((px, py, pz)) = portal else {
        logger.event(RunEvent::PlanB {
            phase: Phase::NetherEntry.name(),
            reason: "aucun portail ruiné à portée — construction de portail : itération suivante"
                .into(),
        });
        return Step::Abort("nether_entry: pas de portail ruiné trouvé".into());
    };
    logger.event(RunEvent::Custom {
        kind: "portal_found".into(),
        detail: format!("{px},{py},{pz}"),
    });
    bot.goto(px, py, pz).await;
    // Attendre l'arrivée puis le changement de dimension (max ~60 s).
    for _ in 0..1200 {
        pump(bot, 1).await;
        if bot.dead {
            return Step::Retry;
        }
        if bot.dimension.as_deref() == Some("minecraft:the_nether") {
            logger.split("nether_entry");
            return Step::Next;
        }
    }
    logger.event(RunEvent::Custom {
        kind: "nether_entry_timeout".into(),
        detail: format!("dimension={:?}", bot.dimension),
    });
    Step::Retry
}

/// Exploration déterministe (aucun aléatoire) pour charger de nouveaux chunks :
/// déplacement de 40 blocs selon un angle fixé par `tries`.
async fn explore(bot: &mut Bot, tries: u32) {
    if let Some(p) = bot.pos {
        let angle = (tries as f64) * 2.4;
        let tx = (p[0] + angle.cos() * 40.0) as i32;
        let tz = (p[2] + angle.sin() * 40.0) as i32;
        let y = p[1] as i32;
        if let Some((gx, gy, gz)) = bot.find_walkable_near(tx, tz, y) {
            bot.goto(gx, gy, gz).await;
            pump(bot, 400).await; // ~20 s de trajet
        }
    }
}

/// Blocs marqueurs d'un bastion (route.md §1).
const BASTION_MARKERS: &[&str] = &["gilded_blackstone", "blackstone"];
/// Blocs conteneurs lootables dans un bastion.
const CHEST_BLOCKS: &[&str] = &["chest", "barrel"];
/// Contrat route.md : sortie du Nether avec >= 8 perles, sinon plan B mini-yeux.
const PEARLS_TARGET: i32 = 8;
const MAX_CHESTS: u32 = 12;
const MAX_BARTERS: u32 = 40;

/// Règle d'abandon d'un bastion pauvre (route.md §1) : 0 perle après avoir vidé
/// les coffres centraux (>= 3) ou tenté >= 10 barters.
fn bastion_verdict(pearls: i32, chests_looted: u32, barters: u32) -> bool {
    pearls <= 0 && (chests_looted >= 3 || barters >= 10)
}

/// Phase 3 `Bastion` (route.md §1) : localiser un bastion (gilded_blackstone),
/// vider ses coffres, barter les piglins pour les perles. Split `bastion_done`.
/// Réactions adaptatives : abandon d'un bastion pauvre (PlanB), arrêt anticipé
/// dès que PEARLS_TARGET est atteint.
async fn phase_bastion(bot: &mut Bot, logger: &mut RunLogger) -> Step {
    if bot.dimension.as_deref() != Some("minecraft:the_nether") {
        logger.event(RunEvent::Custom {
            kind: "bastion_dim".into(),
            detail: format!("dimension={:?}", bot.dimension),
        });
        return Step::Retry;
    }
    // 1. Localiser un bastion.
    let mut spot = None;
    for tries in 0..4u32 {
        if let Some(p) = bot.find_block_any(BASTION_MARKERS, 48 + tries as i32 * 32) {
            spot = Some(p);
            break;
        }
        explore(bot, tries).await;
        if bot.dead {
            return Step::Retry;
        }
    }
    let Some((bx, by, bz)) = spot else {
        logger.event(RunEvent::PlanB {
            phase: Phase::Bastion.name(),
            reason: "aucun bastion détecté → forteresse directement".into(),
        });
        logger.split("bastion_done");
        return Step::Next;
    };
    logger.event(RunEvent::Custom {
        kind: "bastion_found".into(),
        detail: format!("{bx},{by},{bz}"),
    });
    if let Some((gx, gy, gz)) = bot.find_walkable_near(bx, bz, by) {
        bot.goto(gx, gy, gz).await;
        pump(bot, 600).await;
    }
    // 2. Loot des coffres.
    let mut chests_looted = 0u32;
    for _ in 0..MAX_CHESTS {
        if bot.dead {
            return Step::Retry;
        }
        if bot.count_item("ender_pearl") >= PEARLS_TARGET {
            break;
        }
        let Some((cx, cy, cz)) = bot.find_block_any(CHEST_BLOCKS, 24) else {
            break;
        };
        if loot_chest(bot, cx, cy, cz).await {
            chests_looted += 1;
        }
        if bastion_verdict(bot.count_item("ender_pearl"), chests_looted, 0) {
            logger.event(RunEvent::PlanB {
                phase: Phase::Bastion.name(),
                reason: format!(
                    "bastion pauvre : {chests_looted} coffres vidés, 0 perle → on l'abandonne"
                ),
            });
            break;
        }
    }
    // 3. Barter avec les piglins (or en main + interact).
    let mut barters = 0u32;
    while barters < MAX_BARTERS && bot.count_item("ender_pearl") < PEARLS_TARGET {
        if bot.dead {
            return Step::Retry;
        }
        if bot.count_item("gold_ingot") <= 0 {
            break;
        }
        let Some(p) = bot.pos else { break };
        let Some((pid, _)) = bot
            .entities
            .nearest_kind(entities::PIGLIN_ENTITY_TYPE, p, 24.0)
        else {
            break;
        };
        let before = bot.count_item("ender_pearl");
        barter_once(bot, pid).await;
        let gained = (bot.count_item("ender_pearl") - before).max(0) as u32;
        barters += 1;
        logger.event(RunEvent::Barter { pearls: gained });
        if bastion_verdict(bot.count_item("ender_pearl"), chests_looted, barters) {
            break;
        }
    }
    let pearls = bot.count_item("ender_pearl");
    logger.event(RunEvent::Custom {
        kind: "bastion_loot".into(),
        detail: format!("perles={pearls} coffres={chests_looted} barters={barters}"),
    });
    if pearls < PEARLS_TARGET {
        logger.event(RunEvent::PlanB {
            phase: Phase::Bastion.name(),
            reason: format!(
                "perles={pearls} < {PEARLS_TARGET} → plan B mini-yeux à la sortie du Nether"
            ),
        });
    }
    logger.split("bastion_done");
    Step::Next
}

/// Ouvre un coffre/baril et vide son contenu dans l'inventaire. Retourne true
/// si la fenêtre a été ouverte.
async fn loot_chest(bot: &mut Bot, x: i32, y: i32, z: i32) -> bool {
    // Se rapprocher à portée d'interaction (~4,5 blocs).
    if let Some(p) = bot.pos {
        let d = ((p[0] - (x as f64 + 0.5)).powi(2) + (p[2] - (z as f64 + 0.5)).powi(2)).sqrt();
        if d > 4.0 {
            if let Some((gx, gy, gz)) = bot.find_walkable_near(x, z, y) {
                bot.goto(gx, gy, gz).await;
                for _ in 0..400 {
                    if bot.path_len() == 0 {
                        break;
                    }
                    pump(bot, 1).await;
                }
            }
        }
    }
    // use_item_on (BLOCK_PLACE) sur le dessus du coffre, comme pour la table.
    if bot.place(x, y, z, 1).await.is_err() {
        return false;
    }
    let mut opened = false;
    for _ in 0..100 {
        pump(bot, 1).await;
        if bot.open_window_id.is_some() {
            opened = true;
            break;
        }
        if bot.dead {
            return false;
        }
    }
    if !opened {
        return false;
    }
    let Some((win, n)) = bot.inventory.container_slots() else {
        return false;
    };
    // Vider le conteneur : pickup de chaque slot, pose dans un slot joueur libre.
    // Mapping fenêtre conteneur : slots externes n..n+36 = joueur (internal 9..=45).
    for e in 0..n.min(54) {
        if bot.dead {
            break;
        }
        let carried = bot.inventory.carried.as_ref().map(|s| (s.id, s.count));
        if bot
            .click_window_carried(win, e as i16, 0, 0, carried)
            .await
            .is_err()
        {
            break;
        }
        for _ in 0..6 {
            pump(bot, 1).await;
        }
        if let Some(c) = bot.inventory.carried.clone() {
            let free = (9..=44).find(|&f| {
                bot.inventory
                    .slots
                    .get(f)
                    .map(|s| s.is_none())
                    .unwrap_or(true)
            });
            if let Some(f) = free {
                let ext = (f - 9 + n) as i16;
                let _ = bot
                    .click_window_carried(win, ext, 0, 0, Some((c.id, c.count)))
                    .await;
                for _ in 0..6 {
                    pump(bot, 1).await;
                }
            }
        }
    }
    let _ = bot.close_container().await;
    true
}

/// Un barter : approcher le piglin, mettre un lingot d'or en main, interact,
/// puis ramasser les drops.
async fn barter_once(bot: &mut Bot, piglin: i32) {
    if let (Some(p), Some(e)) = (bot.pos, bot.entities.get(piglin)) {
        let ep = e.pos;
        let d = ((p[0] - ep[0]).powi(2) + (p[2] - ep[2]).powi(2)).sqrt();
        if d > 3.5 {
            let (tx, tz) = (ep[0] as i32, ep[2] as i32);
            if let Some((gx, gy, gz)) = bot.find_walkable_near(tx, tz, ep[1] as i32) {
                bot.goto(gx, gy, gz).await;
                for _ in 0..300 {
                    if bot.path_len() == 0 {
                        break;
                    }
                    pump(bot, 1).await;
                    if bot.dead {
                        return;
                    }
                }
            }
        }
    }
    let gold = crate::recipes::item_id("gold_ingot");
    let _ = bot.select_hotbar_by_item(gold).await;
    if bot.interact_entity(piglin, false).await.is_err() {
        return;
    }
    // Attendre le résultat du barter puis ramasser les drops.
    for _ in 0..60 {
        pump(bot, 1).await;
        if bot.dead {
            return;
        }
    }
    let _ = bot.collect_drops(6.0).await;
}

/// Blocs marqueurs d'une forteresse du Nether.
const FORTRESS_MARKERS: &[&str] = &["nether_bricks", "nether_brick_fence", "nether_brick_stairs"];
/// Contrat route.md : 12 yeux = 12 poudres de blaze = 6 bâtons minimum (objectif 10+).
const RODS_MIN: i32 = 6;
const RODS_TARGET: i32 = 10;

/// Phase 4 `Fortress` (route.md §1) : localiser une forteresse (nether_bricks),
/// tuer des blazes (attack 0x16) et ramasser les blaze rods. Split `fortress_done`.
/// Incidents : ghast proche → retraite (pas d'arc), mort → Retry (checkpoint).
async fn phase_fortress(bot: &mut Bot, logger: &mut RunLogger) -> Step {
    if bot.dimension.as_deref() != Some("minecraft:the_nether") {
        return Step::Retry;
    }
    // 1. Localiser une forteresse.
    let mut spot = None;
    for tries in 0..4u32 {
        if let Some(p) = bot.find_block_any(FORTRESS_MARKERS, 48 + tries as i32 * 32) {
            spot = Some(p);
            break;
        }
        explore(bot, tries).await;
        if bot.dead {
            return Step::Retry;
        }
    }
    match spot {
        Some((fx, fy, fz)) => {
            logger.event(RunEvent::Custom {
                kind: "fortress_found".into(),
                detail: format!("{fx},{fy},{fz}"),
            });
            if let Some((gx, gy, gz)) = bot.find_walkable_near(fx, fz, fy) {
                bot.goto(gx, gy, gz).await;
                pump(bot, 600).await;
            }
        }
        None => {
            logger.event(RunEvent::PlanB {
                phase: Phase::Fortress.name(),
                reason: "aucune forteresse détectée → on tente les blazes rencontrés en chemin".into(),
            });
        }
    }
    // 2. Tuer des blazes jusqu'à RODS_TARGET (ou plus de blaze à portée).
    select_best_sword(bot).await;
    for _ in 0..40 {
        if bot.dead {
            return Step::Retry;
        }
        if bot.count_item("blaze_rod") >= RODS_TARGET {
            break;
        }
        // Incident ghast : retraite si un ghast est proche.
        if let Some(p) = bot.pos {
            if let Some((_, gpos)) = bot
                .entities
                .nearest_kind(entities::GHAST_ENTITY_TYPE, p, 14.0)
            {
                logger.event(RunEvent::Custom {
                    kind: "ghast_retreat".into(),
                    detail: format!("ghast à {:.1} blocs", dist(p, gpos)),
                });
                let (tx, tz) = (p[0] - (gpos[0] - p[0]) * 2.0, p[2] - (gpos[2] - p[2]) * 2.0);
                if let Some((gx, gy, gz)) =
                    bot.find_walkable_near(tx as i32, tz as i32, p[1] as i32)
                {
                    bot.goto(gx, gy, gz).await;
                    pump(bot, 200).await;
                }
                continue;
            }
            let Some((bid, _)) = bot
                .entities
                .nearest_kind(entities::BLAZE_ENTITY_TYPE, p, 32.0)
            else {
                break;
            };
            kill_blaze(bot, bid).await;
            let _ = bot.collect_drops(6.0).await;
        } else {
            break;
        }
    }
    let rods = bot.count_item("blaze_rod");
    logger.event(RunEvent::Custom {
        kind: "fortress_loot".into(),
        detail: format!("blaze_rods={rods}"),
    });
    if rods < RODS_MIN {
        logger.event(RunEvent::PlanB {
            phase: Phase::Fortress.name(),
            reason: format!("rods={rods} < {RODS_MIN} → yeux partiels, on continue"),
        });
    }
    logger.split("fortress_done");
    Step::Next
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Sélectionne la meilleure épée disponible (iron > stone > golden > wooden > diamond
/// approché par priorité explicite), sinon la hache.
async fn select_best_sword(bot: &mut Bot) {
    for name in [
        "diamond_sword",
        "iron_sword",
        "stone_sword",
        "golden_sword",
        "wooden_sword",
    ] {
        if bot.count_item(name) > 0
            && bot
                .select_hotbar_by_item(crate::recipes::item_id(name))
                .await
        {
            return;
        }
    }
    for name in ["diamond_axe", "iron_axe", "stone_axe", "wooden_axe"] {
        if bot.count_item(name) > 0
            && bot
                .select_hotbar_by_item(crate::recipes::item_id(name))
                .await
        {
            return;
        }
    }
}

/// Attaque un blaze jusqu'à sa disparition (remove_entities) ou 15 coups.
async fn kill_blaze(bot: &mut Bot, blaze: i32) {
    for _ in 0..15 {
        if bot.dead || bot.entities.get(blaze).is_none() {
            return;
        }
        // Approche à portée de mêlée (~3 blocs).
        if let (Some(p), Some(e)) = (bot.pos, bot.entities.get(blaze)) {
            if dist(p, e.pos) > 3.0 {
                let (tx, tz) = (e.pos[0] as i32, e.pos[2] as i32);
                if let Some((gx, gy, gz)) = bot.find_walkable_near(tx, tz, e.pos[1] as i32) {
                    bot.goto(gx, gy, gz).await;
                }
                for _ in 0..120 {
                    if bot.path_len() == 0 || bot.entities.get(blaze).is_none() {
                        break;
                    }
                    pump(bot, 1).await;
                    if bot.dead {
                        return;
                    }
                }
            }
        }
        if bot.interact_entity(blaze, true).await.is_err() {
            return;
        }
        // ~0,6 s entre deux coups.
        for _ in 0..12 {
            pump(bot, 1).await;
            if bot.dead || bot.entities.get(blaze).is_none() {
                return;
            }
        }
    }
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
    fn regle_abandon_bastion_pauvre() {
        // 0 perle après 3+ coffres vidés -> abandon
        assert!(bastion_verdict(0, 3, 0));
        // 0 perle après 10+ barters -> abandon
        assert!(bastion_verdict(0, 0, 10));
        // des perles obtenues -> on garde le bastion
        assert!(!bastion_verdict(2, 5, 0));
        // peu de coffres vidés et 0 perle -> on continue à looter
        assert!(!bastion_verdict(0, 1, 0));
        assert!(!bastion_verdict(0, 0, 3));
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
