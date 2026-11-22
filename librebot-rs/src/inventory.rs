//! Inventaire du joueur (fenêtre 0) : slots, set_slot, clics.
//! Layout vanilla fenêtre 0 : 0 résultat craft, 1-4 grille 2x2, 5-8 armure,
//! 9-35 stockage, 36-44 hotbar, 45 main secondaire.

use crate::nbt;
use crate::protocol::Body;

pub const HOTBAR_FIRST: usize = 36;
pub const HOTBAR_LAST: usize = 44;
pub const OFFHAND: usize = 45;
pub const STORAGE_FIRST: usize = 9;
/// Nombre de slots joueur dans une fenêtre de conteneur (27 stockage + 9 hotbar + 1 offhand).
const PLAYER_SECTION: usize = 37;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemStack {
    pub id: i32,
    pub count: i32,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub slots: Vec<Option<ItemStack>>,
    pub state_id: i32,
    pub carried: Option<ItemStack>,
    /// Dernière fenêtre non-joueur lue (id, nombre de slots) — pour mapper set_slot.
    last_container: Option<(u8, usize)>,
}

/// Erreur de décodage : l'inventaire reste utilisable partiellement.
#[derive(Debug)]
pub enum InventoryError {
    SlotWithComponents(usize),
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Paquet `window_items` (0x13) : remplace l'inventaire.
    pub fn apply_window_items(&mut self, body: &[u8]) -> Result<(), InventoryError> {
        let mut b = Body(body);
        let window_id = b.u8();
        self.state_id = b.varint();
        let n = b.varint().max(0) as usize;
        let mut external: Vec<Option<ItemStack>> = Vec::with_capacity(n);
        for i in 0..n {
            match read_slot(&mut b) {
                Some(s) => external.push(s),
                None => return Err(InventoryError::SlotWithComponents(i)),
            }
        }
        self.carried = read_slot(&mut b).flatten();
        if window_id == 0 {
            self.slots = external;
        } else {
            self.last_container = Some((window_id, n));
            self.merge_container_slots(&external, n);
        }
        Ok(())
    }

    /// Paquet `set_slot` (0x15) : remplace un slot.
    pub fn apply_set_slot(&mut self, body: &[u8]) -> Result<(), InventoryError> {
        let mut b = Body(body);
        let window_id = b.u8();
        self.state_id = b.varint();
        let slot = b.i16() as i32;
        let item = read_slot(&mut b).flatten();
        if slot == -1 {
            self.carried = item;
            return Ok(());
        }
        if slot < 0 {
            return Ok(());
        }
        let internal = if window_id == 0 {
            Some(slot as usize)
        } else if window_id == 0xff {
            // window -1 : indexation "inventaire brut" (0-8 hotbar, 9-35 stockage, 36-39 armure, 40 offhand)
            match slot {
                0..=8 => Some(HOTBAR_FIRST + slot as usize),
                9..=35 => Some(slot as usize),
                36..=39 => Some(5 + (slot as usize - 36)),
                40 => Some(OFFHAND),
                _ => None,
            }
        } else {
            let n = match self.last_container {
                Some((w, n)) if w == window_id => n,
                _ => return Ok(()),
            };
            container_to_internal(slot as usize, n)
        };
        if let Some(idx) = internal {
            if idx >= self.slots.len() {
                self.slots.resize(idx + 1, None);
            }
            self.slots[idx] = item;
        }
        Ok(())
    }

    /// Copie la section joueur d'une fenêtre de conteneur (ex: table de craft 3x3)
    /// vers les slots internes. Les slots du conteneur lui-même sont ignorés.
    fn merge_container_slots(&mut self, external: &[Option<ItemStack>], n: usize) {
        for (e, item) in external.iter().enumerate() {
            if let Some(internal) = container_to_internal(e, n) {
                if internal >= self.slots.len() {
                    self.slots.resize(internal + 1, None);
                }
                self.slots[internal] = item.clone();
            }
        }
    }

    pub fn held(&self, hotbar_index: usize) -> Option<&ItemStack> {
        self.slots
            .get(HOTBAR_FIRST + hotbar_index)
            .and_then(|s| s.as_ref())
    }

    /// Convertit un index interne joueur en slot de la fenêtre `window_id`
    /// (pour la fenêtre 0, interne == slot ; pour un conteneur, la section joueur
    /// est en fin de fenêtre : inverse de `container_to_internal`).
    pub fn window_slot(&self, window_id: u8, internal: usize) -> Option<i16> {
        if window_id == 0 {
            return Some(internal as i16);
        }
        let (_, window_len) = self.last_container?;
        if window_len < PLAYER_SECTION {
            return None;
        }
        let from_end = match internal {
            OFFHAND => 0,
            h if (HOTBAR_FIRST..=HOTBAR_LAST).contains(&h) => 9 - (h - HOTBAR_FIRST),
            s if (STORAGE_FIRST..HOTBAR_FIRST).contains(&s) => 36 - (s - STORAGE_FIRST),
            _ => return None,
        };
        Some((window_len - 1 - from_end) as i16)
    }

