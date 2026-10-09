use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct LocalCharacter(pub sim::CharacterAppearance);

#[derive(Resource, Default)]
struct PresentationPacer {
    last: f64,
}

pub fn register(app: &mut App) {
    app.init_resource::<LocalCharacter>()
        .init_resource::<PresentationPacer>()
        .add_systems(Update, send.in_set(frame::ClientSet::Send));
}

fn send(
    time: Res<Time>,
    character: Res<LocalCharacter>,
    local: Res<crate::LocalPresentClient>,
    skate: Option<Res<frame::SkateMode>>,
    mut authority: Option<ResMut<crate::AuthorityWorld>>,
    mut link: Option<ResMut<crate::UdpClientLink>>,
    mut pacer: ResMut<PresentationPacer>,
) {
    let pose = skate
        .as_ref()
        .filter(|s| s.active && !s.bones.is_empty())
        .map(|s| sim::SkatePose {
            collision_sequence: s.collision_sequence,
            collision_speed: s.collision_speed,
            collision_normal: s.collision_normal,
            collision_native: s.collision_native,
            sound_flags: s.sound_flags,
            surface: s.surface,
            speed: s.speed,
            tick: s.tick,
            life: s.life,
            impact: s.impact,
            root: s.root.to_cols_array(),
            names: s.names.clone(),
            bones: s.bones.iter().map(Mat4::to_cols_array).collect(),
        })
        .filter(sim::SkatePose::valid);
    if let Some(world) = authority.as_deref_mut() {
        world
            .0
            .set_presentation(local.0, character.0.clone(), pose.clone());
    }
    let now = time.elapsed_secs_f64();
    if now - pacer.last < 0.05 {
        return;
    }
    if let Some(link) = link.as_deref_mut().filter(|l| l.has_entered_match()) {
        if let Err(e) = link.send_presentation(character.0.clone(), pose) {
            diag::warn!(Net, "character update: {e}");
            return;
        }
    }
    pacer.last = (pacer.last + 0.05).max(now - 0.05);
}
