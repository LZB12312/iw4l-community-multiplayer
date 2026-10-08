use bevy::{
    audio::{
        AudioSink, AudioSinkPlayback, Decodable, GlobalVolume, PlaybackMode, PlaybackSettings,
    },
    prelude::*,
};
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

fn device_name(device: &rodio::cpal::Device) -> Option<String> {
    let description = device.description().ok()?;
    Some(
        description
            .extended()
            .first()
            .cloned()
            .unwrap_or_else(|| description.name().to_owned()),
    )
}

pub fn output_device_names() -> Result<Vec<String>, String> {
    rodio::cpal::default_host()
        .output_devices()
        .map(|devices| devices.filter_map(|device| device_name(&device)).collect())
        .map_err(|error| error.to_string())
}

#[derive(Resource, Default)]
pub(crate) struct OutputOverride(Option<MixerDeviceSink>);

#[derive(Component)]
struct OutputFinish(PlaybackMode);

impl OutputOverride {
    fn open() -> Self {
        let Some(name) = std::env::var("IW4L_AUDIO_DEVICE")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            return Self::default();
        };
        let result = (|| {
            let host = rodio::cpal::default_host();
            let devices = host.output_devices().map_err(|error| error.to_string())?;
            let device = devices
                .filter_map(|device| device_name(&device).map(|label| (label, device)))
                .find(|(label, _)| label.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| format!("output device '{name}' is unavailable"))?;
            let mut sink = DeviceSinkBuilder::from_device(device.1)
                .and_then(DeviceSinkBuilder::open_stream)
                .map_err(|error| error.to_string())?;
            sink.log_on_drop(false);
            diag::info!(
                Audio,
                "audio: output override '{}' {:?}",
                device.0,
                sink.config()
            );
            Ok::<_, String>(sink)
        })();
        match result {
            Ok(sink) => Self(Some(sink)),
            Err(error) => {
                diag::warn!(Audio, "audio: {error}; using the Windows default output");
                Self::default()
            }
        }
    }
}

fn route_output<T: Asset + Decodable>(
    mut commands: Commands,
    output: Res<OutputOverride>,
    sources: Res<Assets<T>>,
    global: Res<GlobalVolume>,
    queued: Query<(Entity, &AudioPlayer<T>, &PlaybackSettings), Without<AudioSink>>,
) {
    let Some(output) = &output.0 else { return };
    for (entity, source, settings) in &queued {
        if settings.spatial {
            continue;
        }
        let Some(source) = sources.get(&source.0) else {
            continue;
        };
        let player = Player::connect_new(output.mixer());
        let mut decoder: Box<dyn Source + Send> = Box::new(source.decoder());
        if let Some(position) = settings.start_position {
            decoder = Box::new(decoder.skip_duration(position));
        }
        if let Some(duration) = settings.duration {
            decoder = Box::new(decoder.take_duration(duration));
        }
        if matches!(settings.mode, PlaybackMode::Loop) {
            decoder = Box::new(decoder.repeat_infinite());
        }
        player.append(decoder);
        let mut sink = AudioSink::new(player);
        sink.set_speed(settings.speed);
        sink.set_volume(settings.volume * global.volume);
        if settings.muted {
            sink.mute();
        }
        if settings.paused {
            sink.pause();
        }
        let mut entity = commands.entity(entity);
        entity.insert(sink);
        match settings.mode {
            PlaybackMode::Despawn => {
                entity.insert(OutputFinish(settings.mode));
            }
            PlaybackMode::Remove => {
                entity.insert(OutputFinish(settings.mode));
            }
            _ => {}
        }
    }
}

fn cleanup_output<T: Asset + Decodable>(
    mut commands: Commands,
    finished: Query<(Entity, &AudioSink, &OutputFinish), With<AudioPlayer<T>>>,
) {
    for (entity, sink, finish) in &finished {
        if !sink.empty() {
            continue;
        }
        if matches!(finish.0, PlaybackMode::Despawn) {
            commands.entity(entity).try_despawn();
        } else {
            commands
                .entity(entity)
                .remove::<(AudioPlayer<T>, AudioSink, PlaybackSettings, OutputFinish)>();
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.insert_resource(OutputOverride::open()).add_systems(
        PostUpdate,
        (
            (
                cleanup_output::<crate::pcm::PcmAudio>,
                route_output::<crate::pcm::PcmAudio>,
            )
                .chain(),
            (
                cleanup_output::<crate::pcm::LoopingPcmAudio>,
                route_output::<crate::pcm::LoopingPcmAudio>,
            )
                .chain(),
            (
                cleanup_output::<crate::match_bus::MatchBusAudio>,
                route_output::<crate::match_bus::MatchBusAudio>,
            )
                .chain(),
        )
            .after(crate::match_bus::route_match_voices)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}
