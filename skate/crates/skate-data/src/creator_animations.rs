use crate::abin::{AnimationPart, Bank, Codec, PartEntry, Reader};
use skate_core::animation::{
    output::{NativeMatrix, Sqt, compose_hierarchy_in_place, sqt_to_matrix},
    pose_add, pose_blend,
};
use std::path::Path;

struct StandingClip {
    fps: f32,
    looping: bool,
    frames: Vec<Vec<Option<Sqt>>>,
}

pub struct CreatorAnimations {
    pub names: Vec<String>,
    parents: Vec<i32>,
    reference: Vec<Sqt>,
    standing: Vec<StandingClip>,
}

impl CreatorAnimations {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bank = Bank::load(path).map_err(|e| e.to_string())?;
        let hierarchy = bank.hierarchy().ok_or("Missing creator hierarchy")?;
        let bones = usize::from(hierarchy.bone_count);
        if bones == 0
            || bones > 512
            || hierarchy.parents.len() != bones
            || hierarchy.bone_names.len() != bones
            || hierarchy
                .parents
                .iter()
                .enumerate()
                .any(|(bone, &parent)| parent < -1 || parent >= bone as i32)
        {
            return Err("Invalid creator hierarchy".into());
        }
        let reference_record = bank
            .records()
            .iter()
            .find(|r| matches!(r.data, crate::abin::RecordData::Pose(_)))
            .ok_or("Missing creator reference pose")?;
        if reference_record.header.codec() != Codec::Raw {
            return Err("Unsupported creator reference codec".into());
        }
        let crate::abin::RecordData::Pose(reference_pose) = &reference_record.data else {
            return Err("Invalid creator reference pose".into());
        };
        let reference = decode_parts(&bank, &reference_pose.parts, 1, false)?
            .remove(0)
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or("Incomplete creator reference pose")?;
        let mut standing = Vec::new();
        for name in [
            "CAS_Stand",
            "CAS_Stand_Slouch",
            "CAS_Stand_Buff",
            "CAS_Stand_Stiff",
        ] {
            let (header, clip) = bank
                .clip(name)
                .ok_or("Missing creator standing animation")?;
            if header.codec() != Codec::Raw || clip.channel_animation() {
                return Err("Unsupported creator standing codec".into());
            }
            let count = f32::from_bits(clip.frame_count_bits);
            let fps = f32::from_bits(clip.fps_bits);
            if !count.is_finite()
                || !(2. ..=4096.).contains(&count)
                || count.fract() != 0.
                || count * bones as f32 > 1_000_000.
                || !fps.is_finite()
                || !(1. ..=240.).contains(&fps)
            {
                return Err("Invalid creator animation timing".into());
            }
            standing.push(StandingClip {
                fps,
                looping: clip.looping(),
                frames: decode_parts(&bank, &clip.parts, count as usize, true)?,
            });
        }
        Ok(Self {
            names: hierarchy.bone_names.clone(),
            parents: hierarchy.parents.clone(),
            reference,
            standing,
        })
    }

    pub fn pose(&self, posture: u32, seconds: f32) -> Result<Vec<NativeMatrix>, String> {
        if !seconds.is_finite() || seconds < 0. {
            return Err("Invalid creator animation time".into());
        }
        let clip = self
            .standing
            .get(posture as usize)
            .ok_or("Invalid creator posture")?;
        let end = (clip.frames.len() - 1) as f32;
        let sample = if clip.looping {
            (seconds * clip.fps) % end
        } else {
            (seconds * clip.fps).min(end)
        };
        let first = sample.floor() as usize;
        let second = (first + 1).min(clip.frames.len() - 1);
        let weight = sample - first as f32;
        let mut matrices = Vec::with_capacity(self.names.len());
        for (bone, reference) in self.reference.iter().enumerate() {
            let local = match (clip.frames[first][bone], clip.frames[second][bone]) {
                (Some(a), Some(b)) => {
                    pose_add::add(pose_blend::blend_sample(a, b, weight), *reference, true)
                }
                (None, None) => *reference,
                _ => return Err("Creator channel coverage changed between samples".into()),
            };
            matrices.push(sqt_to_matrix(local));
        }
        compose_hierarchy_in_place(self.names.len() as i32, &self.parents, 0, &mut matrices)
            .map_err(|e| format!("Creator pose: {e:?}"))?;
        if matrices.iter().flatten().flatten().any(|v| !v.is_finite()) {
            return Err("Nonfinite creator pose".into());
        }
        Ok(matrices)
    }
}

