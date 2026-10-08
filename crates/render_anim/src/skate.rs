//! Local Skate gameplay adapter. Rendering and match ownership remain in IW4L.
pub mod collision;
pub mod rails;
pub mod rig;
use bevy::prelude::*;
use frame::{AppScreen, SkateMode};
use skate_host::bridge::{CollisionBuilder, InputFrame, Pose, PreparedCollision, Session};
use std::sync::{Arc, Mutex, mpsc};

enum Job {
    Activate(u64, Vec3, f32, f32),
    Step(u64, f32, InputFrame, f32),
    Impact(u64, [f32; 3], bool),
    Suspend,
}
enum Reply {
    Ready,
    Activated(u64, Pose, u128),
    Pose(u64, Pose),
    Error { message: String, initialized: bool },
}
#[derive(Resource, Default)]
struct Host {
    send: Option<mpsc::Sender<Job>>,
    receive: Option<Mutex<mpsc::Receiver<Reply>>>,
    clip: Option<Arc<asset_world::ClipCollision>>,
    ready: bool,
    enter_requested: bool,
    activating: bool,
    epoch: u64,
    pad_packet: u32,
    previous_buttons: u16,
    input_suspended: bool,
    logged_tick: u64,
    keyboard_jump: bool,
    keyboard_flick_left: f32,
    dead_until: Option<f64>,
}

pub fn register(app: &mut App) {
    app.init_resource::<SkateMode>()
        .init_resource::<Host>()
        .add_systems(Startup, preload_assets)
        .add_systems(
            Update,
            update
                .after(frame::PresentedPublished)
                .before(crate::sync_camera_from_presented)
                .before(render_scene::GfxSceneAdd)
                .in_set(frame::ClientSet::Present),
        );
}

fn preload_assets(mut mode: ResMut<SkateMode>) {
    let Some(root) = std::env::var_os("IW4L_SKATE_ASSETS") else {
        return;
    };
    mode.preload_pending = true;
    if let Err(e) = std::thread::Builder::new()
        .name("skate-preload".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let start = std::time::Instant::now();
            match Session::preload(std::path::Path::new(&root)) {
                Ok(()) => diag::info!(
                    World,
                    "Skate animation banks preloaded in {}ms",
                    start.elapsed().as_millis()
                ),
                Err(e) => diag::warn!(World, "Skate preload: {e}"),
            }
        })
    {
        diag::warn!(World, "Skate preload thread: {e}");
    }
}

/// Blocks across either way of the skater that a Minecraft world's collision
/// covers, blocks up and down, and how far the skater goes before it is
/// rebuilt around them.
const BLOCK_RADIUS: i32 = 40;
const BLOCK_DEPTH: i32 = 20;
const BLOCK_RECENTRE: f32 = 14.0;

/// A Minecraft world's collision around map point `centre`, for Skate. Its
/// block edges become grind rails the same way any map's lips do: the same
/// detector that walks MW2 collision walks the block faces it is handed.
fn block_collision(
    builder: &CollisionBuilder,
    centre: Vec3,
) -> Result<(PreparedCollision, usize), String> {
    let map_triangles: Vec<[Vec3; 3]> =
        sim::voxel::collision_triangles(centre.to_array(), BLOCK_RADIUS, BLOCK_DEPTH)
            .into_iter()
            .map(|t| t.map(Vec3::from_array))
            .collect();
    if map_triangles.is_empty() {
        return Err("no blocks around the skater yet".into());
    }
    let (found, census) = rails::find(&map_triangles);
    diag::debug!(
        World,
        "Skate block rails: {} walkable edges, {} lips, {} runs, {} rails",
        census.candidates,
        census.lips,
        census.runs,
        census.rails,
    );
    let triangles: Vec<[[f32; 3]; 3]> = map_triangles
        .iter()
        .map(|t| t.map(|p| collision::to_skate(p).to_array()))
        .collect();
    let rails: Vec<Vec<[f32; 3]>> = found
        .into_iter()
        .map(|rail| {
            rail.into_iter()
                .map(|p| collision::to_skate(p).to_array())
                .collect()
        })
        .collect();
    let n = triangles.len();
    Ok((builder.build(triangles, rails)?, n))
}

