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

pub(crate) fn neighbors(world: &World, n: Node) -> Vec<(Node, f64)> {
    let mut out = Vec::with_capacity(6);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let x = n.0 + dx;
        let z = n.2 + dz;
        if walkable(world, x, n.1, z) {
            out.push(((x, n.1, z), 1.0));
        }
        // Montée d'un bloc (marche + saut).
        if walkable(world, x, n.1 + 1, z)
            && !world.is_solid(n.0, n.1 + 1, n.2)
            && !world.is_solid(n.0, n.1 + 2, n.2)
        {
            out.push(((x, n.1 + 1, z), 1.6));
        }
        // Chutes jusqu'à 3 blocs (sans dégâts).
        for dy in 1..=3 {
            let y = n.1 - dy;
            if walkable(world, x, y, z)
                && (y + 1..n.1).all(|yy| !world.is_solid(x, yy, z))
            {
                out.push(((x, y, z), 1.0 + 0.3 * dy as f64));
            }
        }
        // Grandes chutes (jusqu'à 16 blocs, ~6,5 coeurs de dégâts max, survivable) :
        // coût très élevé pour n'y recourir qu'en dernier recours (ex: redescendre d'un toit).
        for dy in 4..=16 {
            let y = n.1 - dy;
            if walkable(world, x, y, z)
                && (y + 1..n.1).all(|yy| !world.is_solid(x, yy, z))
            {
                out.push(((x, y, z), 1.0 + 3.0 * dy as f64));
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

/// Résultat détaillé de `find_path` (diagnostics).
pub(crate) struct PathDebug {
    pub path: Option<Vec<Node>>,
    pub expanded: usize,
    pub budget_exceeded: bool,
}

/// Chemin du `start` (exclu) au `goal` (inclus). `None` si introuvable ou budget dépassé.
pub fn find_path(world: &World, start: Node, goal: Node, max_nodes: usize) -> Option<Vec<Node>> {
    find_path_dbg(world, start, goal, max_nodes).path
}

pub(crate) fn find_path_dbg(world: &World, start: Node, goal: Node, max_nodes: usize) -> PathDebug {
    if start == goal {
        return PathDebug { path: Some(Vec::new()), expanded: 0, budget_exceeded: false };
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
            return PathDebug { path: Some(path), expanded, budget_exceeded: false };
        }
        expanded += 1;
        if expanded > max_nodes {
            return PathDebug { path: None, expanded, budget_exceeded: true };
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
    PathDebug { path: None, expanded, budget_exceeded: false }
}
