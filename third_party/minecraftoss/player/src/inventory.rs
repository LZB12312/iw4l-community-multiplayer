//! Player inventory transactions. The UI and renderer may request transactions,
//! but this module owns their semantic results and never loads client resources.
use crate::crafting::RecipeBook;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecipePlacement {
    Placed,
    Ghost,
    Blocked,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemStack {
    pub id: String,
    pub count: u8,
    pub max: u8,
    pub components: Option<Value>,
}
impl ItemStack {
    pub fn new(id: impl Into<String>, count: u8) -> Self {
        Self {
            id: id.into(),
            count,
            max: 64,
            components: None,
        }
    }
    pub fn same_item(&self, other: &Self) -> bool {
        self.id == other.id && self.components == other.components
    }
    /// Evaluate the block and exact-state subset of the pinned
    /// AdventureModePredicate component. Registry tags and block-entity
    /// predicates require the future generated-world registry/BE adapter.
    pub fn allows_adventure_block(&self, component: &str, block: &crate::Block) -> bool {
        let Some(value) = self.components.as_ref().and_then(|v| v.get(component)) else {
            return false;
        };
        let predicates = value.as_array().map(Vec::as_slice);
        let one = std::slice::from_ref(value);
        predicates.unwrap_or(one).iter().any(|predicate| {
            let Some(fields) = predicate.as_object() else {
                return false;
            };
            if fields.contains_key("nbt") || fields.contains_key("components") {
                return false;
            }
            if let Some(blocks) = fields.get("blocks") {
                let ids = blocks.as_array().map(Vec::as_slice);
                let one = std::slice::from_ref(blocks);
                if !ids
                    .unwrap_or(one)
                    .iter()
                    .any(|id| id.as_str() == Some(&block.id))
                {
                    return false;
                }
            }
            fields.get("state").is_none_or(|state| {
                state.as_object().is_some_and(|properties| {
                    properties.iter().all(|(key, expected)| {
                        expected
                            .as_str()
                            .is_some_and(|value| block.property(key) == Some(value))
                    })
                })
            })
        })
    }
}

#[derive(Clone, Debug)]
pub struct Inventory {
    /// 0..9 hotbar, 9..36 main, 36..40 armor, 40 offhand, 41 body, 42 saddle.
    pub slots: Vec<Option<ItemStack>>,
    pub cursor: Option<ItemStack>,
    pub crafting: [Option<ItemStack>; 4],
    pub workbench: [Option<ItemStack>; 9],
    pub recipes: Arc<RecipeBook>,
    pub unlocked_recipes: HashSet<String>,
    unlock_sequence: Vec<String>,
    /// Remainders that could not fit the crafting grid or player inventory.
    pending_drops: Vec<ItemStack>,
    pending_stats: Vec<(String, String, i32)>,
}

fn armor_slot_name(index: usize) -> Option<&'static str> {
    match index {
        39 => Some("head"),
        38 => Some("chest"),
        37 => Some("legs"),
        36 => Some("feet"),
        _ => None,
    }
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            // 0..36 storage; 36..40 armor; 40 offhand; 41 body; 42 saddle.
            slots: vec![None; 43],
            cursor: None,
            crafting: std::array::from_fn(|_| None),
            workbench: std::array::from_fn(|_| None),
            recipes: Arc::new(RecipeBook::default()),
            unlocked_recipes: HashSet::new(),
            unlock_sequence: Vec::new(),
            pending_drops: Vec::new(),
            pending_stats: Vec::new(),
        }
    }
}
impl Inventory {
    /// Return every carried stack for the default keepInventory=false death
    /// path, including transient menu inputs and remainders awaiting a drop.
    pub fn drain_on_death(&mut self) -> Vec<ItemStack> {
        let mut drops: Vec<_> = self.slots.iter_mut().filter_map(Option::take).collect();
        drops.extend(self.crafting.iter_mut().filter_map(Option::take));
        drops.extend(self.workbench.iter_mut().filter_map(Option::take));
        drops.extend(self.cursor.take());
        drops.append(&mut self.pending_drops);
        drops
    }

