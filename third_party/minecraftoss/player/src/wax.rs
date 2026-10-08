//! Pinned 26.3 honeycomb block conversion for dispenser and hand use.
use crate::Block;
use std::{collections::HashSet, sync::OnceLock};

pub fn waxed_block_id(id: &str) -> Option<String> {
    static WAXABLE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let known = WAXABLE.get_or_init(|| {
        include_str!("../data/waxable_blocks_26_3.txt")
            .lines()
            .filter(|line| line.starts_with("minecraft:"))
            .collect()
    });
    let path = id.strip_prefix("minecraft:")?;
    known
        .contains(id)
        .then(|| format!("minecraft:waxed_{path}"))
}

pub fn waxed_block(block: &Block) -> Option<Block> {
    let mut result = block.clone();
    result.id = waxed_block_id(&block.id)?;
    Some(result)
}
