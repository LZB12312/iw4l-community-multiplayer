//! The integrated server's world simulation for a streamed world: the
//! exact `minecraftoss_world::level::Level` owns the loaded FULL chunks,
//! takes the player's edits and uses, ticks at 20 Hz, and hands back the
//! positions whose blocks changed so the client scene and the saved chunks
//! follow it.

use crate::scene::{Block, BlockPos, HandcraftedScene, Scene};
use crate::terrain::BlockStates;
use minecraftoss_core::{BlockStateId, Chunk, ChunkPos};
use minecraftoss_world::chunk_map::WorldGen;
use minecraftoss_world::level::{Level, update};
use minecraftoss_world::natural_spawner::tick::{CensusMob, SpawnPlayer};
use std::sync::Arc;

/// How a player changed a block (`BlockItem.place` sets with flags 11,
/// breaking removes the block with flags 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerEdit {
    Place,
    Break,
}

pub struct ServerSim {
    level: Level<'static>,
    states: Arc<BlockStates>,
    /// Mobs, ticked after the level each tick.
    mobs: minecraftoss_entities::world::EntityWorld,
    /// Each block state as mobs read it.
    mob_tables: crate::server_mobs::MobTables,
    /// The tag each mob was loaded or spawned from, by entity ID: saving
    /// overwrites what the entity world simulates and keeps the rest.
    mob_tags: std::collections::HashMap<u64, minecraftoss_core::nbt::Tag>,
    /// The entity storage the level's entities are saved to, shared with
    /// the chunk map (`EntityStorage`).
    storage: Option<Arc<minecraftoss_world::storage::ChunkStorage>>,
    /// Server ticks since the last autosave.
    ticks_since_save: u32,
    entity_loot: Option<minecraftoss_entities::loot::EntityLootBook>,
    shearing_loot: Option<minecraftoss_entities::loot::ShearingLootBook>,
    /// Spawned or loaded mobs the entity world does not simulate yet, each
    /// with its riders, and the tag that saves them unchanged.
    dormant: Vec<(Vec<CensusMob>, minecraftoss_core::nbt::Tag)>,
    /// Creeper blasts since the client last heard of them.
    explosions: Vec<minecraftoss_entities::creeper::CreeperExplosion>,
    /// The player's `takeXpDelay`: ticks until it can take another orb.
    take_xp_delay: i32,
    /// The client was asked to close its trading screen.
    merchant_closing: bool,
}

/// What a player's hit or use on a mob did, for the client to present.
#[derive(Clone, Debug, Default)]
pub struct MobResult {
    pub sounds: Vec<crate::mob_actions::MobSound>,
    /// The acting player's inventory slots the action changed, with what
    /// they now hold.
    pub slots: Vec<(usize, Option<minecraftoss_player::inventory::ItemStack>)>,
    /// A trading screen the action opened (`openTradingScreen`).
    pub merchant: Option<MerchantView>,
}

/// A player's trading screen as the client shows it: the villager's
/// offers, profession, level and experience
/// (`ClientboundMerchantOffersPacket`) and the menu's slots.
#[derive(Clone, Debug, PartialEq)]
pub struct MerchantView {
    pub villager: u64,
    pub profession: &'static str,
    pub level: i32,
    pub xp: i32,
    pub offers: Vec<minecraftoss_entities::trading::MerchantOffer>,
    pub payment: [Option<minecraftoss_player::inventory::ItemStack>; 2],
    pub result: Option<minecraftoss_player::inventory::ItemStack>,
    pub future_xp: i32,
    /// Items' maximum stack sizes the offers name, for the client's
    /// prices.
    pub max_stacks: Vec<(String, i32)>,
}

/// What the player does on the trading screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MerchantOp {
    /// An offer picked in the list.
    Select(i32),
    /// A payment slot clicked (shift: moved back).
    Payment {
        slot: usize,
        right: bool,
        shift: bool,
    },
    /// The result clicked (shift: traded as often as it goes).
    Result {
        shift: bool,
    },
    Close,
}

/// A trading screen's new state after an operation, or its closing
/// (`view` none), with the acting player's inventory slots it changed and
/// the cursor.
#[derive(Clone, Debug)]
pub struct MerchantUpdate {
    pub view: Option<MerchantView>,
    pub slots: Vec<(usize, Option<minecraftoss_player::inventory::ItemStack>)>,
    pub cursor: Option<minecraftoss_player::inventory::ItemStack>,
    /// The server asks the client to close the screen (the villager went
    /// away or stopped trading); the client answers with `Close`.
    pub closing: bool,
}

/// An item entity on the server, as the client shows it. Components travel
/// as their JSON text.
#[derive(Clone, Debug)]
pub struct ServerItem {
    pub id: i32,
    pub item: String,
    pub count: i32,
    pub components: Option<String>,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub velocity: [f64; 3],
    pub age: i32,
    pub pickup_delay: i32,
    pub on_ground: bool,
}

/// A falling block entity as the client shows it.
#[derive(Clone, Debug)]
pub struct ServerFallingBlock {
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub block: String,
}

/// An experience orb as the client shows it.
#[derive(Clone, Copy, Debug)]
pub struct ServerOrb {
    pub id: i32,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub value: i32,
    /// `tickCount`, which drives its colour.
    pub tick_count: i32,
}

/// The level's entities as the client shows them after a tick.
#[derive(Clone, Debug, Default)]
pub struct EntitySnapshot {
    pub items: Vec<ServerItem>,
    pub tnt: Vec<ServerTnt>,
    pub falling: Vec<ServerFallingBlock>,
    pub orbs: Vec<ServerOrb>,
}

/// A primed TNT entity as the client shows it.
#[derive(Clone, Copy, Debug)]
pub struct ServerTnt {
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub fuse: i32,
}

fn stack_of(
    item: &str,
    count: i32,
    components: Option<&str>,
) -> minecraftoss_core::item::ItemStack {
    let mut stack = minecraftoss_core::item::ItemStack::new(item, count);
    stack.components = components.map(|json| minecraftoss_core::nbt::Tag::String(json.to_owned()));
    stack
}

fn components_of(stack: &minecraftoss_core::item::ItemStack) -> Option<String> {
    match &stack.components {
        Some(minecraftoss_core::nbt::Tag::String(json)) => Some(json.clone()),
        Some(other) => Some(format!("{other:?}")),
        None => None,
    }
}

