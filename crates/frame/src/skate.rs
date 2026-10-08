use bevy::prelude::*;

/// Local skating presentation. Physics remains owned by the optional Skate host.
#[derive(Resource, Default)]
pub struct SkateMode {
    pub collision_sequence: u32,
    pub collision_speed: f32,
    pub collision_normal: [f32; 3],
    pub collision_native: bool,
    pub sound_flags: u8,
    pub surface: u8,
    pub speed: f32,
    pub active: bool,
    pub entering: bool,
    pub preloaded: bool,
    pub preload_pending: bool,
    pub controller: Option<usize>,
    pub toggle_requested: bool,
    pub input_blocked: bool,
    pub client: u32,
    pub life: u32,
    pub impact: u32,
    pub xray: u32,
    pub root: Mat4,
    pub bones: Vec<Mat4>,
    pub names: Vec<String>,
    pub camera: Option<(Transform, f32)>,
    pub tick: u64,
    pub status: String,
}
