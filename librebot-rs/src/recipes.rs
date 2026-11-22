//! Table de recettes de craft simplifiée + helper item_id par nom.
//! Source : recettes vanilla 1.21.1/26.1 (crafting grid 2x2 ou 3x3).

use crate::items_gen::MC_ITEMS;

/// Trouve l'ID d'un item par son nom (ex: "oak_planks").
pub fn item_id(name: &str) -> i32 {
    MC_ITEMS
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("item inconnu: {}", name))
}

/// Vérifie si un nom d'item existe.
pub fn item_exists(name: &str) -> bool {
    MC_ITEMS.iter().any(|(_, n)| *n == name)
}

/// Pattern de la grille (row-major, None = cellule vide) pour les recettes shaped.
/// 9 cellules (3x3, table de craft) ou 4 cellules (2x2, inventaire).
/// `planks_id` = l'item de planches réellement disponible (toute essence convient).
/// Retourne None pour les recettes shapeless (placement séquentiel).
pub fn pattern_for(result_name: &str, planks_id: i32) -> Option<Vec<Option<i32>>> {
    let st = Some(item_id("stick"));
    let m_planks = Some(planks_id);
    let shape: Vec<Vec<Option<i32>>> = if result_name == "crafting_table" {
        // 2x2 rempli de planches
        vec![vec![m_planks, m_planks], vec![m_planks, m_planks]]
    } else if result_name == "stick" {
        // 2 planches en colonne verticale (slots 1 et 3 de la grille 2x2)
        vec![vec![m_planks, None], vec![m_planks, None]]
    } else if result_name.ends_with("_pickaxe") {
        let m = Some(if result_name.starts_with("wooden") { planks_id }
            else if result_name.starts_with("stone") { item_id("cobblestone") }
            else if result_name.starts_with("iron") { item_id("iron_ingot") }
            else if result_name.starts_with("golden") { item_id("gold_ingot") }
            else if result_name.starts_with("diamond") { item_id("diamond") }
            else { planks_id });
        vec![vec![m, m, m], vec![None, st, None], vec![None, st, None]]
    } else if result_name.ends_with("_axe") {
        let m = Some(if result_name.starts_with("wooden") { planks_id } else { item_id("cobblestone") });
        vec![vec![m, m, None], vec![m, st, None], vec![None, st, None]]
    } else if result_name.ends_with("_sword") {
        let m = Some(if result_name.starts_with("wooden") { planks_id } else { item_id("cobblestone") });
        vec![vec![m, None, None], vec![m, None, None], vec![None, st, None]]
    } else if result_name.ends_with("_shovel") {
        let m = Some(if result_name.starts_with("wooden") { planks_id } else { item_id("cobblestone") });
        vec![vec![m, None, None], vec![None, st, None], vec![None, st, None]]
    } else if result_name.ends_with("_hoe") {
        let m = Some(if result_name.starts_with("wooden") { planks_id } else { item_id("cobblestone") });
        vec![vec![m, m, None], vec![None, st, None], vec![None, st, None]]
    } else {
        return None;
    };
    Some(shape.into_iter().flatten().collect())
}

/// L'item de planches est-il une planche (n'importe quelle essence) ?
pub fn is_planks_id(id: i32) -> bool {
    crate::items_gen::item_name(id).ends_with("_planks")
}

/// Nom des planches produites par un type de bûche ("spruce_log" -> "spruce_planks").
pub fn planks_of_log(log_name: &str) -> Option<&'static str> {
    let base = log_name.strip_suffix("_log")?;
    let planks = format!("{}_planks", base);
    MC_ITEMS.iter().find(|(_, n)| **n == planks).map(|(_, n)| *n)
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub result: i32,
    pub result_count: i32,
    /// (item_id, count) nécessaires
    pub ingredients: Vec<(i32, i32)>,
    /// true = nécessite une table de craft (3x3), false = crafting 2x2 (inventaire)
    pub needs_table: bool,
}

