//! Pathfinding A* sur la grille des blocs du monde.
//! Un nœud (x, y, z) désigne une position debout : solide en y-1, libre en y et y+1.

use crate::world::World;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub const MAX_EXPANSIONS: usize = 20_000;

type Node = (i32, i32, i32);

/// Clé ordonnée pour le tas : bits du coût f (toujours >= 0).
fn key(f: f64) -> u64 {
    debug_assert!(f.is_finite() && f >= 0.0);
    f.to_bits()
}

/// Cellule liquide (le bot ne nage pas : on évite de pathfinder dans l'eau/la lave).
fn is_liquid(world: &World, x: i32, y: i32, z: i32) -> bool {
    matches!(
        world.block_state(x, y, z).map(crate::blocks_gen::block_name),
        Some("water") | Some("lava")
    )
}

pub fn walkable(world: &World, x: i32, y: i32, z: i32) -> bool {
    world.is_solid(x, y - 1, z)
        && !world.is_solid(x, y, z)
        && !world.is_solid(x, y + 1, z)
        && !is_liquid(world, x, y, z)
        && !is_liquid(world, x, y + 1, z)
}

/// Coût de danger d'une cellule debout (route.md §3) : lave adjacente (forte
/// pénalité), eau adjacente (modérée), bord de falaise (au moins un voisin sans
/// sol en dessous, y compris chunk non chargé).
fn danger_cost(world: &World, x: i32, y: i32, z: i32) -> f64 {
    let mut cost = 0.0;
    let mut no_ground = 0;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (nx, nz) = (x + dx, z + dz);
        match world.block_state(nx, y, nz).map(crate::blocks_gen::block_name) {
            Some("lava") => cost += 6.0,
            Some("water") => cost += 1.5,
            _ => {}
        }
        if !world.is_solid(nx, y - 1, nz) {
            no_ground += 1;
        }
    }
    if no_ground >= 1 {
        cost += 2.5;
    }
    cost
}

fn neighbors(world: &World, n: Node) -> Vec<(Node, f64)> {
    let mut out = Vec::with_capacity(6);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let x = n.0 + dx;
        let z = n.2 + dz;
        if walkable(world, x, n.1, z) {
            out.push(((x, n.1, z), 1.0 + danger_cost(world, x, n.1, z)));
        }
        // Montée d'un bloc (marche + saut) : interdit de sauter au-dessus d'un
        // liquide (parkour au-dessus de la lave dans le Nether).
        if walkable(world, x, n.1 + 1, z)
            && !is_liquid(world, x, n.1, z)
            && !world.is_solid(n.0, n.1 + 1, n.2)
            && !world.is_solid(n.0, n.1 + 2, n.2)
        {
            out.push(((x, n.1 + 1, z), 1.6 + danger_cost(world, x, n.1 + 1, z)));
        }
        // Chutes jusqu'à 3 blocs : la colonne de chute ne doit contenir aucun liquide.
        for dy in 1..=3 {
            let y = n.1 - dy;
            if walkable(world, x, y, z)
                && (y + 1..n.1).all(|yy| !world.is_solid(x, yy, z) && !is_liquid(world, x, yy, z))
            {
                out.push(((x, y, z), 1.0 + 0.3 * dy as f64 + danger_cost(world, x, y, z)));
            }
        }
    }
    out
}

fn heuristic(n: Node, goal: Node) -> f64 {
    (n.0 - goal.0).abs() as f64
        + (n.2 - goal.2).abs() as f64
        + (n.1 - goal.1).abs() as f64 * 1.1
}