fn decode_parts(
    bank: &Bank,
    parts: &[PartEntry],
    frames: usize,
    active: bool,
) -> Result<Vec<Vec<Option<Sqt>>>, String> {
    let hierarchy = bank.hierarchy().ok_or("Missing creator hierarchy")?;
    let layouts: Vec<_> = hierarchy
        .parts
        .iter()
        .filter(|p| !active || p.flags & 1 != 0)
        .collect();
    if parts.len() != layouts.len() {
        return Err("Creator part table differs from hierarchy".into());
    }
    let bones = usize::from(hierarchy.bone_count);
    let mut output = vec![vec![None; bones]; frames];
    for (entry, layout) in parts.iter().zip(layouts) {
        let part = entry
            .part
            .as_ref()
            .ok_or("Missing creator animation part")?;
        if layout.sqt_offset < 0 || layout.bone_count != u32::from(part.channel_count) {
            return Err("Invalid creator part layout".into());
        }
        let start = layout.sqt_offset as usize;
        let end = start
            .checked_add(layout.bone_count as usize)
            .ok_or("Creator part overflow")?;
        if end > bones || output[0][start..end].iter().any(Option::is_some) {
            return Err("Overlapping creator parts".into());
        }
        let decoded = decode_raw(bank, part, frames)?;
        for (frame, samples) in output.iter_mut().zip(decoded) {
            for (destination, sample) in frame[start..end].iter_mut().zip(samples) {
                *destination = Some(sample);
            }
        }
    }
    Ok(output)
}

fn decode_raw(bank: &Bank, part: &AnimationPart, frames: usize) -> Result<Vec<Vec<Sqt>>, String> {
    let header = Reader::new(bank.bytes())
        .bounded(part.compression_header.clone())
        .map_err(|e| e.to_string())?;
    let at = part.compression_header.start;
    let word = |offset| header.u32(at + offset).map_err(|e| e.to_string());
    let masks = [
        word(0)? as u64 | ((word(4)? as u64) << 32),
        word(8)? as u64 | ((word(12)? as u64) << 32),
        word(16)? as u64 | ((word(20)? as u64) << 32),
    ];
    let channels = usize::from(part.channel_count);
    if channels == 0
        || channels > 64
        || usize::from(header.u8(at + 26).map_err(|e| e.to_string())?) + 1 != channels
        || header.u8(at + 27).map_err(|e| e.to_string())? != 0
        || header.u8(at + 28).map_err(|e| e.to_string())? != 0
        || masks
            .iter()
            .any(|mask| channels < 64 && mask >> channels != 0)
    {
        return Err("Unsupported creator RAW layout".into());
    }
    let has = |channel, lane| masks[lane] & (1u64 << channel) != 0;
    let stride = usize::from(header.u16(at + 24).map_err(|e| e.to_string())?);
    let expected: usize = (0..channels)
        .map(|c| {
            12 * usize::from(has(c, 0)) + 16 * usize::from(has(c, 1)) + 12 * usize::from(has(c, 2))
        })
        .sum();
    if expected != stride || frames.checked_mul(stride) != Some(part.compressed_data.len()) {
        return Err("Creator RAW stream size differs".into());
    }
    let data = Reader::new(bank.bytes())
        .bounded(part.compressed_data.clone())
        .map_err(|e| e.to_string())?;
    let mut output = Vec::with_capacity(frames);
    for frame in 0..frames {
        let mut cursor = part.compressed_data.start + frame * stride;
        let mut samples = Vec::with_capacity(channels);
        for channel in 0..channels {
            let mut sample = Sqt {
                scale: [1., 1., 1., 0.],
                rotation: [0., 0., 0., 1.],
                translation: [0.; 4],
            };
            for (lane, values) in [
                (0, &mut sample.scale[..3]),
                (1, &mut sample.rotation[..]),
                (2, &mut sample.translation[..3]),
            ] {
                if has(channel, lane) {
                    for value in values {
                        *value = f32::from_bits(data.u32(cursor).map_err(|e| e.to_string())?);
                        cursor += 4;
                    }
                }
            }
            if sample
                .scale
                .iter()
                .chain(&sample.rotation)
                .chain(&sample.translation)
                .any(|v| !v.is_finite())
                || sample.rotation.iter().map(|v| v * v).sum::<f32>() < 0.5
            {
                return Err("Invalid creator RAW sample".into());
            }
            samples.push(sample);
        }
        output.push(samples);
    }
    Ok(output)
}