    /// InventoryChangeTrigger tests a single item predicate against the
    /// changed stack, rather than scanning all stacks already carried.
    pub fn notice_item_changed(&mut self, changed_item: &ItemStack) {
        let occupied = self.slots.iter().filter(|slot| slot.is_some()).count();
        let unlocked = self
            .recipes
            .auto_unlocks_for_change(changed_item, occupied)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for id in unlocked {
            if self.unlocked_recipes.insert(id.clone()) {
                self.unlock_sequence.push(id);
            }
        }
    }

    /// ClientRecipeBook stores display IDs in a Java HashMap. Iteration visits
    /// each bucket in index order and keeps insertion order within a bucket.
    pub fn recipe_display_order_map(&self) -> HashMap<&str, (usize, usize)> {
        let known_displays = self
            .unlocked_recipes
            .iter()
            .filter_map(|known| self.recipes.display_indices(known))
            .map(<[u32]>::len)
            .sum::<usize>();
        let mut capacity = 16usize;
        while known_displays > capacity * 3 / 4 {
            capacity *= 2;
        }
        let insertion = self
            .unlock_sequence
            .iter()
            .enumerate()
            .map(|(index, id)| (id.as_str(), index))
            .collect::<HashMap<_, _>>();
        self.unlocked_recipes
            .iter()
            .filter_map(|id| {
                let display = *self.recipes.display_indices(id)?.first()? as usize;
                Some((
                    id.as_str(),
                    (
                        display & (capacity - 1),
                        insertion.get(id.as_str()).copied().unwrap_or(usize::MAX),
                    ),
                ))
            })
            .collect()
    }

    pub fn recipe_display_order(&self, id: &str) -> Option<(usize, usize)> {
        self.recipe_display_order_map().get(id).copied()
    }

    /// Notify recipe advancements when a menu transaction puts a new stack
    /// into a player slot or increases the stack already there.
    pub(crate) fn notice_slot_after_change(&mut self, index: usize, before: Option<ItemStack>) {
        let Some(after) = self.slots.get(index).and_then(Option::as_ref).cloned() else {
            return;
        };
        if before
            .as_ref()
            .is_none_or(|old| !old.same_item(&after) || old.count < after.count)
        {
            self.notice_item_changed(&after);
        }
    }

    pub fn unlock_recipe(&mut self, id: &str) -> bool {
        let known = self
            .recipes
            .crafting_recipes()
            .any(|recipe| recipe.id == id)
            || [
                crate::crafting::CookingKind::Furnace,
                crate::crafting::CookingKind::BlastFurnace,
                crate::crafting::CookingKind::Smoker,
            ]
            .into_iter()
            .any(|kind| {
                self.recipes
                    .cooking_recipes(kind)
                    .any(|recipe| recipe.id == id)
            });
        if known && self.unlocked_recipes.insert(id.to_owned()) {
            self.unlock_sequence.push(id.to_owned());
            true
        } else {
            false
        }
    }

