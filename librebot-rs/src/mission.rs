//! Mission survie : le bot joue en autonomie (bois → pierre → fer → diamant → Nether → End → dragon).
//! Implémenté comme des méthodes sur `Bot` via ce module.

use crate::bot::Bot;
use crate::bot::ids;
use crate::inventory::Inventory;
use crate::protocol::{write_position, write_varint, write_string};
use crate::recipes::{item_id, recipe_for};
use crate::items_gen::item_name;

/// Convertit un index interne d'inventaire en slot de la fenêtre `window_id`
/// (identique pour la fenêtre 0 ; décalé pour un conteneur type table 3x3).
fn wslot(inv: &Inventory, window_id: u8, internal: usize) -> i16 {
    inv.window_slot(window_id, internal).unwrap_or(internal as i16)
}

impl Bot {
    /// Cherche un bloc par nom dans les chunks chargés, dans un rayon autour du bot.
    /// Retourne (x, y, z) du bloc le plus proche.
    pub fn find_block(&self, block_name: &str, radius: i32, skip: &[(i32, i32, i32)]) -> Option<(i32, i32, i32)> {
        let p = self.pos?;
        let (bx, by, bz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        self.world.find_block_named(bx, by, bz, radius, block_name, skip)
    }

    /// Cherche plusieurs noms de blocs (ex: tous les types de troncs).
    pub fn find_block_any(&self, names: &[&str], radius: i32, skip: &[(i32, i32, i32)]) -> Option<(i32, i32, i32)> {
        for name in names {
            if let Some(pos) = self.find_block(name, radius, skip) {
                return Some(pos);
            }
        }
        // Essai avec un rayon plus grand
        for name in names {
            if let Some(pos) = self.find_block(name, radius * 3, skip) {
                return Some(pos);
            }
        }
        None
    }

    /// Suit le chemin en cours jusqu'à épuisement, avec une borne proportionnelle
    /// à sa longueur (~6 ticks par waypoint + marge) — évite d'abandonner trop tôt
    /// sur les longues distances.
    pub async fn follow_path(&mut self, extra_ticks: usize) {
        let bound = self.path_len().saturating_mul(6) + extra_ticks;
        for _ in 0..bound {
            if self.path_len() == 0 { break; }
            self.drain_packets().await;
            if self.tick(0.05).await.is_err() { break; }
        }
    }

    /// Va vers un bloc et le mine. Retourne true si miné.
    pub async fn goto_and_dig(&mut self, x: i32, y: i32, z: i32) -> bool {
        // Se rapprocher si le bloc est loin
        let dist = self.pos.map(|p| {
            let dx = p[0] - (x as f64 + 0.5);
            let dz = p[2] - (z as f64 + 0.5);
            (dx * dx + dz * dz).sqrt()
        }).unwrap_or(0.0);

        let dist_to = |p: [f64; 3]| {
            // Portée vanilla : distance 3D entre l'œil (pieds + 1.62) et le centre du bloc (max 4.5).
            let dx = p[0] - (x as f64 + 0.5);
            let dy = (p[1] + 1.62) - (y as f64 + 0.5);
            let dz = p[2] - (z as f64 + 0.5);
            (dx * dx + dy * dy + dz * dz).sqrt()
        };

        if dist > 3.0 {
            // Aller à côté du bloc : toutes les cellules adjacentes libres, plus une
            // cellule marchable proche (la cible peut être en hauteur ou dans un creux).
            let mut candidates = self.find_adjacent_free_all(x, y, z);
            if let Some(w) = self.find_walkable_near(x, z, y) {
                candidates.push(w);
            }
            // Ne s'engager que vers une cible dont un chemin existe VRAIMENT
            // (évite d'attendre 10 s sur une cible irrémédiablement inaccessible).
            let start = self.pos.map(|p| {
                (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32)
            });
            let mut chosen: Option<(i32, i32, i32)> = None;
            if let Some(start) = start {
                for c in &candidates {
                    if crate::path::find_path(&self.world, start, *c, crate::path::MAX_EXPANSIONS).is_some() {
                        chosen = Some(*c);
                        break;
                    }
                }
            }
            match chosen {
                Some((sx, sy, sz)) => {
                    let n = self.goto(sx, sy, sz).await;
                    if crate::bot::debug_enabled_pub() {
                        eprintln!("[goto] vers ({sx},{sy},{sz}) -> {:?}", n);
                    }
                    self.follow_path(200).await;
                }
                None => {
                    if crate::bot::debug_enabled_pub() {
                        eprintln!("[goto] ({x},{y},{z}) inaccessible depuis ici ({} candidats)", candidates.len());
                    }
                    return false;
                }
            }
        }
        // Portée de minage serveur (4,5 blocs 3D depuis l'œil) : si encore trop loin,
        // tenter une dernière approche vers une cellule marchable au niveau du bloc.
        if let Some(p) = self.pos {
            if dist_to(p) > 4.5 {
                if let Some((gx, gy, gz)) = self.find_walkable_near(x, z, y) {
                    self.goto(gx, gy, gz).await;
                    self.follow_path(200).await;
                }
            }
            if let Some(p) = self.pos {
                if dist_to(p) > 4.5 {
                    println!("bloc trop loin ({:.1} blocs), minage ignoré", dist_to(p));
                    return false;
                }
            }
        }

        // Miner le bloc (peut prendre du temps)
        if let Some(t) = self.dig(x, y, z).await {
            // Attendre la fin du minage
            for _ in 0..(t * 20.0) as usize + 40 {
                if self.mining_done() { break; }
                self.drain_packets().await; self.tick(0.05).await.ok();
            }
            // Attendre la confirmation serveur : block_change reçu avec état air.
            // Marge large : si le bot est aérien côté serveur, le minage est x5 plus lent.
            for _ in 0..(t * 20.0 * 6.0) as usize + 120 {
                self.drain_packets().await;
                if self.air_confirmed.contains(&(x, y, z)) { break; }
                self.tick(0.05).await.ok();
            }
            let broke = self.air_confirmed.contains(&(x, y, z));
            if !broke {
                println!("bloc non cassé côté serveur en ({x}, {y}, {z})");
                return false;
            }
            // Ramasser les drops (entités item) autour du bloc miné
            let n = self.collect_drops(12.0).await;
            if n > 0 {
                println!("drops ramassés: {n}");
            } else {
                // Repli : aller sur le bloc miné et attendre un peu
                self.goto(x, y, z).await;
                self.follow_path(60).await;
                for _ in 0..20 {
                    self.drain_packets().await;
                    self.tick(0.05).await.ok();
                }
            }
            true
        } else {
            false
        }
    }

    /// Cherche une position marchable (solide dessous, air au-dessus, pas de liquide)
    /// près de (x, z), en balayant de `y_hint` vers le haut puis vers le bas.
    pub(crate) fn find_walkable_near(&self, x: i32, z: i32, y_hint: i32) -> Option<(i32, i32, i32)> {
        let y_top = (y_hint + 24).min(crate::world::MIN_Y + crate::world::WORLD_HEIGHT - 2);
        let y_bot = (y_hint - 48).max(crate::world::MIN_Y + 1);
        let mut best: Option<(i32, i32, i32, i32)> = None;
        for (dx, dz) in [(0, 0), (4, 0), (-4, 0), (0, 4), (0, -4), (8, 0), (-8, 0), (0, 8), (0, -8)] {
            let (cx, cz) = (x + dx, z + dz);
            for y in (y_bot..=y_top).rev() {
                if crate::path::walkable(&self.world, cx, y, cz) {
                    let d = dx * dx + dz * dz;
                    if best.map(|(_, _, _, bd)| d < bd).unwrap_or(true) {
                        best = Some((cx, y, cz, d));
                    }
                    break;
                }
            }
        }
        best.map(|(x, y, z, _)| (x, y, z))
    }

    /// Positions libres (solide en dessous, air au niveau pieds et tête) à côté d'un bloc,
    /// classées de la plus proche à la plus loin.
    fn find_adjacent_free_all(&self, x: i32, y: i32, z: i32) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for (dx, dz) in [(1,0),(-1,0),(0,1),(0,-1),(1,1),(-1,-1),(1,-1),(-1,1)] {
            let (nx, nz) = (x + dx, z + dz);
            // Chercher le sol : partir de y+2 vers le bas
            for ny in (y - 5..=y + 3).rev() {
                if self.world.is_solid(nx, ny - 1, nz)
                    && !self.world.is_solid(nx, ny, nz)
                    && !self.world.is_solid(nx, ny + 1, nz)
                {
                    out.push((nx, ny, nz));
                    break;
                }
            }
        }
        out
    }