impl ServerSim {
    /// A level for a dimension. The world generation data lives as long as
    /// the process (one small leak per world opened).
    pub fn new(worldgen: Arc<WorldGen>, states: Arc<BlockStates>, dimension_type: &str) -> Self {
        let leaked: &'static Arc<WorldGen> = Box::leak(Box::new(worldgen));
        let worldgen: &'static WorldGen = leaked;
        let range = states.vertical_range();
        let mut level = Level::new(&worldgen.library, range.start, range.end - range.start);
        level.random_sequences =
            minecraftoss_core::loot::RandomSequences::new(worldgen.terrain.seed);
        if let Err(e) = level.set_dimension(dimension_type) {
            eprintln!("world simulation without environment: {e}");
        }
        let mob_tables = crate::server_mobs::MobTables::new(&level, &states);
        // Natural spawning, when the entity catalog can create mobs.
        if level.registries().entities.is_some() {
            match minecraftoss_world::natural_spawner::CreatureSpawns::load(
                worldgen.terrain.registries.clone(),
                dimension_type,
                false,
            ) {
                Ok(spawns) => {
                    level.natural_spawning = Some(
                        minecraftoss_world::level::spawning::NaturalSpawning::new(Arc::new(spawns)),
                    )
                }
                Err(e) => eprintln!("natural spawning unavailable: {e}"),
            }
        }
        // The players are real entities: arrows hit them.
        let mut mobs = minecraftoss_entities::world::EntityWorld::default();
        mobs.set_players_pickable(true);
        // New mobs' UUIDs differ from session to session, as vanilla's
        // random ones do.
        mobs.set_uuid_salt(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64),
        );
        mobs.pois =
            minecraftoss_entities::poi::PoiManager::new(range.start >> 4, (range.end - 1) >> 4);
        Self {
            level,
            states,
            mobs,
            mob_tables,
            mob_tags: std::collections::HashMap::new(),
            storage: None,
            ticks_since_save: 0,
            entity_loot: None,
            shearing_loot: None,
            dormant: Vec::new(),
            explosions: Vec::new(),
            take_xp_delay: 0,
            merchant_closing: false,
        }
    }

    /// Loads the entity and shearing loot tables and the villager trade
    /// sets from the data JAR, with the world seed's random sequences.
    pub fn load_loot(&mut self, jar: &std::path::Path, seed: u64) {
        self.entity_loot = minecraftoss_entities::loot::EntityLootBook::from_jar(jar, seed)
            .map_err(|e| eprintln!("server entity loot unavailable: {e:#}"))
            .ok();
        self.shearing_loot = minecraftoss_entities::loot::ShearingLootBook::from_jar(jar, seed)
            .map_err(|e| eprintln!("server shearing loot unavailable: {e:#}"))
            .ok();
        // The trade sets read the items' enchantability and stack sizes
        // from the exported item catalog: beside a JAR fetched into a
        // MinecraftOSS-shaped folder, in a checkout, or under the working
        // directory.
        const CATALOG: &str = "artifacts/item-catalog/26.3.json";
        let catalog = [
            jar.parent().map(|root| root.join(CATALOG)),
            std::env::var_os("MINECRAFTOSS_ROOT")
                .map(|root| std::path::Path::new(&root).join(CATALOG)),
            Some(std::path::PathBuf::from(CATALOG)),
        ]
        .into_iter()
        .flatten()
        .find(|path| path.is_file());
        match catalog.map(|path| minecraftoss_entities::trading::TradeBook::from_jar(jar, &path)) {
            Some(Ok(book)) => self.mobs.set_trades(Arc::new(book), seed),
            Some(Err(e)) => eprintln!("server villager trades unavailable: {e:#}"),
            None => eprintln!("server villager trades unavailable: no item catalog"),
        }
    }

    /// A player's hit (`attack`, with what the player brings to it) or item
    /// use on a mob, with a copy of the player's inventory. Drops enter the
    /// level as item entities.
    pub fn mob_action(
        &mut self,
        hit: minecraftoss_entities::world::MobHit,
        attack: Option<minecraftoss_entities::world::PlayerAttack>,
        mut inventory: minecraftoss_player::inventory::Inventory,
        selected: usize,
        infinite: bool,
    ) -> MobResult {
        let before = inventory.slots.clone();
        let mut actor = crate::mob_actions::Actor {
            inventory: &mut inventory,
            selected,
            infinite,
            entity_loot: self.entity_loot.as_mut(),
            shearing_loot: self.shearing_loot.as_mut(),
        };
        let outcome = if let Some(attack) = &attack {
            crate::mob_actions::attack(&mut self.mobs, hit, &mut actor, attack)
        } else {
            crate::mob_actions::interact(&mut self.mobs, hit, &mut actor)
        };
        for (stack, position) in outcome.drops {
            let components = stack.components.as_ref().map(|c| c.to_string());
            let stack = stack_of(&stack.id, i32::from(stack.count), components.as_deref());
            self.level.spawn_at_location(position.to_array(), stack);
        }
        for (position, amount) in outcome.experience {
            self.level.award_experience(position.to_array(), amount);
        }
        let slots = inventory
            .slots
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(_, (now, was))| now != was)
            .map(|(slot, (now, _))| (slot, now.clone()))
            .collect();
        self.spawn_trade_experience();
        MobResult {
            sounds: outcome.sounds,
            slots,
            merchant: self.merchant_view(0),
        }
    }

    /// The player's trading screen as the client shows it.
    pub fn merchant_view(&mut self, player: u64) -> Option<MerchantView> {
        let menu = self.mobs.merchant_menu(player)?.clone();
        let offers = self
            .mobs
            .villager_offers(menu.villager)
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        let villager = self
            .mobs
            .villagers()
            .iter()
            .find(|e| e.id == menu.villager)?;
        let mut max_stacks = Vec::new();
        for offer in &offers {
            for id in std::iter::once(&offer.buy.id)
                .chain(offer.buy_b.as_ref().map(|b| &b.id))
                .chain(std::iter::once(&offer.sell.id))
            {
                if !max_stacks
                    .iter()
                    .any(|(item, _): &(String, i32)| item == id)
                {
                    max_stacks.push((id.clone(), self.mobs.item_max_stack(id)));
                }
            }
        }
        Some(MerchantView {
            villager: menu.villager,
            profession: villager.villager.profession.id(),
            level: villager.villager.level,
            xp: villager.villager.xp,
            offers,
            payment: menu.payment.clone(),
            result: menu.result.clone(),
            future_xp: menu.future_xp,
            max_stacks,
        })
    }

    /// An operation on the player's trading screen, with a copy of the
    /// player's inventory (and the hotbar slot selected): the screen's new
    /// state, the slots it changed and the cursor. Closing returns the
    /// payments and the cursor to the inventory; what does not fit drops
    /// at the player's feet.
    pub fn merchant(
        &mut self,
        op: MerchantOp,
        mut inventory: minecraftoss_player::inventory::Inventory,
        selected: usize,
        feet: [f64; 3],
    ) -> MerchantUpdate {
        let before = inventory.slots.clone();
        match op {
            MerchantOp::Select(index) => self.mobs.merchant_select(0, index, &mut inventory),
            MerchantOp::Payment { slot, right, shift } => {
                self.mobs
                    .merchant_click_payment(0, slot, right, shift, &mut inventory)
            }
            MerchantOp::Result { shift } => {
                self.mobs.merchant_click_result(0, shift, &mut inventory)
            }
            MerchantOp::Close => {
                for stack in self.mobs.merchant_close(0, &mut inventory, selected) {
                    let components = stack.components.as_ref().map(|c| c.to_string());
                    self.level.spawn_at_location(
                        feet,
                        stack_of(&stack.id, i32::from(stack.count), components.as_deref()),
                    );
                }
                self.merchant_closing = false;
            }
        }
        self.spawn_trade_experience();
        let slots = inventory
            .slots
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(_, (now, was))| now != was)
            .map(|(slot, (now, _))| (slot, now.clone()))
            .collect();
        MerchantUpdate {
            view: self.merchant_view(0),
            slots,
            cursor: inventory.cursor.clone(),
            closing: false,
        }
    }

    /// `ServerPlayer.tick`'s `stillValid` check on an open trading screen:
    /// once the villager is out of reach, dead or done trading, the client
    /// is asked (once) to close it.
    fn check_merchant(
        &mut self,
        players: &[minecraftoss_entities::tempt::PlayerCandidate],
    ) -> Option<MerchantUpdate> {
        if self.merchant_closing || self.mobs.merchant_menu(0).is_none() {
            return None;
        }
        let player = players.iter().find(|p| p.id == 0)?;
        let eyes = player.position + glam::DVec3::Y * f64::from(player.eye_height);
        if self.mobs.merchant_still_valid(0, eyes) {
            return None;
        }
        self.merchant_closing = true;
        Some(MerchantUpdate {
            view: None,
            slots: Vec::new(),
            cursor: None,
            closing: true,
        })
    }

    /// The orbs trades dropped, each whole into the level.
    fn spawn_trade_experience(&mut self) {
        for (position, value) in self.mobs.take_trade_experience() {
            self.level.spawn_experience_orb(position.to_array(), value);
        }
    }

    /// Where the level's entities are saved (the chunk map's storage).
    pub fn set_entity_storage(
        &mut self,
        storage: Option<Arc<minecraftoss_world::storage::ChunkStorage>>,
    ) {
        self.storage = storage;
    }

    /// A chunk joins the level with its entities (`EntityStorage`): those
    /// saved for it, or for a chunk the level never saved, the ones
    /// generation made. A chunk sent again keeps the entities it has.
    pub fn load_chunk(&mut self, chunk: &Chunk) {
        let fresh = self.level.chunk(chunk.pos).is_none();
        self.level.insert_chunk(chunk.clone());
        if !fresh {
            return;
        }
        self.load_pois(chunk);
        let saved = self.storage.as_ref().and_then(|storage| {
            storage.load_entities(chunk.pos).unwrap_or_else(|e| {
                eprintln!("entities of chunk {:?} failed to load: {e}", chunk.pos);
                None
            })
        });
        for tag in saved.unwrap_or_else(|| chunk.generation.entities.clone()) {
            self.add_saved_entity(tag);
        }
    }

    /// A chunk's points of interest (`PoiManager.checkConsistencyWithBlocks`
    /// for each section that may hold one).
    fn load_pois(&mut self, chunk: &Chunk) {
        let table = &self.mob_tables.pois;
        let kind = |state: BlockStateId| table.get(state.0 as usize).copied().flatten();
        for (i, section) in chunk.sections().iter().enumerate() {
            let sy = chunk.min_section_y() + i as i32;
            let maybe = match &section.blocks {
                minecraftoss_core::palette::PalettedContainer::Single(state) => {
                    kind(*state).is_some()
                }
                minecraftoss_core::palette::PalettedContainer::Direct(values) => {
                    values.iter().any(|&s| kind(s).is_some())
                }
            };
            if maybe {
                self.mobs
                    .pois
                    .load_section((chunk.pos.x, sy, chunk.pos.z), |(x, y, z)| {
                        kind(section.block((x & 15) as usize, (y & 15) as usize, (z & 15) as usize))
                    });
            }
        }
    }

    /// The blocks set since the last call move the points of interest
    /// (`ServerLevel.updatePOIOnBlockStateChange`).
    fn sync_pois(&mut self) {
        for (x, y, z) in self.level.take_block_log() {
            let state = self.level.block(minecraftoss_core::BlockPos::new(x, y, z));
            let new = self
                .mob_tables
                .pois
                .get(state.0 as usize)
                .copied()
                .flatten();
            let old = self.mobs.pois.kind((x, y, z));
            self.mobs.pois.block_changed((x, y, z), old, new);
        }
    }

    /// A saved entity joins the level: items and experience orbs as level
    /// entities, mobs the entity world simulates into it, and the rest
    /// dormant.
    fn add_saved_entity(&mut self, tag: minecraftoss_core::nbt::Tag) {
        self.mobs.set_day_time(self.level.overworld_clock());
        if crate::server_mobs::load_level_entity(&mut self.level, &tag) {
            return;
        }
        match crate::server_mobs::spawn_saved(&mut self.mobs, &tag) {
            Some(id) => {
                self.mob_tags.insert(id, tag);
            }
            None => self.dormant.push((
                minecraftoss_world::natural_spawner::tick::census_of(&tag),
                tag,
            )),
        }
    }

    /// Saves a chunk's entities: its mobs (from the tag each came from,
    /// with what the entity world changed), its dormant mobs as they came,
    /// and its items and experience orbs. Unloading also takes them out.
    fn save_chunk_entities(&mut self, pos: ChunkPos, unload: bool) {
        let inside = |p: glam::DVec3| crate::server_mobs::in_chunk(p, pos);
        let mut tags: Vec<minecraftoss_core::nbt::Tag> =
            crate::server_mobs::mob_tags(&self.mobs, inside, &self.mob_tags)
                .into_iter()
                .map(|(_, tag)| tag)
                .collect();
        tags.extend(
            self.dormant
                .iter()
                .filter(|(group, _)| inside(glam::DVec3::from_array(group[0].pos)))
                .map(|(_, tag)| tag.clone()),
        );
        tags.extend(crate::server_mobs::level_entity_tags(&self.level, inside));
        if unload {
            self.mobs.remove_where(inside);
            self.dormant
                .retain(|(group, _)| !inside(glam::DVec3::from_array(group[0].pos)));
            self.level.entities.retain(|e| {
                !(inside(glam::DVec3::from_array(e.pos))
                    && (e.item_data().is_some() || e.orb_data().is_some()))
            });
            let alive: std::collections::HashSet<u64> = self.mobs.mob_ids().collect();
            self.mob_tags.retain(|id, _| alive.contains(id));
        }
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.save_entities(pos, &tags) {
                eprintln!("entities of chunk {pos:?} failed to save: {e}");
            }
        }
    }

    /// Saves every loaded chunk's entities and writes the entity storage
    /// (autosave and shutdown).
    pub fn save_all_entities(&mut self) {
        let loaded: Vec<ChunkPos> = self.level.chunks().map(|c| c.pos).collect();
        for pos in loaded {
            self.save_chunk_entities(pos, false);
        }
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.flush() {
                eprintln!("entity storage write failed: {e}");
            }
        }
    }

    /// Before the level ticks: natural spawning's players (registered with
    /// its spawn counter), the mob census for its caps, and the simulation
    /// area.
    pub fn prepare_spawning(&mut self, players: &[minecraftoss_entities::tempt::PlayerCandidate]) {
        let Some(natural) = &mut self.level.natural_spawning else {
            return;
        };
        let spawn_players: Vec<SpawnPlayer> = players
            .iter()
            .map(|p| SpawnPlayer {
                pos: p.position.to_array(),
                spectator: p.spectator,
            })
            .collect();
        natural.set_players(&spawn_players);
        let mut census: Vec<CensusMob> = self
            .mobs
            .census()
            .into_iter()
            .map(|m| {
                let half = f64::from(m.width / 2.0);
                let p = m.position;
                CensusMob {
                    kind: m.kind.to_owned(),
                    pos: p.to_array(),
                    bb: [
                        p.x - half,
                        p.y,
                        p.z - half,
                        p.x + half,
                        p.y + f64::from(m.height),
                        p.z + half,
                    ],
                    persistent: m.persistent,
                    riding: false,
                }
            })
            .collect();
        census.extend(
            self.dormant
                .iter()
                .flat_map(|(group, _)| group.iter().cloned()),
        );
        natural.census = census;
        natural.simulation = self
            .level
            .ticking_chunks
            .iter()
            .flatten()
            .copied()
            .collect();
    }

    /// Mobs natural spawning made join the entity world; types it does not
    /// simulate yet (and jockeys) wait dormant: they count toward the caps
    /// and block spawns until they despawn.
    fn take_spawned(&mut self) {
        for tag in std::mem::take(&mut self.level.spawned) {
            self.add_saved_entity(tag);
        }
    }

    /// `Mob.checkDespawn` for the mobs in loaded chunks, before they tick.
    fn despawn_mobs(
        &mut self,
        players: &[minecraftoss_entities::tempt::PlayerCandidate],
        difficulty: i32,
    ) {
        let feet: Vec<glam::DVec3> = players
            .iter()
            .filter(|p| !p.spectator)
            .map(|p| p.position)
            .collect();
        let level = &self.level;
        let loaded = |p: glam::DVec3| {
            level
                .chunk(ChunkPos::new(
                    (p.x.floor() as i32) >> 4,
                    (p.z.floor() as i32) >> 4,
                ))
                .is_some()
        };
        self.mobs.check_despawn(&feet, difficulty == 0, &loaded);
        // Dormant monsters go in peaceful and beyond 128 blocks (they never
        // idle: they do not tick).
        self.dormant.retain(|(group, _)| {
            let root = &group[0];
            if root.persistent
                || minecraftoss_world::natural_spawner::spawning::MobCategory::of_type(&root.kind)
                    != Some(minecraftoss_world::natural_spawner::spawning::MobCategory::Monster)
            {
                return true;
            }
            if difficulty == 0 {
                return false;
            }
            let at = glam::DVec3::from_array(root.pos);
            feet.iter()
                .map(|p| p.distance_squared(at))
                .min_by(f64::total_cmp)
                .is_none_or(|d| d <= 128.0 * 128.0)
        });
    }

    /// The mobs spawned that the entity world does not simulate.
    pub fn dormant_count(&self) -> usize {
        self.dormant.len()
    }

    /// The recipe book mobs consult (breeding colours).
    pub fn set_recipe_book(&mut self, recipes: Arc<minecraftoss_player::crafting::RecipeBook>) {
        self.mobs.set_recipe_book(recipes);
    }

    /// Mobs tick after the level, inside the entity-ticking range (the
    /// level's ticking chunks, `DistanceManager.inEntityTickingRange`).
    pub fn tick_mobs(
        &mut self,
        players: &[minecraftoss_entities::tempt::PlayerCandidate],
        bright_outside: bool,
    ) {
        self.sync_pois();
        let ticking: std::collections::HashSet<ChunkPos> = self
            .level
            .ticking_chunks
            .iter()
            .flatten()
            .copied()
            .collect();
        self.mobs.set_bright_outside(bright_outside);
        self.mobs.set_monsters_burn(self.level.monsters_burn());
        let Self {
            level,
            states,
            mobs,
            mob_tables,
            ..
        } = self;
        let mut world = crate::server_mobs::MobWorld {
            level: std::cell::RefCell::new(level),
            states,
            tables: mob_tables,
        };
        let ticks = |p: glam::DVec3| {
            ticking.contains(&ChunkPos::new(
                (p.x.floor() as i32) >> 4,
                (p.z.floor() as i32) >> 4,
            ))
        };
        // Brains draw from the level's own random, and villagers keep the
        // overworld clock's schedule.
        mobs.set_day_time(world.level.borrow().overworld_clock());
        let shared = match &world.level.borrow().random {
            minecraftoss_core::random::AnyRandom::Legacy(random) => {
                Some((random.state(), random.gaussian_cache()))
            }
            _ => None,
        };
        if let Some((state, _)) = shared {
            *mobs.level_random_mut() =
                minecraftoss_player::rng::LegacyRandom::from_raw_state(state);
        }
        mobs.tick_with_players_where(&mut world, players, &ticks);
        // Births and summons are listed for replays that tag them; the game
        // needs no list.
        let _ = (
            mobs.take_born_villagers(),
            mobs.take_born_wolves(),
            mobs.take_summoned_golems(),
        );
        if let Some((_, gaussian)) = shared {
            if let minecraftoss_core::random::AnyRandom::Legacy(random) =
                &mut world.level.borrow_mut().random
            {
                random.set_seed((mobs.level_random_mut().raw_state() ^ 0x5DEE_CE66D) as i64);
                random.set_gaussian_cache(gaussian);
            }
        }
        // `ServerPlayer.doTick` (the connection tick, after the levels):
        // the players push the mobs they walk into.
        mobs.push_from_players(players, &ticks);
        // Entity events (a villager's hearts, anger, happiness) are for the
        // client's particles, which it does not draw yet.
        let _ = mobs.take_entity_events();
        // A blast's victims drop their loot before its blocks break
        // (`ServerExplosion.explode`: `hurtEntities`, then the blocks).
        self.drop_death_loot();
        self.explode_creepers();
    }

    /// The loot and experience of the mobs that died, into the level as
    /// item entities and experience orbs (`dropAllDeathLoot`).
    fn drop_death_loot(&mut self) {
        let (drops, experience) =
            crate::mob_actions::death_remains(&mut self.mobs, self.entity_loot.as_mut());
        for (stack, position) in drops {
            let components = stack.components.as_ref().map(|c| c.to_string());
            let stack = stack_of(&stack.id, i32::from(stack.count), components.as_deref());
            self.level.spawn_at_location(position.to_array(), stack);
        }
        for (position, amount) in experience {
            self.level.award_experience(position.to_array(), amount);
        }
    }

    /// `Player.tick`'s `takeXpDelay` countdown, then `Player.aiStep` touching
    /// one orb (at random) when the player at `feet` can take one: returns
    /// the orb's ID, position and value.
    pub fn take_experience(&mut self, feet: Option<[f64; 3]>) -> Option<(i32, [f64; 3], i32)> {
        if self.take_xp_delay > 0 {
            self.take_xp_delay -= 1;
        }
        if self.take_xp_delay != 0 {
            return None;
        }
        let taken = self.level.player_touch_orb(feet?)?;
        self.take_xp_delay = 2;
        Some(taken)
    }

    /// The server's experience orbs.
    pub fn orbs(&self) -> Vec<ServerOrb> {
        self.level
            .entities
            .iter()
            .filter_map(|e| {
                e.orb_data().map(|d| ServerOrb {
                    id: e.id,
                    position: e.pos,
                    previous_position: e.old_position(),
                    value: d.value,
                    tick_count: e.tick_count,
                })
            })
            .collect()
    }

    /// Experience at a position (`ExperienceOrb.award`), as orbs.
    pub fn award_experience(&mut self, position: [f64; 3], amount: i32) {
        self.level.award_experience(position, amount);
    }

    /// `summon minecraft:experience_orb` with its `Value`: a still orb.
    pub fn summon_orb(&mut self, position: [f64; 3], value: i32) {
        self.level
            .add_entity(minecraftoss_world::level::entity::Entity::experience_orb(
                position, value,
            ));
    }

    /// `/summon` for a mob: the level makes its tag (`SummonCommand`), and
    /// it joins the entity world as a loaded mob would.
    pub fn summon(
        &mut self,
        kind: &str,
        position: [f64; 3],
        nbt: Option<&minecraftoss_core::nbt::Tag>,
        y_rot: f32,
    ) -> Result<(), String> {
        let tag = self.level.summon_mob(kind, position, nbt, y_rot)?;
        // A new brain reads the schedule for the time it is.
        self.mobs.set_day_time(self.level.overworld_clock());
        self.add_saved_entity(tag);
        Ok(())
    }

    /// Creeper blasts reach the level (`ServerLevel.explode` with
    /// `ExplosionInteraction.MOB`): blocks break when mobs may grief, with
    /// the drops decaying (`mob_explosion_drop_decay`), and items, TNT and
    /// falling blocks are hurt and pushed. The entity world already hurt
    /// its mobs and the players.
    fn explode_creepers(&mut self) {
        use minecraftoss_world::level::explosion::{BlockInteraction, Explosion};
        for blast in self.mobs.take_creeper_explosions() {
            let interaction = if self.mobs.mob_griefing() {
                BlockInteraction::DestroyWithDecay
            } else {
                BlockInteraction::Keep
            };
            self.level.explode(Explosion {
                center: blast.position.to_array(),
                radius: blast.radius,
                fire: false,
                interaction,
                source: None,
            });
            self.explosions.push(blast);
        }
    }

    /// The blasts since the last call, for the client's sound and particles.
    pub fn take_explosions(&mut self) -> Vec<minecraftoss_entities::creeper::CreeperExplosion> {
        std::mem::take(&mut self.explosions)
    }

    /// The mobs a player at `position` tracks (`ChunkMap.TrackedEntity`:
    /// within `range` blocks horizontally).
    pub fn tracked_mobs(
        &self,
        position: [f64; 3],
        range: f64,
    ) -> minecraftoss_entities::world::EntityWorld {
        let range_sq = range * range;
        self.mobs.clone_where(|p| {
            let (dx, dz) = (p.x - position[0], p.z - position[2]);
            dx * dx + dz * dz <= range_sq
        })
    }

    /// A chunk leaves the level; its entities are saved and taken out.
    pub fn unload_chunk(&mut self, pos: ChunkPos) {
        self.save_chunk_entities(pos, true);
        self.level.remove_chunk(pos);
        self.mobs.pois.unload_chunk(pos.x, pos.z);
    }

    fn state_of(&self, block: Option<&Block>) -> BlockStateId {
        block
            .and_then(|b| self.states.state_of(b))
            .unwrap_or(BlockStateId::AIR)
    }

    /// Applies a player's edit that the client scene already shows.
    pub fn player_edit(&mut self, scene: &HandcraftedScene, pos: BlockPos, edit: PlayerEdit) {
        self.player_edit_block(pos, Scene::block(scene, pos), edit);
    }

    /// A player's edit to `block` (`None` for air) at a position.
    pub fn player_edit_block(&mut self, pos: BlockPos, block: Option<&Block>, edit: PlayerEdit) {
        let target = self.state_of(block);
        let at = minecraftoss_core::BlockPos::new(pos.0, pos.1, pos.2);
        if self.level.block(at) == target {
            return;
        }
        let flags = match edit {
            PlayerEdit::Place => update::ALL | 8,
            PlayerEdit::Break => update::ALL,
        };
        self.level.set_block(at, target, flags, update::LIMIT);
    }

    /// `useWithoutItem` on a simulated block; false when not simulated.
    pub fn use_block(&mut self, pos: BlockPos, player_facing: &str) -> bool {
        let facing = minecraftoss_core::pos::Direction::from_name(player_facing);
        self.level.use_block_facing(
            minecraftoss_core::BlockPos::new(pos.0, pos.1, pos.2),
            facing,
        )
    }

    /// `BoneMealItem.useOn` on a clicked face: grow the block, or water
    /// plants in front of a sturdy face. True when the bone meal is used.
    pub fn bone_meal(&mut self, pos: BlockPos, face: &str) -> bool {
        let at = minecraftoss_core::BlockPos::new(pos.0, pos.1, pos.2);
        if self.level.grow_crop(at) {
            return true;
        }
        let Some(face) = minecraftoss_core::pos::Direction::from_name(face) else {
            return false;
        };
        let state = self.level.block(at);
        self.level.registries().blocks.is_face_sturdy(
            state,
            face,
            minecraftoss_core::SupportType::Full,
        ) && self.level.grow_water_plant(at.relative(face, 1))
    }

    pub fn attack_block(&mut self, pos: BlockPos) -> bool {
        self.level
            .attack_block(minecraftoss_core::BlockPos::new(pos.0, pos.1, pos.2))
    }

    /// The players the level can see (fire spreads near them), and the
    /// difficulty (`Difficulty.getId`).
    pub fn set_players(&mut self, positions: &[[f64; 3]], difficulty: i32) {
        self.level.players = positions.to_vec();
        self.level.difficulty = difficulty;
    }

    /// The chunks random ticks run in: every loaded chunk within
    /// `distance` (Chebyshev) of `center`, in X then Z order.
    pub fn set_simulation_area(&mut self, center: (i32, i32), distance: i32) {
        let mut list = Vec::with_capacity(((2 * distance + 1) * (2 * distance + 1)) as usize);
        for x in center.0 - distance..=center.0 + distance {
            for z in center.1 - distance..=center.1 + distance {
                let pos = ChunkPos::new(x, z);
                if self.level.chunk(pos).is_some() {
                    list.push(pos);
                }
            }
        }
        self.level.ticking_chunks = Some(list);
    }

    /// Keeps the level's default clock with the client's day time.
    pub fn set_time(&mut self, ticks: i64) {
        self.level.set_time(ticks);
    }

    pub fn tick(&mut self) {
        self.level.tick();
        self.level.unsupported.clear();
    }

    /// Hands an item entity the client created (a drop, a spill) to the
    /// server, which simulates it from then on; returns its entity ID.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_item(
        &mut self,
        item: &str,
        count: i32,
        components: Option<&str>,
        position: [f64; 3],
        velocity: [f64; 3],
        pickup_delay: i32,
        age: i32,
    ) -> i32 {
        self.level.spawn_item_with(
            position,
            stack_of(item, count, components),
            velocity,
            pickup_delay,
            age,
        )
    }

    /// The server's item entities.
    pub fn items(&self) -> Vec<ServerItem> {
        self.level
            .entities
            .iter()
            .filter_map(|e| e.item_data().map(|d| (e, d)))
            .map(|(e, d)| ServerItem {
                id: e.id,
                item: d.stack.id.clone(),
                count: d.stack.count,
                components: components_of(&d.stack),
                position: e.pos,
                previous_position: e.old_position(),
                velocity: e.delta,
                age: d.age,
                pickup_delay: d.pickup_delay,
                on_ground: e.on_ground,
            })
            .collect()
    }

    /// The server's falling blocks.
    pub fn falling_blocks(&self) -> Vec<ServerFallingBlock> {
        self.level
            .entities
            .iter()
            .filter_map(|e| {
                e.falling_data().map(|d| ServerFallingBlock {
                    position: e.pos,
                    previous_position: e.old_position(),
                    block: self
                        .level
                        .registries()
                        .blocks
                        .block(self.level.registries().blocks.block_of(d.state))
                        .name
                        .to_string(),
                })
            })
            .collect()
    }

    /// The server's primed TNT.
    pub fn primed_tnt(&self) -> Vec<ServerTnt> {
        self.level
            .entities
            .iter()
            .filter_map(|e| {
                e.tnt_data().map(|d| ServerTnt {
                    position: e.pos,
                    previous_position: e.old_position(),
                    fuse: d.fuse,
                })
            })
            .collect()
    }

    /// `ItemEntity.playerTouch` for a player standing at `feet`: `take`
    /// offers each touching stack (item, count, components) and returns how
    /// many the inventory took. Returns (entity ID, position, item, count,
    /// components) per pickup.
    pub fn pickup(
        &mut self,
        feet: [f64; 3],
        mut take: impl FnMut(&str, i32, Option<&str>) -> i32,
    ) -> Vec<(i32, [f64; 3], String, i32, Option<String>)> {
        self.level
            .player_touch_items(feet, |stack| {
                let components = components_of(stack);
                take(&stack.id, stack.count, components.as_deref())
            })
            .into_iter()
            .map(|(id, position, stack)| {
                let components = components_of(&stack);
                (id, position, stack.id, stack.count, components)
            })
            .collect()
    }

    /// Positions changed since the last call, with the block now there.
    pub fn take_changes(&mut self) -> Vec<(BlockPos, Option<Block>)> {
        let changed = self.level.take_changed();
        if std::env::var_os("MINECRAFTOSS_DEBUG_SERVER").is_some() && !changed.is_empty() {
            eprintln!(
                "server changes at tick {}: {} (first {:?})",
                self.level.game_time,
                changed.len(),
                &changed[..changed.len().min(4)]
            );
        }
        changed
            .into_iter()
            .map(|(x, y, z)| {
                let state = self.level.block(minecraftoss_core::BlockPos::new(x, y, z));
                let block = if self.level.registries().blocks.is_air(state) {
                    None
                } else {
                    self.states.block(state).cloned()
                };
                ((x, y, z), block)
            })
            .collect()
    }
}