    /// Slot interne contenant l'item donné (hotbar en priorité, puis stockage).
    pub fn find(&self, item_id: i32) -> Option<usize> {
        let hotbar = (HOTBAR_FIRST..=HOTBAR_LAST).filter(|&i| i < self.slots.len());
        let storage = (STORAGE_FIRST..HOTBAR_FIRST).filter(|&i| i < self.slots.len());
        let hotbar_set = HOTBAR_FIRST..=HOTBAR_LAST;
        let storage_set = STORAGE_FIRST..HOTBAR_FIRST;
        let rest = (0..self.slots.len()).filter(|&i| {
            !hotbar_set.contains(&i) && !storage_set.contains(&i)
        });
        hotbar
            .chain(storage)
            .chain(rest)
            .find(|&i| self.slots[i].as_ref().map(|s| s.id) == Some(item_id))
    }

    pub fn count(&self, item_id: i32) -> i32 {
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.id == item_id)
            .map(|s| s.count)
            .sum()
    }
}

/// Mappe un slot de fenêtre conteneur (fenêtre != 0) vers l'index interne joueur.
/// La section joueur est en fin de fenêtre : stockage, hotbar, offhand.
/// Retourne None pour les slots appartenant au conteneur lui-même.
fn container_to_internal(external: usize, window_len: usize) -> Option<usize> {
    if window_len < PLAYER_SECTION {
        return None;
    }
    let from_end = window_len - 1 - external;
    match from_end {
        0 => Some(OFFHAND),
        1..=9 => Some(HOTBAR_FIRST + (9 - from_end)),
        10..=36 => Some(STORAGE_FIRST + (36 - from_end)),
        _ => None,
    }
}

/// Décode un slot (protocole 767 / 1.21.1) :
/// itemCount varint ; si > 0 : itemId varint, composants ajoutés, composants retirés.
pub fn read_slot(b: &mut Body) -> Option<Option<ItemStack>> {
    let count = b.varint();
    if count == 0 {
        return Some(None);
    }
    let id = b.varint();
    let added = b.varint().max(0) as usize;
    let removed = b.varint().max(0) as usize;
    for _ in 0..added {
        if !skip_component(b) {
            return None;
        }
    }
    for _ in 0..removed {
        b.varint();
    }
    Some(Some(ItemStack { id, count }))
}

