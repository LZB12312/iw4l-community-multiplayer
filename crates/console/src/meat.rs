use crate::feature_dispatch::ConsoleEcho;
use crate::{CommandSpec, ConsoleCommand, ConsoleRegistry};
use bevy::prelude::*;
use frame::{UiMenuDvars, UiMenuRequest};

#[derive(Resource)]
struct Settings {
    tolerance: u32,
    window: u32,
    impulse: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            tolerance: 35,
            window: 750,
            impulse: 1.,
        }
    }
}
impl Settings {
    fn set(&mut self, name: &str, value: &str) -> bool {
        match name {
            "tolerance" => match value.parse::<u32>() {
                Ok(v) if v <= 1000 => self.tolerance = v,
                _ => return false,
            },
            "window" => match value.parse::<u32>() {
                Ok(v) if (50..=5000).contains(&v) => self.window = v,
                _ => return false,
            },
            "impulse" => match value.parse::<f32>() {
                Ok(v) if v.is_finite() && (0. ..=4.).contains(&v) => self.impulse = v,
                _ => return false,
            },
            _ => return false,
        }
        true
    }
    fn payload(&self) -> String {
        format!(
            "tolerance {}\nwindow {}\nimpulse {}\n",
            self.tolerance, self.window, self.impulse
        )
    }
}
fn load(mut settings: ResMut<Settings>) {
    if let Ok(text) = std::fs::read_to_string("iw4l-meat.txt") {
        for line in text.lines() {
            if let Some((name, value)) = line.split_once(' ') {
                settings.set(name, value);
            }
        }
    }
}
pub fn register(app: &mut App) {
    app.init_resource::<Settings>()
        .add_systems(Startup, load)
        .add_systems(Update, route.in_set(frame::ClientSet::Ui));
}

pub fn commands(registry: &mut ConsoleRegistry) {
    registry.register(CommandSpec::new("meat").usage("meat [tolerance 0..1000 | window 50..5000 | impulse 0..4] — host damage-bail settings; tolerance 0 disables"));
    registry.register(CommandSpec::new("ui_meat_open"));
}
fn route(
    mut events: MessageReader<ConsoleCommand>,
    mut settings: ResMut<Settings>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
    mut dvars: ResMut<UiMenuDvars>,
    mut menus: MessageWriter<UiMenuRequest>,
    mut lines: ConsoleEcho,
    screen: Res<frame::AppScreen>,
) {
    for c in events.read() {
        if c.name == "ui_meat_open" {
            menus.write(UiMenuRequest::Open("hall_of_meat_settings".into()));
        }
        if c.name != "meat" {
            continue;
        }
        if let [name, value] = c.args.as_slice() {
            if authority.is_none() && *screen == frame::AppScreen::InGame {
                lines.write("Only the match host can change damage tolerance during play.");
                continue;
            }
            if !settings.set(name, value) {
                lines.write("Use meat tolerance 0..1000, window 50..5000, or impulse 0..4.");
                continue;
            }
            if let Err(e) = std::fs::write("iw4l-meat.txt", settings.payload()) {
                lines.write(format!("Could not save Hall of Meat settings: {e}"));
            }
        }
        lines.write(format!(
            "Damage tolerance={} HP, accumulation={} ms, impulse={}x. 0 HP disables combat bails.",
            settings.tolerance, settings.window, settings.impulse
        ));
    }
    dvars.set("ui_meat_tolerance", settings.tolerance.to_string());
    if let Some(world) = authority.as_deref_mut() {
        world
            .0
            .set_skate_damage_settings(settings.tolerance, settings.window, settings.impulse);
    }
}
