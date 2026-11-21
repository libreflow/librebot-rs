//! Couche d'autonomie du bot : survie infinie sur le serveur.
//! Fourneau, chasse, nourriture, défense, et boucle de mission qui ne renonce jamais.

use crate::bot::Bot;
use crate::bot::ids;
use crate::entities;
use crate::inventory::internal_to_window_slot;
use crate::protocol::write_varint;
use crate::recipes::item_id;

/// Aliments classés par priorité (les meilleurs d'abord).
const ALIMENTS: &[&str] = &[
    "cooked_beef",
    "cooked_porkchop",
    "cooked_mutton",
    "cooked_chicken",
    "cooked_rabbit",
    "cooked_cod",
    "cooked_salmon",
    "bread",
    "baked_potato",
    "golden_apple",
    "apple",
    "carrot",
    "beef",
    "porkchop",
    "mutton",
    "chicken",
    "rabbit",
    "rotten_flesh",
];

/// Viandes crues à cuire au fourneau.
const CRUS_CUISSON: &[(&str, &str)] = &[
    ("beef", "cooked_beef"),
    ("porkchop", "cooked_porkchop"),
    ("chicken", "cooked_chicken"),
    ("mutton", "cooked_mutton"),
    ("rabbit", "cooked_rabbit"),
    ("cod", "cooked_cod"),
    ("salmon", "cooked_salmon"),
];

/// Fenêtre fourneau : slot 0 = ingrédient, 1 = carburant, 2 = résultat.
const F_SLOT_INGREDIENT: i16 = 0;
const F_SLOT_CARBURANT: i16 = 1;
const F_SLOT_RESULTAT: i16 = 2;

impl Bot {
    /// Clic dans une fenêtre donnée (mode 0 : ramasser/poser un stack entier).
    pub async fn click_slot_in(&mut self, window_id: u8, slot: i16) -> std::io::Result<i32> {
        let state = self.inventory.state_id;
        let body = crate::inventory::window_click_body(window_id, state, slot, 0, 0);
        self.conn.send(ids::play_s::WINDOW_CLICK, &body).await?;
        Ok(state)
    }

    /// Interaction (use_item_on) sur un bloc pour ouvrir son conteneur.
    async fn interact_open(&mut self, cx: i32, cy: i32, cz: i32) -> Option<u8> {
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        write_varint(0, &mut b); // main hand
        crate::protocol::write_position(cx, cy, cz, &mut b);
        write_varint(1, &mut b); // face haut
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.push(0); // insideBlock
        write_varint(seq, &mut b);
        self.conn.send(ids::play_s::BLOCK_PLACE, &b).await.ok()?;
        for _ in 0..60 {
            if self.open_window_id.is_some() {
                return self.open_window_id;
            }
            self.drain_packets().await;
            self.tick(0.05).await.ok();
        }
        eprintln!("pas de fenêtre ouverte à ({}, {}, {})", cx, cy, cz);
        None
    }

