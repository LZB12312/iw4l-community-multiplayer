use super::wire::{WireError, WireReader, WireWriter};
use sim::{CharacterAppearance, SkatePose};

pub fn encode_damage(out: &mut WireWriter, d: sim::presentation::SkateDamage) {
    for v in [
        d.impact.sequence,
        d.impact.tick,
        d.impact.damage,
        d.accumulated,
        d.last_tick,
        d.score,
        d.bails,
        d.injuries,
        d.accumulated_injuries,
    ] {
        out.put_u32(v);
    }
    for v in d.impact.impulse {
        out.put_f32(v);
    }
    out.put_u8(d.impact.lethal.into());
}

pub fn decode_damage(
    input: &mut WireReader<'_>,
) -> Result<sim::presentation::SkateDamage, WireError> {
    let sequence = input.get_u32()?;
    let tick = input.get_u32()?;
    let damage = input.get_u32()?;
    let accumulated = input.get_u32()?;
    let last_tick = input.get_u32()?;
    let score = input.get_u32()?;
    let bails = input.get_u32()?;
    let injuries = input.get_u32()?;
    let accumulated_injuries = input.get_u32()?;
    if (injuries | accumulated_injuries) >> sim::presentation::MEAT_BONES.len() != 0 {
        return Err(WireError::Malformed("invalid injury regions"));
    }
    let impulse = [input.get_f32()?, input.get_f32()?, input.get_f32()?];
    let lethal = match input.get_u8()? {
        0 => false,
        1 => true,
        _ => return Err(WireError::Malformed("invalid impact flag")),
    };
    if impulse.iter().any(|v| !v.is_finite() || v.abs() > 64.) {
        return Err(WireError::Malformed("invalid impact impulse"));
    }
    Ok(sim::presentation::SkateDamage {
        impact: sim::presentation::SkateImpact {
            sequence,
            tick,
            damage,
            impulse,
            lethal,
        },
        accumulated,
        last_tick,
        score,
        bails,
        injuries,
        accumulated_injuries,
    })
}

pub fn encode_appearance(out: &mut WireWriter, a: &CharacterAppearance) {
    for v in [
        u8::from(a.skater),
        a.skin,
        a.shirt,
        a.pants,
        a.hair,
        a.board,
    ] {
        out.put_u8(v);
    }
    out.put_u8(u8::from(a.profile.is_some()));
    if let Some(profile) = &a.profile {
        encode_profile(out, profile);
    }
}

pub fn decode_appearance(input: &mut WireReader<'_>) -> Result<CharacterAppearance, WireError> {
    let kind = input.get_u8()?;
    let mut a = CharacterAppearance {
        skater: kind == 1,
        skin: input.get_u8()?,
        shirt: input.get_u8()?,
        pants: input.get_u8()?,
        hair: input.get_u8()?,
        board: input.get_u8()?,
        profile: None,
    };
    if kind > 1
        || [a.skin, a.shirt, a.pants, a.hair, a.board]
            .iter()
            .any(|v| *v > 15)
    {
        return Err(WireError::Malformed("invalid character selection"));
    }
    a.profile = match input.get_u8()? {
        0 => None,
        1 => Some(std::sync::Arc::new(decode_profile(input)?)),
        _ => return Err(WireError::Malformed("invalid character profile flag")),
    };
    Ok(a)
}

