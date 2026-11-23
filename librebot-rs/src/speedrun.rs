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
        Phase::Stronghold => phase_stronghold(bot, &mut sr.logger).await,
        Phase::End => phase_stub(sr).await,
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
                reason: "aucune forteresse détectée → on tente les blazes rencontrés en chemin"
                    .into(),
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

/// Contrat route.md : 12 yeux pour le portail de l'End.
const EYES_TARGET: i32 = 12;
/// Divergence max entre deux lancers d'yeux avant recalcul (route.md §1).
const MAX_EYE_DIVERGENCE_DEG: f64 = 5.0;

/// Phase 5 `Stronghold` (route.md §1) : contrat transverse (sortie du Nether avec
/// >= 8 perles, sinon plan B mini-yeux), craft des yeux, triangulation par
/// >= 2 lancers d'yeux de l'Ender (recalc si divergence > 5°). Split `stronghold_located`.
async fn phase_stronghold(bot: &mut Bot, logger: &mut RunLogger) -> Step {
    // 0. Contrat transverse Nether -> Overworld.
    if bot.dimension.as_deref() == Some("minecraft:the_nether") && !exit_nether(bot, logger).await {
        return Step::Retry;
    }
    if bot.dimension.as_deref() != Some("minecraft:overworld") {
        logger.event(RunEvent::Custom {
            kind: "stronghold_dim".into(),
            detail: format!("dimension={:?}", bot.dimension),
        });
        return Step::Retry;
    }
    // 1. Crafter les yeux (plan B mini-yeux : autant que perles + poudre dispo).
    craft_eyes(bot, logger).await;
    // 2. Triangulation : (point de lancer, direction) par œil lancé.
    let mut shots: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut stronghold: Option<[f64; 2]> = None;
    for _ in 0..4 {
        if bot.dead {
            return Step::Retry;
        }
        if bot.count_item("ender_eye") <= 0 || stronghold.is_some() {
            break;
        }
        let Some((p, v)) = throw_eye(bot, logger).await else {
            break;
        };
        shots.push((p, v));
        logger.event(RunEvent::Custom {
            kind: "eye_thrown".into(),
            detail: format!(
                "depuis ({:.0},{:.0}) dir ({:.2},{:.2})",
                p[0], p[1], v[0], v[1]
            ),
        });
        if shots.len() >= 2 {
            let (p1, v1) = shots[shots.len() - 2];
            let (p2, v2) = shots[shots.len() - 1];
            match triangulate(p1, v1, p2, v2) {
                Some(s) => {
                    // 3e lancer de vérification : divergence > 5° -> recalc (route.md).
                    if bot.count_item("ender_eye") > 0 {
                        if let Some((p3, v3)) = throw_eye(bot, logger).await {
                            shots.push((p3, v3));
                            let pred = [s[0] - p3[0], s[1] - p3[1]];
                            let ang = angle_deg(v3, pred);
                            if ang > MAX_EYE_DIVERGENCE_DEG {
                                logger.event(RunEvent::Custom {
                                    kind: "eye_divergence".into(),
                                    detail: format!(
                                        "{ang:.1}° > {MAX_EYE_DIVERGENCE_DEG}° → recalc"
                                    ),
                                });
                                stronghold = None;
                                continue;
                            }
                        }
                    }
                    stronghold = Some(s);
                }
                None => {
                    // Droites quasi parallèles : avancer de ~100 blocs le long de v1.
                    advance_along(
                        bot,
                        shots.last().map(|(_, v)| *v).unwrap_or([1.0, 0.0]),
                        100,
                    )
                    .await;
                }
            }
        } else {
            // Premier lancer : avancer de ~100 blocs dans la direction de l'œil.
            advance_along(bot, v, 100).await;
        }
    }
    let Some([sx, sz]) = stronghold else {
        logger.event(RunEvent::PlanB {
            phase: Phase::Stronghold.name(),
            reason: "triangulation impossible (yeux insuffisants ou parallèles)".into(),
        });
        return Step::Abort("stronghold: pas de triangulation".into());
    };
    logger.event(RunEvent::Custom {
        kind: "stronghold_triangulated".into(),
        detail: format!("({sx:.0},{sz:.0}) avec {} lancers", shots.len()),
    });
    // 3. Aller au stronghold.
    if let Some((gx, gy, gz)) = bot.find_walkable_near(sx as i32, sz as i32, 64) {
        bot.goto(gx, gy, gz).await;
        pump(bot, 1200).await; // ~60 s de trajet
    }
    if bot.dead {
        return Step::Retry;
    }
    logger.split("stronghold_located");
    Step::Next
}

