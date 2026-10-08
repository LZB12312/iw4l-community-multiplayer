//! Crops as villagers tend them (26.3 `CropBlock` and its kinds): their
//! ages, what planting a seed makes, and bone meal's growth.
use minecraftoss_player::{Block, rng::LegacyRandom};

/// `CropBlock.getMaxAge` for the crop kinds (`CarrotBlock`, `PotatoBlock`,
/// `BeetrootBlock`, `TorchflowerCropBlock`, and wheat itself); none for
/// other blocks. The torchflower crop's last age is the torchflower, so it
/// never stands at it.
pub fn max_age(id: &str) -> Option<i32> {
    match id {
        "minecraft:wheat" | "minecraft:carrots" | "minecraft:potatoes" => Some(7),
        "minecraft:beetroots" => Some(3),
        "minecraft:torchflower_crop" => Some(2),
        _ => None,
    }
}

/// `CropBlock.getAge`.
pub fn age(block: &Block) -> i32 {
    block
        .property("age")
        .and_then(|a| a.parse().ok())
        .unwrap_or(0)
}

/// `block instanceof CropBlock crop && crop.isMaxAge(state)`.
pub fn ripe(block: &Block) -> bool {
    max_age(&block.id).is_some_and(|max| age(block) >= max)
}

/// `block instanceof CropBlock crop && !crop.isMaxAge(state)`.
pub fn growing(block: &Block) -> bool {
    max_age(&block.id).is_some_and(|max| age(block) < max)
}

/// `BlockItem.getBlock().defaultBlockState()` for the plantable seeds.
pub fn planted(item: &str) -> Option<Block> {
    let crop = match item {
        "minecraft:wheat_seeds" => "minecraft:wheat",
        "minecraft:carrot" => "minecraft:carrots",
        "minecraft:potato" => "minecraft:potatoes",
        "minecraft:beetroot_seeds" => "minecraft:beetroots",
        "minecraft:torchflower_seeds" => "minecraft:torchflower_crop",
        "minecraft:pitcher_pod" => {
            return Some(
                Block::new("minecraft:pitcher_crop")
                    .with("age", "0")
                    .with("half", "lower"),
            );
        }
        _ => return None,
    };
    Some(Block::new(crop).with("age", "0"))
}

/// `CropBlock.growCrops` with bone meal: the age grows by
/// `getBonemealAgeIncrease` (`Mth.nextInt(random, 2, 5)` from the level
/// random; a third of that for beetroots, one for the torchflower crop), up
/// to its last (`getStateForAge`: the torchflower crop's is the torchflower).
pub fn bonemealed(block: &Block, random: &mut LegacyRandom) -> Block {
    let increase = match block.id.as_str() {
        "minecraft:torchflower_crop" => 1,
        "minecraft:beetroots" => (random.next_int(4) as i32 + 2) / 3,
        _ => random.next_int(4) as i32 + 2,
    };
    let max = max_age(&block.id).unwrap_or(7);
    let grown = (age(block) + increase).min(max);
    if block.id == "minecraft:torchflower_crop" && grown == 2 {
        return Block::new("minecraft:torchflower");
    }
    block.clone().with("age", &grown.to_string())
}
