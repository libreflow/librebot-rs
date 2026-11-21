//! Inventaire du joueur (fenêtre 0) : slots, set_slot, clics.
//! Slots vanilla : 0-8 hotbar, 9-35 stockage, 36-39 armure, 40 off-hand.

use crate::protocol::Body;

pub const HOTBAR_SLOTS: usize = 9;

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

    /// Paquet `window_items` (0x13) : remplace tout l'inventaire.
    pub fn apply_window_items(&mut self, body: &[u8]) -> Result<(), InventoryError> {
        let mut b = Body(body);
        let _window_id = b.u8();
        self.state_id = b.varint();
        let n = b.varint() as usize;
        let mut slots = Vec::with_capacity(n);
        for i in 0..n {
            match read_slot(&mut b) {
                Some(s) => slots.push(s),
                None => return Err(InventoryError::SlotWithComponents(i)),
            }
        }
        self.carried = read_slot(&mut b).flatten();
        self.slots = slots;
        Ok(())
    }

    /// Paquet `set_slot` (0x15) : remplace un slot.
    pub fn apply_set_slot(&mut self, body: &[u8]) -> Result<(), InventoryError> {
        let mut b = Body(body);
        let _window_id = b.u8();
        self.state_id = b.varint();
        let slot = b.i16() as i32;
        let item = read_slot(&mut b).flatten();
        if slot >= 0 {
            if (slot as usize) >= self.slots.len() {
                self.slots.resize(slot as usize + 1, None);
            }
            self.slots[slot as usize] = item;
        } else if slot == -1 {
            self.carried = item;
        }
        Ok(())
    }

    pub fn held(&self, hotbar_index: usize) -> Option<&ItemStack> {
        self.slots.get(hotbar_index).and_then(|s| s.as_ref())
    }

    /// Premier slot (hotbar en priorité) contenant l'item donné.
    pub fn find(&self, item_id: i32) -> Option<usize> {
        (0..HOTBAR_SLOTS.min(self.slots.len()))
            .chain(HOTBAR_SLOTS..self.slots.len())
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

/// Décode un slot (protocole 767 / 1.21.1) : bool present ; si true → itemId(varint) + count(i8) + NBT.
pub fn read_slot(b: &mut Body) -> Option<Option<ItemStack>> {
    let present = b.bool();
    if !present {
        return Some(None);
    }
    let id = b.varint();
    let count = b.u8() as i32; // count = i8 en 767
    // Skip NBT (peut être présent)
    let _ = crate::nbt::parse(&mut b.0);
    Some(Some(ItemStack { id, count }))
}

/// Corps d'un clic de fenêtre : ramasser / poser un stack entier (mode 0)
/// ou moitié (bouton 1). Item curseur supposé vide côté bot.
pub fn window_click_body(
    window_id: u8,
    state_id: i32,
    slot: i16,
    button: i8,
    mode: i32,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(window_id);
    crate::protocol::write_varint(state_id, &mut out);
    out.extend_from_slice(&slot.to_be_bytes());
    out.push(button as u8);
    crate::protocol::write_varint(mode, &mut out);
    crate::protocol::write_varint(0, &mut out); // changedSlots vide
    crate::protocol::write_varint(0, &mut out); // cursorItem vide
    out
}