fn encode_profile(out: &mut WireWriter, p: &sim::character::CharacterProfile) {
    out.put_u8(p.male.into());
    for part in &p.parts {
        out.put_u64(part.model.0);
        out.put_u8(part.materials.len() as u8);
        for material in &part.materials {
            out.put_u64(material.0);
        }
        for value in part.tint {
            out.put_f32(value.value());
        }
        encode_palette(out, part.palette.as_deref());
    }
    out.put_u8(p.morphs.len() as u8);
    for (name, value) in &p.morphs {
        out.put_u8(name.len() as u8);
        out.put_bytes(name.as_bytes());
        out.put_f32(value.value());
    }
    for value in p.skin_tint.iter().chain(&p.hair_tint) {
        out.put_f32(value.value());
    }
    encode_palette(out, p.skin_palette.as_deref());
    encode_palette(out, p.hair_palette.as_deref());
    out.put_u8(p.stance.into());
    for asset in [p.style, p.posture].iter().chain(&p.gestures) {
        out.put_u64(asset.0);
    }
    out.put_f32(p.trucks.value());
    out.put_f32(p.wheels.value());
    out.put_u8(p.stamps.len() as u8);
    for stamp in &p.stamps {
        out.put_u64(stamp.asset.0);
        out.put_u8(stamp.zone);
        for value in stamp.transform.iter().chain(&stamp.tint) {
            out.put_f32(value.value());
        }
    }
}

fn decode_profile(
    input: &mut WireReader<'_>,
) -> Result<sim::character::CharacterProfile, WireError> {
    use sim::character::*;
    let boolean = |input: &mut WireReader<'_>| match input.get_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(WireError::Malformed("invalid character boolean")),
    };
    let scalar = |input: &mut WireReader<'_>| {
        CharacterScalar::new(input.get_f32()?)
            .ok_or(WireError::Malformed("nonfinite character parameter"))
    };
    let mut profile = CharacterProfile {
        male: boolean(input)?,
        ..Default::default()
    };
    for part in &mut profile.parts {
        part.model = CharacterAsset(input.get_u64()?);
        let count = input.get_u8()? as usize;
        if count > MAX_CHARACTER_MATERIALS {
            return Err(WireError::Malformed("too many character materials"));
        }
        for _ in 0..count {
            part.materials.push(CharacterAsset(input.get_u64()?));
        }
        for value in &mut part.tint {
            *value = scalar(input)?;
        }
        part.palette = decode_palette(input)?;
    }
    let count = input.get_u8()? as usize;
    if count > MAX_CHARACTER_MORPHS {
        return Err(WireError::Malformed("too many character morphs"));
    }
    for _ in 0..count {
        let length = input.get_u8()? as usize;
        if length == 0 || length > 64 {
            return Err(WireError::Malformed("invalid character morph name"));
        }
        let mut bytes = vec![0; length];
        input.get_bytes(&mut bytes)?;
        let name = String::from_utf8(bytes)
            .map_err(|_| WireError::Malformed("invalid character morph text"))?;
        if profile.morphs.insert(name, scalar(input)?).is_some() {
            return Err(WireError::Malformed("duplicate character morph"));
        }
    }
    for value in profile.skin_tint.iter_mut().chain(&mut profile.hair_tint) {
        *value = scalar(input)?;
    }
    profile.skin_palette = decode_palette(input)?;
    profile.hair_palette = decode_palette(input)?;
    profile.stance = boolean(input)?;
    profile.style = CharacterAsset(input.get_u64()?);
    profile.posture = CharacterAsset(input.get_u64()?);
    for gesture in &mut profile.gestures {
        *gesture = CharacterAsset(input.get_u64()?);
    }
    profile.trucks = scalar(input)?;
    profile.wheels = scalar(input)?;
    let count = input.get_u8()? as usize;
    if count > MAX_CHARACTER_STAMPS {
        return Err(WireError::Malformed("too many character tattoos"));
    }
    for _ in 0..count {
        let mut stamp = CharacterStamp {
            asset: CharacterAsset(input.get_u64()?),
            zone: input.get_u8()?,
            transform: [CharacterScalar::default(); 6],
            tint: [CharacterScalar::ONE; 3],
        };
        for value in stamp.transform.iter_mut().chain(&mut stamp.tint) {
            *value = scalar(input)?;
        }
        profile.stamps.push(stamp);
    }
    if !profile.valid() {
        return Err(WireError::Malformed("invalid character profile"));
    }
    Ok(profile)
}

fn encode_palette(out: &mut WireWriter, palette: Option<&str>) {
    out.put_u8(u8::from(palette.is_some()));
    if let Some(name) = palette {
        out.put_u8(name.len() as u8);
        out.put_bytes(name.as_bytes());
    }
}

