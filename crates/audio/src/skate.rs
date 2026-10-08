use crate::{
    LivePan, PcmAudio,
    backend::{AudioScope, spawn_loop, spawn_oneshot},
};
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, Volume},
    prelude::*,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[derive(Default)]
struct Actor {
    life: u32,
    flags: u8,
    position: Vec3,
    collision: u32,
    push_at: f64,
    rattle_at: f64,
    seam_at: f64,
    grounded: bool,
    air_since: Option<f64>,
    land_at: f64,
    loop_voice: Option<(String, Entity, LivePan)>,
}

#[derive(Resource, Default)]
struct Sounds {
    root: PathBuf,
    categories: HashMap<String, Vec<String>>,
    clips: HashMap<String, Option<PcmAudio>>,
    cursor: usize,
    actors: HashMap<u32, Actor>,
}

impl Sounds {
    fn clip(&mut self, category: &str, surface: u8) -> Option<PcmAudio> {
        let category = if category == "land"
            && matches!(surface, 13 | 28 | 29)
            && self.categories.contains_key("land_metal")
        {
            "land_metal"
        } else {
            category
        };
        let paths = self.categories.get(category)?;
        if paths.is_empty() {
            return None;
        }
        let path = if category == "rolling" {
            let material = match surface {
                13 | 28 | 29 => "metal_smooth_hard",
                1 | 21 => "wood_ramp_soft",
                22 => "asphalt_smooth_soft",
                6 | 8 | 10 | 11 | 14 | 18 | 19 | 30 => "asphalt_rough_soft",
                2 | 17 => "concrete_aggregate_soft",
                _ => "concrete_smooth_soft",
            };
            paths
                .iter()
                .find(|p| p.contains(material))
                .unwrap_or(&paths[0])
                .clone()
        } else {
            let path = paths[self.cursor % paths.len()].clone();
            self.cursor = self.cursor.wrapping_add(1);
            path
        };
        let root = &self.root;
        if self.clips.len() >= 64
            && !self.clips.contains_key(&path)
            && let Some(oldest) = self.clips.keys().next().cloned()
        {
            self.clips.remove(&oldest);
        }
        self.clips
            .entry(path.clone())
            .or_insert_with(|| {
                let pcm = std::fs::read(root.join(&path))
                    .ok()
                    .and_then(|bytes| crate::decode_audio_bytes(&bytes))
                    .and_then(|clip| {
                        if category != "cloth" {
                            return Some(clip);
                        }
                        let samples = (clip.rate() as usize / 2) * clip.channel_count() as usize;
                        PcmAudio::from_prepared(
                            Arc::from(&clip.samples()[..samples.min(clip.samples().len())]),
                            clip.channel_count(),
                            clip.rate(),
                        )
                    });
                if pcm.is_none() {
                    diag::warn!(Audio, "Skate sound could not decode: {path}");
                }
                pcm
            })
            .clone()
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<Sounds>()
        .add_systems(Startup, load)
        .add_systems(
            Update,
            play.after(net::PresentedPublished)
                .in_set(frame::ClientSet::Present),
        );
}

fn load(mut sounds: ResMut<Sounds>) {
    let Some(root) = std::env::var_os("IW4L_SKATE_ASSETS") else {
        return;
    };
    sounds.root = PathBuf::from(root).join("private/audio");
    let bank = std::fs::read(sounds.root.join("sounds.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let Some(categories) = bank
        .as_ref()
        .and_then(|b| b.get("categories"))
        .and_then(|v| v.as_object())
    else {
        diag::warn!(
            Audio,
            "Skate board audio is missing; prepare it with the bundled converter's --audio-only option"
        );
        return;
    };
    for (category, paths) in categories {
        let Some(paths) = paths.as_array() else {
            continue;
        };
        sounds.categories.insert(
            category.clone(),
            paths
                .iter()
                .filter_map(|p| p.as_str())
                .filter(|p| {
                    !p.starts_with('/')
                        && !p.contains('\\')
                        && !p.contains(':')
                        && !p.split('/').any(|s| s == "..")
                })
                .map(str::to_owned)
                .collect(),
        );
    }
    diag::info!(
        Audio,
        "Skate board sound bank loaded: {} categories",
        sounds.categories.len()
    );
}

fn oneshot(
    sounds: &mut Sounds,
    commands: &mut Commands,
    pcm: &mut Assets<PcmAudio>,
    category: &str,
    gain: f32,
    pan: (f32, f32),
    epoch: u64,
    surface: u8,
) {
    let Some(clip) = sounds.clip(category, surface) else {
        return;
    };
    let clip = if category == "land"
        && matches!(
            surface,
            1 | 3 | 4 | 6 | 8 | 10 | 11 | 14 | 18 | 19 | 21 | 30
        ) {
        let channels = clip.channel_count() as usize;
        let mut previous = vec![0.; channels];
        let alpha = if matches!(surface, 1 | 21) { 0.5 } else { 0.2 };
        let samples: Vec<f32> = clip
            .samples()
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let value = previous[i % channels] + alpha * (sample - previous[i % channels]);
                previous[i % channels] = value;
                value
            })
            .collect();
        PcmAudio::from_prepared(Arc::from(samples), clip.channel_count(), clip.rate())
            .unwrap_or(clip)
    } else {
        clip
    };
    let clip = clip.with_live_pan();
    if let Some(live) = clip.live_pan() {
        live.set(pan.0, pan.1);
    }
    spawn_oneshot(
        commands,
        pcm.add(clip),
        Volume::Linear(
            gain * if matches!(surface, 3 | 4 | 6 | 8 | 10 | 11 | 14 | 18 | 19 | 30) {
                0.65
            } else {
                1.
            },
        ),
        if category == "land" && matches!(surface, 1 | 21) {
            0.9
        } else {
            1.
        },
        epoch,
        AudioScope::Match,
    );
    diag::debug!(
        Audio,
        "Skate sound: {category} surface={surface} gain={gain:.2}"
    );
}

fn play(
    time: Res<Time>,
    screen: Res<frame::AppScreen>,
    local: Res<net::LocalPresentClient>,
    mode: Option<Res<frame::SkateMode>>,
    snapshot: Res<net::PresentedSnapshot>,
    epoch: Res<crate::backend::MatchEpoch>,
    listeners: Query<&Transform, With<crate::AmbientListener>>,
    mut sounds: ResMut<Sounds>,
    mut commands: Commands,
    mut pcm: ResMut<Assets<PcmAudio>>,
    mut looping: ResMut<Assets<crate::LoopingPcmAudio>>,
    mut sinks: Query<&mut AudioSink>,
) {
    let mut actors = Vec::new();
    if *screen == frame::AppScreen::InGame && !crate::AudioSilent::active() {
        if let Some(mode) = mode.filter(|m| m.active && !m.bones.is_empty())
            && snapshot.player(local.0).is_some_and(|p| p.pm_type == 0)
        {
            actors.push((
                local.0.0,
                mode.life,
                mode.sound_flags,
                mode.speed,
                mode.root.w_axis.truncate(),
                mode.collision_sequence,
                mode.surface,
            ));
        }
        if let Some(snapshot) = snapshot.snapshot() {
            for (id, meta) in &snapshot.meta.clients {
                if *id == local.0
                    || !snapshot
                        .players
                        .iter()
                        .any(|(client, p)| client == id && p.pm_type == 0)
                {
                    continue;
                }
                if let Some(pose) = &meta.skate {
                    actors.push((
                        id.0,
                        pose.life,
                        pose.sound_flags,
                        pose.speed,
                        Vec3::new(pose.root[12], pose.root[13], pose.root[14]),
                        pose.collision_sequence,
                        pose.surface,
                    ));
                }
            }
        }
    }
    let stale: Vec<_> = sounds
        .actors
        .keys()
        .copied()
        .filter(|id| !actors.iter().any(|a| a.0 == *id))
        .collect();
    for id in stale {
        if let Some(actor) = sounds.actors.remove(&id)
            && let Some((_, entity, _)) = actor.loop_voice
        {
            commands.entity(entity).despawn();
        }
    }
    let listener = listeners
        .iter()
        .next()
        .map(|t| (t.translation, t.rotation * Vec3::X));
    let now = time.elapsed_secs_f64();
    for (id, life, flags, speed, position, collision, surface) in actors {
        let mut actor = sounds.actors.remove(&id).unwrap_or_default();
        if actor.life != life {
            if let Some((_, entity, _)) = actor.loop_voice.take() {
                commands.entity(entity).despawn();
            }
            actor = Actor {
                life,
                flags,
                position,
                collision,
                grounded: flags & 1 != 0,
                ..Default::default()
            };
        }
        let (falloff, pan) = if id == local.0.0 {
            (1., (0.707, 0.707))
        } else if let Some((ear, right)) = listener {
            (
                (1. - (position - ear).length() / 1400.).clamp(0., 1.),
                crate::world_oneshot_channel_gains(ear, right, position, 1.),
            )
        } else {
            (0., (0., 0.))
        };
        let was_grounded = actor.grounded;
        if flags & 1 != 0 {
            actor.grounded = true;
            actor.air_since = None;
        } else {
            let since = *actor.air_since.get_or_insert(now);
            if now - since > 0.06 {
                actor.grounded = false;
            }
        }
        let grounded = actor.grounded;
        let bailed = flags & 16 != 0;
        let category = if falloff < 0.01 || speed < 0.25 {
            ""
        } else if bailed {
            "scrape"
        } else if flags & 2 != 0 {
            "grind"
        } else if grounded && flags & 32 != 0 {
            "slide"
        } else if grounded && flags & 4 != 0 {
            "brake"
        } else if grounded {
            "rolling"
        } else {
            "wheels"
        };
        let loop_key = format!("{category}:{surface}");
        if actor
            .loop_voice
            .as_ref()
            .map(|v| v.0.as_str())
            .unwrap_or("")
            != loop_key
        {
            if let Some((_, entity, _)) = actor.loop_voice.take() {
                commands.entity(entity).despawn();
            }
            if !category.is_empty()
                && let Some(clip) = sounds.clip(category, surface)
            {
                let clip = clip.with_live_pan();
                let pan = clip.live_pan().unwrap().clone();
                let entity = spawn_loop(
                    &mut commands,
                    looping.add(clip.into_looping()),
                    Volume::Linear(0.),
                    epoch.0,
                    AudioScope::Match,
                );
                actor.loop_voice = Some((loop_key, entity, pan));
                diag::debug!(
                    Audio,
                    "Skate loop started: {category} surface={surface} client={id}"
                );
            }
        }
        if let Some((_, entity, live_pan)) = &actor.loop_voice {
            live_pan.set(pan.0, pan.1);
            if let Ok(mut sink) = sinks.get_mut(*entity) {
                sink.set_volume(Volume::Linear(
                    falloff
                        * (speed / 7.).clamp(0.05, 0.8)
                        * if category == "wheels" { 0.3 } else { 1. },
                ));
                sink.set_speed((0.65 + speed / 14.).clamp(0.65, 1.5));
            }
        }
        if falloff > 0.01 {
            if grounded && !was_grounded && !bailed && now >= actor.land_at {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "land",
                    0.8 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
                actor.land_at = now + 0.25;
            }
            if !grounded && was_grounded && !bailed && position.z > actor.position.z + 0.1 {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "pop",
                    0.6 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "cloth",
                    0.1 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
            }
            if bailed && actor.flags & 16 == 0 || collision != actor.collision && collision != 0 {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "impact",
                    0.8 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
            }
            if grounded && !bailed && flags & 8 != 0 && now >= actor.push_at {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "push",
                    0.45 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
                actor.push_at = now + 0.65;
            }
            if grounded && !bailed && speed > 3. && now >= actor.rattle_at {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "rattle",
                    0.18 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
                actor.rattle_at = now + 1.2;
            }
            if grounded
                && !bailed
                && speed > 2.
                && matches!(surface, 2 | 6 | 11 | 17 | 22)
                && now >= actor.seam_at
            {
                oneshot(
                    &mut sounds,
                    &mut commands,
                    &mut pcm,
                    "seams",
                    0.12 * falloff,
                    pan,
                    epoch.0,
                    surface,
                );
                actor.seam_at = now + (5. / speed as f64).clamp(0.4, 2.);
            }
        }
        actor.flags = flags;
        actor.position = position;
        actor.collision = collision;
        sounds.actors.insert(id, actor);
    }
}
