use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub output: String,
    pub output_count: u32,
    pub inputs: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockInfo {
    pub hardness: f32,
    pub needs_tool: Option<String>,
    pub solid: bool,
    pub opaque: bool,
}

pub struct Knowledge {
    pub recipes: HashMap<String, Recipe>,
    pub blocks: HashMap<String, BlockInfo>,
}

impl Default for Knowledge {
    fn default() -> Self {
        Self::vanilla()
    }
}

impl Knowledge {
    pub fn vanilla() -> Self {
        let mut recipes = HashMap::new();
        let mut add = |output: &str, count: u32, inputs: &[(&str, u32)]| {
            recipes.insert(
                output.to_string(),
                Recipe {
                    output: output.to_string(),
                    output_count: count,
                    inputs: inputs
                        .iter()
                        .map(|(i, n)| (i.to_string(), *n))
                        .collect(),
                },
            );
        };
        add("minecraft:oak_planks", 4, &[("minecraft:oak_log", 1)]);
        add("minecraft:stick", 4, &[("minecraft:oak_planks", 2)]);
        add("minecraft:crafting_table", 1, &[("minecraft:oak_planks", 4)]);
        add(
            "minecraft:wooden_pickaxe",
            1,
            &[("minecraft:oak_planks", 3), ("minecraft:stick", 2)],
        );
        add(
            "minecraft:stone_pickaxe",
            1,
            &[("minecraft:cobblestone", 3), ("minecraft:stick", 2)],
        );
        add("minecraft:furnace", 1, &[("minecraft:cobblestone", 8)]);
        add(
            "minecraft:torch",
            4,
            &[("minecraft:coal", 1), ("minecraft:stick", 1)],
        );
        add(
            "minecraft:chest",
            1,
            &[("minecraft:oak_planks", 8)],
        );

        let mut blocks = HashMap::new();
        let mut add_block = |name: &str, hardness: f32, needs_tool: Option<&str>, solid: bool, opaque: bool| {
            blocks.insert(
                name.to_string(),
                BlockInfo {
                    hardness,
                    needs_tool: needs_tool.map(|s| s.to_string()),
                    solid,
                    opaque,
                },
            );
        };
        add_block("minecraft:oak_log", 2.0, None, true, true);
        add_block("minecraft:oak_planks", 2.0, None, true, true);
        add_block("minecraft:crafting_table", 2.5, None, true, false);
        add_block("minecraft:stone", 6.0, Some("pickaxe"), true, true);
        add_block("minecraft:cobblestone", 6.0, Some("pickaxe"), true, true);
        add_block("minecraft:dirt", 0.5, None, true, false);
        add_block("minecraft:grass_block", 0.6, None, true, false);
        add_block("minecraft:air", 0.0, None, false, false);

        Self { recipes, blocks }
    }

    pub fn recipe(&self, item: &str) -> Option<&Recipe> {
        self.recipes.get(item)
    }

    pub fn can_craft(&self, item: &str, has: impl Fn(&str) -> u32) -> bool {
        match self.recipes.get(item) {
            Some(r) => r.inputs.iter().all(|(i, n)| has(i) >= *n),
            None => false,
        }
    }

    pub fn missing(&self, item: &str, has: impl Fn(&str) -> u32) -> Vec<(String, u32)> {
        match self.recipes.get(item) {
            Some(r) => r
                .inputs
                .iter()
                .filter_map(|(i, n)| {
                    let have = has(i);
                    if have < *n {
                        Some((i.clone(), n - have))
                    } else {
                        None
                    }
                })
                .collect(),
            None => vec![(item.to_string(), 1)],
        }
    }

    pub fn block_info(&self, block: &str) -> Option<&BlockInfo> {
        self.blocks.get(block)
    }
}
