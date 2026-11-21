//! Monde : réception des chunks (palets compacts) et interrogation des blocs.

use crate::blocks_gen;
use crate::protocol::{decode_varint, Body};
use std::collections::HashMap;

pub const MIN_Y: i32 = -64;
pub const WORLD_HEIGHT: i32 = 384;
pub const SECTION_COUNT: usize = 24;
const SECTION_VOLUME: usize = 16 * 16 * 16;

/// Conteneur paletted d'une section (blocs ou biomes).
pub struct Paletted {
    palette: Vec<u32>,
    single: Option<u32>,
    data: Vec<u64>,
    bits: u32,
}

impl Paletted {
    /// `idx` : ordre Y*(256) + Z*(16) + X dans la section.
    pub fn get(&self, idx: usize, _count: usize) -> u32 {
        if let Some(v) = self.single {
            return v;
        }
        if self.bits == 0 || self.data.is_empty() {
            return 0;
        }
        let epl = (64 / self.bits as usize).max(1);
        let long = self.data[idx / epl];
        let shift = (idx % epl) * self.bits as usize;
        let value = (long >> shift) as u32 & ((1u32 << self.bits) - 1);
        if self.palette.is_empty() {
            value
        } else {
            *self.palette.get(value as usize).unwrap_or(&0)
        }
    }
}

pub struct Chunk {
    pub sections: Vec<Paletted>,
}

/// Indexation Y-major comme vanilla : (y & 15) * 256 + (z & 15) * 16 + (x & 15).
fn section_index(x: i32, y: i32, z: i32) -> usize {
    ((y & 15) as usize) * 256 + ((z & 15) as usize) * 16 + (x & 15) as usize
}

#[derive(Default)]
pub struct World {
    chunks: HashMap<(i32, i32), Chunk>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Le chunk (cx, cz) est-il chargé ?
    pub fn has_chunk(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    /// Paquet `map_chunk` (0x2d en protocole 775).
    pub fn apply_map_chunk(&mut self, body: &[u8]) -> std::io::Result<()> {
        let mut b = Body(body);
        let x = b.i32();
        let z = b.i32();
        // heightmaps : composant NBT anonyme (protocole 767)
        if crate::nbt::parse(&mut b.0).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "heightmaps NBT illisibles",
            ));
        }
        // chunkData : ByteArray (varint length + bytes)
        let size = b.varint().max(0) as usize;
        let mut data = b.take(size);
        let mut sections = Vec::with_capacity(SECTION_COUNT);
        for _ in 0..SECTION_COUNT {
            let _block_count = data.u16();
            let blocks = read_paletted(&mut data, SECTION_VOLUME)?;
            let _biomes = read_paletted(&mut data, 64)?;
            sections.push(blocks);
        }
        self.chunks.insert((x, z), Chunk { sections });
        // Le reste (blockEntities, masques de lumière, skyLight, blockLight) est ignoré
        Ok(())
    }

    /// Paquet `unload_chunk` (0x21).
    pub fn apply_unload_chunk(&mut self, body: &[u8]) {
        let mut b = Body(body);
        let x = b.i32();
        let z = b.i32();
        self.chunks.remove(&(x, z));
    }

    /// ID d'état du bloc aux coordonnées monde. `None` si chunk non chargé ou hors monde.
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return None;
        }
        let chunk = self.chunks.get(&(x >> 4, z >> 4))?;
        let local_y = (y - MIN_Y) >> 4;
        let section = chunk.sections.get(local_y as usize)?;
        Some(section.get(section_index(x, y, z), SECTION_VOLUME))
    }

    pub fn is_solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.block_state(x, y, z)
            .map(blocks_gen::is_solid)
            .unwrap_or(false)
    }

    pub fn block_name(&self, x: i32, y: i32, z: i32) -> Option<&'static str> {
        self.block_state(x, y, z).map(blocks_gen::block_name)
    }

    /// Remplace l'état d'un bloc (paquet `block_change` et pose future).
    /// La section est reconstruite en stockage direct 15 bits.
    pub fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return false;
        }
        let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) else {
            return false;
        };
        let local_y = ((y - MIN_Y) >> 4) as usize;
        let idx = section_index(x, y, z);
        let old = chunk.sections[local_y].get(idx, SECTION_VOLUME);
        if old == state {
            return true;
        }
        let bits = 15u32;
        let epl = 64 / bits as usize;
        let mut data = vec![0u64; SECTION_VOLUME.div_ceil(epl)];
        for i in 0..SECTION_VOLUME {
            let v = if i == idx {
                state
            } else {
                chunk.sections[local_y].get(i, SECTION_VOLUME)
            } as u64;
            data[i / epl] |= v << ((i % epl) * bits as usize);
        }
        chunk.sections[local_y] = Paletted {
            palette: Vec::new(),
            single: None,
            data,
            bits,
        };
        true
    }
}

fn read_paletted(b: &mut Body, count: usize) -> std::io::Result<Paletted> {
    let bits = b.u8() as u32;
    if bits == 0 {
        return Ok(Paletted {
            palette: Vec::new(),
            single: Some(b.varint() as u32),
            data: Vec::new(),
            bits: 0,
        });
    }
    let mut palette = Vec::new();
    if bits <= 8 {
        let n = b.varint() as usize;
        for _ in 0..n {
            palette.push(b.varint() as u32);
        }
    }
    let epl = if bits >= 64 { 1 } else { 64 / bits as usize };
    if epl == 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bits invalide")); }
    let longs = count.div_ceil(epl);
    let mut data = Vec::with_capacity(longs);
    for _ in 0..longs {
        data.push(b.i64() as u64);
    }
    Ok(Paletted {
        palette,
        single: None,
        data,
        bits,
    })
}

/// Utilitaire : décode un VarInt au début de `buf` (exposé pour les tests).
pub fn varint(buf: &mut &[u8]) -> i32 {
    decode_varint(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_section(bits: u8, palette: &[u32], first_value: u32) -> Vec<u8> {
        let mut out = vec![bits];
        crate::protocol::write_varint(palette.len() as i32, &mut out);
        for p in palette {
            crate::protocol::write_varint(*p as i32, &mut out);
        }
        let epl = 64 / bits as usize;
        let longs = SECTION_VOLUME.div_ceil(epl);
        for i in 0..longs {
            let value = if i == 0 { first_value as u64 } else { 0 };
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    #[test]
    fn decode_chunk_et_requete() {
        let mut body = Vec::new();
        body.extend_from_slice(&0i32.to_be_bytes());
        body.extend_from_slice(&0i32.to_be_bytes());
        body.push(0x0a);
        body.push(0x00);
        let mut sections_blob = Vec::new();
        for s in 0..SECTION_COUNT {
            sections_blob.extend_from_slice(&4096u16.to_be_bytes());
            if s == 0 {
                sections_blob.extend_from_slice(&build_section(4, &[0, 1], 1));
            } else {
                sections_blob.push(0x00);
                sections_blob.push(0x00);
            }
            sections_blob.push(0x00);
            sections_blob.push(0x00);
        }
        crate::protocol::write_varint(sections_blob.len() as i32, &mut body);
        body.extend_from_slice(&sections_blob);

        let mut w = World::new();
        w.apply_map_chunk(&body).unwrap();
        assert_eq!(w.chunk_count(), 1);
        let stone = w.block_state(0, MIN_Y, 0).unwrap();
        assert_eq!(blocks_gen::block_name(stone), "stone");
        assert!(w.is_solid(0, MIN_Y, 0));
        assert!(!w.is_solid(5, MIN_Y, 5));
        assert!(!w.is_solid(0, 10000, 0));
    }
}