    /// Place a known crafting recipe through a transactional grid/inventory
    /// move. The selected ingredient types follow the external data JAR.
    pub fn place_crafting_recipe(
        &mut self,
        recipe_id: &str,
        workbench: bool,
        use_max_items: bool,
    ) -> RecipePlacement {
        if !self.unlocked_recipes.contains(recipe_id) {
            return RecipePlacement::Blocked;
        }
        let side = if workbench { 3 } else { 2 };
        let Some(candidates) = self.recipes.crafting_grid_candidates(recipe_id, side, side) else {
            return RecipePlacement::Blocked;
        };
        let current_grid = if workbench {
            &self.workbench[..]
        } else {
            &self.crafting[..]
        };
        let already_matches = self
            .recipes
            .matches_crafting_id(recipe_id, current_grid, side, side);
        let desired = if already_matches && !use_max_items {
            current_grid
                .iter()
                .filter_map(Option::as_ref)
                .map(|stack| u16::from(stack.count))
                .min()
                .unwrap_or(0)
                + 1
        } else {
            1
        };
        let mut trial = self.clone();
        for index in 0..side * side {
            let grid = if workbench {
                &mut trial.workbench[..]
            } else {
                &mut trial.crafting[..]
            };
            if let Some(stack) = grid[index].take() {
                if trial.add_item(stack, 0).is_some() {
                    return RecipePlacement::Blocked;
                }
            }
        }
        let mut available = HashMap::<String, u16>::new();
        for stack in trial.slots[..36].iter().filter_map(Option::as_ref) {
            if stack.components.is_none() {
                *available.entry(stack.id.clone()).or_default() += u16::from(stack.count);
            }
        }
        let occupied = candidates
            .iter()
            .enumerate()
            .filter_map(|(index, choices)| choices.as_ref().map(|choices| (index, choices)))
            .collect::<Vec<_>>();
        fn assign(
            choices: &[(usize, &Vec<String>)],
            available: &mut HashMap<String, u16>,
            max_stack: &impl Fn(&str) -> u8,
            amount: u16,
            index: usize,
            selected: &mut Vec<(usize, String)>,
        ) -> bool {
            if index == choices.len() {
                return true;
            }
            for id in choices[index].1 {
                let count = available.get(id).copied().unwrap_or(0);
                if count < amount || u16::from(max_stack(id)) < amount {
                    continue;
                }
                *available.get_mut(id).unwrap() -= amount;
                selected.push((choices[index].0, id.clone()));
                if assign(choices, available, max_stack, amount, index + 1, selected) {
                    return true;
                }
                selected.pop();
                *available.get_mut(id).unwrap() += amount;
            }
            false
        }
        let mut choice = None;
        for amount in (desired..=if use_max_items { 64 } else { desired }).rev() {
            let mut counts = available.clone();
            let mut selected = Vec::new();
            if assign(
                &occupied,
                &mut counts,
                &|id| trial.recipes.max_stack(id),
                amount,
                0,
                &mut selected,
            ) {
                choice = Some((amount, selected));
                break;
            }
        }
        let Some((amount, selected)) = choice else {
            if already_matches {
                return RecipePlacement::Blocked;
            }
            *self = trial;
            return RecipePlacement::Ghost;
        };
        for (grid_index, id) in selected {
            let mut remaining = amount as u8;
            for slot in &mut trial.slots[..36] {
                let Some(stack) = slot.as_mut() else { continue };
                if stack.id != id || stack.components.is_some() {
                    continue;
                }
                let moved = remaining.min(stack.count);
                stack.count -= moved;
                remaining -= moved;
                if stack.count == 0 {
                    *slot = None;
                }
                if remaining == 0 {
                    break;
                }
            }
            debug_assert_eq!(remaining, 0);
            let placed = trial.recipes.stack(&id, amount as u8);
            if workbench {
                trial.workbench[grid_index] = Some(placed);
            } else {
                trial.crafting[grid_index] = Some(placed);
            }
        }
        *self = trial;
        RecipePlacement::Placed
    }
    /// Apply pinned Item.mineBlock wear after a nonzero-hardness survival
    /// break. Returns true if the selected stack broke and disappeared.
    pub fn wear_tool_after_mining(&mut self, selected: usize, hardness: f32) -> bool {
        if hardness == 0.0 || selected >= self.slots.len() {
            return false;
        }
        let Some(stack) = self.slots[selected].as_ref() else {
            return false;
        };
        let Some((_, per_block)) = self.recipes.mining_tool_wear(stack) else {
            return false;
        };
        self.wear_tool(selected, per_block)
    }

    /// Apply ItemStack.hurtAndBreak-style durability from a successful item use.
    /// Returns true when the selected stack breaks and disappears.
    pub fn wear_tool(&mut self, selected: usize, amount: u32) -> bool {
        if amount == 0 || selected >= self.slots.len() {
            return false;
        }
        let Some(stack) = self.slots[selected].as_ref() else {
            return false;
        };
        let Some((current_damage, max_damage)) = self.recipes.durability(stack) else {
            return false;
        };
        let new_damage = current_damage.saturating_add(amount).min(max_damage);
        if new_damage >= max_damage {
            self.slots[selected] = None;
            return true;
        }
        let stack = self.slots[selected].as_mut().unwrap();
        let components = stack
            .components
            .get_or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(components) = components.as_object_mut() {
            components.insert("minecraft:damage".into(), Value::from(new_damage));
        }
        false
    }

