//! Tick-driven furnace block entity. The caller owns its world position and
//! supplies the external 26.3 recipe book; this module owns item transactions.
use crate::{
    crafting::{CookingKind, RecipeBook},
    inventory::{Inventory, ItemStack, click_stack},
};

#[derive(Clone, Debug)]
pub struct Furnace {
    pub kind: CookingKind,
    /// Input, fuel, output.
    pub slots: [Option<ItemStack>; 3],
    pub lit_remaining: u32,
    pub lit_total: u32,
    pub cook_progress: u32,
    pub cook_total: u32,
    pub speed_multiplier: f32,
    pub completed_recipes: u32,
}

impl Default for Furnace {
    fn default() -> Self {
        Self {
            kind: CookingKind::Furnace,
            slots: [None, None, None],
            lit_remaining: 0,
            lit_total: 0,
            cook_progress: 0,
            cook_total: 0,
            speed_multiplier: 1.0,
            completed_recipes: 0,
        }
    }
}

fn cooking_ticks(base: u32, speed: f32) -> u32 {
    if speed > 0.0 {
        ((base as f32 / speed).ceil()) as u32
    } else {
        base
    }
}

impl Furnace {
    pub fn new(kind: CookingKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    pub fn is_lit(&self) -> bool {
        self.lit_remaining > 0
    }

    /// Apply a cooking recipe-book placement as one inventory transaction.
    /// Vanilla clears input and output into the player inventory, leaves fuel
    /// alone, then moves the selected ingredient into input slot zero.
    pub fn place_recipe(
        &mut self,
        recipe_id: &str,
        inventory: &mut Inventory,
        use_max_items: bool,
    ) -> bool {
        if !inventory.unlocked_recipes.contains(recipe_id) {
            return false;
        }
        let recipes = inventory.recipes.clone();
        if !recipes
            .cooking_recipes(self.kind)
            .any(|recipe| recipe.id == recipe_id)
        {
            return false;
        }
        let old_matches = self.slots[0]
            .as_ref()
            .is_some_and(|stack| recipes.cooking_recipe_accepts(self.kind, recipe_id, &stack.id));
        let ingredient_id = self.slots[0]
            .as_ref()
            .filter(|_| old_matches)
            .map(|stack| stack.id.clone())
            .or_else(|| {
                inventory.slots[..36]
                    .iter()
                    .filter_map(Option::as_ref)
                    .find(|stack| recipes.cooking_recipe_accepts(self.kind, recipe_id, &stack.id))
                    .map(|stack| stack.id.clone())
            });
        let Some(ingredient_id) = ingredient_id else {
            return false;
        };
        let available = inventory.slots[..36]
            .iter()
            .filter_map(Option::as_ref)
            .filter(|stack| stack.id == ingredient_id)
            .map(|stack| stack.count as u32)
            .sum::<u32>()
            + self.slots[0]
                .as_ref()
                .filter(|stack| stack.id == ingredient_id)
                .map_or(0, |stack| stack.count as u32);
        let current = self.slots[0]
            .as_ref()
            .filter(|_| old_matches)
            .map_or(0, |s| s.count as u32);
        let count = if use_max_items {
            available.min(64)
        } else if old_matches {
            current + 1
        } else {
            1
        };
        if count == 0 || count > available || count > recipes.max_stack(&ingredient_id) as u32 {
            return false;
        }

        let mut trial_furnace = self.clone();
        let mut trial_inventory = inventory.clone();
        for slot in [0, 2] {
            if let Some(stack) = trial_furnace.slots[slot].take() {
                if trial_inventory.add_item(stack, 0).is_some() {
                    return false;
                }
            }
        }
        let mut remaining = count;
        let mut selected: Option<ItemStack> = None;
        for slot in &mut trial_inventory.slots[..36] {
            let Some(stack) = slot else { continue };
            if stack.id != ingredient_id
                || selected
                    .as_ref()
                    .is_some_and(|chosen| !chosen.same_item(stack))
            {
                continue;
            }
            let moved = remaining.min(stack.count as u32) as u8;
            if selected.is_none() {
                selected = Some(ItemStack {
                    count: 0,
                    ..stack.clone()
                });
            }
            selected.as_mut().unwrap().count += moved;
            stack.count -= moved;
            remaining -= moved as u32;
            if stack.count == 0 {
                *slot = None;
            }
            if remaining == 0 {
                break;
            }
        }
        if remaining != 0 {
            return false;
        }
        trial_furnace.slots[0] = selected;
        trial_furnace.refresh_input_after_change(None, &recipes);
        *self = trial_furnace;
        *inventory = trial_inventory;
        true
    }

    pub fn click_slot(
        &mut self,
        index: usize,
        right: bool,
        shift: bool,
        inventory: &mut Inventory,
    ) {
        if index == 2 {
            self.take_output(shift, inventory);
            return;
        }
        if index > 2 {
            return;
        }
        let old_input = if index == 0 {
            self.slots[0].clone()
        } else {
            None
        };
        if shift {
            if let Some(stack) = self.slots[index].take() {
                self.slots[index] = inventory.add_item(stack, 0);
            }
        } else {
            if index == 1
                && inventory.cursor.as_ref().is_some_and(|stack| {
                    !inventory.recipes.is_fuel(&stack.id) && stack.id != "minecraft:bucket"
                })
            {
                return;
            }
            click_stack(&mut self.slots[index], &mut inventory.cursor, right);
        }
        if index == 0 {
            self.refresh_input_after_change(old_input.as_ref(), &inventory.recipes);
        }
    }

    pub fn distribute(&mut self, slots: &[usize], right: bool, inventory: &mut Inventory) {
        let old_input = self.slots[0].clone();
        let recipes = inventory.recipes.clone();
        inventory.distribute_external(slots, right, &mut self.slots, |index, stack| match index {
            0 => true,
            1 => recipes.is_fuel(&stack.id) || stack.id == "minecraft:bucket",
            _ => false,
        });
        self.refresh_input_after_change(old_input.as_ref(), &inventory.recipes);
    }

    pub fn quick_move_from_inventory(&mut self, index: usize, inventory: &mut Inventory) {
        if index >= 36 {
            return;
        }
        let Some(mut moving) = inventory.slots[index].take() else {
            return;
        };
        let old_input = self.slots[0].clone();
        let destination = if inventory.recipes.cooking_for(self.kind, &moving).is_some() {
            0
        } else if inventory.recipes.is_fuel(&moving.id) {
            1
        } else {
            inventory.slots[index] = Some(moving);
            inventory.quick_move(index);
            return;
        };
        if let Some(target) = &mut self.slots[destination] {
            if target.same_item(&moving) {
                let moved = (target.max - target.count).min(moving.count);
                target.count += moved;
                moving.count -= moved;
            }
        } else {
            let moved = moving.count.min(moving.max);
            self.slots[destination] = Some(ItemStack {
                count: moved,
                ..moving.clone()
            });
            moving.count -= moved;
        }
        if moving.count > 0 {
            inventory.slots[index] = Some(moving);
        }
        if destination == 0 {
            self.refresh_input_after_change(old_input.as_ref(), &inventory.recipes);
        }
    }

    fn refresh_input_after_change(&mut self, old: Option<&ItemStack>, recipes: &RecipeBook) {
        let same = old
            .zip(self.slots[0].as_ref())
            .is_some_and(|(before, after)| before.same_item(after));
        if same {
            return;
        }
        self.cook_progress = 0;
        self.cook_total = self.slots[0]
            .as_ref()
            .and_then(|stack| recipes.cooking_for(self.kind, stack))
            .map_or(0, |recipe| {
                cooking_ticks(recipe.cooking_ticks, self.speed_multiplier)
            });
    }

    pub fn take_output(&mut self, shift: bool, inventory: &mut Inventory) -> bool {
        let Some(output) = self.slots[2].as_ref().cloned() else {
            return false;
        };
        if shift {
            let mut trial = inventory.clone();
            if trial.add_item(output.clone(), 0).is_some() {
                return false;
            }
            let _ = inventory.add_item(output.clone(), 0);
        } else {
            match inventory.cursor.as_mut() {
                None => inventory.cursor = Some(output.clone()),
                Some(cursor)
                    if cursor.same_item(&output)
                        && cursor.count as u16 + output.count as u16 <= cursor.max as u16 =>
                {
                    cursor.count += output.count;
                }
                _ => return false,
            }
        }
        self.slots[2] = None;
        inventory.record_crafted(&output);
        true
    }

    pub fn tick(&mut self, recipes: &RecipeBook) {
        if self.lit_remaining > 0 {
            self.lit_remaining -= 1;
        }
        let mut lit = self.is_lit();
        let has_fuel = self.slots[1].is_some();
        let Some(input) = self.slots[0].as_ref() else {
            if lit {
                self.cook_progress = 0;
            } else {
                self.cook_progress = self.cook_progress.saturating_sub(2);
            }
            return;
        };
        let wet_sponge = input.id == "minecraft:wet_sponge";
        if !lit && !has_fuel {
            self.cook_progress = self.cook_progress.saturating_sub(2);
            return;
        }
        let Some(recipe) = recipes.cooking_for(self.kind, input) else {
            return;
        };
        let output = &recipe.result;
        let can_burn = self.slots[2].as_ref().is_none_or(|existing| {
            existing.same_item(output)
                && existing.count as u16 + output.count as u16 <= existing.max as u16
        });
        if !can_burn {
            self.cook_progress = 0;
            return;
        }
        if self.cook_total == 0 {
            self.cook_total = cooking_ticks(recipe.cooking_ticks, self.speed_multiplier);
        }
        if !lit {
            let (duration, speed) = self.slots[1].as_ref().map_or((0, 1.0), |stack| {
                (recipes.fuel_ticks(&stack.id), recipes.fuel_speed(&stack.id))
            });
            let duration = duration / self.kind.default_burn_divisor();
            let speed = speed * self.kind.default_speed_multiplier();
            self.lit_remaining = duration;
            self.lit_total = duration;
            self.speed_multiplier = speed;
            if self.cook_total > 0 && self.cook_progress < self.cook_total {
                let ratio = self.cook_progress as f32 / self.cook_total as f32;
                self.cook_total = cooking_ticks(recipe.cooking_ticks, speed);
                self.cook_progress = (ratio * self.cook_total as f32).ceil() as u32;
            }
            if duration > 0 {
                self.consume_fuel(recipes);
                lit = true;
            }
        }
        if lit {
            self.cook_progress += 1;
            if self.cook_progress >= self.cook_total {
                self.cook_progress = 0;
                self.cook_total = cooking_ticks(recipe.cooking_ticks, self.speed_multiplier);
                if let Some(existing) = &mut self.slots[2] {
                    existing.count += output.count;
                } else {
                    self.slots[2] = Some(output.clone());
                }
                if wet_sponge
                    && self.slots[1]
                        .as_ref()
                        .is_some_and(|stack| stack.id == "minecraft:bucket")
                {
                    self.slots[1] = Some(recipes.stack("minecraft:water_bucket", 1));
                }
                if let Some(input) = &mut self.slots[0] {
                    input.count -= 1;
                    if input.count == 0 {
                        self.slots[0] = None;
                    }
                }
                self.completed_recipes += 1;
            }
        } else {
            self.cook_progress = 0;
        }
    }

    fn consume_fuel(&mut self, recipes: &RecipeBook) {
        let Some(fuel) = &mut self.slots[1] else {
            return;
        };
        let remainder = recipes.crafting_remainder(&fuel.id);
        fuel.count -= 1;
        if fuel.count == 0 {
            self.slots[1] = remainder;
        }
    }

    pub fn take_contents(&mut self) -> Vec<ItemStack> {
        self.slots.iter_mut().filter_map(Option::take).collect()
    }
}

/// Current supported 26.3 item-component values in a normal Overworld furnace.
/// More fuels need the item component registry, rather than a tag-only shortcut.
pub fn fuel_ticks(id: &str) -> u32 {
    match id {
        "minecraft:coal" | "minecraft:charcoal" => 1600,
        "minecraft:coal_block" => 16000,
        "minecraft:lava_bucket" => 20000,
        "minecraft:stick" => 100,
        "minecraft:oak_slab" => 150,
        "minecraft:oak_log" | "minecraft:oak_planks" | "minecraft:crafting_table" => 300,
        _ => 0,
    }
}
