use core::fmt;
use core::ops::{Deref, DerefMut};

use bevy_ecs::prelude::{Entity, World};
use playerstate_iw4::PlayerState;

use crate::frame::{
    FrameWorld, PayloadIndex, collect_dropped_items, collect_players, collect_projectiles,
    collect_script_movers, install_state_entity, player_ref, spawn_payload_rows,
};
use crate::gentity::ScriptMoverGentity;
use crate::identities::ScriptModelId;
use crate::input::TickInput;
use crate::snapshot::Snapshot;
use crate::world::{ClientId, SimState, Tick};

pub struct SimWorld {
    ecs: World,
    state_entity: Entity,
    schedule: bevy_ecs::schedule::Schedule,
}

impl Default for SimWorld {
    fn default() -> Self {
        let mut ecs = World::new();
        let state_entity = ecs.spawn(SimState::default()).id();
        ecs.entity_mut(state_entity).insert(PayloadIndex::default());
        install_state_entity(&mut ecs, state_entity);
        ecs.insert_resource(crate::script::Runtime::default());
        ecs.insert_resource(crate::script::Mechanics::default());
        ecs.insert_resource(crate::script::NativeRegistry::default());
        Self {
            ecs,
            state_entity,
            schedule: crate::step::schedule(),
        }
    }
}

impl Clone for SimWorld {
    fn clone(&self) -> Self {
        let mut ecs = World::new();
        let state_entity = ecs.spawn((**self).clone()).id();
        ecs.entity_mut(state_entity).insert(PayloadIndex::default());
        install_state_entity(&mut ecs, state_entity);
        spawn_payload_rows(
            &mut ecs,
            collect_players(&self.ecs),
            collect_projectiles(&self.ecs),
            collect_script_movers(&self.ecs),
            collect_dropped_items(&self.ecs),
        );
        crate::script::copy_state(&self.ecs, &mut ecs);
        Self {
            ecs,
            state_entity,
            schedule: crate::step::schedule(),
        }
    }
}

impl fmt::Debug for SimWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimWorld").field("state", &**self).finish()
    }
}

impl Deref for SimWorld {
    type Target = SimState;

    fn deref(&self) -> &Self::Target {
        self.ecs
            .get::<SimState>(self.state_entity)
            .expect("simulation state entity is missing its SimState component")
    }
}

impl DerefMut for SimWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ecs
            .get_mut::<SimState>(self.state_entity)
            .expect("simulation state entity is missing its SimState component")
            .into_inner()
    }
}