    /// Sample-only creative palette used by the handcrafted viewer scene.
    pub fn demo_hotbar() -> Self {
        let mut slots = vec![None; 43];
        for (i, id) in [
            "stone",
            "dirt",
            "grass_block",
            "oak_planks",
            "cobblestone",
            "glass",
            "oak_slab",
            "oak_stairs",
            "water",
        ]
        .iter()
        .enumerate()
        {
            slots[i] = Some(ItemStack::new(format!("minecraft:{id}"), 64));
        }
        let mut shears = ItemStack::new("minecraft:shears", 1);
        shears.max = 1;
        shears.components = Some(serde_json::json!({"minecraft:max_damage": 238}));
        slots[9] = Some(shears);
        slots[10] = Some(ItemStack::new("minecraft:wheat", 64));
        slots[11] = Some(ItemStack::new("minecraft:golden_dandelion", 64));
        Self {
            slots,
            ..Self::default()
        }
    }
    pub fn with_recipes(mut self, recipes: RecipeBook) -> Self {
        self.recipes = Arc::new(recipes);
        self
    }
    /// Returns a stack dropped outside the menu. The caller must create its
    /// world item entity; discarding this result would destroy survival items.
    pub fn click(&mut self, slot: Option<usize>, right: bool, shift: bool) -> Option<ItemStack> {
        let Some(index) = slot else {
            if let Some(cursor) = self.cursor.as_mut() {
                let dropped = if right {
                    cursor.count -= 1;
                    let dropped = Some(ItemStack {
                        count: 1,
                        ..cursor.clone()
                    });
                    if cursor.count == 0 {
                        self.cursor = None;
                    }
                    dropped
                } else {
                    self.cursor.take()
                };
                if let Some(stack) = dropped.as_ref() {
                    self.record_dropped(stack);
                }
                return dropped;
            }
            return None;
        };
        if index >= self.slots.len() {
            return None;
        }
        if shift {
            self.quick_move(index);
            return None;
        }
        if let Some(slot_name) = armor_slot_name(index) {
            if let Some(carried) = self.cursor.as_ref() {
                if self.recipes.equipment_slot(carried) != Some(slot_name) {
                    return None;
                }
                if self.slots[index].is_none() {
                    let before = self.slots[index].clone();
                    let mut placed = carried.clone();
                    placed.count = 1;
                    self.slots[index] = Some(placed);
                    let carried = self.cursor.as_mut().unwrap();
                    carried.count -= 1;
                    if carried.count == 0 {
                        self.cursor = None;
                    }
                    self.notice_slot_after_change(index, before);
                    return None;
                }
                if carried.count > 1
                    || self.slots[index]
                        .as_ref()
                        .is_some_and(|existing| existing.same_item(carried))
                {
                    return None;
                }
            }
        }
        let before = self.slots[index].clone();
        click_stack(&mut self.slots[index], &mut self.cursor, right);
        self.notice_slot_after_change(index, before);
        None
    }
    pub fn click_crafting_slot(&mut self, index: usize, right: bool, shift: bool) {
        if index >= 4 {
            return;
        }
        if shift {
            if let Some(stack) = self.crafting[index].take() {
                self.crafting[index] = self.add_item(stack, 0);
            }
        } else {
            click_stack(&mut self.crafting[index], &mut self.cursor, right);
        }
    }
    pub fn crafting_output(&self) -> Option<ItemStack> {
        self.recipes.matching(&self.crafting, 2, 2)
    }
    pub fn take_crafting_output(&mut self, shift: bool) -> bool {
        let mut crafted = false;
        loop {
            let Some(output) = self.crafting_output() else {
                break;
            };
            if shift {
                let mut trial = self.clone();
                if trial.add_item(output.clone(), 0).is_some() {
                    break;
                }
                let _ = self.add_item(output.clone(), 0);
            } else {
                match self.cursor.as_mut() {
                    None => self.cursor = Some(output.clone()),
                    Some(cursor)
                        if cursor.same_item(&output)
                            && cursor.count as u16 + output.count as u16 <= cursor.max as u16 =>
                    {
                        cursor.count += output.count;
                    }
                    _ => break,
                }
            }
            self.record_crafted(&output);
            self.consume_crafting_ingredients(false);
            crafted = true;
            if !shift {
                break;
            }
        }
        crafted
    }
    pub fn settle_crafting(&mut self) -> Vec<ItemStack> {
        let mut overflow = Vec::new();
        for index in 0..4 {
            if let Some(stack) = self.crafting[index].take() {
                if let Some(rest) = self.add_item(stack, 0) {
                    overflow.push(rest);
                }
            }
        }
        overflow
    }
    pub fn click_workbench_slot(&mut self, index: usize, right: bool, shift: bool) {
        if index >= 9 {
            return;
        }
        if shift {
            if let Some(stack) = self.workbench[index].take() {
                self.workbench[index] = self.add_item(stack, 0);
            }
        } else {
            click_stack(&mut self.workbench[index], &mut self.cursor, right);
        }
    }
    pub fn workbench_output(&self) -> Option<ItemStack> {
        self.recipes.matching(&self.workbench, 3, 3)
    }
    pub fn take_workbench_output(&mut self, shift: bool) -> bool {
        let mut crafted = false;
        loop {
            let Some(output) = self.workbench_output() else {
                break;
            };
            if shift {
                let mut trial = self.clone();
                if trial.add_item(output.clone(), 0).is_some() {
                    break;
                }
                let _ = self.add_item(output.clone(), 0);
            } else {
                match self.cursor.as_mut() {
                    None => self.cursor = Some(output.clone()),
                    Some(cursor)
                        if cursor.same_item(&output)
                            && cursor.count as u16 + output.count as u16 <= cursor.max as u16 =>
                    {
                        cursor.count += output.count;
                    }
                    _ => break,
                }
            }
            self.record_crafted(&output);
            self.consume_crafting_ingredients(true);
            crafted = true;
            if !shift {
                break;
            }
        }
        crafted
    }
    /// ResultSlot.onTake consumes one item from each occupied input slot,
    /// then places the recipe's remainder in that slot, the player inventory,
    /// or the world if neither has space.
    fn consume_crafting_ingredients(&mut self, workbench: bool) {
        let size = if workbench { 9 } else { 4 };
        for index in 0..size {
            let grid = if workbench {
                &mut self.workbench[..]
            } else {
                &mut self.crafting[..]
            };
            let remainder = grid[index]
                .as_ref()
                .and_then(|stack| self.recipes.crafting_remainder(&stack.id));
            if let Some(stack) = grid[index].as_mut() {
                stack.count -= 1;
                if stack.count == 0 {
                    grid[index] = None;
                }
            }
            let Some(mut remainder) = remainder else {
                continue;
            };
            let grid = if workbench {
                &mut self.workbench[..]
            } else {
                &mut self.crafting[..]
            };
            if grid[index].is_none() {
                grid[index] = Some(remainder);
                continue;
            }
            if let Some(target) = grid[index].as_mut() {
                if target.same_item(&remainder) && target.count < target.max {
                    let moved = (target.max - target.count).min(remainder.count);
                    target.count += moved;
                    remainder.count -= moved;
                    if remainder.count == 0 {
                        continue;
                    }
                }
            }
            if let Some(overflow) = self.add_item(remainder, 0) {
                self.pending_drops.push(overflow);
            }
        }
    }

