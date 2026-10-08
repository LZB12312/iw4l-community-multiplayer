use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

pub const CHARACTER_SLOTS: usize = 18;
pub const MAX_CHARACTER_MORPHS: usize = 64;
pub const MAX_CHARACTER_STAMPS: usize = 128;
pub const MAX_CHARACTER_MATERIALS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum CharacterSlot {
    Arm,
    Leg,
    Hair,
    Face,
    Feet,
    Accessory,
    Underwear,
    Deck,
    Trucks,
    Wheels,
    Pants,
    Socks,
    Hat,
    Glasses,
    InnerTop,
    OuterTop,
    Jewellery,
    Wrist,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CharacterAsset(pub u64);

impl TryFrom<String> for CharacterAsset {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 16 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Character asset must contain 16 hexadecimal digits");
        }
        u64::from_str_radix(&value, 16)
            .map(Self)
            .map_err(|_| "Invalid character asset")
    }
}

impl From<CharacterAsset> for String {
    fn from(value: CharacterAsset) -> Self {
        format!("{:016x}", value.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CharacterScalar(u32);

impl CharacterScalar {
    pub const ONE: Self = Self(1.0f32.to_bits());

    pub fn new(value: f32) -> Option<Self> {
        value.is_finite().then_some(Self(value.to_bits()))
    }

    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
    }

    pub fn unit(self) -> bool {
        (0.0..=1.0).contains(&self.value())
    }
}

impl Serialize for CharacterScalar {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f32(self.value())
    }
}

impl<'de> Deserialize<'de> for CharacterScalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(f32::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("Nonfinite character parameter"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharacterPart {
    pub model: CharacterAsset,
    pub materials: Vec<CharacterAsset>,
    pub tint: [CharacterScalar; 3],
    pub palette: Option<String>,
}

impl Default for CharacterPart {
    fn default() -> Self {
        Self {
            model: CharacterAsset::default(),
            materials: Vec::new(),
            tint: [CharacterScalar::ONE; 3],
            palette: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharacterStamp {
    pub asset: CharacterAsset,
    pub zone: u8,
    pub transform: [CharacterScalar; 6],
    pub tint: [CharacterScalar; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharacterProfile {
    pub male: bool,
    pub parts: [CharacterPart; CHARACTER_SLOTS],
    pub morphs: BTreeMap<String, CharacterScalar>,
    pub skin_tint: [CharacterScalar; 3],
    pub hair_tint: [CharacterScalar; 3],
    pub skin_palette: Option<String>,
    pub hair_palette: Option<String>,
    pub stance: bool,
    pub style: CharacterAsset,
    pub posture: CharacterAsset,
    pub gestures: [CharacterAsset; 4],
    pub trucks: CharacterScalar,
    pub wheels: CharacterScalar,
    pub stamps: Vec<CharacterStamp>,
}

impl CharacterProfile {
    pub fn valid(&self) -> bool {
        self.parts.iter().any(|p| p.model.0 != 0)
            && self.parts.iter().all(|p| {
                p.materials.len() <= MAX_CHARACTER_MATERIALS
                    && if p.model.0 == 0 {
                        p.materials.is_empty()
                    } else {
                        !p.materials.is_empty() && p.materials.iter().all(|m| m.0 != 0)
                    }
                    && p.tint.iter().all(|v| v.unit())
                    && valid_palette(&p.palette)
            })
            && self.morphs.len() <= MAX_CHARACTER_MORPHS
            && self.morphs.iter().all(|(name, value)| {
                !name.is_empty()
                    && name.len() <= 64
                    && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    && value.unit()
            })
            && self
                .skin_tint
                .iter()
                .chain(&self.hair_tint)
                .all(|v| v.unit())
            && self.trucks.unit()
            && self.wheels.unit()
            && valid_palette(&self.skin_palette)
            && valid_palette(&self.hair_palette)
            && self.stamps.len() <= MAX_CHARACTER_STAMPS
            && self.stamps.iter().all(|s| {
                s.asset.0 != 0
                    && s.transform.iter().all(|v| v.value().abs() <= 65536.0)
                    && s.tint.iter().all(|v| v.unit())
            })
    }
}

fn valid_palette(palette: &Option<String>) -> bool {
    palette.as_ref().is_none_or(|name| {
        !name.is_empty()
            && name.len() <= 64
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    })
}