/// Sortie du Nether (contrat transverse route.md) : trouver un portail, y aller,
/// attendre `minecraft:overworld`. Split `nether_exit` + event perles (PlanB
/// mini-yeux si < PEARLS_TARGET).
async fn exit_nether(bot: &mut Bot, logger: &mut RunLogger) -> bool {
    let mut portal = None;
    for tries in 0..4u32 {
        if let Some(p) = bot.find_block_any(&["nether_portal"], 48 + tries as i32 * 32) {
            portal = Some(p);
            break;
        }
        explore(bot, tries).await;
        if bot.dead {
            return false;
        }
    }
    let Some((px, py, pz)) = portal else {
        logger.event(RunEvent::PlanB {
            phase: Phase::Stronghold.name(),
            reason: "pas de portail de retour trouvé dans le Nether (nether_exit)".into(),
        });
        return false;
    };
    bot.goto(px, py, pz).await;
    for _ in 0..1200 {
        pump(bot, 1).await;
        if bot.dead {
            return false;
        }
        if bot.dimension.as_deref() == Some("minecraft:overworld") {
            let pearls = bot.count_item("ender_pearl");
            logger.event(RunEvent::Custom {
                kind: "nether_exit".into(),
                detail: format!("perles={pearls} yeux={}", bot.count_item("ender_eye")),
            });
            if pearls < PEARLS_TARGET {
                logger.event(RunEvent::PlanB {
                    phase: Phase::Stronghold.name(),
                    reason: format!("perles={pearls} < {PEARLS_TARGET} → plan B mini-yeux"),
                });
            }
            logger.split("nether_exit");
            return true;
        }
    }
    false
}

/// Crafte les yeux de l'Ender (poudre depuis les rods, puis perle + poudre).
async fn craft_eyes(bot: &mut Bot, logger: &mut RunLogger) {
    while bot.count_item("blaze_powder") < EYES_TARGET && bot.count_item("blaze_rod") > 0 {
        if !bot.craft("blaze_powder").await {
            break;
        }
    }
    let mut made = 0;
    while bot.count_item("ender_eye") + made < EYES_TARGET
        && bot.count_item("ender_pearl") > 0
        && bot.count_item("blaze_powder") > 0
    {
        if !bot.craft("ender_eye").await {
            break;
        }
        made += 1;
    }
    logger.event(RunEvent::Custom {
        kind: "eyes_crafted".into(),
        detail: format!(
            "yeux={} perles={} poudre={}",
            bot.count_item("ender_eye"),
            bot.count_item("ender_pearl"),
            bot.count_item("blaze_powder")
        ),
    });
}

/// Lance un œil de l'Ender et mesure sa direction via la trajectoire de
/// l'entité `eye_of_ender` (kind 39). Retourne (point de lancer, direction 2D normalisée).
async fn throw_eye(bot: &mut Bot, logger: &mut RunLogger) -> Option<([f64; 2], [f64; 2])> {
    let eye = crate::recipes::item_id("ender_eye");
    if !bot.select_hotbar_by_item(eye).await {
        logger.event(RunEvent::Custom {
            kind: "eye_missing".into(),
            detail: "pas d'œil de l'Ender en main".into(),
        });
        return None;
    }
    let p0 = bot.pos?;
    if bot.use_item().await.is_err() {
        return None;
    }
    // Attendre le spawn de l'entité œil.
    let mut eye_id = None;
    for _ in 0..40 {
        pump(bot, 1).await;
        if bot.dead {
            return None;
        }
        if eye_id.is_none() {
            eye_id = bot
                .entities
                .nearest_kind(entities::EYE_OF_ENDER_ENTITY_TYPE, p0, 8.0)
                .map(|(id, _)| id);
        }
    }
    let id = eye_id?;
    let start = bot.entities.get(id)?.pos;
    // Suivre la trajectoire ~1,5 s.
    for _ in 0..30 {
        pump(bot, 1).await;
        if bot.entities.get(id).is_none() {
            break;
        }
    }
    let end = bot.entities.get(id)?.pos;
    let d = [end[0] - start[0], end[2] - start[2]];
    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if n < 0.5 {
        return None;
    }
    Some(([p0[0], p0[2]], [d[0] / n, d[1] / n]))
}