/// Recettes vanilla essentielles pour la survie jusqu'au dragon.
/// Ordre : on essaie dans l'ordre, la première réalisable gagne.
pub fn recipe_for(result_name: &str) -> Option<Recipe> {
    if !item_exists(result_name) {
        return None;
    }
    let id = item_id(result_name);
    let r = match result_name {
        // --- Bois ---
        "oak_planks" | "spruce_planks" | "birch_planks" | "jungle_planks" | "acacia_planks"
        | "dark_oak_planks" | "mangrove_planks" | "cherry_planks" | "bamboo_planks" => Recipe {
            result: id, result_count: 4,
            ingredients: vec![(log_of_planks(result_name), 1)],
            needs_table: false,
        },
        "crafting_table" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 4)],
            needs_table: false,
        },
        "stick" => Recipe {
            result: id, result_count: 4,
            ingredients: vec![(item_id("oak_planks"), 2)],
            needs_table: false,
        },
        "wooden_pickaxe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "wooden_axe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "wooden_sword" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 2), (item_id("stick"), 1)],
            needs_table: true,
        },
        "wooden_shovel" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 1), (item_id("stick"), 2)],
            needs_table: true,
        },
        // --- Pierre ---
        "stone_pickaxe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("cobblestone"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "stone_axe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("cobblestone"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "stone_sword" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("cobblestone"), 2), (item_id("stick"), 1)],
            needs_table: true,
        },
        "stone_shovel" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("cobblestone"), 1), (item_id("stick"), 2)],
            needs_table: true,
        },
        "furnace" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("cobblestone"), 8)],
            needs_table: true,
        },
        // --- Fer ---
        "iron_pickaxe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "iron_axe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "iron_sword" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 2), (item_id("stick"), 1)],
            needs_table: true,
        },
        "iron_shovel" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 1), (item_id("stick"), 2)],
            needs_table: true,
        },
        "iron_helmet" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 5)],
            needs_table: true,
        },
        "iron_chestplate" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 8)],
            needs_table: true,
        },
        "iron_leggings" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 7)],
            needs_table: true,
        },
        "iron_boots" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 4)],
            needs_table: true,
        },
        "shield" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 1), (item_id("oak_planks"), 6)],
            needs_table: true,
        },
        "bucket" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 3)],
            needs_table: true,
        },
        "flint_and_steel" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("iron_ingot"), 1), (item_id("flint"), 1)],
            needs_table: false,
        },
        // --- Diamant ---
        "diamond_pickaxe" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 3), (item_id("stick"), 2)],
            needs_table: true,
        },
        "diamond_sword" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 2), (item_id("stick"), 1)],
            needs_table: true,
        },
        "diamond_helmet" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 5)],
            needs_table: true,
        },
        "diamond_chestplate" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 8)],
            needs_table: true,
        },
        "diamond_leggings" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 7)],
            needs_table: true,
        },
        "diamond_boots" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("diamond"), 4)],
            needs_table: true,
        },
        // --- Utilitaires ---
        "torch" => Recipe {
            result: id, result_count: 4,
            ingredients: vec![(item_id("coal"), 1), (item_id("stick"), 1)],
            needs_table: false,
        },
        "chest" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("oak_planks"), 8)],
            needs_table: true,
        },
        "bowl" => Recipe {
            result: id, result_count: 4,
            ingredients: vec![(item_id("oak_planks"), 3)],
            needs_table: true,
        },
        "bow" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("stick"), 3), (item_id("string"), 3)],
            needs_table: true,
        },
        "arrow" => Recipe {
            result: id, result_count: 4,
            ingredients: vec![(item_id("flint"), 1), (item_id("stick"), 1), (item_id("feather"), 1)],
            needs_table: true,
        },
        // --- Yeux de l'Ender ---
        "ender_eye" => Recipe {
            result: id, result_count: 1,
            ingredients: vec![(item_id("ender_pearl"), 1), (item_id("blaze_powder"), 1)],
            needs_table: false,
        },
        _ => return None,
    };
    Some(r)
}

/// Log correspondant aux planches demandées.
fn log_of_planks(planks: &str) -> i32 {
    let log = planks.replace("planks", "log");
    if item_exists(&log) {
        item_id(&log)
    } else {
        item_id("oak_log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recettes_connues() {
        let r = recipe_for("crafting_table").unwrap();
        assert_eq!(r.result, item_id("crafting_table"));
        assert_eq!(r.result_count, 1);
        assert!(!r.needs_table);

        let r = recipe_for("wooden_pickaxe").unwrap();
        assert!(r.needs_table);
        assert_eq!(r.ingredients.len(), 2);

        assert!(recipe_for("ender_eye").is_some());
        assert!(recipe_for("inexistant_xyz").is_none());
    }

    #[test]
    fn patterns_2x2_et_materiau() {
        let spruce = item_id("spruce_planks");
        // crafting_table : 2x2 plein de planches (4 cellules)
        let p = pattern_for("crafting_table", spruce).unwrap();
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|c| *c == Some(spruce)));
        // stick : colonne verticale (slots 1 et 3)
        let p = pattern_for("stick", spruce).unwrap();
        assert_eq!(p, vec![Some(spruce), None, Some(spruce), None]);
        // pioche en bois : 3x3 avec le matériau passé (pas forcément oak)
        let p = pattern_for("wooden_pickaxe", spruce).unwrap();
        assert_eq!(p.len(), 9);
        assert_eq!(p[0], Some(spruce));
        assert_eq!(p[4], Some(item_id("stick")));
        // planks : shapeless -> None
        assert!(pattern_for("oak_planks", spruce).is_none());
    }

    #[test]
    fn planks_of_log_mapping() {
        assert_eq!(planks_of_log("spruce_log"), Some("spruce_planks"));
        assert_eq!(planks_of_log("oak_log"), Some("oak_planks"));
        assert_eq!(planks_of_log("stone"), None);
        assert!(is_planks_id(item_id("birch_planks")));
        assert!(!is_planks_id(item_id("oak_log")));
    }
}
