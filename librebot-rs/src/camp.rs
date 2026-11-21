//! Campement évolutif du bot : lit, coffres, dormance nocturne et stockage du surplus.
//! Le camp est établi une fois (position persistée dans `camp.json`), puis amélioré
//! par paliers idempotents à mesure que les ressources s'accumulent.

use crate::bot::Bot;
use crate::entities;
use crate::inventory::internal_to_window_slot;
use crate::recipes::item_id;

const CAMP_FILE: &str = "camp.json";

/// Fenêtre de nuit vanilla (ticks du jour) : 12542..23459.
pub const NUIT_DEBUT: i64 = 12542;
pub const NUIT_FIN: i64 = 23459;

/// Noms de blocs des lits (toutes les couleurs vanilla).
const LITS: &[&str] = &[
    "white_bed", "orange_bed", "magenta_bed", "light_blue_bed",
    "yellow_bed", "lime_bed", "pink_bed", "gray_bed",
    "light_gray_bed", "cyan_bed", "purple_bed", "blue_bed",
    "brown_bed", "green_bed", "red_bed", "black_bed",
];

/// Quantités au-delà desquelles le surplus part au coffre.
const CAPS_SURPLUS: &[(&str, i32)] = &[
    ("cobblestone", 32),
    ("oak_log", 16),
    ("oak_planks", 24),
    ("dirt", 32),
    ("iron_ingot", 24),
    ("coal", 24),
];

/// État du campement, persisté dans `camp.json`.
#[derive(Debug, Clone)]
pub struct Camp {
    pub centre: (i32, i32, i32),
    pub lit: bool,
    pub coffres: i32,
    pub _palier_sol: bool,
}

impl Camp {
    fn to_json(&self) -> String {
        format!(
            "{{\"cx\":{},\"cy\":{},\"cz\":{},\"lit\":{},\"coffres\":{}}}",
            self.centre.0, self.centre.1, self.centre.2, self.lit, self.coffres
        )
    }

    pub fn save(&self) {
        let _ = std::fs::write(CAMP_FILE, self.to_json());
    }

    pub fn load() -> Option<Camp> {
        let raw = std::fs::read_to_string(CAMP_FILE).ok()?;
        let n = |k: &str| -> Option<i32> {
            let pat = format!("\"{}\"", k);
            let i = raw.find(&pat)? + pat.len();
            let rest = raw[i..].trim_start().strip_prefix(':')?.trim_start();
            let end = rest
                .find(|c: char| c == ',' || c == '}')
                .unwrap_or(rest.len());
            rest[..end].trim().parse().ok()
        };
        let b = |k: &str| -> bool {
            let pat = format!("\"{}\"", k);
            let i = match raw.find(&pat) {
                Some(i) => i + pat.len(),
                None => return false,
            };
            let rest = raw[i..].trim_start().strip_prefix(':').unwrap_or("").trim_start();
            rest.starts_with("true")
        };
        Some(Camp {
            centre: (n("cx")?, n("cy")?, n("cz")?),
            lit: b("lit"),
            coffres: n("coffres").unwrap_or(0),
            _palier_sol: false,
        })
    }
}

impl Bot {
    /// Établit le campement à la position actuelle du bot et le persiste.
    pub async fn ensure_camp(&mut self) -> bool {
        if self.camp.is_some() {
            return true;
        }
        let Some(p) = self.pos else { return false };
        let centre = (
            p[0].floor() as i32,
            p[1].floor() as i32,
            p[2].floor() as i32,
        );
        let camp = Camp { centre, lit: false, coffres: 0, _palier_sol: false };
        println!("campement établi en {:?}", centre);
        camp.save();
        self.camp = Some(camp);
        true
    }