    pub fn take_pending_drops(&mut self) -> Vec<ItemStack> {
        std::mem::take(&mut self.pending_drops)
    }
    pub fn settle_workbench(&mut self) -> Vec<ItemStack> {
        let mut overflow = Vec::new();
        for index in 0..9 {
            if let Some(stack) = self.workbench[index].take() {
                if let Some(rest) = self.add_item(stack, 0) {
                    overflow.push(rest);
                }
            }
        }
        overflow
    }
    pub fn number_swap(&mut self, slot: usize, hotbar: usize) {
        if slot < self.slots.len() && hotbar < 9 && slot != hotbar {
            if let Some(slot_name) = armor_slot_name(slot) {
                if self.slots[hotbar].as_ref().is_some_and(|stack| {
                    stack.count != 1 || self.recipes.equipment_slot(stack) != Some(slot_name)
                }) {
                    return;
                }
            }
            let slot_before = self.slots[slot].clone();
            let hotbar_before = self.slots[hotbar].clone();
            self.slots.swap(slot, hotbar);
            self.notice_slot_after_change(slot, slot_before);
            self.notice_slot_after_change(hotbar, hotbar_before);
        }
    }
    pub fn quick_move(&mut self, index: usize) {
        if index >= self.slots.len() {
            return;
        }
        let Some(mut source) = self.slots[index].take() else {
            return;
        };
        if index < 36 {
            let equipment_index = match self.recipes.equipment_slot(&source) {
                Some("head") => Some(39),
                Some("chest") => Some(38),
                Some("legs") => Some(37),
                Some("feet") => Some(36),
                Some("offhand") => Some(40),
                _ => None,
            };
            if let Some(target) = equipment_index.filter(|&target| self.slots[target].is_none()) {
                let mut equipped = source.clone();
                equipped.count = 1;
                self.slots[target] = Some(equipped);
                source.count -= 1;
                if source.count > 0 {
                    self.slots[index] = Some(source);
                }
                return;
            }
        }
        let ranges = if index < 9 {
            [9..36, 0..0]
        } else if index < 36 {
            [0..9, 0..0]
        } else {
            [9..36, 0..9]
        };
        for i in ranges.iter().flat_map(Clone::clone) {
            if let Some(target) = self.slots[i].as_mut() {
                if target.same_item(&source) && target.count < target.max {
                    let moved = (target.max - target.count).min(source.count);
                    target.count += moved;
                    source.count -= moved;
                    if source.count == 0 {
                        return;
                    }
                }
            }
        }
        for i in ranges.into_iter().flatten() {
            if self.slots[i].is_none() {
                self.slots[i] = Some(source);
                return;
            }
        }
        self.slots[index] = Some(source);
    }
    pub fn creative_take(&mut self, mut stack: ItemStack, right: bool, hotbar: Option<usize>) {
        stack.count = if right { 1 } else { stack.max };
        if let Some(index) = hotbar.filter(|&i| i < 9) {
            let before = self.slots[index].clone();
            self.slots[index] = Some(stack);
            self.notice_slot_after_change(index, before);
        } else {
            self.cursor = Some(stack);
        }
    }
    pub fn drop_selected(&mut self, selected: usize, entire_stack: bool) -> Option<ItemStack> {
        let dropped = if entire_stack {
            self.slots.get_mut(selected)?.take()
        } else {
            let slot = self.slots.get_mut(selected)?.as_mut()?;
            slot.count -= 1;
            let drop = ItemStack {
                count: 1,
                ..slot.clone()
            };
            if slot.count == 0 {
                self.slots[selected] = None;
            }
            Some(drop)
        };
        if let Some(stack) = dropped.as_ref() {
            self.record_dropped(stack);
        }
        dropped
    }