/// Avance d'environ `dist` blocs le long de la direction 2D `v` (déterministe).
async fn advance_along(bot: &mut Bot, v: [f64; 2], dist: i32) {
    if let Some(p) = bot.pos {
        let tx = (p[0] + v[0] * dist as f64) as i32;
        let tz = (p[2] + v[1] * dist as f64) as i32;
        if let Some((gx, gy, gz)) = bot.find_walkable_near(tx, tz, p[1] as i32) {
            bot.goto(gx, gy, gz).await;
            pump(bot, 600).await;
        }
    }
}

/// Intersection de deux demi-droites 2D (p1 + t*v1) et (p2 + s*v2), t >= 0.
/// None si quasi parallèles ou intersection derrière le premier lancer.
fn triangulate(p1: [f64; 2], v1: [f64; 2], p2: [f64; 2], v2: [f64; 2]) -> Option<[f64; 2]> {
    let det = v1[0] * v2[1] - v1[1] * v2[0];
    if det.abs() < 1e-6 {
        return None;
    }
    let t = ((p2[0] - p1[0]) * v2[1] - (p2[1] - p1[1]) * v2[0]) / det;
    if t < 0.0 {
        return None;
    }
    Some([p1[0] + v1[0] * t, p1[1] + v1[1] * t])
}

/// Angle en degrés entre deux vecteurs 2D (non normalisés acceptés).
fn angle_deg(a: [f64; 2], b: [f64; 2]) -> f64 {
    let na = (a[0] * a[0] + a[1] * a[1]).sqrt();
    let nb = (b[0] * b[0] + b[1] * b[1]).sqrt();
    if na < 1e-9 || nb < 1e-9 {
        return 0.0;
    }
    let cos = (a[0] * b[0] + a[1] * b[1]) / (na * nb);
    cos.clamp(-1.0, 1.0).acos().to_degrees()
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
    fn triangulation_intersection_et_divergence() {
        // Deux lancers perpendiculaires : intersection en (100, 0) depuis (0,0) dir +x
        // et depuis (100,-50) dir +z.
        let s = triangulate([0.0, 0.0], [1.0, 0.0], [100.0, -50.0], [0.0, 1.0]);
        assert_eq!(s, Some([100.0, 0.0]));
        // Parallèles -> None
        assert_eq!(
            triangulate([0.0, 0.0], [1.0, 0.0], [0.0, 5.0], [1.0, 0.0]),
            None
        );
        // Intersection derrière le lancer -> None
        assert_eq!(
            triangulate([0.0, 0.0], [1.0, 0.0], [-50.0, 1.0], [0.0, 1.0]),
            None
        );
        // Angle : 0° identique, 90° perpendiculaire, 180° opposé
        assert!(angle_deg([1.0, 0.0], [2.0, 0.0]) < 1e-9);
        assert!((angle_deg([1.0, 0.0], [0.0, 1.0]) - 90.0).abs() < 1e-9);
        assert!((angle_deg([1.0, 0.0], [-1.0, 0.0]) - 180.0).abs() < 1e-9);
        // Divergence de 3° < seuil 5° ; 10° > seuil
        let v = [1.0f64, 0.0];
        let v3 = [3.0f64.mul_add(1.0, 0.0), 3.0f64.to_radians().sin()]; // ~3°
        assert!(angle_deg(v, v3) < MAX_EYE_DIVERGENCE_DEG);
        let v10 = [10.0f64.to_radians().cos(), 10.0f64.to_radians().sin()];
        assert!(angle_deg(v, v10) > MAX_EYE_DIVERGENCE_DEG);
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
