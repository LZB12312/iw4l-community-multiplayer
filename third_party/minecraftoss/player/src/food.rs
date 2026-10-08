//! Default-item food components measured from the pinned 26.3 vanilla registry.
//! `FoodUse` follows Consumable.startConsuming/shouldEmitParticlesAndSounds/onConsume.
use crate::{
    GameMode,
    inventory::{Inventory, ItemStack},
    survival::{EffectKind, FoodData, SurvivalStatus},
};
use std::{collections::HashMap, sync::OnceLock};

#[derive(Clone, Debug)]
pub struct FoodInfo {
    pub nutrition: u8,
    pub saturation: f32,
    pub can_always_eat: bool,
    pub consume_ticks: u32,
    pub animation: String,
    pub sound: String,
    pub particles: bool,
    pub remainder: Option<ItemStack>,
}

pub fn catalog() -> &'static HashMap<String, FoodInfo> {
    static CATALOG: OnceLock<HashMap<String, FoodInfo>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../data/food-26.3.json"))
                .expect("valid measured food catalog");
        assert_eq!(document["minecraft_version"], "26.3");
        document["items"]
            .as_object()
            .expect("food entries")
            .iter()
            .map(|(id, value)| {
                let saturation_bits = u32::from_str_radix(
                    value["saturation_bits"].as_str().expect("saturation bits"),
                    16,
                )
                .expect("hex saturation bits");
                let remainder = value.get("remainder").map(|rem| {
                    ItemStack::new(
                        rem["id"].as_str().expect("remainder id"),
                        rem["count"].as_u64().expect("remainder count") as u8,
                    )
                });
                (
                    id.clone(),
                    FoodInfo {
                        nutrition: value["nutrition"].as_u64().expect("nutrition") as u8,
                        saturation: f32::from_bits(saturation_bits),
                        can_always_eat: value["can_always_eat"].as_bool().expect("can_always_eat"),
                        consume_ticks: value["consume_ticks"].as_u64().expect("consume_ticks")
                            as u32,
                        animation: value["animation"].as_str().expect("animation").to_owned(),
                        sound: value["sound"]
                            .as_str()
                            .expect("sound")
                            .trim_start_matches("minecraft:")
                            .to_owned(),
                        particles: value["particles"].as_bool().expect("particles"),
                        remainder,
                    },
                )
            })
            .collect()
    })
}

/// Default 26.3 Consumables.onConsume effects. The boolean requests the
/// separate collision-checked random teleport used by chorus fruit.
pub fn apply_consumed_food_effects(
    id: &str,
    status: &mut SurvivalStatus,
    mut next_random: impl FnMut() -> f32,
) -> bool {
    use EffectKind::*;
    match id {
        "minecraft:chicken" if next_random() < 0.3 => status.add_effect(Hunger, 600, 0),
        "minecraft:poisonous_potato" if next_random() < 0.6 => status.add_effect(Poison, 100, 0),
        "minecraft:rotten_flesh" if next_random() < 0.8 => status.add_effect(Hunger, 600, 0),
        "minecraft:spider_eye" => status.add_effect(Poison, 100, 0),
        "minecraft:pufferfish" => {
            status.add_effect(Poison, 1200, 1);
            status.add_effect(Hunger, 300, 2);
            status.add_effect(Nausea, 300, 0);
        }
        "minecraft:golden_apple" => {
            status.add_effect(Regeneration, 100, 1);
            status.add_effect(Absorption, 2400, 0);
        }
        "minecraft:enchanted_golden_apple" => {
            status.add_effect(Regeneration, 400, 1);
            status.add_effect(Resistance, 6000, 0);
            status.add_effect(FireResistance, 6000, 0);
            status.add_effect(Absorption, 2400, 3);
        }
        "minecraft:honey_bottle" => status.remove_effect(Poison),
        "minecraft:chorus_fruit" => return true,
        _ => {}
    }
    false
}

#[derive(Clone, Debug)]
pub struct FoodUse {
    pub slot: usize,
    pub stack: ItemStack,
    pub info: FoodInfo,
    pub elapsed_ticks: u32,
}

#[derive(Debug, PartialEq)]
pub enum FoodUseTick {
    Continuing { emit_sound: bool },
    Finished { overflow: Option<ItemStack> },
    Cancelled,
}

impl FoodUse {
    pub fn start(slot: usize, stack: &ItemStack, food: &FoodData, mode: GameMode) -> Option<Self> {
        let info = catalog().get(&stack.id)?.clone();
        if stack.count == 0
            || (mode != GameMode::Creative && food.level >= 20 && !info.can_always_eat)
        {
            return None;
        }
        Some(Self {
            slot,
            stack: stack.clone(),
            info,
            elapsed_ticks: 0,
        })
    }

    pub fn remaining_ticks(&self) -> u32 {
        self.info.consume_ticks.saturating_sub(self.elapsed_ticks)
    }

    pub fn tick(
        &mut self,
        inventory: &mut Inventory,
        food: &mut FoodData,
        mode: GameMode,
    ) -> FoodUseTick {
        let Some(current) = inventory.slots.get(self.slot).and_then(Option::as_ref) else {
            return FoodUseTick::Cancelled;
        };
        if !current.same_item(&self.stack) || current.count == 0 {
            return FoodUseTick::Cancelled;
        }
        self.elapsed_ticks += 1;
        let remaining = self.remaining_ticks();
        if remaining > 0 {
            // Consumable uses elapsed > floor(duration * .21875), then every fourth remaining tick.
            let lead_in = (self.info.consume_ticks as f32 * 0.21875) as u32;
            return FoodUseTick::Continuing {
                emit_sound: self.elapsed_ticks > lead_in && remaining % 4 == 0,
            };
        }
        food.eat(self.info.nutrition, self.info.saturation);
        let mut overflow = None;
        if mode != GameMode::Creative {
            let current = inventory.slots[self.slot].as_mut().expect("validated slot");
            current.count -= 1;
            if current.count == 0 {
                inventory.slots[self.slot] = self.info.remainder.clone();
            } else if let Some(remainder) = &self.info.remainder {
                overflow = inventory.add_item(remainder.clone(), self.slot);
            }
        }
        FoodUseTick::Finished { overflow }
    }
}