/// Saute la charge utile d'un composant d'item (protocole 767, registre SlotComponentType 0..=56).
/// Retourne false uniquement pour un type inconnu (longueur indécidable).
fn skip_component(b: &mut Body) -> bool {
    /// NBT anonyme.
    fn skip_nbt(b: &mut Body) -> bool {
        nbt::parse(&mut b.0).is_some()
    }
    /// Option<NBT anonyme> : bool puis NBT si présent.
    fn opt_nbt(b: &mut Body) -> bool {
        !b.bool() || skip_nbt(b)
    }
    /// registryEntryHolder : un varint (id de registre).
    fn holder(b: &mut Body) -> bool {
        b.varint();
        true
    }
    /// registryEntryHolderSet / IDSet : varint count puis count varints.
    fn holder_set(b: &mut Body) -> bool {
        let n = b.varint().max(0) as usize;
        for _ in 0..n {
            b.varint();
        }
        true
    }
    /// Tableau de slots.
    fn slot_array(b: &mut Body) -> bool {
        let n = b.varint().max(0) as usize;
        (0..n).all(|_| read_slot(b).is_some())
    }
    /// ItemBlockPredicate (can_place_on / can_break).
    fn block_predicate(b: &mut Body) -> bool {
        if b.bool() && !holder_set(b) {
            return false;
        }
        if b.bool() {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.string();
                let exact = b.bool();
                b.string();
                if !exact {
                    b.string();
                }
            }
        }
        opt_nbt(b)
    }
    /// ItemEffectDetail (potion_contents).
    fn effect_detail(b: &mut Body) -> bool {
        b.varint();
        b.varint();
        b.bool();
        b.bool();
        b.bool();
        if b.bool() {
            effect_detail(b)
        } else {
            true
        }
    }
    /// ItemFireworkExplosion (firework_explosion).
    fn explosion(b: &mut Body) -> bool {
        b.varint();
        let n = b.varint().max(0) as usize;
        for _ in 0..n {
            b.i32();
        }
        let n = b.varint().max(0) as usize;
        for _ in 0..n {
            b.i32();
        }
        b.bool();
        b.bool();
        true
    }

    let ctype = b.varint();
    match ctype {
        // anonymousNbt
        0 | 5 | 6 | 19 | 27 | 36 | 37 | 38 | 39 | 43 | 55 | 56 => skip_nbt(b),
        // varint
        1 | 2 | 3 | 8 | 13 | 16 | 26 | 28 | 40 | 41 | 50 => {
            b.varint();
            true
        }
        // bool
        4 | 18 => {
            b.bool();
            true
        }
        // void (rien sur le fil)
        14 | 15 | 17 | 21 => true,
        // i32
        25 => {
            b.i32();
            true
        }
        // string
        48 => {
            b.string();
            true
        }
        // lore : tableau d'option<NBT>
        7 => {
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| opt_nbt(b))
        }
        // enchantments / stored_enchantments : { {id, level}* , showTooltip }
        9 | 23 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                b.varint();
            }
            b.bool();
            true
        }
        // can_place_on / can_break : { predicates*, showTooltip }
        10 | 11 => {
            let n = b.varint().max(0) as usize;
            let ok = (0..n).all(|_| block_predicate(b));
            b.bool();
            ok
        }
        // attribute_modifiers : { {typeId, name, value, operation, slot}*, showTooltip }
        12 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                b.string();
                b.f64();
                b.varint();
                b.varint();
            }
            b.bool();
            true
        }
        // food : {nutrition, saturation, canAlwaysEat, secondsToEat, usingConvertsTo, effects*}
        20 => {
            b.varint();
            b.f32();
            b.bool();
            b.f32();
            if !read_slot(b).is_some() {
                return false;
            }
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                b.f32();
            }
            true
        }
        // tool : { rules* {blocks, speed?, correctDrop?}, defaultMiningSpeed, damagePerBlock }
        22 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                holder_set(b);
                if b.bool() {
                    b.f32();
                }
                if b.bool() {
                    b.bool();
                }
            }
            b.f32();
            b.varint();
            true
        }
        // dyed_color : {color i32, showTooltip}
        24 => {
            b.i32();
            b.bool();
            true
        }
        // charged_projectiles / bundle_contents / container : { slots* }
        29 | 30 | 52 => slot_array(b),
        // potion_contents : {potionId?, customColor?, customEffects*, customName?}
        31 => {
            if b.bool() {
                b.varint();
            }
            if b.bool() {
                b.i32();
            }
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                if !effect_detail(b) {
                    return false;
                }
            }
            if b.bool() {
                b.string();
            }
            true
        }
        // suspicious_stew_effects : { {effect, duration}* }
        32 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                b.varint();
            }
            true
        }
        // writable_book_content : { pages* {raw, filtered?} }
        33 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.string();
                if b.bool() {
                    b.string();
                }
            }
            true
        }
        // written_book_content : {rawTitle, filteredTitle?, author, generation, pages*, resolved}
        34 => {
            b.string();
            if b.bool() {
                b.string();
            }
            b.string();
            b.varint();
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                if !skip_nbt(b) {
                    return false;
                }
                if !opt_nbt(b) {
                    return false;
                }
            }
            b.bool();
            true
        }
        // trim : {material holder, pattern holder, showInTooltip}
        35 => {
            holder(b) && holder(b) && {
                b.bool();
                true
            }
        }
        // jukebox_playable : {hasHolder, song (holder si true, string sinon), showInTooltip}
        42 => {
            let has = b.bool();
            if has {
                b.varint();
            } else {
                b.string();
            }
            b.bool();
            true
        }
        // lodestone_tracker : {globalPosition? {dimension, position}, tracked}
        44 => {
            if b.bool() {
                b.string();
                b.i64();
            }
            b.bool();
            true
        }
        // firework_explosion
        45 => explosion(b),
        // fireworks : {flightDuration, explosions*}
        46 => {
            b.varint();
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| explosion(b))
        }
        // profile : {name?, uuid?, properties* {name, value, signature?}}
        47 => {
            if b.bool() {
                b.string();
            }
            if b.bool() {
                b.take(16);
            }
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.string();
                b.string();
                if b.bool() {
                    b.string();
                }
            }
            true
        }
        // banner_patterns : { layers* {pattern holder, colorId} }
        49 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
                b.varint();
            }
            true
        }
        // pot_decorations : { decorations* varint }
        51 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.varint();
            }
            true
        }
        // block_state : { properties* {property, value} }
        53 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                b.string();
                b.string();
            }
            true
        }
        // bees : { bees* {nbt, ticksInHive, minTicksInHive} }
        54 => {
            let n = b.varint().max(0) as usize;
            for _ in 0..n {
                if !skip_nbt(b) {
                    return false;
                }
                b.varint();
                b.varint();
            }
            true
        }
        _ => false,
    }
}