    /// Crafte un item par son nom. Ouvre la table si nécessaire.
    /// Retourne true si le craft a été demandé avec succès.
    pub async fn craft(&mut self, result_name: &str) -> bool {
        let Some(recipe) = recipe_for(result_name) else {
            eprintln!("pas de recette pour {}", result_name);
            return false;
        };

        // Résoudre chaque ingrédient en un item réellement présent : toute essence de
        // planches/bûche convient (sauf pour fabriquer des planches, où la bûche doit
        // correspondre au type de planches demandé, sinon le serveur ne reconnaît pas
        // la recette).
        let mut resolved: Vec<(i32, i32)> = Vec::with_capacity(recipe.ingredients.len());
        for &(ing_id, count) in &recipe.ingredients {
            if self.inventory.count(ing_id) >= count {
                resolved.push((ing_id, count));
                continue;
            }
            let name = item_name(ing_id);
            let suffix = if name.ends_with("_planks") {
                Some("_planks")
            } else if name.ends_with("_log") {
                Some("_log")
            } else {
                None
            };
            let alt = match (suffix, result_name.ends_with("_planks")) {
                (Some(s), false) => crate::items_gen::MC_ITEMS
                    .iter()
                    .find(|(id, n)| n.ends_with(s) && self.inventory.count(*id) >= count)
                    .map(|(id, _)| *id),
                _ => None,
            };
            match alt {
                Some(id) => resolved.push((id, count)),
                None => {
                    let have = self.inventory.count(ing_id);
                    eprintln!("craft {}: il manque {} x{} (have {})", result_name, count - have, item_name(ing_id), have);
                    return false;
                }
            }
        }

        // Identifiant de la recette : celui déclaré par le serveur s'il est connu,
        // sinon l'identifiant vanilla "minecraft:<nom>" (le serveur le valide de toute façon).
        let full_name = format!("minecraft:{}", result_name);
        let name = self
            .recipes
            .iter()
            .find(|r| r.as_str() == full_name || r.as_str() == result_name || r.ends_with(&format!(":{}", result_name)))
            .cloned()
            .unwrap_or_else(|| {
                if crate::bot::debug_enabled_pub() {
                    eprintln!("recette {} absente de la liste serveur ({} recettes), on tente {}", result_name, self.recipes.len(), full_name);
                }
                full_name
            });

        let ok = self.send_craft(&name, &recipe, &resolved).await;
        if ok {
            // Vérifier que le résultat est bien dans l'inventaire
            let have = self.inventory.count(recipe.result);
            if have == 0 {
                eprintln!("craft {}: résultat absent de l'inventaire", result_name);
                return false;
            }
        }
        ok
    }