    pub fn record_crafted(&mut self, stack: &ItemStack) {
        self.pending_stats.push((
            crate::statistics::CRAFTED.into(),
            stack.id.clone(),
            stack.count.into(),
        ));
    }

    fn record_dropped(&mut self, stack: &ItemStack) {
        self.pending_stats.push((
            crate::statistics::DROPPED.into(),
            stack.id.clone(),
            stack.count.into(),
        ));
        self.pending_stats
            .push((crate::statistics::CUSTOM.into(), "minecraft:drop".into(), 1));
    }

    pub fn take_stat_events(&mut self) -> Vec<(String, String, i32)> {
        std::mem::take(&mut self.pending_stats)
    }
    pub fn distribute(&mut self, slots: &[usize], right: bool) {
        self.distribute_external(slots, right, &mut [], |_, _| false);
    }
    /// Menu quick-craft indices use 0..43 for player slots and 43 onward for
    /// the open container. This lets a drag cross the container boundary.
    pub fn distribute_external(
        &mut self,
        slots: &[usize],
        right: bool,
        external: &mut [Option<ItemStack>],
        may_place: impl Fn(usize, &ItemStack) -> bool,
    ) {
        let Some(mut carried) = self.cursor.take() else {
            return;
        };
        let eligible = slots
            .iter()
            .copied()
            .filter(|&i| {
                let target = if i < self.slots.len() {
                    if !armor_slot_name(i).is_none_or(|slot_name| {
                        self.recipes.equipment_slot(&carried) == Some(slot_name)
                    }) {
                        return false;
                    }
                    self.slots.get(i)
                } else {
                    let external_index = i - self.slots.len();
                    if !may_place(external_index, &carried) {
                        return false;
                    }
                    external.get(external_index)
                };
                target.is_some_and(|target| {
                    target.as_ref().is_none_or(|stack| {
                        stack.same_item(&carried)
                            && stack.count
                                < if armor_slot_name(i).is_some() {
                                    1
                                } else {
                                    stack.max
                                }
                    })
                })
            })
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            self.cursor = Some(carried);
            return;
        }
        let each = if right {
            1
        } else {
            carried.count / eligible.len() as u8
        };
        for index in eligible {
            if carried.count == 0 {
                break;
            }
            let before = self.slots.get(index).cloned().flatten();
            let slot = if index < self.slots.len() {
                &mut self.slots[index]
            } else {
                &mut external[index - self.slots.len()]
            };
            let target = slot.get_or_insert_with(|| ItemStack {
                count: 0,
                ..carried.clone()
            });
            let capacity = if armor_slot_name(index).is_some() {
                1
            } else {
                target.max
            };
            let moved = each.min(capacity - target.count).min(carried.count);
            target.count += moved;
            carried.count -= moved;
            if moved > 0 && index < self.slots.len() {
                self.notice_slot_after_change(index, before);
            }
        }
        if carried.count > 0 {
            self.cursor = Some(carried);
        }
    }
    pub fn distribute_crafting(&mut self, slots: &[usize], right: bool, workbench: bool) {
        if workbench {
            let mut grid = std::mem::take(&mut self.workbench);
            self.distribute_external(slots, right, &mut grid, |_, _| true);
            self.workbench = grid;
        } else {
            let mut grid = std::mem::take(&mut self.crafting);
            self.distribute_external(slots, right, &mut grid, |_, _| true);
            self.crafting = grid;
        }
    }
    /// AbstractContainerMenu.PICKUP_ALL: consume partial matching stacks first,
    /// then full stacks, stopping at the carried stack's maximum.
    pub fn pickup_all(&mut self, right: bool) {
        let Some(mut carried) = self.cursor.take() else {
            return;
        };
        for pass in 0..2 {
            let indices: Box<dyn Iterator<Item = usize>> = if right {
                Box::new((0..self.slots.len()).rev())
            } else {
                Box::new(0..self.slots.len())
            };
            for index in indices {
                if carried.count >= carried.max {
                    break;
                }
                let Some(stack) = self.slots[index].as_ref() else {
                    continue;
                };
                if !stack.same_item(&carried) || (pass == 0 && stack.count == stack.max) {
                    continue;
                }
                let before = self.slots[index].clone();
                let stack = self.slots[index].as_mut().unwrap();
                let moved = stack.count.min(carried.max - carried.count);
                stack.count -= moved;
                carried.count += moved;
                if stack.count == 0 {
                    self.slots[index] = None;
                }
                self.notice_slot_after_change(index, before);
            }
        }
        self.cursor = Some(carried);
    }
    pub fn settle_cursor(&mut self) -> Option<ItemStack> {
        let Some(mut carried) = self.cursor.take() else {
            return None;
        };
        for index in 0..36 {
            if let Some(target) = self.slots[index].as_mut() {
                if target.same_item(&carried) && target.count < target.max {
                    let changed_item = carried.clone();
                    let moved = (target.max - target.count).min(carried.count);
                    target.count += moved;
                    carried.count -= moved;
                    if moved > 0 {
                        self.notice_item_changed(&changed_item);
                    }
                    if carried.count == 0 {
                        return None;
                    }
                }
            }
        }
        if let Some(index) = self.slots[..36].iter().position(Option::is_none) {
            let changed_item = carried.clone();
            self.slots[index] = Some(carried);
            self.notice_item_changed(&changed_item);
            None
        } else {
            Some(carried)
        }
    }
    /// Insert an item entity's stack, returning what did not fit.
    pub fn add_item(&mut self, mut stack: ItemStack, selected: usize) -> Option<ItemStack> {
        let changed_item = stack.clone();
        let original_count = stack.count;
        if stack.components.is_none() {
            stack.max = stack.max.min(self.recipes.max_stack(&stack.id));
        }
        let mut indices = Vec::with_capacity(36);
        if selected < 9 {
            indices.push(selected);
        }
        indices.push(40);
        indices.extend(0..36);
        let mut seen = [false; 43];
        for index in indices.iter().copied() {
            if seen[index] {
                continue;
            }
            seen[index] = true;
            if let Some(target) = self.slots[index].as_mut() {
                if target.same_item(&stack) && target.count < target.max {
                    let moved = (target.max - target.count).min(stack.count);
                    target.count += moved;
                    stack.count -= moved;
                    if stack.count == 0 {
                        self.notice_item_changed(&changed_item);
                        return None;
                    }
                }
            }
        }
        for index in 0..36 {
            if self.slots[index].is_none() {
                let moved = stack.count.min(stack.max);
                self.slots[index] = Some(ItemStack {
                    count: moved,
                    ..stack.clone()
                });
                stack.count -= moved;
                if stack.count == 0 {
                    self.notice_item_changed(&changed_item);
                    return None;
                }
            }
        }
        if stack.count < original_count {
            self.notice_item_changed(&changed_item);
        }
        Some(stack)
    }
    /// The HUD uses LivingEntity.getArmorValue: floor the effective armor
    /// attribute after equipped items contribute their default modifiers.
    pub fn armor_value(&self) -> u8 {
        let value = (36..40)
            .filter_map(|index| {
                let stack = self.slots.get(index)?.as_ref()?;
                Some(self.recipes.armor_points(stack, armor_slot_name(index)?))
            })
            .sum::<f64>();
        value.floor().clamp(0.0, u8::MAX as f64) as u8
    }
    /// The worn armor's `ARMOR_TOUGHNESS` and `KNOCKBACK_RESISTANCE`.
    pub fn armor_toughness(&self) -> (f64, f64) {
        (36..40)
            .filter_map(|index| {
                let stack = self.slots.get(index)?.as_ref()?;
                Some(self.recipes.armor_toughness(stack, armor_slot_name(index)?))
            })
            .fold((0.0, 0.0), |(t, k), (dt, dk)| (t + dt, k + dk))
    }
    pub fn count(&self, id: &str) -> u32 {
        self.slots
            .iter()
            .filter_map(Option::as_ref)
            .chain(self.cursor.iter())
            .chain(self.crafting.iter().filter_map(Option::as_ref))
            .chain(self.workbench.iter().filter_map(Option::as_ref))
            .filter(|s| s.id == id)
            .map(|s| s.count as u32)
            .sum()
    }
}