impl SimWorld {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn frame(&mut self) -> FrameWorld<'_> {
        FrameWorld::new(&mut self.ecs, self.state_entity)
    }

    fn run(
        &mut self,
        tick: Tick,
        input: &TickInput,
        msec: i32,
        reason: StepReason,
    ) -> Result<Snapshot, crate::script::Fault> {
        crate::step::run_schedule(&mut self.ecs, &mut self.schedule, tick, input, msec, reason)
    }

    pub fn visit_projectiles(&self, visit: impl FnMut(&crate::ProjectileState)) {
        crate::frame::visit_projectiles(&self.ecs, visit);
    }

    pub fn script_mover_count(&self) -> usize {
        crate::frame::script_mover_row_count(&self.ecs)
    }

    pub fn visit_script_movers(&self, visit: impl FnMut(&ScriptMoverGentity)) {
        crate::frame::visit_script_movers(&self.ecs, visit);
    }

    pub fn script_mover_by_number(&self, number: i32) -> Option<ScriptMoverGentity> {
        crate::frame::script_mover_by_number(&self.ecs, number)
    }

    pub fn script_mover_by_id(&self, id: ScriptModelId) -> Option<ScriptMoverGentity> {
        crate::frame::script_mover_by_id(&self.ecs, id)
    }

    pub fn install_gsc_program(
        &mut self,
        program: crate::script::Program,
        natives: crate::script::NativeRegistry,
        level: crate::script::LevelData,
    ) -> Result<(), crate::script::Fault> {
        crate::script::install(&mut self.ecs, program, natives, level)
    }

    pub fn start_gsc(
        &mut self,
        name: &str,
        receiver: crate::script::Value,
        arguments: Vec<crate::script::Value>,
    ) -> Result<u64, crate::script::Fault> {
        crate::script::start(&mut self.ecs, name, receiver, arguments)
    }

    pub fn set_gsc_dvar(&mut self, name: &str, value: &str) {
        crate::script::set_dvar(&mut self.ecs, name, value);
    }

    pub fn gsc_program_fingerprint(&self) -> Option<[u8; 32]> {
        self.ecs
            .resource::<crate::script::Runtime>()
            .program_fingerprint()
    }

    pub fn script_seats(&self) -> Vec<(ClientId, crate::ScriptSeat)> {
        crate::script::script_seats(&self.ecs)
    }

    pub fn take_script_fault(&mut self) -> Option<crate::script::Fault> {
        let mut runtime = self.ecs.resource_mut::<crate::script::Runtime>();
        if runtime.fault_reported {
            return None;
        }
        runtime.fault_reported = runtime.fault.is_some();
        runtime.fault.clone()
    }

    pub fn take_script_exit_level(&mut self) -> bool {
        std::mem::take(&mut self.ecs.resource_mut::<crate::script::Runtime>().exit_level)
    }

    pub fn spawn_script_mover(
        &mut self,
        id: ScriptModelId,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<i32, crate::EntityAllocError> {
        self.frame().spawn_script_mover(id, origin, angles)
    }

    pub fn spawn_brush_mover(
        &mut self,
        id: ScriptModelId,
        cmodel: u32,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<i32, crate::EntityAllocError> {
        let mut frame = self.frame();
        let number = frame.spawn_script_mover(id, origin, angles)?;
        if let Some(mover) = frame.script_mover_mut_by_number(number) {
            mover.state.index = cmodel as i32;
            mover.state.solid = entity_iw4::SCRIPT_MOVER_BMODEL_SOLID;
        }
        Ok(number)
    }

    pub fn gentity_number(&self, id: ScriptModelId) -> Option<i32> {
        self.script_mover_by_id(id).map(|mover| mover.state.number)
    }

    pub fn begin_script_mover_rotate_velocity(
        &mut self,
        id: ScriptModelId,
        speed: [f32; 3],
        total_time_seconds: f32,
        level_time_ms: i32,
    ) -> bool {
        self.frame().begin_script_mover_rotate_velocity(
            id,
            speed,
            total_time_seconds,
            level_time_ms,
        )
    }

    pub fn begin_script_movers_rotate_velocity_supplied(
        &mut self,
        speed: f32,
        level_time_ms: i32,
    ) -> usize {
        self.frame()
            .begin_script_movers_rotate_velocity_supplied(speed, level_time_ms)
    }

    pub fn set_script_mover_r_box(
        &mut self,
        number: i32,
        box_mid: [f32; 3],
        box_half: [f32; 3],
    ) -> bool {
        self.frame()
            .set_script_mover_r_box(number, box_mid, box_half)
    }

    pub fn set_script_mover_origin(&mut self, number: i32, origin: [f32; 3]) -> bool {
        self.frame().set_script_mover_origin(number, origin)
    }

    pub fn player(&self, id: ClientId) -> Option<&PlayerState> {
        player_ref(&self.ecs, id)
    }

    pub fn visit_players(&self, visit: impl FnMut(ClientId, &PlayerState)) {
        crate::frame::visit_players(&self.ecs, visit);
    }

    pub fn player_count(&self) -> usize {
        crate::frame::player_row_count(&self.ecs)
    }

    pub fn take_script_kicks(&mut self) -> Vec<(ClientId, String)> {
        std::mem::take(&mut self.ecs.resource_mut::<crate::script::Runtime>().kicks)
            .into_iter()
            .map(|(client, reason)| (ClientId(client), reason))
            .collect()
    }

    pub fn retire_client(&mut self, id: ClientId) {
        let mut runtime = self.ecs.resource_mut::<crate::script::Runtime>();
        if runtime.players.contains_key(&id.0) {
            runtime.disconnects.insert(id.0);
        } else {
            self.frame().retire_client(id);
        }
    }

    pub fn set_skate_damage_settings(&mut self, tolerance: u32, window: u32, impulse: f32) {
        let impulse = if impulse.is_finite() {
            impulse.clamp(0., 4.)
        } else {
            1.
        };
        let runtime = self.ecs.resource_mut::<crate::script::Runtime>();
        let mut runtime = runtime;
        for (name, value) in [
            ("skate_damage_tolerance", tolerance.min(1000).to_string()),
            ("skate_damage_window_ms", window.clamp(50, 5000).to_string()),
            ("skate_damage_impulse", impulse.to_string()),
        ] {
            runtime.dvars.insert(name.into(), value);
            runtime.server_info.insert(name.into());
        }
    }

    pub fn set_external_motion(&mut self, id: ClientId, enabled: bool) {
        if enabled {
            self.frame().external_motion.insert(id);
        } else {
            self.frame().external_motion.remove(&id);
        }
    }

    pub fn set_presentation(
        &mut self,
        id: ClientId,
        appearance: crate::CharacterAppearance,
        skate: Option<crate::SkatePose>,
    ) {
        if !appearance.valid() {
            return;
        }
        let meta = self.client_meta(id);
        let life = meta.map_or(0, |m| m.life_sequence.0);
        if skate
            .as_ref()
            .is_some_and(|pose| !pose.valid() || pose.life != life)
        {
            return;
        }
        let alive = self.player(id).is_some_and(|p| p.pm_type == 0)
            && meta.is_some_and(|m| m.lifecycle == crate::ClientLifecycle::Alive);
        let collision = skate
            .as_ref()
            .filter(|pose| {
                pose.collision_sequence != 0
                    && meta.and_then(|m| m.skate.as_ref()).is_none_or(|previous| {
                        pose.tick >= previous.tick
                            && pose.collision_sequence != previous.collision_sequence
                    })
            })
            .map(|pose| {
                (
                    pose.collision_speed,
                    pose.collision_normal,
                    pose.collision_native,
                )
            });
        if let Some(pose) = &skate
            && meta
                .and_then(|m| m.skate.as_ref())
                .is_some_and(|previous| pose.tick < previous.tick)
        {
            return;
        }
        if !alive {
            let impact = meta.map(|m| m.skate_damage.impact);
            if let Some(pose) = &skate
                && impact.is_some_and(|i| i.lethal && i.sequence == pose.impact)
            {
                let mut frame = self.frame();
                let now = frame
                    .ecs()
                    .resource::<crate::script::Runtime>()
                    .last_tick
                    .unwrap_or(Tick(0))
                    .0;
                let impact = impact.unwrap();
                if now
                    .wrapping_sub(impact.tick)
                    .saturating_mul(crate::MATCH_TICK_MS)
                    <= 5000
                {
                    let pool = frame.corpses_mut();
                    if let Some((i, slot)) = pool
                        .slots
                        .iter()
                        .enumerate()
                        .find(|(_, s)| s.occupied && s.victim == id && s.life == life)
                        && pose.root[12..15]
                            .iter()
                            .zip(slot.tr_base)
                            .all(|(a, b)| (*a - b).abs() < 4096.)
                    {
                        if pool.skates[i]
                            .as_ref()
                            .is_none_or(|previous| pose.tick > previous.tick)
                        {
                            pool.skates[i] = Some(pose.clone());
                            let slot = &mut pool.slots[i];
                            slot.origin = [pose.root[12], pose.root[13], pose.root[14]];
                            slot.tr_base = slot.origin;
                            slot.tr_delta = [0.; 3];
                            slot.tr_type = entity_iw4::TR_INTERPOLATE;
                            slot.falling = false;
                        }
                    }
                }
            }
            self.set_external_motion(id, false);
            if self.player(id).is_some() {
                let mut frame = self.frame();
                let meta = frame.client_meta_mut(id);
                meta.appearance = appearance;
                meta.skate = None;
            }
            return;
        }
        if let Some(pose) = &skate {
            if !pose.valid() {
                return;
            }
            self.set_origin(id, [pose.root[12], pose.root[13], pose.root[14]]);
        }
        self.set_external_motion(id, skate.is_some());
        let mut frame = self.frame();
        let meta = frame.client_meta_mut(id);
        meta.appearance = appearance;
        meta.skate = skate;
        if let Some((speed, normal, native)) = collision {
            let floor = if normal[2] > 0.5 { 6. } else { 4.5 };
            let amount = ((speed - floor).max(0.).powi(2) * 4.).clamp(0., 1000.) as i32;
            let amount = if native { amount.max(35) } else { amount };
            let sequence = frame
                .client_meta(id)
                .map_or(0, |m| m.skate_damage.impact.sequence);
            crate::script_player::record_skate_damage(
                &mut frame,
                id,
                amount,
                Some(normal),
                false,
                if normal[2] > 0.5 {
                    "left_leg_upper"
                } else {
                    "torso_upper"
                },
            );
            let bailed = frame
                .client_meta(id)
                .is_some_and(|m| m.skate_damage.impact.sequence != sequence);
            if bailed {
                frame.client_meta_mut(id).skate_damage.impact.impulse = [0.; 3];
            }
            diag::info!(
                Sim,
                "Skate collision client={} speed={speed:.2} damage={amount} bail={bailed}",
                id.0
            );
        }
    }

    pub fn set_origin(&mut self, id: ClientId, origin: [f32; 3]) -> bool {
        self.frame().set_origin(id, origin)
    }

    pub fn set_legs_anim(&mut self, id: ClientId, legs_anim: i32) -> bool {
        self.frame().set_legs_anim(id, legs_anim)
    }

    pub fn teleport(&mut self, id: ClientId, origin: [f32; 3]) -> bool {
        self.frame().teleport(id, origin)
    }

    pub fn set_viewangles(&mut self, id: ClientId, viewangles: [f32; 3]) -> bool {
        self.frame().set_viewangles(id, viewangles)
    }

    pub fn set_e_flags(&mut self, id: ClientId, e_flags: u32) -> bool {
        self.frame().set_e_flags(id, e_flags)
    }

    pub fn set_leanf(&mut self, id: ClientId, leanf: f32) -> bool {
        self.frame().set_leanf(id, leanf)
    }

    pub fn set_view_height_target(&mut self, id: ClientId, view_height_target: i32) -> bool {
        self.frame().set_view_height_target(id, view_height_target)
    }

    pub fn gate_nudge_origin(&mut self, id: ClientId, delta: [f32; 3]) {
        self.frame().gate_nudge_origin(id, delta);
    }

    pub fn snapshot(&self, tick: Tick) -> Snapshot {
        let state = self
            .ecs
            .get::<SimState>(self.state_entity)
            .expect("simulation state entity is missing its SimState component");
        state.snapshot_with_dynamic_rows(
            tick,
            collect_players(&self.ecs),
            collect_projectiles(&self.ecs),
            collect_script_movers(&self.ecs),
            collect_dropped_items(&self.ecs),
        )
    }

    pub fn slow_motion(&self) -> Option<crate::ScriptSlowMotion> {
        self.ecs
            .resource::<crate::script::Runtime>()
            .engine
            .slow_motion
    }

    pub fn adopt_snapshot(&mut self, snapshot: &Snapshot) -> crate::AdoptReport {
        assert!(
            self.gsc_program_fingerprint().is_none(),
            "snapshot lacks GSC state; restore a full SimWorld checkpoint"
        );
        self.frame().adopt_snapshot(snapshot)
    }

    pub fn adopt_prediction_snapshot(
        &mut self,
        snapshot: &Snapshot,
        local: ClientId,
    ) -> crate::AdoptReport {
        assert!(
            self.gsc_program_fingerprint().is_none(),
            "prediction snapshots cannot restore authority GSC state"
        );
        self.frame().adopt_prediction_snapshot(snapshot, local)
    }

    pub fn shutdown_game(&mut self) {
        self.frame().shutdown_game();
        crate::script::reset(&mut self.ecs);
    }

    pub fn hitvol_dump(&self) -> Vec<crate::world::HitvolDumpRow> {
        SimState::hitvol_dump(self, |id| player_ref(&self.ecs, id).copied())
    }

    pub fn lagcomp_query_for(
        &self,
        attacker: ClientId,
        current: Tick,
    ) -> crate::bullet_collision::LagcompQuery {
        SimState::lagcomp_query_for(self, attacker, current, |id| {
            player_ref(&self.ecs, id).copied()
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepReason {
    #[default]
    AuthorityFrame,

    PredictNew,

    Replay,
}

impl StepReason {
    pub const fn advances_authority_world(self) -> bool {
        matches!(self, Self::AuthorityFrame)
    }

    pub const fn is_replay(self) -> bool {
        matches!(self, Self::Replay)
    }
}

pub fn step(
    world: &mut SimWorld,
    tick: Tick,
    input: &TickInput,
    msec: i32,
    reason: StepReason,
) -> Snapshot {
    try_step(world, tick, input, msec, reason)
        .unwrap_or_else(|fault| panic!("GSC execution failed: {fault}"))
}

pub fn try_step(
    world: &mut SimWorld,
    tick: Tick,
    input: &TickInput,
    msec: i32,
    reason: StepReason,
) -> Result<Snapshot, crate::script::Fault> {
    world.run(tick, input, msec, reason)
}