/// Saute un slot complet (utilisé pour ignorer les ingrédients des recettes).
/// Retourne false si le slot contient un composant non supporté.
pub fn skip_slot(b: &mut Body) -> bool {
    read_slot(b).is_some()
}

/// Écrit un slot dans un corps de paquet (count varint ; si > 0 : id, 0 composants ajoutes, 0 retires).
pub fn write_slot(out: &mut Vec<u8>, slot: Option<(i32, i32)>) {
    match slot {
        None => crate::protocol::write_varint(0, out),
        Some((id, count)) => {
            crate::protocol::write_varint(count, out);
            crate::protocol::write_varint(id, out);
            crate::protocol::write_varint(0, out);
            crate::protocol::write_varint(0, out);
        }
    }
}

/// Corps d'un clic de fenêtre (container_click 767) avec l'item curseur réel du bot.
pub fn window_click_body(
    window_id: u8,
    state_id: i32,
    slot: i16,
    button: i8,
    mode: i32,
    carried: Option<(i32, i32)>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(window_id);
    crate::protocol::write_varint(state_id, &mut out);
    out.extend_from_slice(&slot.to_be_bytes());
    out.push(button as u8);
    crate::protocol::write_varint(mode, &mut out);
    crate::protocol::write_varint(0, &mut out); // changedSlots vide
    write_slot(&mut out, carried); // cursorItem
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::write_varint;

    fn slot_bytes(id: Option<(i32, i32)>) -> Vec<u8> {
        match id {
            None => {
                let mut b = Vec::new();
                write_varint(0, &mut b);
                b
            }
            Some((id, count)) => {
                let mut b = Vec::new();
                write_varint(count, &mut b);
                write_varint(id, &mut b);
                write_varint(0, &mut b); // composants ajoutés
                write_varint(0, &mut b); // composants retirés
                b
            }
        }
    }

    #[test]
    fn lecture_slots_767() {
        let mut body = Vec::new();
        body.push(0); // window id
        write_varint(0, &mut body); // state id
        write_varint(3, &mut body); // 3 slots
        body.extend_from_slice(&slot_bytes(None));
        body.extend_from_slice(&slot_bytes(Some((324, 32)))); // item 324 x32
        body.extend_from_slice(&slot_bytes(None));
        body.extend_from_slice(&slot_bytes(None)); // carried vide
        let mut inv = Inventory::new();
        inv.apply_window_items(&body).unwrap();
        assert_eq!(inv.slots.len(), 3);
        assert_eq!(inv.slots[1].as_ref().map(|s| s.count), Some(32));
        assert_eq!(inv.slots[1].as_ref().map(|s| s.id), Some(324));
    }

    #[test]
    fn find_hotbar_prioritaire() {
        let mut inv = Inventory::new();
        inv.slots.resize(46, None);
        inv.slots[10] = Some(ItemStack { id: 7, count: 1 }); // stockage
        inv.slots[40] = Some(ItemStack { id: 7, count: 1 }); // hotbar
        assert_eq!(inv.find(7), Some(40));
        assert_eq!(inv.held(4).map(|s| s.id), Some(7));
    }

    #[test]
    fn mapping_fenetre_conteneur() {
        // Table 3x3 : 10 slots conteneur + 37 joueur
        for e in 10..=46 {
            let internal = container_to_internal(e, 47).unwrap();
            assert!(
                internal == OFFHAND || (STORAGE_FIRST..=HOTBAR_LAST).contains(&internal),
                "e={} -> {:?}",
                e,
                internal
            );
        }
        assert_eq!(container_to_internal(10, 47), Some(9)); // 1er slot joueur = début stockage
        assert_eq!(container_to_internal(46, 47), Some(45)); // dernier = offhand
        assert_eq!(container_to_internal(9, 47), None); // slot du conteneur
    }

    #[test]
    fn window_slot_inverse_de_container_to_internal() {
        let mut inv = Inventory::new();
        inv.slots.resize(46, None);
        inv.last_container = Some((1, 47)); // table de craft : 47 slots
        // fenêtre 0 : identité
        assert_eq!(inv.window_slot(0, 40), Some(40));
        // table 3x3 : la section joueur (37 slots) est en fin de fenêtre
        for internal in [9usize, 20, 35, 36, 40, 44, 45] {
            let w = inv.window_slot(1, internal).unwrap();
            assert_eq!(
                container_to_internal(w as usize, 47),
                Some(internal),
                "internal {internal} -> window {w}"
            );
        }
        assert_eq!(inv.window_slot(1, 40), Some(41));
    }
}