    /// Place les ingrédients dans la grille de craft par clics, envoie place_recipe,
    /// récupère le résultat sur le curseur dans un slot vide, puis vérifie.
    async fn send_craft(&mut self, recipe_name: &str, recipe: &crate::recipes::Recipe, resolved: &[(i32, i32)]) -> bool {
        // Si la recette nécessite une table et qu'aucune fenêtre n'est ouverte,
        // on ne peut pas crafter (il faut poser une table d'abord — géré par la mission)
        let window_id = if recipe.needs_table {
            match self.open_window_id {
                Some(id) => id,
                None => {
                    eprintln!("craft 3x3 nécessite une table de craft ouverte");
                    return false;
                }
            }
        } else {
            0 // inventaire (2x2)
        };
        let grid: &[i16] = if recipe.needs_table {
            &[1, 2, 3, 4, 5, 6, 7, 8, 9]
        } else {
            &[1, 2, 3, 4]
        };

        // Plan de placement : (item, slot grille, count)
        // `planks_id` = planches réellement disponibles (toute essence), pour les patterns.
        let planks_id = resolved
            .iter()
            .find(|(id, _)| crate::recipes::is_planks_id(*id))
            .map(|(id, _)| *id)
            .unwrap_or_else(|| item_id("oak_planks"));
        let mut plan: Vec<(i32, i16, i32)> = Vec::new();
        match crate::recipes::pattern_for(recipe_name.trim_start_matches("minecraft:"), planks_id) {
            Some(pattern) => {
                // pattern shaped : 1 item par cellule de la grille (3x3 ou 2x2)
                for (i, cell) in pattern.iter().enumerate() {
                    if let Some(item) = cell {
                        if i < grid.len() {
                            plan.push((*item, grid[i], 1));
                        }
                    }
                }
            }
            None => {
                // shapeless : répartir les items un par cellule de la grille
                let mut gi = 0;
                for (item, count) in resolved.iter() {
                    for _ in 0..*count {
                        if gi >= grid.len() { break; }
                        plan.push((*item, grid[gi], 1));
                        gi += 1;
                    }
                }
            }
        }

        // Vider la grille d'abord (placer le contenu ailleurs) : on suppose la grille vide
        // (le serveur met a jour via set_slot ; on drain entre chaque clic).
        let mut carried: Option<(i32, i32)> = None;
        for (item, gslot, count) in &plan {
            let Some(src) = self.inventory.find(*item) else {
                eprintln!("craft: ingrédient {item} introuvable");
                return false;
            };
            let src_count = self.inventory.slots.get(src).and_then(|s| s.as_ref()).map(|s| s.count).unwrap_or(0);
            // ramasser le stack source (slot de fenêtre, pas index interne, si conteneur)
            let src_w = wslot(&self.inventory, window_id, src);
            self.click_window_carried(window_id, src_w, 0, 0, carried).await.ok();
            carried = Some((*item, src_count));
            // poser `count` items un par un dans la cellule de grille
            for _ in 0..*count {
                self.click_window_carried(window_id, *gslot, 1, 0, carried).await.ok();
                if let Some((_, c)) = carried.as_mut() {
                    *c -= 1;
                }
            }
            // remettre le reste sur la source
            if carried.map(|(_, c)| c) > Some(0) {
                self.click_window_carried(window_id, src_w, 0, 0, carried).await.ok();
                carried = None;
            }
            // Attendre la reponse du serveur (set_slot/window_items fait avancer state_id)
            let before = self.inventory.state_id;
            for _ in 0..30 {
                self.drain_packets().await;
                if self.inventory.state_id != before { break; }
            }
        }

        // Envoyer place_recipe (makeAll)
        let mut b = Vec::new();
        b.push(window_id);
        write_string(recipe_name, &mut b);
        b.push(1); // makeAll = true
        if self.conn.send(ids::play_s::CRAFT_RECIPE, &b).await.is_err() {
            return false;
        }
        // Attendre que le résultat arrive sur le curseur
        for _ in 0..40 {
            self.drain_packets().await;
            self.tick(0.05).await.ok();
            if self.inventory.carried.is_some() {
                break;
            }
        }
        // Prendre le curseur dans un slot vide de l'inventaire
        if let Some(c) = self.inventory.carried.clone() {
            if c.id == recipe.result {
                let empty = (9..45usize).find(|&i| {
                    self.inventory.slots.get(i).map_or(true, |s| s.is_none())
                });
                if let Some(slot) = empty {
                    let slot_w = wslot(&self.inventory, window_id, slot);
                    self.click_window_carried(window_id, slot_w, 0, 0, Some((c.id, c.count)))
                        .await
                        .ok();
                    for _ in 0..10 {
                        self.drain_packets().await;
                    }
                }
            }
        }
        true
    }