/// What the client asks of the integrated server thread, in order.
pub enum Command {
    LoadChunk(Arc<Chunk>),
    UnloadChunk(ChunkPos),
    /// A block the client set, as the client scene now shows it.
    PlayerEdit {
        pos: BlockPos,
        block: Option<Block>,
        edit: PlayerEdit,
    },
    UseBlock {
        pos: BlockPos,
        facing: &'static str,
    },
    BoneMeal {
        pos: BlockPos,
        face: &'static str,
    },
    Attack(BlockPos),
    SpawnItem {
        item: String,
        count: i32,
        components: Option<String>,
        position: [f64; 3],
        velocity: [f64; 3],
        pickup_delay: i32,
        age: i32,
    },
    /// The recipe book, for mobs.
    RecipeBook(Arc<minecraftoss_player::crafting::RecipeBook>),
    /// The data JAR and world seed the server's loot tables come from.
    LootTables {
        jar: std::path::PathBuf,
        seed: u64,
    },
    /// `summon minecraft:experience_orb`.
    SummonOrb {
        position: [f64; 3],
        value: i32,
    },
    /// `/summon` for a mob, with the command's NBT and the new mob's own
    /// random yaw.
    Summon {
        kind: String,
        position: [f64; 3],
        nbt: Option<minecraftoss_core::nbt::Tag>,
        y_rot: f32,
    },
    /// A player's hit (`attack`) or item use on a mob, with a copy of its
    /// inventory.
    MobAction {
        hit: minecraftoss_entities::world::MobHit,
        attack: Option<minecraftoss_entities::world::PlayerAttack>,
        inventory: Box<minecraftoss_player::inventory::Inventory>,
        selected: usize,
        infinite: bool,
    },
    /// An operation on the player's trading screen.
    Merchant {
        op: MerchantOp,
        inventory: Box<minecraftoss_player::inventory::Inventory>,
        selected: usize,
        feet: [f64; 3],
    },
    /// One server tick, with the player's state for it.
    Tick(Box<TickInput>),
}

