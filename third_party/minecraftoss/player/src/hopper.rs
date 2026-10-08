//! Headless hopper transfers. Block-entity storage is independent of chunk
//! generation; a world adapter supplies neighboring container slots.
use crate::inventory::ItemStack;

#[derive(Clone, Debug)]
pub struct Hopper {
    pub slots: [Option<ItemStack>; 5],
    pub enabled: bool,
    pub cooldown: i32,
    pub last_tick: u64,
}

impl Default for Hopper {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            enabled: true,
            cooldown: -1,
            last_tick: 0,
        }
    }
}

impl Hopper {
    pub fn item_count(&self) -> u32 {
        self.slots
            .iter()
            .flatten()
            .map(|stack| stack.count as u32)
            .sum()
    }

    /// One server block-entity tick. The hopper ejects one item before it
    /// tries to pull one item from above. A successful cycle waits eight ticks.
    pub fn tick(
        &mut self,
        destination: Option<&mut [Option<ItemStack>]>,
        source: Option<&mut [Option<ItemStack>]>,
    ) -> bool {
        self.tick_at(self.last_tick + 1, destination, source)
    }

    pub fn tick_at(
        &mut self,
        game_time: u64,
        destination: Option<&mut [Option<ItemStack>]>,
        source: Option<&mut [Option<ItemStack>]>,
    ) -> bool {
        self.tick_at_with(game_time, destination, source, |_| false)
    }

    /// `suck_entities` is used only when there is no container above. Its
    /// return value follows vanilla: a partial entity-stack transfer changes
    /// inventory but does not start the eight-tick cooldown.
    pub fn tick_at_with(
        &mut self,
        game_time: u64,
        destination: Option<&mut [Option<ItemStack>]>,
        source: Option<&mut [Option<ItemStack>]>,
        suck_entities: impl FnOnce(&mut [Option<ItemStack>]) -> bool,
    ) -> bool {
        self.cooldown -= 1;
        self.last_tick = game_time;
        if self.cooldown > 0 || !self.enabled {
            return false;
        }
        self.cooldown = 0;
        let mut changed = destination.is_some_and(|slots| move_one(&mut self.slots, slots));
        if self
            .slots
            .iter()
            .any(|slot| slot.as_ref().is_none_or(|stack| stack.count < stack.max))
        {
            changed |= match source {
                Some(slots) => move_one(slots, &mut self.slots),
                None => suck_entities(&mut self.slots),
            };
        }
        if changed {
            self.cooldown = 8;
        }
        changed
    }
}

/// `AbstractContainerMenu.getRedstoneSignalFromContainer` in pinned 26.3:
/// average slot fullness followed by `Mth.lerpDiscrete(fullness, 0, 15)`.
pub fn container_signal(slots: &[Option<ItemStack>]) -> u8 {
    if slots.is_empty() {
        return 0;
    }
    let fullness = slots
        .iter()
        .flatten()
        .map(|stack| stack.count as f32 / stack.max.max(1) as f32)
        .sum::<f32>()
        / slots.len() as f32;
    ((fullness * 14.0).floor() as u8 + u8::from(fullness > 0.0)).min(15)
}

/// Item entities offer their entire stack to the hopper. A partial fit updates
/// both inventories, while the caller observes `false` for cooldown purposes.
pub fn absorb_stack(entity: &mut ItemStack, slots: &mut [Option<ItemStack>]) -> bool {
    for target in slots {
        if entity.count == 0 {
            break;
        }
        match target {
            Some(current) if current.same_item(entity) && current.count < current.max => {
                let moved = entity.count.min(current.max - current.count);
                current.count += moved;
                entity.count -= moved;
            }
            None => {
                let moved = entity.count.min(entity.max);
                let mut inserted = entity.clone();
                inserted.count = moved;
                *target = Some(inserted);
                entity.count -= moved;
            }
            _ => {}
        }
    }
    entity.count == 0
}

/// Vanilla scans source slots then destination slots; it transfers one item
/// and merges only stacks with identical item data components.
pub fn move_one(source: &mut [Option<ItemStack>], destination: &mut [Option<ItemStack>]) -> bool {
    for from in source.iter_mut() {
        let Some(stack) = from.as_mut() else {
            continue;
        };
        if stack.count == 0 {
            continue;
        }
        let to = destination.iter_mut().find(|to| {
            to.as_ref()
                .is_none_or(|current| current.same_item(stack) && current.count < current.max)
        });
        let Some(to) = to else {
            continue;
        };
        match to {
            Some(target) => target.count += 1,
            None => {
                let mut moved = stack.clone();
                moved.count = 1;
                *to = Some(moved);
            }
        }
        stack.count -= 1;
        if stack.count == 0 {
            *from = None;
        }
        return true;
    }
    false
}