pub(crate) fn click_stack(
    target: &mut Option<ItemStack>,
    cursor: &mut Option<ItemStack>,
    right: bool,
) {
    match (target.as_mut(), cursor.as_mut()) {
        (None, None) => {}
        (Some(_), None) => {
            let mut taken = target.take().unwrap();
            if right {
                let half = taken.count.div_ceil(2);
                taken.count -= half;
                *cursor = Some(ItemStack {
                    count: half,
                    ..taken.clone()
                });
                if taken.count > 0 {
                    *target = Some(taken);
                }
            } else {
                *cursor = Some(taken);
            }
        }
        (None, Some(carried)) => {
            let count = if right { 1 } else { carried.count };
            *target = Some(ItemStack {
                count,
                ..carried.clone()
            });
            carried.count -= count;
            if carried.count == 0 {
                *cursor = None;
            }
        }
        (Some(existing), Some(carried))
            if existing.same_item(carried) && existing.count < existing.max =>
        {
            let moved = if right {
                1
            } else {
                (existing.max - existing.count).min(carried.count)
            };
            existing.count += moved;
            carried.count -= moved;
            if carried.count == 0 {
                *cursor = None;
            }
        }
        (Some(_), Some(_)) if !right => std::mem::swap(target, cursor),
        _ => {}
    }
}