    /// Paliers d'aménagement idempotents, dans l'ordre des ressources.
    pub async fn camp_expand(&mut self) {
        let Some(camp) = self.camp.clone() else { return };
        let (cx, cy, cz) = camp.centre;

        // Palier 1 : le lit (laine 3 + planches 3)
        if !camp.lit {
            if self.count_item("white_wool") < 3 || self.count_item_any(&["oak_planks", "spruce_planks"]) < 3 {
                self.hunt_wool(3).await;
            }
            if self.count_item("white_wool") >= 3
                && self.count_item_any(&["oak_planks", "spruce_planks"]) >= 3
            {
                if self.craft_avec_table("white_bed").await {
                    if self.camp_place_item("white_bed", cx + 1, cy, cz + 1).await {
                        if let Some(c) = self.camp.as_mut() {
                            c.lit = true;
                            c.save();
                        }
                        println!("campement : lit posé");
                    }
                }
            }
            return;
        }

        // Palier 2 : coffre de stockage (8 planches)
        if camp.coffres == 0 && self.count_item_any(&["oak_planks", "spruce_planks"]) >= 8 {
            if self.craft_avec_table("chest").await
                && self.camp_place_item("chest", cx - 1, cy, cz - 1).await
            {
                if let Some(c) = self.camp.as_mut() {
                    c.coffres = 1;
                    c.save();
                }
                println!("campement : coffre posé");
            }
        }
    }

    /// Ouvre (ou pose) une table de craft, crafte l'item, puis referme la fenêtre.
    async fn craft_avec_table(&mut self, nom: &str) -> bool {
        if self.open_window_id.is_none() {
            if self.count_item("crafting_table") == 0 && !self.craft("crafting_table").await {
                return false;
            }
            if !self.place_and_open_crafting_table().await {
                return false;
            }
        }
        let ok = self.craft(nom).await;
        self.close_window().await.ok();
        ok
    }

    /// Sélectionne un item dans la hotbar, s'approche et le pose sur `face haut`.
    async fn camp_place_item(&mut self, item: &str, x: i32, y: i32, z: i32) -> bool {
        if !self.select_hotbar_by_item(item_id(item)).await {
            eprintln!("campement : pas de {} en hotbar", item);
            return false;
        }
        if !self.approach_block(x, y, z).await {
            eprintln!("campement : impossible d'approcher ({}, {}, {})", x, y, z);
            return false;
        }
        if self.place(x, y - 1, z, 1).await.is_err() {
            return false;
        }
        self.drain_packets().await;
        self.tick(0.3).await.ok();
        true
    }

    /// Chasse des moutons jusqu'à récolter `nb` laines blanches.
    pub async fn hunt_wool(&mut self, nb: i32) -> bool {
        for arme in ["iron_sword", "stone_sword", "wooden_sword"] {
            if self.inventory.find(item_id(arme)).is_some() {
                self.select_hotbar_by_item(item_id(arme)).await;
                break;
            }
        }
        let mut essais = 0;
        while self.count_item("white_wool") < nb && essais < 10 && !self.disconnected {
            essais += 1;
            let Some(p) = self.pos else { return false };
            let Some((id, cible)) = self
                .entities
                .plus_proche(p[0], p[1], p[2], &[entities::SHEEP], 32.0)
            else {
                return false;
            };
            eprintln!(
                "chasse-laine : mouton à {:.0} blocs",
                (cible.x - p[0]).hypot(cible.z - p[2])
            );
            let mut coups = 0;
            while !self.disconnected && coups < 25 {
                let Some(np) = self.pos else { return false };
                let (ex, ey, ez) = match self.entities.alive.get(&id) {
                    Some(e) => (e.x, e.y, e.z),
                    None => break,
                };
                let d = (ex - np[0]).hypot(ez - np[2]);
                let d3 = ((ex - np[0]).powi(2)
                    + (ey - (np[1] + 1.0)).powi(2)
                    + (ez - np[2]).powi(2))
                .sqrt();
                if d3 <= 3.0 {
                    let dx = ex - np[0];
                    let dz = ez - np[2];
                    self.yaw = (-dx).atan2(dz).to_degrees() as f32;
                    self.pitch = ((ey + 0.5) - (np[1] + 1.62)).atan2(dx.hypot(dz)) as f32;
                    if self.attack_entity(id).await.is_err() {
                        break;
                    }
                    coups += 1;
                    for _ in 0..12 {
                        self.drain_packets().await;
                        self.tick(0.07).await.ok();
                    }
                } else if d > 2.5 {
                    let pas = d.min(12.0);
                    let dirx = (ex - np[0]) / d;
                    let dirz = (ez - np[2]) / d;
                    let tx = (np[0] + dirx * pas).floor() as i32;
                    let tz = (np[2] + dirz * pas).floor() as i32;
                    match self.find_walkable_near(tx, np[1].floor() as i32, tz) {
                        Some((gx, gy, gz)) => {
                            self.goto(gx, gy, gz).await;
                            for _ in 0..150 {
                                if self.path_len() == 0 {
                                    break;
                                }
                                self.drain_packets().await;
                                self.tick(0.05).await.ok();
                            }
                        }
                        None => break,
                    }
                } else {
                    break;
                }
                if !self.entities.alive.contains_key(&id) {
                    break;
                }
            }
            self.pickup_drops(12.0).await;
        }
        self.count_item("white_wool") >= nb
    }