    /// Ouvre un fourneau : celui déjà posé à proximité sinon on en pose un neuf.
    async fn open_furnace(&mut self) -> Option<u8> {
        // Fourneau existant dans le monde connu ?
        if let Some((fx, fy, fz)) = self.find_block("furnace", 16) {
            if self.approach_block(fx, fy, fz).await {
                return self.interact_open(fx, fy, fz).await;
            }
        }
        // En posséder un dans l'inventaire ?
        if self.inventory.find(item_id("furnace")).is_none() {
            eprintln!("pas de fourneau dans l'inventaire ni posé à proximité");
            return None;
        }
        let p = self.pos?;
        let (fx, fy, fz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        let (cx, cy, cz) = self.find_free_cell_near(fx, fy, fz)?;
        self.select_hotbar_by_item(item_id("furnace")).await;
        self.place(cx, cy - 1, cz, 1).await.ok()?;
        self.drain_packets().await;
        self.tick(0.3).await.ok();
        self.interact_open(cx, cy, cz).await
    }

    /// S'approche d'un bloc jusqu'à être à portée d'interaction (4,5 blocs).
    pub(crate) async fn approach_block(&mut self, x: i32, y: i32, z: i32) -> bool {
        for _ in 0..10 {
            let Some(p) = self.pos else { return false };
            let dx = p[0] - (x as f64 + 0.5);
            let dy = p[1] + 1.62 - (y as f64 + 0.5);
            let dz = p[2] - (z as f64 + 0.5);
            if (dx * dx + dy * dy + dz * dz).sqrt() <= 4.0 {
                return true;
            }
            // marcher vers le bloc par étapes
            let horiz = (dx * dx + dz * dz).sqrt();
            if horiz < 1.0 {
                return false;
            }
            let pas = horiz.min(8.0);
            let tx = (p[0] - dx / horiz * pas).floor() as i32;
            let tz = (p[2] - dz / horiz * pas).floor() as i32;
            match self.find_walkable_near(tx, p[1].floor() as i32, tz) {
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
                None => return false,
            }
        }
        false
    }

    /// Fait fondre jusqu'à `nb` exemplaires au fourneau déjà ouvert ou à ouvrir.
    /// Le stack d'ingrédients et un carburant sont posés d'un coup, puis on
    /// récupère les résultats au fil de la cuisson (~10 s par item).
    pub async fn smelt(&mut self, ingredient: &str, carburant: &str, resultat: &str, nb: i32) -> bool {
        let ing = item_id(ingredient);
        let fuel = item_id(carburant);
        let res = item_id(resultat);
        let Some(wid) = self.open_furnace().await else {
            return false;
        };
        let window_len = self.inventory.container_slots.len() + 37;
        let obtenus_avant = self.inventory.count(res);

        // Déposer le stack d'ingrédients
        if let Some(src) = self.inventory.find(ing) {
            if let Some(ws) = internal_to_window_slot(src, window_len) {
                self.click_slot_in(wid, ws).await.ok();
                self.click_slot_in(wid, F_SLOT_INGREDIENT).await.ok();
                self.drain_packets().await;
            }
        }
        // Déposer du carburant
        if let Some(fuel_src) = self.inventory.find(fuel) {
            if let Some(ws) = internal_to_window_slot(fuel_src, window_len) {
                self.click_slot_in(wid, ws).await.ok();
                self.click_slot_in(wid, F_SLOT_CARBURANT).await.ok();
                self.drain_packets().await;
            }
        }

        // Récupérer les résultats au fur et à mesure (10 s par item + marge)
        let duree_max = std::time::Duration::from_secs(nb as u64 * 12 + 15);
        let debut = std::time::Instant::now();
        while debut.elapsed() < duree_max && !self.disconnected {
            let pret = match self.inventory.container_slots.get(F_SLOT_RESULTAT as usize) {
                Some(Some(stack)) => stack.id == res && stack.count >= 1,
                _ => false,
            };
            if pret {
                if self.click_slot_in(wid, F_SLOT_RESULTAT).await.is_err() {
                    break;
                }
                if let Some(cible) = (crate::inventory::HOTBAR_FIRST..=crate::inventory::HOTBAR_LAST)
                    .find(|&i| {
                        self.inventory
                            .slots
                            .get(i)
                            .map(|s| s.is_none())
                            .unwrap_or(false)
                    })
                {
                    if let Some(ws) = internal_to_window_slot(cible, window_len) {
                        self.click_slot_in(wid, ws).await.ok();
                    }
                }
                let cible_res = obtenus_avant + nb;
                if self.inventory.count(res) >= cible_res.min(obtenus_avant + nb) {
                    break;
                }
                if self.inventory.count(res) - obtenus_avant >= nb {
                    break;
                }
                continue;
            }
            // Plus d'ingrédient dans le fourneau ?
            let entree_vide = match self.inventory.container_slots.get(F_SLOT_INGREDIENT as usize) {
                Some(Some(stack)) => stack.id != ing || stack.count == 0,
                _ => true,
            };
            let carburant_vide = match self.inventory.container_slots.get(F_SLOT_CARBURANT as usize) {
                Some(Some(stack)) => stack.count == 0,
                _ => true,
            };
            if entree_vide || (carburant_vide && !pret) {
                // attendre un peu : le stack restant brûle encore si résultats en attente
                self.drain_packets().await;
                self.tick(0.5).await.ok();
                let encore = match self.inventory.container_slots.get(F_SLOT_RESULTAT as usize) {
                    Some(Some(stack)) => stack.id == res && stack.count >= 1,
                    _ => false,
                };
                if !encore {
                    break;
                }
            }
            self.drain_packets().await;
            self.tick(0.5).await.ok();
        }
        self.close_window().await.ok();
        let gagne = self.inventory.count(res) - obtenus_avant;
        println!("fondage {} : {} obtenu(s)", resultat, gagne);
        gagne > 0
    }

    /// Chasse les animaux proches jusqu'à avoir assez de viande crue.
    pub async fn hunt(&mut self, nb_viandes: i32) -> bool {
        // Prendre la meilleure arme
        for arme in ["iron_sword", "stone_sword", "wooden_sword"] {
            if self.inventory.find(item_id(arme)).is_some() {
                self.select_hotbar_by_item(item_id(arme)).await;
                break;
            }
        }
        let mut viandes: i32 = CRUS_CUISSON
            .iter()
            .map(|(crus, _)| self.inventory.count(item_id(crus)))
            .sum();
        let arme_main = 3.0f64;
        let rayon_chasse = 32.0f64;
        let mut cibles_epuisees = 7;

        while viandes < nb_viandes && cibles_epuisees > 0 && !self.disconnected {
            let Some(p) = self.pos else { return false };
            let Some((id, cible)) = self
                .entities
                .plus_proche(p[0], p[1], p[2], entities::GIBIERS, rayon_chasse)
            else {
                break;
            };
            eprintln!(
                "chasse : {} à {:.0} blocs",
                entities::entity_name(cible.etype),
                (cible.x - p[0]).hypot(cible.z - p[2])
            );
            // Approche par étages jusqu'à portée d'attaque (3 blocs)
            let mut coups = 0;
            while !self.disconnected && coups < 25 {
                let Some(np) = self.pos else { return false };
                let (ent_x, ent_y, ent_z) = match self.entities.alive.get(&id) {
                    Some(e) => (e.x, e.y, e.z),
                    None => break, // mort
                };
                let d = (ent_x - np[0]).hypot(ent_z - np[2]);
                let d3 = ((ent_x - np[0]).powi(2)
                    + (ent_y - (np[1] + 1.0)).powi(2)
                    + (ent_z - np[2]).powi(2))
                .sqrt();
                if d3 <= arme_main {
                    // Orienter vers la cible avant la frappe
                    let dx = ent_x - np[0];
                    let dz = ent_z - np[2];
                    let yaw_cible = (-dx).atan2(dz).to_degrees() as f32;
                    let d_y = (ent_y + 0.5) - (np[1] + 1.62);
                    let pitch_cible = (-d_y).atan2(dx.hypot(dz)) as f32;
                    self.yaw = yaw_cible;
                    self.pitch = pitch_cible;
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
                    let dirx = (ent_x - np[0]) / d;
                    let dirz = (ent_z - np[2]) / d;
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
                    // Au-dessus ou en dessous : impossible d'atteindre
                    break;
                }
                if !self.entities.alive.contains_key(&id) {
                    break; // cible morte
                }
            }
            // Ramasser les drops : marcher sur les items proches
            self.pickup_drops(12.0).await;
            viandes = CRUS_CUISSON
                .iter()
                .map(|(crus, _)| self.inventory.count(item_id(crus)))
                .sum();
            if viandes < nb_viandes {
                // la cible la plus proche est peut-être la même zone : élargir
                cibles_epuisees -= 1;
            }
        }
        viandes > 0
    }

    /// Marche sur chaque item jeté proche du bot pour le ramasser.
    pub async fn pickup_drops(&mut self, rayon: f64) {
        for _ in 0..12 {
            let Some(p) = self.pos else { return };
            let Some((_, item)) = self
                .entities
                .plus_proche(p[0], p[1], p[2], &[entities::ITEM], rayon * 3.0)
            else {
                return;
            };
            match self.find_walkable_near(item.x.floor() as i32, item.y.floor() as i32, item.z.floor() as i32)
            {
                Some((gx, gy, gz)) => {
                    self.goto(gx, gy, gz).await;
                    for _ in 0..120 {
                        if self.path_len() == 0 {
                            break;
                        }
                        self.drain_packets().await;
                        self.tick(0.05).await.ok();
                    }
                }
                None => return,
            }
            self.drain_packets().await;
            self.tick(0.3).await.ok();
        }
    }

    /// Mange si le niveau de faim est bas ; retourne true si le bot a mangé.
    pub async fn eat_if_needed(&mut self) -> bool {
        if self.food >= 15 {
            return false;
        }
        println!("faim à {} : on mange", self.food);
        let mange = self.eat_one_food().await;
        mange
    }

    async fn eat_one_food(&mut self) -> bool {
        for aliment in ALIMENTS {
            let iid = item_id(aliment);
            if self.inventory.find(iid).is_none() { continue; };
            let nb_avant = self.inventory.count(iid);
            if !self.select_hotbar_by_item(iid).await {
                continue;
            }
            // Croquer : l'item met ~1,6 s à être consommé (use_item répété)
            for _ in 0..20 {
                if self.use_held_item().await.is_err() {
                    return false;
                }
                self.drain_packets().await;
                self.tick(0.15).await.ok();
                if self.food >= 20 {
                    return true;
                }
                if self.inventory.count(iid) < nb_avant {
                    // consommé
                    return true;
                }
            }
            if self.inventory.count(iid) < nb_avant {
                return true;
            }
        }
        false
    }

    /// Cuit toutes les viandes crues de l'inventaire (carburant : planches ou charbon).
    pub async fn cook_all_meats(&mut self) {
        // S'assurer d'avoir du carburant
        let fuel = if self.inventory.count(item_id("coal")) > 0 {
            "coal"
        } else if self.inventory.count(item_id("oak_planks")) > 0
            || self.inventory.count(item_id("spruce_planks")) > 0
        {
            "oak_planks"
        } else {
            return;
        };
        for (crus, cuit) in CRUS_CUISSON {
            let dispo = self.inventory.count(item_id(crus));
            if dispo > 0 && !self.smelt(crus, fuel, cuit, dispo).await {
                break;
            }
        }
    }

    /// Réflexe de survie : riposte contre la menace la plus proche ou fuit.
    /// Retourne true si une action a été prise.
    pub async fn self_defend(&mut self) -> bool {
        if self.disconnected {
            return false;
        }
        let Some(p) = self.pos else { return false };
        // Les creepers proches : fuir immédiatement
        if let Some((_, creeper)) = self
            .entities
            .plus_proche(p[0], p[1], p[2], &[entities::CREEPER], 8.0)
        {
            eprintln!("creeper à {:.1} blocs : fuite", (creeper.x - p[0]).hypot(creeper.z - p[2]));
            let dx = p[0] - creeper.x;
            let dz = p[2] - creeper.z;
            let d = dx.hypot(dz).max(1.0);
            let tx = (p[0] + dx / d * 18.0).floor() as i32;
            let tz = (p[2] + dz / d * 18.0).floor() as i32;
            if let Some((gx, gy, gz)) = self.find_walkable_near(tx, p[1].floor() as i32, tz) {
                self.goto(gx, gy, gz).await;
                for _ in 0..150 {
                    if self.path_len() == 0 {
                        break;
                    }
                    self.drain_packets().await;
                    self.tick(0.05).await.ok();
                }
                return true;
            }
        }
        // Mob hostile très proche : riposter avec la meilleure épée
        let mut menaces = entities::HOSTILES.to_vec();
        menaces.push(entities::CREEPER);
        let menace = self
            .entities
            .plus_proche(p[0], p[1], p[2], &menaces, 4.5);
        match menace {
            Some((id, _)) => {
                for arme in ["iron_sword", "stone_sword", "wooden_sword"] {
                    if self.inventory.find(item_id(arme)).is_some() {
                        self.select_hotbar_by_item(item_id(arme)).await;
                        break;
                    }
                }
                for _ in 0..3 {
                    if self.attack_entity(id).await.is_err() {
                        break;
                    }
                    for _ in 0..10 {
                        self.drain_packets().await;
                        self.tick(0.07).await.ok();
                    }
                    if !self.entities.alive.contains_key(&id) {
                        break;
                    }
                }
                true
            }
            None => false,
        }
    }

    /// Boucle de survie autonome : phases de progression + entretien, à l'infini.
    pub async fn run_mission(&mut self) {
        let mut echecs_consecutifs = 0;
        while !self.disconnected {
            // 1) Réflexes vitaux d'abord
            if self.self_defend().await {
                continue;
            }
            self.pickup_drops(6.0).await;
            self.eat_if_needed().await;

            // 2) Progression par paliers d'équipement
            let a_outil = |nom: &str| self.inventory.count(item_id(nom)) > 0;

            if a_outil("iron_pickaxe") {
                // Plein régime : chasse, cuisine, mange, recommence
                let viandes: i32 = CRUS_CUISSON
                    .iter()
                    .map(|(crus, _)| self.inventory.count(item_id(crus)))
                    .sum();
                let cuites: i32 = CRUS_CUISSON
                    .iter()
                    .map(|(_, cuit)| self.inventory.count(item_id(cuit)))
                    .sum();
                if viandes > 0 {
                    self.cook_all_meats().await;
                } else if cuites == 0 {
                    if !self.hunt(4).await {
                        println!("pas de gibier trouvé : pause");
                    } else {
                        // ramassage différé la prochaine itération
                    }
                }
            } else if a_outil("stone_pickaxe") {
                if self.phase_iron().await {
                    println!("=== Équipement en fer atteint ===");
                } else {
                    echecs_consecutifs += 1;
                }
            } else if a_outil("wooden_pickaxe") {
                if self.phase_stone().await {
                    println!("=== Outillage pierre OK ===");
                } else {
                    echecs_consecutifs += 1;
                }
            } else {
                self.phase_wood().await;
            }
            if echecs_consecutifs >= 3 {
                println!("phase bloquée 3 fois de suite : pause 2 min");
                for _ in 0..240 {
                    if self.disconnected {
                        break;
                    }
                    self.self_defend().await;
                    self.drain_packets().await;
                    self.tick(0.05).await.ok();
                }
                echecs_consecutifs = 0;
            }
            for _ in 0..20 {
                if self.disconnected {
                    break;
                }
                self.drain_packets().await;
                self.tick(0.1).await.ok();
            }
        }
        println!("mission interrompue (déconnexion)");
    }
}