    /// Cherche une cellule libre (solide dessous, air dessus) à proximité.
    fn find_free_cell_near(&self, x: i32, y: i32, z: i32) -> Option<(i32, i32, i32)> {
        for dist in 1..=3 {
            for (dx, dz) in [(dist, 0), (-dist, 0), (0, dist), (0, -dist), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
                let (cx, cz) = (x + dx, z + dz);
                for cy in [y, y + 1] {
                    if self.world.is_solid(cx, cy - 1, cz)
                        && !self.world.is_solid(cx, cy, cz)
                        && !self.world.is_solid(cx, cy + 1, cz)
                    {
                        return Some((cx, cy, cz));
                    }
                }
            }
        }
        None
    }

    /// Pose une table de craft à côté de soi et l'ouvre.
    pub async fn place_and_open_crafting_table(&mut self) -> bool {
        let table_id = item_id("crafting_table");
        if self.inventory.find(table_id).is_none() {
            eprintln!("pas de table de craft dans l'inventaire");
            return false;
        }
        if !self.select_hotbar_by_item(table_id).await {
            eprintln!("impossible de prendre la table en main");
            return false;
        }

        let p = match self.pos { Some(p) => p, None => return false };
        let (fx, fy, fz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        let Some((cx, cy, cz)) = self.find_free_cell_near(fx, fy, fz) else {
            eprintln!("aucune cellule libre autour pour poser la table");
            return false;
        };

        // Poser la table sur le dessus du bloc sous la cellule libre
        if self.place(cx, cy - 1, cz, 1).await.is_err() {
            return false;
        }
        // Mettre à jour le monde local : la cellule n'est plus libre.
        if let Some(state) = crate::blocks_gen::block_state_by_name("crafting_table") {
            self.world.set_block_state(cx, cy, cz, state);
        }
        self.drain_packets().await; self.tick(0.3).await.ok();

        // Ouvrir la table : use_item_on (block_place) sur la table, face haut (767)
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        write_varint(0, &mut b); // main hand
        write_position(cx, cy, cz, &mut b);
        write_varint(1, &mut b); // face haut
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.push(0); // insideBlock
        write_varint(seq, &mut b);
        if self.conn.send(ids::play_s::BLOCK_PLACE, &b).await.is_err() {
            return false;
        }
        // Attendre l'open_window
        for _ in 0..40 {
            if self.open_window_id.is_some() { return true; }
            self.drain_packets().await; self.tick(0.05).await.ok();
        }
        self.open_window_id.is_some()
    }

    /// Sélectionne dans la hotbar le slot contenant l'item donné.
    /// Si l'item est hors hotbar, le déplace par clics vers un slot libre.
    pub async fn select_hotbar_by_item(&mut self, item: i32) -> bool {
        // Fermer toute fenêtre de conteneur ouverte : les clics doivent viser la fenêtre 0.
        if let Some(wid) = self.open_window_id {
            self.conn.send(ids::play_s::CONTAINER_CLOSE, &[wid]).await.ok();
            self.open_window_id = None;
            for _ in 0..5 { self.drain_packets().await; }
        }
        let Some(slot) = self.inventory.find(item) else { return false; };
        if (crate::inventory::HOTBAR_FIRST..=crate::inventory::HOTBAR_LAST).contains(&slot) {
            self.select_hotbar(slot - crate::inventory::HOTBAR_FIRST).await.ok();
            return true;
        }
        let Some(target) = (crate::inventory::HOTBAR_FIRST..=crate::inventory::HOTBAR_LAST)
            .find(|&i| self.inventory.slots.get(i).map(|s| s.is_none()).unwrap_or(false))
        else { return false; };
        if self.click_slot(slot as i16).await.is_err() {
            return false;
        }
        for _ in 0..20 { self.drain_packets().await; }
        if self.click_slot(target as i16).await.is_err() {
            return false;
        }
        for _ in 0..20 { self.drain_packets().await; }
        self.select_hotbar(target - crate::inventory::HOTBAR_FIRST).await.ok();
        self.inventory.held(target - crate::inventory::HOTBAR_FIRST).map(|s| s.id) == Some(item)
    }

    /// Affiche l'inventaire (debug).
    pub fn dump_inventory(&self) {
        println!("=== Inventaire ===");
        for (i, slot) in self.inventory.slots.iter().enumerate() {
            if let Some(s) = slot {
                println!("  slot {:2}: {} x{} (id {})", i, item_name(s.id), s.count, s.id);
            }
        }
    }

    /// Compte combien d'un item on a.
    pub fn count_item(&self, name: &str) -> i32 {
        self.inventory.count(item_id(name))
    }

    // ==================== MISSION ====================

    /// Phase 1 : récolter du bois et crafter une table + outils en bois.
    pub async fn phase_wood(&mut self) -> bool {
        println!("=== Phase 1 : Bois === ({} chunks chargés)", self.world.chunk_count());
        let logs = ["oak_log", "spruce_log", "birch_log", "jungle_log", "acacia_log", "dark_oak_log", "mangrove_log", "cherry_log"];

        // Attendre que les chunks autour du spawn soient chargés (min 25 chunks, max 15s)
        for _ in 0..150 {
            self.drain_packets().await;
            if self.world.chunk_count() >= 25 { break; }
        }
        println!("chunks chargés: {}", self.world.chunk_count());

        // Attendre d'avoir atterri (sinon les déplacements partent d'une position aérienne)
        for _ in 0..200 {
            if self.on_ground { break; }
            self.drain_packets().await;
            self.tick(0.05).await.ok();
        }

        // Récolter 6 bûches
        let mut got = self.count_item_any(&logs);
        let mut tries = 0;
        let mut skip: Vec<(i32, i32, i32)> = Vec::new(); // cibles déjà essayées sans succès
        while got < 6 && tries < 30 {
            tries += 1;
            // Chercher de plus en plus loin
            let radius = 20 + tries * 10;
            if let Some((x, y, z)) = self.find_block_any(&logs, radius.min(80), &skip) {
                println!("tronc trouvé en ({}, {}, {}) dist~{}, minage...", x, y, z, radius);
                if !self.goto_and_dig(x, y, z).await {
                    skip.push((x, y, z)); // inaccessible : ne plus la re-choisir
                }
                // Miner aussi les blocs au-dessus (tronc vertical)
                for dy in 1..4 {
                    if self.world.block_state(x, y + dy, z).is_some() {
                        let name = crate::blocks_gen::block_name(self.world.block_state(x, y + dy, z).unwrap());
                        if name.ends_with("_log") {
                            self.goto_and_dig(x, y + dy, z).await;
                        } else { break; }
                    } else { break; }
                }
            } else {
                println!("pas de tronc trouvé (rayon {}), on avance au hasard...", radius);
                // Avancer dans une direction aléatoire pour découvrir de nouveaux chunks
                if let Some(p) = self.pos {
                    let angle = (tries as f64) * 2.4;
                    let tx = (p[0] + angle.cos() * 30.0) as i32;
                    let tz = (p[2] + angle.sin() * 30.0) as i32;
                    // Viser une cellule réellement marchable près de la cible aléatoire
                    if let Some((gx, gy, gz)) = self.find_walkable_near(tx, tz, p[1] as i32) {
                        self.goto(gx, gy, gz).await;
                        self.follow_path(150).await;
                    }
                }
            }
            got = self.count_item_any(&logs);
            println!("bûches: {}", got);
        }
        println!("Bûches récoltées: {}", got);
        if got < 3 { return false; }

        // Crafter des planches du type de bûche réellement ramassé
        let planks = self.count_item("oak_planks") + self.count_item("spruce_planks") + self.count_item("birch_planks");
        if planks < 8 {
            let log = crate::items_gen::MC_ITEMS
                .iter()
                .find(|(id, n)| n.ends_with("_log") && self.inventory.count(*id) > 0)
                .map(|(_, n)| *n);
            match log.and_then(crate::recipes::planks_of_log) {
                Some(plank) => { self.craft(plank).await; }
                None => { self.craft("oak_planks").await; }
            }
        }

        // Crafter une table de craft (n'importe quelle essence de planches convient)
        if self.count_item("crafting_table") == 0 {
            self.craft("crafting_table").await;
        }
        if self.count_item("crafting_table") == 0 {
            eprintln!("impossible de crafter une table");
            return false;
        }
        println!("Table de craft OK");

        // Poser et ouvrir la table
        if !self.place_and_open_crafting_table().await {
            eprintln!("impossible d'ouvrir la table");
            return false;
        }
        println!("Table ouverte");

        // Crafter stick, pioche, épée, hache en bois
        self.craft("stick").await;
        self.craft("wooden_pickaxe").await;
        self.craft("wooden_axe").await;
        self.craft("wooden_sword").await;

        self.dump_inventory();
        self.count_item("wooden_pickaxe") > 0
    }

    /// Compte les items dont le nom est dans la liste.
    pub fn count_item_any(&self, names: &[&str]) -> i32 {
        names.iter().map(|n| self.count_item(n)).sum()
    }

    /// Phase 2 : miner de la pierre avec la pioche, crafter outils en pierre + fourneau.
    pub async fn phase_stone(&mut self) -> bool {
        println!("=== Phase 2 : Pierre ===");
        self.select_hotbar_by_item(item_id("wooden_pickaxe")).await;

        let mut tries = 0;
        while self.count_item("cobblestone") < 16 && tries < 30 {
            tries += 1;
            if let Some((x, y, z)) = self.find_block_any(&["cobblestone", "stone", "deepslate", "tuff", "andesite", "diorite", "granite"], 20, &[]) {
                self.goto_and_dig(x, y, z).await;
            } else if let Some((x, y, z)) = self.find_block_any(&["stone", "cobblestone"], 60, &[]) {
                self.goto(x, y - 1, z).await;
            } else {
                break;
            }
        }
        println!("Cobblestone: {}", self.count_item("cobblestone"));
        if self.count_item("cobblestone") < 12 { return false; }

        self.craft("stone_pickaxe").await;
        self.craft("stone_axe").await;
        self.craft("stone_sword").await;
        self.craft("furnace").await;
        self.dump_inventory();
        self.count_item("stone_pickaxe") > 0
    }

    /// Phase 3 : miner du fer, le fondre, crafter équipement en fer.
    pub async fn phase_iron(&mut self) -> bool {
        println!("=== Phase 3 : Fer ===");
        self.select_hotbar_by_item(item_id("stone_pickaxe")).await;

        let mut tries = 0;
        while self.count_item("raw_iron") < 8 && tries < 40 {
            tries += 1;
            if let Some((x, y, z)) = self.find_block_any(&["iron_ore", "deepslate_iron_ore"], 30, &[]) {
                println!("minerai de fer en ({}, {}, {})", x, y, z);
                self.goto_and_dig(x, y, z).await;
            } else {
                // Descendre un peu et chercher
                if let Some(p) = self.pos {
                    let y = (p[1] as i32) - 10;
                    if let Some((x, _, z)) = self.find_block_any(&["iron_ore", "deepslate_iron_ore"], 40, &[]) {
                        self.goto(x, y.max(-59), z).await;
                    } else {
                        break;
                    }
                } else { break; }
            }
        }
        println!("Raw iron: {}", self.count_item("raw_iron"));
        if self.count_item("raw_iron") < 5 { return false; }

        // Fondre le fer dans le fourneau (simplifié : on suppose qu'on a un fourneau posé)
        // TODO: ouvrir le fourneau et fondre
        // Pour l'instant, on crafte directement si on a assez de fer
        if self.count_item("iron_ingot") < 5 {
            eprintln!("pas assez de lingots de fer (fondage fourneau pas encore implémenté)");
            // Le fondage sera ajouté quand on posera un fourneau
        }

        self.craft("iron_pickaxe").await;
        self.craft("iron_sword").await;
        self.craft("iron_axe").await;
        self.dump_inventory();
        self.count_item("iron_pickaxe") > 0
    }
}
