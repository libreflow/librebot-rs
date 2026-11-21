//! Mission survie : le bot joue en autonomie (bois → pierre → fer → diamant → Nether → End → dragon).
//! Implémenté comme des méthodes sur `Bot` via ce module.

use crate::bot::Bot;
use crate::bot::ids;
use crate::protocol::{write_position, write_varint, write_string};
use crate::recipes::{item_id, recipe_for};
use crate::items_gen::item_name;

impl Bot {
    /// Mémorise un bloc inaccessible pour ne pas le re-cibler sans cesse.
    pub fn remember_failed(&mut self, x: i32, y: i32, z: i32) {
        if self.failed_targets.len() >= 64 {
            self.failed_targets.remove(0);
        }
        if !self.failed_targets.contains(&(x, y, z)) {
            self.failed_targets.push((x, y, z));
        }
    }

    /// Trouve une cellule praticable (debout) proche de (x, y, z).
    pub fn find_walkable_near(&self, x: i32, y: i32, z: i32) -> Option<(i32, i32, i32)> {
        for dy in -8..=8 {
            let ny = y + dy;
            if crate::path::walkable(&self.world, x, ny, z) {
                return Some((x, ny, z));
            }
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2)] {
            for dy in -8..=8 {
                let ny = y + dy;
                if crate::path::walkable(&self.world, x + dx, ny, z + dz) {
                    return Some((x + dx, ny, z + dz));
                }
            }
        }
        None
    }

    /// Cherche un bloc par nom dans les chunks chargés, dans un rayon autour du bot.
    /// Retourne (x, y, z) du bloc le plus proche.
    pub fn find_block(&self, block_name: &str, radius: i32) -> Option<(i32, i32, i32)> {
        let p = self.pos?;
        let (bx, by, bz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        let target_state = crate::blocks_gen::block_state_by_name(block_name)?;
        let mut best: Option<(i32, i32, i32, i64)> = None;
        // Parcourir uniquement les chunks chargés autour du bot (16 blocs par chunk)
        let r = radius.max(16);
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    let (x, y, z) = (bx + dx, by + dy, bz + dz);
                    // Ne chercher que dans les chunks chargés
                    let cx = x.div_euclid(16);
                    let cz = z.div_euclid(16);
                    if !self.world.has_chunk(cx, cz) { continue; }
                    // Ignorer les blocs bien plus bas que le bot : la plupart
                    // sont dans des grottes inaccessibles (leurres).
                    if y < by - 24 {
                        continue;
                    }
                    if self.world.block_state(x, y, z) == Some(target_state)
                        && !self.failed_targets.contains(&(x, y, z))
                    {
                        let dist = (dx * dx + dy * dy + dz * dz) as i64;
                        if best.map(|(_, _, _, d)| dist < d).unwrap_or(true) {
                            best = Some((x, y, z, dist));
                        }
                    }
                }
            }
        }
        best.map(|(x, y, z, _)| (x, y, z))
    }

    /// Cherche plusieurs noms de blocs (ex: tous les types de troncs).
    pub fn find_block_any(&self, names: &[&str], radius: i32) -> Option<(i32, i32, i32)> {
        for name in names {
            if let Some(pos) = self.find_block(name, radius) {
                return Some(pos);
            }
        }
        // Essai avec un rayon plus grand
        for name in names {
            if let Some(pos) = self.find_block(name, radius * 3) {
                return Some(pos);
            }
        }
        None
    }

    /// Va vers un bloc et le mine. Retourne true si miné.
    pub async fn goto_and_dig(&mut self, x: i32, y: i32, z: i32) -> bool {
        // Le serveur n'accepte le minage que dans un rayon de 4,5 blocs
        // autour des yeux : s'approcher jusqu'à être à portée.
        let mut proche = false;
        for _approche in 0..3 {
            let Some(p) = self.pos else { return false };
            let dx = p[0] - (x as f64 + 0.5);
            let dy = p[1] + 1.62 - (y as f64 + 0.5);
            let dz = p[2] - (z as f64 + 0.5);
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            if d <= 4.0 {
                proche = true;
                break;
            }
            // Aller sur une cellule libre adjacente au bloc
            match self.find_adjacent_free(x, y, z) {
                Some((sx, sy, sz)) => {
                    self.goto(sx, sy, sz).await;
                    for _ in 0..200 {
                        if self.path_len() == 0 { break; }
                        self.drain_packets().await; self.tick(0.05).await.ok();
                    }
                }
                None => break,
            }
        }
        if !proche {
            if let Some(p) = self.pos {
                let dx = p[0] - (x as f64 + 0.5);
                let dy = p[1] + 1.62 - (y as f64 + 0.5);
                let dz = p[2] - (z as f64 + 0.5);
                if (dx * dx + dy * dy + dz * dz).sqrt() > 4.5 {
                    // Hors de portée : le serveur rejetterait le minage en silence.
                    eprintln!("bloc ({}, {}, {}) hors de portée, on cherche ailleurs", x, y, z);
                    self.remember_failed(x, y, z);
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
            if !self.mining_done() {
                self.drain_packets().await; self.tick(0.05).await.ok();
            }
            // Aller SUR le bloc miné pour ramasser le drop
            self.goto(x, y, z).await;
            for _ in 0..60 {
                self.drain_packets().await;
                if self.path_len() == 0 { break; }
                self.tick(0.05).await.ok();
            }
            // Petit délai pour le ramassage
            for _ in 0..20 {
                self.drain_packets().await;
                self.tick(0.05).await.ok();
            }
            true
        } else {
            false
        }
    }

    /// Trouve une position libre (solide en dessous, air au niveau pieds et tête) à côté d'un bloc.
    fn find_adjacent_free(&self, x: i32, y: i32, z: i32) -> Option<(i32, i32, i32)> {
        for (dx, dz) in [(1,0),(-1,0),(0,1),(0,-1),(1,1),(-1,-1),(1,-1),(-1,1)] {
            let (nx, nz) = (x + dx, z + dz);
            // Chercher le sol : partir de y+2 vers le bas
            for ny in (y - 5..=y + 3).rev() {
                if self.world.is_solid(nx, ny - 1, nz)
                    && !self.world.is_solid(nx, ny, nz)
                    && !self.world.is_solid(nx, ny + 1, nz)
                {
                    return Some((nx, ny, nz));
                }
            }
        }
        None
    }

    /// Crafte un item par son nom. Ouvre la table si nécessaire.
    /// Retourne true si le craft a été demandé avec succès.
    pub async fn craft(&mut self, result_name: &str) -> bool {
        let Some(recipe) = recipe_for(result_name) else {
            eprintln!("pas de recette pour {}", result_name);
            return false;
        };

        // Vérifier les ingrédients
        for &(ing_id, count) in &recipe.ingredients {
            if self.inventory.count(ing_id) < count {
                let have = self.inventory.count(ing_id);
                eprintln!("craft {}: il manque {} x{} (have {})", result_name, count - have, item_name(ing_id), have);
                return false;
            }
        }

        // Trouver l'identifiant de la recette parmi celles déclarées par le serveur
        let full_name = format!("minecraft:{}", result_name);
        let Some(name) = self
            .recipes
            .iter()
            .find(|r| r.as_str() == full_name || r.as_str() == result_name || r.ends_with(&format!(":{}", result_name)))
            .cloned()
        else {
            eprintln!("recette {} non déclarée par le serveur ({} recettes)", result_name, self.recipes.len());
            return false;
        };

        self.send_craft(&name, recipe.needs_table).await
    }

    async fn send_craft(&mut self, recipe_name: &str, needs_table: bool) -> bool {
        // Si la recette nécessite une table et qu'aucune fenêtre n'est ouverte,
        // on ne peut pas crafter (il faut poser une table d'abord — géré par la mission)
        let window_id = if needs_table {
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

        let mut b = Vec::new();
        b.push(window_id);
        write_string(recipe_name, &mut b);
        b.push(1); // makeAll = true
        if self.conn.send(ids::play_s::CRAFT_RECIPE, &b).await.is_ok() {
            // Attendre que l'item arrive dans l'inventaire
            for _ in 0..40 {
                self.drain_packets().await; self.tick(0.05).await.ok();
            }
            true
        } else {
            false
        }
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

        // Attendre que des chunks autour du spawn soient chargés.
        // Serveurs lents (Aternos) : borne en TEMPS, pas en itérations
        // (chaque drain peut durer jusqu'à 15 s si le flux est continu).
        let debut = std::time::Instant::now();
        while self.world.chunk_count() < 12 && debut.elapsed() < std::time::Duration::from_secs(90) {
            self.drain_packets().await;
        }
        println!("chunks chargés: {}", self.world.chunk_count());

        // Récolter 6 bûches
        let mut got = self.count_item_any(&logs);
        let mut tries = 0;
        while got < 6 && tries < 30 {
            tries += 1;
            // Chercher de plus en plus loin
            let radius = 20 + tries * 10;
            if let Some((x, y, z)) = self.find_block_any(&logs, radius.min(80)) {
                println!("tronc trouvé en ({}, {}, {}) dist~{}, minage...", x, y, z, radius);
                self.goto_and_dig(x, y, z).await;
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
                    let tx = (p[0] + angle.cos() * 30.0).floor() as i32;
                    let tz = (p[2] + angle.sin() * 30.0).floor() as i32;
                    let ty = p[1].floor() as i32;
                    // Viser une cellule réellement praticable, sinon le chemin échoue
                    // silencieusement et le bot reste sur place.
                    if let Some((gx, gy, gz)) = self.find_walkable_near(tx, ty, tz) {
                        self.goto(gx, gy, gz).await;
                        for _ in 0..100 {
                            if self.path_len() == 0 { break; }
                            self.drain_packets().await; self.tick(0.05).await.ok();
                        }
                    }
                }
            }
            got = self.count_item_any(&logs);
            println!("bûches: {}", got);
        }
        println!("Bûches récoltées: {}", got);
        if got < 3 { return false; }

        // Crafter des planches
        let planks = self.count_item("oak_planks") + self.count_item("spruce_planks") + self.count_item("birch_planks");
        if planks < 8 {
            self.craft("oak_planks").await;
        }

        // Crafter une table de craft
        if self.count_item("crafting_table") == 0
            && !self.craft("crafting_table").await {
                // Essayer avec d'autres planches
                self.craft("spruce_planks").await;
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
        self.close_window().await.ok();

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
            if let Some((x, y, z)) = self.find_block_any(&["cobblestone", "stone", "deepslate", "tuff", "andesite", "diorite", "granite"], 20) {
                self.goto_and_dig(x, y, z).await;
            } else if let Some((x, y, z)) = self.find_block_any(&["stone", "cobblestone"], 60) {
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
        self.close_window().await.ok();
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
            if let Some((x, y, z)) = self.find_block_any(&["iron_ore", "deepslate_iron_ore"], 30) {
                println!("minerai de fer en ({}, {}, {})", x, y, z);
                self.goto_and_dig(x, y, z).await;
            } else {
                // Descendre un peu et chercher
                if let Some(p) = self.pos {
                    let y = (p[1] as i32) - 10;
                    if let Some((x, _, z)) = self.find_block_any(&["iron_ore", "deepslate_iron_ore"], 40) {
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
        self.close_window().await.ok();
        self.dump_inventory();
        self.count_item("iron_pickaxe") > 0
    }
}