/// Chemin du `start` (exclu) au `goal` (inclus). `None` si introuvable ou budget dépassé.
pub fn find_path(world: &World, start: Node, goal: Node, max_nodes: usize) -> Option<Vec<Node>> {
    if start == goal {
        return Some(Vec::new());
    }
    let mut open: BinaryHeap<Reverse<(u64, Node)>> = BinaryHeap::new();
    let mut g: HashMap<Node, f64> = HashMap::new();
    let mut parent: HashMap<Node, Node> = HashMap::new();
    g.insert(start, 0.0);
    open.push(Reverse((key(heuristic(start, goal)), start)));
    let mut expanded = 0usize;

    while let Some(Reverse((_, current))) = open.pop() {
        if current == goal {
            let mut path = vec![goal];
            let mut c = goal;
            while let Some(&p) = parent.get(&c) {
                path.push(p);
                c = p;
            }
            path.reverse();
            path.remove(0);
            return Some(path);
        }
        expanded += 1;
        if expanded > max_nodes {
            return None;
        }
        let gc = *g.get(&current).unwrap_or(&f64::INFINITY);
        for (nb, cost) in neighbors(world, current) {
            let t = gc + cost;
            if t < *g.get(&nb).unwrap_or(&f64::INFINITY) {
                g.insert(nb, t);
                parent.insert(nb, current);
                let f = t + heuristic(nb, goal);
                open.push(Reverse((key(f), nb)));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks_gen::block_state_by_name;
    use crate::world::{World, MIN_Y};

    /// Insère un chunk tout-air (map_chunk minimal) pour que set_block_state marche.
    fn add_chunk(w: &mut World, cx: i32, cz: i32) {
        let mut body = Vec::new();
        body.extend_from_slice(&cx.to_be_bytes());
        body.extend_from_slice(&cz.to_be_bytes());
        body.push(0x0a);
        body.push(0x00);
        let mut blob = Vec::new();
        for _ in 0..24 {
            blob.extend_from_slice(&0u16.to_be_bytes()); // count = 0
            blob.push(0x00); // section : bits = 0 (single)
            crate::protocol::write_varint(0, &mut blob); // air
            blob.push(0x00); // biome : bits = 0
            crate::protocol::write_varint(0, &mut blob);
        }
        crate::protocol::write_varint(blob.len() as i32, &mut body);
        body.extend_from_slice(&blob);
        w.apply_map_chunk(&body).unwrap();
    }

    fn set(world: &mut World, name: &str, x: i32, y: i32, z: i32) {
        let s = block_state_by_name(name).unwrap();
        assert!(world.set_block_state(x, y, z, s));
    }

    /// Plateforme de pierre en y=0 sur [x0..x1] x [z0..z1], air au-dessus.
    fn platform(world: &mut World, x0: i32, x1: i32, z0: i32, z1: i32) {
        for x in x0..=x1 {
            for z in z0..=z1 {
                set(world, "stone", x, 0, z);
            }
        }
    }

    #[test]
    fn chute_a_travers_la_lave_interdite() {
        let mut w = World::new();
        for cx in [-1, 0] {
            for cz in [-1, 0] {
                add_chunk(&mut w, cx, cz);
            }
        }
        platform(&mut w, -2, 6, -2, 2);
        // Colonne de départ en (0, 1..=3) -> start (0, 4, 0)
        for y in 1..=3 {
            set(&mut w, "stone", 0, y, 0);
        }
        // Lave dans la colonne de chute vers (2, 1, 0)
        set(&mut w, "lava", 2, 2, 0);
        set(&mut w, "lava", 2, 3, 0);
        let path = find_path(&w, (0, 4, 0), (2, 1, 0), MAX_EXPANSIONS);
        assert!(path.is_none(), "aucune chute ne doit traverser la lave");
    }

    #[test]
    fn pas_de_montee_au_dessus_d_un_liquide() {
        let mut w = World::new();
        add_chunk(&mut w, 0, 0);
        platform(&mut w, 0, 3, 0, 0);
        set(&mut w, "lava", 1, 1, 0); // liquide au niveau des pieds du voisin
        set(&mut w, "stone", 1, 2, 0); // plateforme au-dessus
        let ns = neighbors(&w, (0, 1, 0));
        assert!(!ns.iter().any(|(n, _)| *n == (1, 2, 0)), "parkour au-dessus de la lave interdit");
    }

    #[test]
    fn danger_cost_lave_eau_bord() {
        let mut w = World::new();
        add_chunk(&mut w, 0, 0);
        platform(&mut w, 0, 6, 0, 6);
        // Centre : aucun voisin liquide, 4 voisins solides -> 0
        assert_eq!(danger_cost(&w, 3, 1, 3), 0.0);
        // Bord de falaise (z = -1 hors plateforme) -> pénalité
        assert!(danger_cost(&w, 3, 1, 0) >= 2.5);
        // Lave adjacente -> forte pénalité
        set(&mut w, "lava", 4, 1, 3);
        let c = danger_cost(&w, 3, 1, 3);
        assert!(c >= 6.0, "lave adjacente: {c}");
        // Eau adjacente -> pénalité modérée
        let mut w2 = World::new();
        add_chunk(&mut w2, 0, 0);
        platform(&mut w2, 0, 6, 0, 6);
        set(&mut w2, "water", 4, 1, 3);
        let c2 = danger_cost(&w2, 3, 1, 3);
        assert!(c2 >= 1.5 && c2 < 6.0, "eau adjacente: {c2}");
    }

    #[test]
    fn le_chemin_evite_le_bord_de_falaise_quand_cest_possible() {
        let mut w = World::new();
        add_chunk(&mut w, 0, 0);
        platform(&mut w, 0, 9, 0, 4);
        let path = find_path(&w, (0, 1, 2), (9, 1, 2), MAX_EXPANSIONS).expect("chemin existe");
        assert!(path.iter().all(|&(_, _, z)| z >= 1), "ne longe pas le bord z=0: {path:?}");
    }

    #[test]
    fn min_y_ne_panique_pas_sur_chunk_vide() {
        let w = World::new();
        assert!(!walkable(&w, 0, MIN_Y + 1, 0));
        // chunk non chargé : pénalité de bord, aucun panic
        assert!(danger_cost(&w, 0, MIN_Y + 1, 0) >= 2.5);
    }
}
