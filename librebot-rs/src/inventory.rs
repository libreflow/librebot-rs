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

    /// Slot interne contenant l'item donné (hotbar en priorité, puis stockage).
    /// Dernière fenêtre conteneur lue : (window_id, nombre de slots conteneur).
    pub fn container_slots(&self) -> Option<(u8, usize)> {
        self.last_container
    }

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

/// Saute la charge utile d'un composant d'item (protocole 767/768).
/// Retourne false uniquement pour un type totalement inconnu (longueur indecidable).
fn skip_component(b: &mut Body) -> bool {
    let ctype = b.varint();
    match ctype {
        // composants NBT (compound/chat)
        0 | 5 | 6 | 9 | 13 | 20 | 22 | 23 | 27 | 31 | 33 | 34 | 35 | 36 | 37 | 38 | 39 | 42
        | 44 | 45 | 46 | 47 | 48 | 53 | 55 => nbt::parse(&mut b.0).is_some(),
        // varint
        1 | 2 | 3 | 8 | 16 | 26 | 28 | 40 | 41 | 50 => {
            b.varint();
            true
        }
        // bool
        4 | 14 | 15 | 17 | 18 | 19 | 21 => {
            b.bool();
            true
        }
        // i32
        24 | 25 => {
            b.i32();
            true
        }
        // liste de composants NBT (lore, can_place_on, can_break, attribute_modifiers, ...)
        7 | 10 | 11 | 12 | 32 | 49 | 51 | 54 => {
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| nbt::parse(&mut b.0).is_some())
        }
        // liste de slots (charged_projectiles, bundle_contents, container)
        29 | 30 | 52 => {
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| read_slot(b).is_some())
        }
        // liste de strings (recipes)
        43 => {
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| {
                b.string();
                true
            })
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
}