fn decode_palette(input: &mut WireReader<'_>) -> Result<Option<String>, WireError> {
    match input.get_u8()? {
        0 => return Ok(None),
        1 => {}
        _ => return Err(WireError::Malformed("invalid character palette flag")),
    }
    let length = input.get_u8()? as usize;
    if length == 0 || length > 64 {
        return Err(WireError::Malformed("invalid character palette length"));
    }
    let mut bytes = vec![0; length];
    input.get_bytes(&mut bytes)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| WireError::Malformed("invalid character palette text"))
}

pub fn encode_skate(out: &mut WireWriter, skate: Option<&SkatePose>) {
    let Some(p) = skate else {
        out.put_u8(0);
        return;
    };
    out.put_u8(1);
    out.put_u64(p.tick);
    out.put_u32(p.life);
    out.put_u32(p.impact);
    out.put_u32(p.collision_sequence);
    out.put_f32(p.collision_speed);
    for value in p.collision_normal {
        out.put_f32(value);
    }
    out.put_u8(p.sound_flags);
    out.put_u8(u8::from(p.collision_native));
    out.put_u8(p.surface);
    out.put_f32(p.speed);
    for value in p.root {
        out.put_f32(value);
    }
    out.put_u16(p.bones.len() as u16);
    for (name, bone) in p.names.iter().zip(&p.bones) {
        out.put_u8(name.len() as u8);
        out.put_bytes(name.as_bytes());
        for column in [0, 4, 8, 12] {
            for value in &bone[column..column + 3] {
                out.put_f32(*value);
            }
        }
    }
}

pub fn decode_skate(input: &mut WireReader<'_>) -> Result<Option<SkatePose>, WireError> {
    match input.get_u8()? {
        0 => return Ok(None),
        1 => {}
        _ => return Err(WireError::Malformed("invalid skate pose tag")),
    }
    let tick = input.get_u64()?;
    let life = input.get_u32()?;
    let impact = input.get_u32()?;
    let collision_sequence = input.get_u32()?;
    let collision_speed = input.get_f32()?;
    let collision_normal = [input.get_f32()?, input.get_f32()?, input.get_f32()?];
    let sound_flags = input.get_u8()?;
    let collision_native = match input.get_u8()? {
        0 => false,
        1 => true,
        _ => return Err(WireError::Malformed("invalid skate collision flag")),
    };
    let surface = input.get_u8()?;
    let speed = input.get_f32()?;
    let mut root = [0.; 16];
    for v in &mut root {
        *v = input.get_f32()?;
    }
    let count = input.get_u16()? as usize;
    if count == 0 || count > sim::presentation::MAX_SKATE_BONES {
        return Err(WireError::Malformed("invalid skate bone count"));
    }
    let mut names = Vec::with_capacity(count);
    let mut bones = Vec::with_capacity(count);
    for _ in 0..count {
        let n = input.get_u8()? as usize;
        if n == 0 || n > sim::presentation::MAX_SKATE_BONE_NAME {
            return Err(WireError::Malformed("invalid skate bone name"));
        }
        let mut name = vec![0; n];
        input.get_bytes(&mut name)?;
        names.push(
            String::from_utf8(name).map_err(|_| WireError::Malformed("invalid skate bone name"))?,
        );
        let mut bone = [0.; 16];
        bone[15] = 1.;
        for column in [0, 4, 8, 12] {
            for v in &mut bone[column..column + 3] {
                *v = input.get_f32()?;
            }
        }
        bones.push(bone);
    }
    let pose = SkatePose {
        collision_sequence,
        collision_speed,
        collision_normal,
        sound_flags,
        collision_native,
        surface,
        speed,
        tick,
        life,
        impact,
        root,
        names,
        bones,
    };
    if !pose.valid() {
        return Err(WireError::Malformed("invalid skate pose"));
    }
    Ok(Some(pose))
}