/// The client state a server tick reads.
pub struct TickInput {
    pub day_ticks: i64,
    pub players: Vec<[f64; 3]>,
    pub difficulty: i32,
    pub simulation_center: (i32, i32),
    pub simulation_distance: i32,
    /// The player's feet, when it can pick items up, with its inventory and
    /// selected slot (the server offers touching stacks to this copy).
    pub pickup: Option<(
        [f64; 3],
        Box<minecraftoss_player::inventory::Inventory>,
        usize,
    )>,
    /// The player as mobs see it (held food, whether it can be targeted).
    pub mob_players: Vec<minecraftoss_entities::tempt::PlayerCandidate>,
    /// Where each of those players looks (endermen read stares from it).
    pub mob_views: Vec<(u64, minecraftoss_entities::enderman::PlayerView)>,
    /// Each of those players' health, effects and motion (witches pick
    /// their potions by them).
    pub mob_vitals: Vec<(u64, minecraftoss_entities::monster_ai::PlayerVitals)>,
    /// `Level.isBrightOutside`.
    pub bright_outside: bool,
    /// The player's position and how far away it tracks mobs.
    pub tracking: ([f64; 3], f64),
    /// The hurts that landed on the player since the last tick: the mob
    /// behind each and the damage type (its tame wolves avenge it).
    pub player_hurts: Vec<(Option<u64>, &'static str)>,
    /// The `spawn_mobs` game rule.
    pub spawn_mobs: bool,
}

/// What the server thread sends back after handling commands.
#[derive(Default)]
pub struct Output {
    /// Blocks that changed, with the block now there.
    pub changes: Vec<(BlockPos, Option<Block>)>,
    /// Entity snapshots, after a tick.
    pub entities: Option<EntitySnapshot>,
    /// Experience orbs the player took: (entity ID, position, value).
    pub orbs_taken: Vec<(i32, [f64; 3], i32)>,
    /// The mobs the player tracks, after a tick.
    pub mobs: Option<Box<minecraftoss_entities::world::EntityWorld>>,
    /// What the player's mob actions did, in order.
    pub mob_results: Vec<MobResult>,
    /// The trading screen's updates, in order.
    pub merchant: Vec<MerchantUpdate>,
    /// Mobs' hits on the players, in order.
    pub player_hits: Vec<minecraftoss_entities::world::PlayerHit>,
    /// Splash potions that broke near players, in order.
    pub player_splashes: Vec<(u64, minecraftoss_entities::world::PlayerSplash)>,
    /// Where splash potions broke (their sound and colour).
    pub potion_breaks: Vec<minecraftoss_entities::world::PotionBreak>,
    /// Creeper blasts this tick (`ClientboundExplodePacket`).
    pub explosions: Vec<minecraftoss_entities::creeper::CreeperExplosion>,
    /// Sounds mobs made this tick.
    pub mob_sounds: Vec<minecraftoss_entities::world::MobSound>,
    /// Items the player picked up: (entity ID, position, item, count, components).
    pub picked: Vec<(i32, [f64; 3], String, i32, Option<String>)>,
    /// What each `/summon` did: the type it made, or why it failed.
    pub summoned: Vec<Result<String, String>>,
    /// Positions where bone meal was used.
    pub bone_meal_used: Vec<BlockPos>,
    /// How many commands the server has handled so far.
    pub handled: u64,
    /// The last tick's phases, light solves and total milliseconds.
    pub tick_phases: Option<([f64; 6], (u32, f64), f64)>,
}

/// The integrated server on its own thread, as vanilla runs it: the client
/// sends commands and never waits for a tick.
pub struct ServerHandle {
    commands: Option<std::sync::mpsc::Sender<Command>>,
    outputs: std::sync::mpsc::Receiver<Output>,
    states: Arc<BlockStates>,
    /// Per block state: whether the level acts on a use or an attack.
    uses: Arc<Vec<bool>>,
    attacks: Arc<Vec<bool>>,
    sent: u64,
    /// Outputs [`Self::wait_idle`] received ahead of the next poll.
    waited: Vec<Output>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ServerHandle {
    pub fn spawn(sim: ServerSim) -> Self {
        let states = sim.states.clone();
        let count = sim.level.registries().blocks.state_count();
        let uses: Vec<bool> = (0..count)
            .map(|i| sim.level.handles_use(BlockStateId(i as u16)))
            .collect();
        let attacks: Vec<bool> = (0..count)
            .map(|i| sim.level.handles_attack(BlockStateId(i as u16)))
            .collect();
        let (commands, receiver) = std::sync::mpsc::channel::<Command>();
        let (sender, outputs) = std::sync::mpsc::channel::<Output>();
        let thread = std::thread::Builder::new()
            .name("Server thread".into())
            .spawn(move || server_loop(sim, receiver, sender))
            .expect("server thread starts");
        Self {
            commands: Some(commands),
            outputs,
            states,
            uses: Arc::new(uses),
            attacks: Arc::new(attacks),
            sent: 0,
            waited: Vec::new(),
            thread: Some(thread),
        }
    }

    fn send(&mut self, command: Command) {
        self.sent += 1;
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// Commands sent so far (compare with `Output::handled`).
    pub fn sent(&self) -> u64 {
        self.sent
    }

    pub fn load_chunk(&mut self, chunk: &Arc<Chunk>) {
        self.send(Command::LoadChunk(chunk.clone()));
    }

    pub fn unload_chunk(&mut self, pos: ChunkPos) {
        self.send(Command::UnloadChunk(pos));
    }

    pub fn player_edit(&mut self, scene: &HandcraftedScene, pos: BlockPos, edit: PlayerEdit) {
        let block = Scene::block(scene, pos).cloned();
        self.send(Command::PlayerEdit { pos, block, edit });
    }

    fn state_in(&self, scene: &HandcraftedScene, pos: BlockPos) -> Option<BlockStateId> {
        Scene::block(scene, pos).and_then(|b| self.states.state_of(b))
    }

    /// `useWithoutItem` on a simulated block: true (and sent) when the level
    /// acts on the block the scene shows.
    pub fn use_block(
        &mut self,
        scene: &HandcraftedScene,
        pos: BlockPos,
        facing: &'static str,
    ) -> bool {
        let handled = self
            .state_in(scene, pos)
            .is_some_and(|s| self.uses.get(usize::from(s.0)).copied().unwrap_or(false));
        if handled {
            self.send(Command::UseBlock { pos, facing });
        }
        handled
    }

    /// Bone meal on a clicked face; whether it was used arrives in an output.
    pub fn bone_meal(&mut self, pos: BlockPos, face: &'static str) {
        self.send(Command::BoneMeal { pos, face });
    }

    /// A player's attack; true (and sent) when the level acts on the block.
    pub fn attack_block(&mut self, scene: &HandcraftedScene, pos: BlockPos) -> bool {
        let handled = self
            .state_in(scene, pos)
            .is_some_and(|s| self.attacks.get(usize::from(s.0)).copied().unwrap_or(false));
        if handled {
            self.send(Command::Attack(pos));
        }
        handled
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn_item(
        &mut self,
        item: &str,
        count: i32,
        components: Option<&str>,
        position: [f64; 3],
        velocity: [f64; 3],
        pickup_delay: i32,
        age: i32,
    ) {
        self.send(Command::SpawnItem {
            item: item.to_owned(),
            count,
            components: components.map(str::to_owned),
            position,
            velocity,
            pickup_delay,
            age,
        });
    }

    pub fn tick(&mut self, input: TickInput) {
        self.send(Command::Tick(Box::new(input)));
    }

    pub fn set_recipe_book(&mut self, recipes: Arc<minecraftoss_player::crafting::RecipeBook>) {
        self.send(Command::RecipeBook(recipes));
    }

    pub fn load_loot(&mut self, jar: std::path::PathBuf, seed: u64) {
        self.send(Command::LootTables { jar, seed });
    }

    /// An operation on the player's trading screen, with a copy of the
    /// player's inventory.
    pub fn merchant(
        &mut self,
        op: MerchantOp,
        inventory: &minecraftoss_player::inventory::Inventory,
        selected: usize,
        feet: [f64; 3],
    ) {
        self.send(Command::Merchant {
            op,
            inventory: Box::new(inventory.clone()),
            selected,
            feet,
        });
    }

    /// Summons a still experience orb worth `value`.
    pub fn summon_orb(&mut self, position: [f64; 3], value: i32) {
        self.send(Command::SummonOrb { position, value });
    }

    /// `/summon` for a mob; what it did arrives in an output. The new
    /// mob's yaw comes from its own random (`LivingEntity`'s constructor:
    /// `nextFloat() * (float)(Math.PI * 2)`, in degrees), seeded here from
    /// the clock as a fresh entity's is.
    pub fn summon(
        &mut self,
        kind: String,
        position: [f64; 3],
        nbt: Option<minecraftoss_core::nbt::Tag>,
    ) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        let y_rot = minecraftoss_core::random::LegacyRandom::new(nanos).next_f32()
            * (std::f64::consts::PI * 2.0) as f32;
        self.send(Command::Summon {
            kind,
            position,
            nbt,
            y_rot,
        });
    }

    /// Hands a player's hit or use on a mob to the server; its result
    /// arrives in an output.
    pub fn mob_action(
        &mut self,
        hit: minecraftoss_entities::world::MobHit,
        attack: Option<minecraftoss_entities::world::PlayerAttack>,
        inventory: &minecraftoss_player::inventory::Inventory,
        selected: usize,
        infinite: bool,
    ) {
        self.send(Command::MobAction {
            hit,
            attack,
            inventory: Box::new(inventory.clone()),
            selected,
            infinite,
        });
    }

    /// Outputs the server has produced since the last call.
    pub fn poll(&mut self) -> Vec<Output> {
        let mut outputs = std::mem::take(&mut self.waited);
        outputs.extend(self.outputs.try_iter());
        outputs
    }

    /// Blocks until the server has handled every command sent so far (a
    /// capture's settle ticks, each answered before the next), keeping the
    /// outputs for the next [`Self::poll`].
    pub fn wait_idle(&mut self) {
        while self.waited.last().is_none_or(|out| out.handled < self.sent) {
            match self.outputs.recv() {
                Ok(out) => self.waited.push(out),
                Err(_) => break,
            }
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // Closing the channel ends the loop.
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn make_stack(
    recipes: &minecraftoss_player::crafting::RecipeBook,
    item: &str,
    count: i32,
    components: Option<&str>,
) -> minecraftoss_player::inventory::ItemStack {
    let mut stack = minecraftoss_player::inventory::ItemStack::new(item, count.clamp(0, 255) as u8);
    stack.components = components.and_then(|c| serde_json::from_str(c).ok());
    if stack.components.is_none() {
        stack.max = stack.max.min(recipes.max_stack(item));
    }
    stack
}

fn server_loop(
    mut sim: ServerSim,
    commands: std::sync::mpsc::Receiver<Command>,
    outputs: std::sync::mpsc::Sender<Output>,
) {
    let mut handled = 0u64;
    while let Ok(first) = commands.recv() {
        let mut out = Output::default();
        // Everything already queued is handled before answering.
        let mut next = Some(first);
        while let Some(command) = next.take().or_else(|| commands.try_recv().ok()) {
            handled += 1;
            match command {
                Command::LoadChunk(chunk) => sim.load_chunk(&chunk),
                Command::UnloadChunk(pos) => sim.unload_chunk(pos),
                Command::PlayerEdit { pos, block, edit } => {
                    sim.player_edit_block(pos, block.as_ref(), edit)
                }
                Command::UseBlock { pos, facing } => {
                    sim.use_block(pos, facing);
                }
                Command::BoneMeal { pos, face } => {
                    if sim.bone_meal(pos, face) {
                        out.bone_meal_used.push(pos);
                    }
                }
                Command::Attack(pos) => {
                    sim.attack_block(pos);
                }
                Command::SpawnItem {
                    item,
                    count,
                    components,
                    position,
                    velocity,
                    pickup_delay,
                    age,
                } => {
                    sim.spawn_item(
                        &item,
                        count,
                        components.as_deref(),
                        position,
                        velocity,
                        pickup_delay,
                        age,
                    );
                }
                Command::RecipeBook(recipes) => sim.set_recipe_book(recipes),
                Command::LootTables { jar, seed } => sim.load_loot(&jar, seed),
                Command::SummonOrb { position, value } => sim.summon_orb(position, value),
                Command::Summon {
                    kind,
                    position,
                    nbt,
                    y_rot,
                } => {
                    out.summoned.push(
                        sim.summon(&kind, position, nbt.as_ref(), y_rot)
                            .map(|()| kind),
                    );
                }
                Command::MobAction {
                    hit,
                    attack,
                    inventory,
                    selected,
                    infinite,
                } => {
                    out.mob_results
                        .push(sim.mob_action(hit, attack, *inventory, selected, infinite));
                }
                Command::Merchant {
                    op,
                    inventory,
                    selected,
                    feet,
                } => {
                    out.merchant
                        .push(sim.merchant(op, *inventory, selected, feet));
                }
                Command::Tick(input) => {
                    let started = std::time::Instant::now();
                    let input = *input;
                    // `MinecraftServer.autoSave`: every five minutes.
                    sim.ticks_since_save += 1;
                    if sim.ticks_since_save >= 6000 {
                        sim.ticks_since_save = 0;
                        sim.save_all_entities();
                    }
                    sim.set_time(input.day_ticks);
                    sim.set_players(&input.players, input.difficulty);
                    // Living, non-spectating players draw experience orbs.
                    sim.level.living_players = input
                        .mob_players
                        .iter()
                        .filter(|p| p.alive && !p.spectator)
                        .map(|p| (p.position.to_array(), f64::from(p.eye_height)))
                        .collect();
                    sim.set_simulation_area(input.simulation_center, input.simulation_distance);
                    if let Some(natural) = &mut sim.level.natural_spawning {
                        natural.spawn_mobs = input.spawn_mobs;
                    }
                    sim.prepare_spawning(&input.mob_players);
                    sim.tick();
                    let mobs_started = std::time::Instant::now();
                    sim.take_spawned();
                    sim.despawn_mobs(&input.mob_players, input.difficulty);
                    // The player's fight memory: its `tickCount` moves on and
                    // the hurts the client took land in it.
                    sim.mobs.tick_player(0);
                    for &(source, kind) in &input.player_hurts {
                        sim.mobs.player_hurt(0, source, kind);
                    }
                    sim.mobs.set_player_views(input.mob_views.clone());
                    sim.mobs.set_player_vitals(input.mob_vitals.clone());
                    // What the player holds, as villagers see it
                    // (`ShowTradesToPlayer`).
                    let held = input.pickup.as_ref().and_then(|(_, inventory, selected)| {
                        inventory
                            .slots
                            .get(*selected)
                            .and_then(Option::as_ref)
                            .map(|s| s.id.clone())
                    });
                    sim.mobs.set_player_main_hand(0, held.as_deref());
                    sim.tick_mobs(&input.mob_players, input.bright_outside);
                    sim.spawn_trade_experience();
                    out.merchant.extend(sim.check_merchant(&input.mob_players));
                    out.player_hits.extend(sim.mobs.take_player_hits());
                    out.player_splashes.extend(sim.mobs.take_player_splashes());
                    out.potion_breaks.extend(sim.mobs.take_potion_breaks());
                    out.explosions.extend(sim.take_explosions());
                    out.mob_sounds.extend(sim.mobs.take_sounds());
                    sim.level.last_tick_phases[5] += mobs_started.elapsed().as_secs_f64() * 1000.0;
                    out.mobs = Some(Box::new(
                        sim.tracked_mobs(input.tracking.0, input.tracking.1),
                    ));
                    let pickup_feet = input.pickup.as_ref().map(|(feet, _, _)| *feet);
                    if let Some((feet, mut inventory, selected)) = input.pickup {
                        let recipes = inventory.recipes.clone();
                        let picked = sim.pickup(feet, |item, count, components| {
                            let stack = make_stack(&recipes, item, count, components);
                            match inventory.add_item(stack, selected) {
                                None => count,
                                Some(rest) => count - i32::from(rest.count),
                            }
                        });
                        out.picked.extend(picked);
                    }
                    // After the items, one experience orb.
                    out.orbs_taken.extend(sim.take_experience(pickup_feet));
                    out.entities = Some(EntitySnapshot {
                        items: sim.items(),
                        tnt: sim.primed_tnt(),
                        falling: sim.falling_blocks(),
                        orbs: sim.orbs(),
                    });
                    let (solves, ms) = sim.level.light_solves.replace((0, 0.0));
                    out.tick_phases = Some((
                        sim.level.last_tick_phases,
                        (solves, ms),
                        started.elapsed().as_secs_f64() * 1000.0,
                    ));
                }
            }
        }
        out.changes = sim.take_changes();
        out.handled = handled;
        if outputs.send(out).is_err() {
            break;
        }
    }
    // The client closed the world: its entities are saved.
    sim.save_all_entities();
}