/// One retained session per map. Leaving skating only pauses this worker;
/// collision, decoded animation banks, graphs and the rig remain resident.
fn preload_map(host: &mut Host, clip: Arc<asset_world::ClipCollision>) -> Result<(), String> {
    let root =
        std::env::var_os("IW4L_SKATE_ASSETS").ok_or("IW4L_SKATE_ASSETS is not configured")?;
    for file in [
        "private/game.json",
        "private/skater.glb",
        "private/stock/physics-skeletons.json",
        "private/stock/skater-collections.json",
    ] {
        if !std::path::Path::new(&root).join(file).is_file() {
            return Err(format!("Skate data is incomplete: missing {file}"));
        }
    }
    rig::reference().ok_or("Skate rig.json could not be loaded")?;
    assets::bot_model::local_skate_board().ok_or("Skate board.json could not be loaded")?;
    let (send, receive) = mpsc::channel();
    let (publish, results) = mpsc::channel();
    let geometry = clip.clone();
    std::thread::Builder::new()
        .name("iw4l-skate".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut initialized = false;
            let result = (|| -> Result<(), String> {
                let start = std::time::Instant::now();
                let world = collision::extract(&geometry);
                let mut session = Session::new(
                    std::path::Path::new(&root),
                    world.triangles,
                    world.rails,
                    [0., 0., 0.],
                    0.,
                )?;
                initialized = true;
                diag::info!(
                    World,
                    "Skate map session preloaded in {}ms",
                    start.elapsed().as_millis()
                );
                if publish.send(Reply::Ready).is_err() {
                    return Ok(());
                }
                // On a Minecraft world the collision streams: built around
                // the skater off this thread and swapped in as they move or
                // the blocks change.
                let builder = session.collision_builder();
                let (build_send, build_jobs) = mpsc::channel::<Vec3>();
                let (built_send, built) =
                    mpsc::channel::<(u64, Vec3, Result<(PreparedCollision, usize), String>)>();
                std::thread::Builder::new()
                    .name("iw4l-skate-blocks".into())
                    .spawn(move || {
                        while let Ok(mut centre) = build_jobs.recv() {
                            while let Ok(newer) = build_jobs.try_recv() {
                                centre = newer;
                            }
                            let revision = sim::voxel::revision();
                            let prepared = block_collision(&builder, centre);
                            if built_send.send((revision, centre, prepared)).is_err() {
                                break;
                            }
                        }
                    })
                    .map_err(|e| e.to_string())?;
                let mut blocks: Option<(u64, Vec3)> = None;
                let mut building = false;
                let mut requested = std::time::Instant::now();
                let mut skater_at: Option<Vec3> = None;
                let mut accumulated = 0.;
                let mut epoch = 0;
                while let Ok(job) = receive.recv() {
                    match job {
                        Job::Activate(new_epoch, spawn, yaw, aspect_ratio) => {
                            epoch = new_epoch;
                            accumulated = 0.;
                            let start = std::time::Instant::now();
                            session.set_aspect_ratio(aspect_ratio);
                            if sim::voxel::active() {
                                let revision = sim::voxel::revision();
                                match block_collision(&session.collision_builder(), spawn) {
                                    Ok((prepared, n)) => {
                                        session.install_collision(prepared)?;
                                        blocks = Some((revision, spawn));
                                        diag::info!(
                                            World,
                                            "Skate: {n} block collision triangles around the spawn"
                                        );
                                    }
                                    Err(e) => diag::warn!(World, "Skate block collision: {e}"),
                                }
                                skater_at = Some(spawn);
                            }
                            let p = session.activate(
                                collision::to_skate(spawn).to_array(),
                                yaw.to_radians() + std::f32::consts::FRAC_PI_2,
                            )?;
                            if publish
                                .send(Reply::Activated(epoch, p, start.elapsed().as_millis()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Job::Suspend => {
                            accumulated = 0.;
                            session.suspend_input();
                        }
                        Job::Impact(request, impulse, lethal) => {
                            if request == epoch {
                                session.combat_impact(impulse, lethal)?;
                            }
                        }
                        Job::Step(request, dt, input, aspect_ratio) => {
                            if request != epoch {
                                continue;
                            }
                            if let Ok((revision, centre, prepared)) = built.try_recv() {
                                building = false;
                                match prepared {
                                    Ok((prepared, _)) => {
                                        session.install_collision(prepared)?;
                                        blocks = Some((revision, centre));
                                    }
                                    Err(e) => diag::warn!(World, "Skate block collision: {e}"),
                                }
                            }
                            if sim::voxel::active()
                                && !building
                                && let Some(at) = skater_at
                            {
                                let far = blocks.is_none_or(|(_, centre)| {
                                    let d = (at - centre) / sim::voxel::BLOCK;
                                    d.truncate().length() > BLOCK_RECENTRE
                                        || d.z.abs() > BLOCK_DEPTH as f32 * 0.5
                                });
                                // Only changes that reach the blocks it covers: chunks
                                // stream in and out far away the whole time.
                                let changed = requested.elapsed().as_secs_f32() > 0.25
                                    && blocks.is_some_and(|(revision, centre)| {
                                        sim::voxel::changed_near(
                                            revision,
                                            centre.to_array(),
                                            BLOCK_RADIUS,
                                            BLOCK_DEPTH,
                                        )
                                    });
                                if (far || changed) && build_send.send(at).is_ok() {
                                    building = true;
                                    requested = std::time::Instant::now();
                                }
                            }
                            session.set_aspect_ratio(aspect_ratio);
                            session.collect(input, dt);
                            accumulated = (accumulated + dt).min(0.15);
                            let mut advanced = false;
                            // The native camera can change the simulation period.
                            while accumulated >= session.period() {
                                accumulated -= session.period();
                                session.advance()?;
                                advanced = true;
                            }
                            if advanced {
                                let p = session.pose();
                                if !p.root.is_finite() || p.bones.iter().any(|b| !b.is_finite()) {
                                    return Err("Skate published a non-finite pose".into());
                                }
                                skater_at = Some(collision::from_skate(p.root.w_axis.truncate()));
                                if publish.send(Reply::Pose(epoch, p)).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = publish.send(Reply::Error {
                    message: e,
                    initialized,
                });
            }
        })
        .map_err(|e| e.to_string())?;
    host.send = Some(send);
    host.receive = Some(Mutex::new(results));
    host.clip = Some(clip);
    host.ready = false;
    Ok(())
}

fn stop(host: &mut Host, mode: &mut SkateMode, authority: Option<&mut net::AuthorityWorld>) {
    if let Some(authority) = authority {
        authority
            .0
            .set_external_motion(sim::ClientId(mode.client), false);
    }
    host.enter_requested = false;
    host.dead_until = None;
    host.activating = false;
    host.epoch = host.epoch.wrapping_add(1);
    if let Some(send) = &host.send {
        let _ = send.send(Job::Suspend);
    }
    mode.active = false;
    mode.entering = false;
    mode.camera = None;
    mode.bones.clear();
    mode.status.clear();
    diag::info!(World, "Skate mode stopped; map session retained");
}

fn present(mode: &mut SkateMode, p: Pose, authority: Option<&mut net::AuthorityWorld>) {
    mode.collision_sequence = p.collision_sequence;
    mode.collision_speed = p.collision_speed;
    mode.collision_native = p.collision_native;
    mode.collision_normal = [
        p.collision_normal[0],
        -p.collision_normal[2],
        p.collision_normal[1],
    ];
    mode.sound_flags = p.sound_flags;
    mode.speed = p.velocity.length();
    let b = collision::basis();
    let mut root = b * p.root * b.inverse();
    root.w_axis = collision::from_skate(p.root.w_axis.truncate()).extend(1.);
    mode.root = root;
    mode.bones = p.bones;
    mode.names = p.names;
    mode.tick = p.tick;
    mode.status = p.state;
    mode.camera = p.camera.map(|(position, basis, fov)| {
        (
            Transform::from_translation(collision::from_skate(position)).looking_to(
                b.transform_vector3(basis.z_axis).normalize(),
                b.transform_vector3(basis.y_axis).normalize(),
            ),
            fov,
        )
    });
    if let Some(authority) = authority {
        authority.0.set_origin(
            sim::ClientId(mode.client),
            root.w_axis.truncate().to_array(),
        );
    }
}

fn update(
    time: Res<Time>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    screen: Res<AppScreen>,
    local: Res<net::LocalPresentClient>,
    actions: Res<net::ClientActionInput>,
    presented: Res<net::PresentedSnapshot>,
    clip: Res<crate::DynEntPhysClip>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut mode: ResMut<SkateMode>,
    mut host: ResMut<Host>,
    (gamepads, active): (
        Query<&bevy::input::gamepad::Gamepad>,
        Option<Res<frame::ActivePad>>,
    ),
) {
    let aspect_ratio = windows
        .single()
        .map(|w| w.width() / w.height().max(1.))
        .unwrap_or(16. / 9.);
    let ps = presented.player(local.0);
    let alive = ps.is_some_and(|p| p.pm_type == 0) && *screen == AppScreen::InGame;
    let meta = presented
        .snapshot()
        .and_then(|s| s.meta.for_client(local.0));
    let same_life = meta.is_some_and(|m| m.life_sequence.0 == mode.life);
    mode.xray = meta
        .filter(|m| m.life_sequence.0 == mode.life)
        .filter(|m| {
            presented.snapshot().is_some_and(|s| {
                s.tick
                    .0
                    .wrapping_sub(m.skate_damage.impact.tick)
                    .saturating_mul(sim::MATCH_TICK_MS)
                    < 5000
            })
        })
        .map_or(0, |m| m.skate_damage.injuries);
    if mode.active
        && same_life
        && let Some(meta) = meta
    {
        let impact = meta.skate_damage.impact;
        if impact.sequence != 0 && impact.sequence != mode.impact {
            mode.impact = impact.sequence;
            let i = impact.impulse;
            if let Some(send) = &host.send {
                let _ = send.send(Job::Impact(host.epoch, [i[0], i[2], -i[1]], impact.lethal));
            }
            if impact.lethal {
                host.dead_until = Some(time.elapsed_secs_f64() + 4.);
            }
            diag::info!(
                World,
                "Skate combat impact={} damage={} lethal={}",
                impact.sequence,
                impact.damage,
                impact.lethal
            );
        }
    }
    let corpse_active = mode.active
        && same_life
        && *screen == AppScreen::InGame
        && host
            .dead_until
            .is_some_and(|until| time.elapsed_secs_f64() < until);
    let same_map = host
        .clip
        .as_ref()
        .is_none_or(|a| clip.0.as_ref().is_some_and(|b| Arc::ptr_eq(a, b)));
    if (mode.active || host.enter_requested || host.activating)
        && ((!alive && !corpse_active) || (mode.active && !same_life) || !same_map)
    {
        stop(&mut host, &mut mode, authority.as_deref_mut());
    }
    if !same_map {
        host.send = None;
        host.receive = None;
        host.clip = None;
        host.ready = false;
        mode.preloaded = false;
        mode.preload_pending = std::env::var_os("IW4L_SKATE_ASSETS").is_some();
    }
    // This runs during map preparation/class selection, without waiting for J.
    if host.clip.is_none()
        && std::env::var_os("IW4L_SKATE_ASSETS").is_some()
        && let Some(geometry) = clip.0.clone()
    {
        mode.preload_pending = true;
        host.clip = Some(geometry.clone()); // A failed load retries on a new map, never every frame.
        if let Err(e) = preload_map(&mut host, geometry) {
            stop(&mut host, &mut mode, authority.as_deref_mut());
            diag::warn!(World, "Skate map preload: {e}");
            mode.preloaded = false;
            mode.preload_pending = false;
            mode.status = e;
        }
    }
    // Skating reads the same controller as the rest of the game, whatever
    // kind it is, converted to the Xbox layout the skate input expects.
    let pad = active
        .and_then(|active| active.0)
        .and_then(|entity| gamepads.get(entity).ok());
    host.pad_packet = host.pad_packet.wrapping_add(1);
    let input = if let Some(pad) = pad {
        host.keyboard_jump = false;
        host.keyboard_flick_left = 0.;
        pad_frame(pad, host.pad_packet)
    } else {
        let kb = &actions.client.kb;
        let jump = kb.gostand.active;
        if jump {
            host.keyboard_flick_left = 0.;
        } else if host.keyboard_jump {
            // Keep the flick across several native input ticks, including when
            // rendering runs faster than the fixed skating simulation.
            host.keyboard_flick_left = 0.06;
        }
        let flick = if jump {
            -32767
        } else if host.keyboard_flick_left > 0. {
            32767
        } else {
            0
        };
        host.keyboard_flick_left = (host.keyboard_flick_left - time.delta_secs()).max(0.);
        host.keyboard_jump = jump;
        let axis =
            |positive: bool, negative: bool| (i16::from(positive) - i16::from(negative)) * 32767;
        let buttons =
            if kb.forward.active { 0x1000 } else { 0 } | if kb.back.active { 0x2000 } else { 0 };
        InputFrame::from_pad(
            buttons,
            [0, 0],
            [axis(kb.moveright.active, kb.moveleft.active), 0],
            [axis(kb.right.active, kb.left.active), flick],
            host.pad_packet,
        )
    };
    mode.controller = pad.and_then(|_| input.controller());
    host.previous_buttons = input.buttons();

    let mut replies = Vec::new();
    if let Some(receiver) = &host.receive {
        let receiver = receiver.lock().unwrap();
        loop {
            match receiver.try_recv() {
                Ok(reply) => replies.push(reply),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    replies.push(Reply::Error {
                        message: "Skate worker disconnected".into(),
                        initialized: host.ready,
                    });
                    break;
                }
            }
        }
    }
    for reply in replies {
        match reply {
            Reply::Ready => {
                host.ready = true;
                mode.preloaded = true;
                mode.preload_pending = false;
                diag::info!(World, "Skate ready before toggle");
            }
            Reply::Activated(epoch, p, ms) if epoch == host.epoch && host.activating && alive => {
                host.activating = false;
                mode.entering = false;
                mode.active = true;
                host.input_suspended = false;
                if let Some(authority) = authority.as_deref_mut() {
                    authority.0.set_external_motion(local.0, true);
                }
                present(&mut mode, p, authority.as_deref_mut());
                diag::info!(World, "Skate activation from retained session: {ms}ms");
            }
            Reply::Pose(epoch, p) if epoch == host.epoch && mode.active => {
                if p.tick / 120 != host.logged_tick / 120 {
                    diag::info!(
                        World,
                        "Skate tick={} speed={:.2} state={}",
                        p.tick,
                        p.velocity.length(),
                        p.state
                    );
                    host.logged_tick = p.tick;
                }
                present(
                    &mut mode,
                    p,
                    if alive {
                        authority.as_deref_mut()
                    } else {
                        None
                    },
                );
                if !alive {
                    mode.camera = None;
                }
            }
            Reply::Error {
                message: e,
                initialized,
            } => {
                stop(&mut host, &mut mode, authority.as_deref_mut());
                host.send = None;
                host.receive = None;
                host.ready = false;
                mode.preloaded = false;
                mode.preload_pending = false;
                if initialized {
                    host.clip = None;
                    diag::warn!(World, "Skate stopped: {e}; preparing a new session");
                } else {
                    diag::warn!(
                        World,
                        "Skate preparation failed: {e}; repair data and retry with J"
                    );
                }
                mode.status = e;
                return;
            }
            _ => {}
        }
    }
    if std::mem::take(&mut mode.toggle_requested) && alive {
        if mode.active || host.enter_requested || host.activating {
            stop(&mut host, &mut mode, authority.as_deref_mut());
            return;
        }
        if host.send.is_none() {
            if host.clip.is_some() && std::env::var_os("IW4L_SKATE_ASSETS").is_some() {
                host.clip = None;
                mode.preload_pending = true;
                diag::info!(World, "Skate preparation retry requested");
            } else {
                diag::warn!(World, "Skate session unavailable: {}", mode.status);
                return;
            }
        }
        host.enter_requested = true;
        mode.entering = true;
        mode.client = local.0.0;
    }
    if host.enter_requested
        && host.ready
        && let Some(ps) = ps.filter(|_| alive)
    {
        host.epoch = host.epoch.wrapping_add(1);
        host.enter_requested = false;
        host.activating = true;
        mode.life = meta.map_or(0, |m| m.life_sequence.0);
        mode.impact = meta.map_or(0, |m| m.skate_damage.impact.sequence);
        host.dead_until = None;
        if let Some(send) = &host.send {
            let _ = send.send(Job::Activate(
                host.epoch,
                Vec3::from_array(ps.origin) + Vec3::Z * 2.,
                ps.viewangles[1],
                aspect_ratio,
            ));
        }
    }
    if !mode.active {
        return;
    }
    if let Some(clip) = &clip.0 {
        let at = mode.root.w_axis.truncate();
        let hit = clip.sweep_box(
            (at + Vec3::Z * 12.).to_array(),
            (at - Vec3::Z * 48.).to_array(),
            [-2., -2., 0.],
            [2., 2., 2.],
            0x10001,
        );
        if hit.fraction < 1. {
            mode.surface = ((hit.surface_flags >> 20) & 31).min(30) as u8;
        }
    }
    if mode.input_blocked && alive {
        if !host.input_suspended {
            if let Some(send) = &host.send {
                let _ = send.send(Job::Suspend);
            }
        }
        host.input_suspended = true;
        return;
    }
    host.input_suspended = false;
    if let Some(send) = &host.send {
        if send
            .send(Job::Step(
                host.epoch,
                time.delta_secs().min(0.1),
                if alive {
                    input
                } else {
                    InputFrame::from_pad(0, [0; 2], [0; 2], [0; 2], host.pad_packet)
                },
                aspect_ratio,
            ))
            .is_err()
        {
            stop(&mut host, &mut mode, authority.as_deref_mut());
        }
    }
}

/// A controller's state in XInput's layout, for the skate input.
fn pad_frame(pad: &bevy::input::gamepad::Gamepad, packet: u32) -> InputFrame {
    use bevy::input::gamepad::GamepadButton as B;
    const BITS: [(B, u16); 14] = [
        (B::DPadUp, 0x0001),
        (B::DPadDown, 0x0002),
        (B::DPadLeft, 0x0004),
        (B::DPadRight, 0x0008),
        (B::Start, 0x0010),
        (B::Select, 0x0020),
        (B::LeftThumb, 0x0040),
        (B::RightThumb, 0x0080),
        (B::LeftTrigger, 0x0100),
        (B::RightTrigger, 0x0200),
        (B::South, 0x1000),
        (B::East, 0x2000),
        (B::West, 0x4000),
        (B::North, 0x8000),
    ];
    let buttons = BITS
        .iter()
        .filter(|(button, _)| pad.pressed(*button))
        .fold(0, |bits, (_, bit)| bits | bit);
    // An analog trigger reports its travel; a digital one only pressed.
    let trigger = |button: B| {
        let value = pad
            .get(button)
            .unwrap_or(if pad.pressed(button) { 1.0 } else { 0.0 });
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let (left, right) = (pad.left_stick(), pad.right_stick());
    InputFrame::from_pad(
        buttons,
        [trigger(B::LeftTrigger2), trigger(B::RightTrigger2)],
        [axis(left.x), axis(left.y)],
        [axis(right.x), axis(right.y)],
        packet,
    )
}