    /// La nuit, retourne au campement et dort jusqu'au matin.
    pub async fn sleep_if_night(&mut self) -> bool {
        if self.disconnected || !(self.day_time >= NUIT_DEBUT && self.day_time < NUIT_FIN) {
            return false;
        }
        let Some(camp) = self.camp.clone() else { return false };
        if !camp.lit {
            return false;
        }
        let (cx, cy, cz) = camp.centre;
        // Retourner d'abord près du camp, puis chercher le lit
        if let Some((gx, gy, gz)) = self.find_walkable_near(cx, cy, cz) {
            self.goto(gx, gy, gz).await;
            for _ in 0..200 {
                if self.disconnected || self.path_len() == 0 {
                    break;
                }
                self.drain_packets().await;
                self.tick(0.05).await.ok();
            }
        }
        let Some((bx, by, bz)) = self.find_block_any(LITS, 12) else {
            return false;
        };
        eprintln!("nuit ({}) : dodo au lit ({}, {}, {})", self.day_time, bx, by, bz);
        if !self.approach_block(bx, by, bz).await {
            return false;
        }
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        crate::protocol::write_varint(0, &mut b);
        crate::protocol::write_position(bx, by, bz, &mut b);
        crate::protocol::write_varint(1, &mut b);
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.push(0);
        crate::protocol::write_varint(seq, &mut b);
        let _ = self.conn.send(crate::bot::ids::play_s::BLOCK_PLACE, &b).await;
        // Le serveur fait sauter la nuit : attendre le matin (au pire ~10 min réelles).
        let debut = std::time::Instant::now();
        while self.day_time >= NUIT_DEBUT && self.day_time < NUIT_FIN && !self.disconnected {
            if debut.elapsed() > std::time::Duration::from_secs(600) {
                break;
            }
            self.drain_packets().await;
            self.tick(0.25).await.ok();
        }
        true
    }

    /// Quand l'inventaire déborde, range le surplus dans un coffre du camp.
    pub async fn deposit_surplus_if_full(&mut self, seuil: usize) -> bool {
        if self.disconnected {
            return false;
        }
        let occupes = self.inventory.slots.iter().filter(|s| s.is_some()).count();
        if occupes < seuil {
            return false;
        }
        let Some((chx, chy, chz)) = self.find_block("chest", 16) else {
            return false;
        };
        if !self.approach_block(chx, chy, chz).await {
            return false;
        }
        let Some(wid) = self.interact_open(chx, chy, chz).await else {
            return false;
        };
        let window_len = self.inventory.container_slots.len() + 37;
        let n_conteneur = (self.inventory.container_slots.len() as i16).min(27);
        let mut ranges = 0;
        for (nom, cap) in CAPS_SURPLUS {
            let iid = item_id(nom);
            while self.inventory.count(iid) > *cap {
                let Some(vid) = (0..n_conteneur).find(|&s| {
                    self.inventory
                        .container_slots
                        .get(s as usize)
                        .map(|slot| slot.is_none())
                        .unwrap_or(false)
                }) else {
                    break;
                };
                let Some(src) = self.inventory.find(iid) else { break };
                let Some(wsrc) = internal_to_window_slot(src, window_len) else { break };
                if self.click_slot_in(wid, wsrc).await.is_err() {
                    return false;
                }
                if self.click_slot_in(wid, vid).await.is_err() {
                    return false;
                }
                self.drain_packets().await;
                ranges += 1;
                if ranges > 20 {
                    break;
                }
            }
        }
        self.close_window().await.ok();
        ranges > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recette_lit_disponible() {
        assert!(crate::recipes::recipe_for("white_bed").is_some());
        assert!(crate::recipes::recipe_for("chest").is_some());
    }

    #[test]
    fn bornes_de_nuit() {
        assert!(NUIT_DEBUT < NUIT_FIN && NUIT_FIN <= 24000);
    }
}
