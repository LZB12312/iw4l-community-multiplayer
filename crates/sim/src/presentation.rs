#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharacterAppearance {
    pub skater: bool,
    pub skin: u8,
    pub shirt: u8,
    pub pants: u8,
    pub hair: u8,
    pub board: u8,
    pub profile: Option<std::sync::Arc<crate::character::CharacterProfile>>,
}

impl CharacterAppearance {
    pub fn valid(&self) -> bool {
        self.selections().iter().all(|v| *v <= 15)
            && self.profile.as_ref().is_none_or(|p| p.valid())
    }

    pub fn selections(&self) -> [u8; 5] {
        [self.skin, self.shirt, self.pants, self.hair, self.board]
    }
}

pub const MAX_SKATE_BONES: usize = 160;
pub const MAX_SKATE_BONE_NAME: usize = 48;

#[derive(Clone, Debug, PartialEq)]
pub struct SkatePose {
    pub collision_sequence: u32,
    pub collision_speed: f32,
    pub collision_normal: [f32; 3],
    pub collision_native: bool,
    pub sound_flags: u8,
    pub surface: u8,
    pub speed: f32,
    pub tick: u64,
    pub life: u32,
    pub impact: u32,
    pub root: [f32; 16],
    pub names: Vec<String>,
    pub bones: Vec<[f32; 16]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkateImpact {
    pub sequence: u32,
    pub tick: u32,
    pub damage: u32,
    pub impulse: [f32; 3],
    pub lethal: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkateDamage {
    pub impact: SkateImpact,
    pub accumulated: u32,
    pub last_tick: u32,
    pub score: u32,
    pub bails: u32,
    pub injuries: u32,
    pub accumulated_injuries: u32,
}

impl SkateDamage {
    pub fn display_active(self, tick: u32) -> bool {
        self.impact.sequence != 0
            && tick
                .wrapping_sub(self.impact.tick)
                .saturating_mul(crate::MATCH_TICK_MS)
                < 5000
    }
}

pub const MEAT_BONES: [&str; 19] = [
    "Bones_Ankle_Left",
    "Bones_Ankle_Right",
    "Bones_Bicep_Left",
    "Bones_Bicep_Right",
    "Bones_Calf_Left",
    "Bones_Calf_Right",
    "Bones_Forearm_Left",
    "Bones_Forearm_Right",
    "Bones_Hand_Left",
    "Bones_Hand_Right",
    "Bones_Hips",
    "Bones_Lower_Spine",
    "Bones_Neck",
    "Bones_Rib_Cage",
    "Bones_Skull",
    "Bones_Thigh_Left",
    "Bones_Thigh_Right",
    "Bones_Toes_Left",
    "Bones_Toes_Right",
];
pub fn meat_bone_bit(name: &str) -> u32 {
    MEAT_BONES
        .iter()
        .position(|n| *n == name)
        .map_or(0, |i| 1 << i)
}
pub fn meat_hit_region(hit: &str) -> u32 {
    let name = match hit {
        "head" | "helmet" => "Bones_Skull",
        "neck" => "Bones_Neck",
        "torso_upper" => "Bones_Rib_Cage",
        "torso_lower" => "Bones_Lower_Spine",
        "left_arm_upper" => "Bones_Bicep_Left",
        "right_arm_upper" => "Bones_Bicep_Right",
        "left_arm_lower" => "Bones_Forearm_Left",
        "right_arm_lower" => "Bones_Forearm_Right",
        "left_hand" => "Bones_Hand_Left",
        "right_hand" => "Bones_Hand_Right",
        "left_leg_upper" => "Bones_Thigh_Left",
        "right_leg_upper" => "Bones_Thigh_Right",
        "left_leg_lower" => "Bones_Calf_Left",
        "right_leg_lower" => "Bones_Calf_Right",
        "left_foot" => "Bones_Ankle_Left",
        "right_foot" => "Bones_Ankle_Right",
        _ => "Bones_Hips",
    };
    meat_bone_bit(name)
}

impl SkatePose {
    pub fn valid(&self) -> bool {
        let affine = |m: &[f32; 16]| {
            m.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.)
                && m[3].abs() < 0.001
                && m[7].abs() < 0.001
                && m[11].abs() < 0.001
                && (m[15] - 1.).abs() < 0.001
        };
        !self.bones.is_empty()
            && self.collision_speed.is_finite()
            && (0.0..=64.0).contains(&self.collision_speed)
            && self
                .collision_normal
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.001)
            && self.speed.is_finite()
            && (0.0..=256.0).contains(&self.speed)
            && self.sound_flags & !63 == 0
            && self.surface <= 30
            && self.bones.len() <= MAX_SKATE_BONES
            && self.names.len() == self.bones.len()
            && affine(&self.root)
            && self.bones.iter().all(affine)
            && self
                .names
                .iter()
                .all(|n| !n.is_empty() && n.len() <= MAX_SKATE_BONE_NAME && n.is_ascii())
            && self
                .names
                .iter()
                .enumerate()
                .all(|(i, n)| !self.names[..i].contains(n))
    }
}
