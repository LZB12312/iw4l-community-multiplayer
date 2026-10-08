//! Transport-neutral, bounded snapshot protocol. No platform identity or SDK types.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
pub mod blob;
mod codec;
pub mod directory;
pub mod interpolation;
pub mod lobby;
pub mod packed;
pub mod socket;

pub const MAGIC: &[u8; 8] = b"SK8NET01";
pub const MAX_FRAME: usize = 48_000;
const PAYLOAD: usize = 1_000;
const HEADER: usize = 36;
pub const BODY_COUNT: usize = 33;
pub const DEFAULT_APPEARANCE: &str = "default-skater-v1";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pose {
    pub p: [f32; 3],
    pub q: [f32; 4],
}
impl Pose {
    pub fn valid(&self) -> bool {
        self.p.iter().all(|v| v.is_finite() && v.abs() < 100_000.)
            && self.q.iter().all(|v| v.is_finite())
            && (0.98..1.02).contains(&self.q.iter().map(|v| v * v).sum::<f32>())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub pose: Pose,
    pub velocity: [f32; 3],
    pub angular: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bone {
    pub index: u16,
    pub pose: Pose,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Shape {
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    Capsule {
        center: [f32; 3],
        axis: [f32; 3],
        half: f32,
        radius: f32,
    },
    Box {
        pose: Pose,
        half: [f32; 3],
        radius: f32,
    },
    Triangle {
        vertices: [[f32; 3]; 3],
        radius: f32,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Volume {
    pub body: u8,
    pub shape: Shape,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    pub map: u64,
    pub rig: u64,
    pub tick: u64,
    pub appearance: String,
    pub root: Pose,
    /// Physical animation anchors and board parts, never the complete skin.
    pub bones: Vec<Bone>,
    pub bodies: Vec<Body>,
    pub volumes: Vec<Volume>,
}
fn vector(v: &[f32; 3], bound: f32) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= bound)
}
impl Frame {
    pub fn validate(&self) -> bool {
        if self.appearance.len() > 128
            || !self.root.valid()
            || self.bones.len() > 32
            || self.bodies.len() != BODY_COUNT
            || self.volumes.len() > 64
        {
            return false;
        }
        if !self.bones.iter().all(|b| b.index < 256 && b.pose.valid()) {
            return false;
        }
        if self
            .bones
            .iter()
            .enumerate()
            .any(|(i, b)| self.bones[..i].iter().any(|a| a.index == b.index))
        {
            return false;
        }
        if !self
            .bodies
            .iter()
            .all(|b| b.pose.valid() && vector(&b.velocity, 250.) && vector(&b.angular, 500.))
        {
            return false;
        }
        self.volumes.iter().all(|v| {
            let Some(body) = self.bodies.get(v.body as usize) else {
                return false;
            };
            let near = |p: &[f32; 3]| {
                vector(p, 100_000.)
                    && p.iter()
                        .zip(body.pose.p)
                        .map(|(a, b)| (a - b) * (a - b))
                        .sum::<f32>()
                        < 16.
            };
            let radius = |r: f32| r.is_finite() && (0.0..=1.0).contains(&r);
            match &v.shape {
                Shape::Sphere { center, radius: r } => near(center) && radius(*r),
                Shape::Capsule {
                    center,
                    axis,
                    half,
                    radius: r,
                } => {
                    near(center)
                        && vector(axis, 1.01)
                        && (0.98..1.02).contains(&axis.iter().map(|v| v * v).sum::<f32>())
                        && radius(*half)
                        && radius(*r)
                }
                Shape::Box {
                    pose,
                    half,
                    radius: r,
                } => pose.valid() && near(&pose.p) && half.iter().all(|v| radius(*v)) && radius(*r),
                Shape::Triangle {
                    vertices,
                    radius: r,
                } => vertices.iter().all(near) && radius(*r),
            }
        })
    }
}
pub fn appearance_or_default(requested: &str) -> &'static str {
    // Only the canonical rig/outfit is implemented. Never turn a peer identifier into a path.
    let _ = requested;
    DEFAULT_APPEARANCE
}
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
pub fn packets(
    session: u64,
    node: u64,
    sequence: u64,
    frame: &Frame,
) -> Result<Vec<Vec<u8>>, String> {
    if !frame.validate() {
        return Err("Invalid snapshot".into());
    }
    let data = codec::encode(frame);
    if !frame.validate() || data.len() > MAX_FRAME {
        return Err("Snapshot exceeds protocol bounds".into());
    }
    let count = data.len().div_ceil(PAYLOAD) as u16;
    Ok(data
        .chunks(PAYLOAD)
        .enumerate()
        .map(|(index, chunk)| {
            let mut out = Vec::with_capacity(HEADER + chunk.len());
            out.extend(MAGIC);
            out.extend(session.to_le_bytes());
            out.extend(node.to_le_bytes());
            out.extend(sequence.to_le_bytes());
            out.extend((index as u16).to_le_bytes());
            out.extend(count.to_le_bytes());
            out.extend(chunk);
            out
        })
        .collect())
}
pub fn envelope(packet: &[u8]) -> Option<(u64, u64, u64)> {
    if packet.len() <= HEADER || packet.len() > HEADER + PAYLOAD || &packet[..8] != MAGIC {
        return None;
    }
    Some((
        u64::from_le_bytes(packet[8..16].try_into().ok()?),
        u64::from_le_bytes(packet[16..24].try_into().ok()?),
        u64::from_le_bytes(packet[24..32].try_into().ok()?),
    ))
}
struct Partial {
    sequence: u64,
    count: usize,
    chunks: Vec<Option<Vec<u8>>>,
    start: Instant,
}
#[derive(Default)]
pub struct Receiver {
    pending: Vec<Partial>,
    completed: u64,
}
impl Receiver {
    pub fn receive(&mut self, bytes: &[u8]) -> Option<Frame> {
        let (_, _, sequence) = envelope(bytes)?;
        if sequence <= self.completed {
            return None;
        }
        let index = u16::from_le_bytes(bytes[32..34].try_into().ok()?) as usize;
        let count = u16::from_le_bytes(bytes[34..36].try_into().ok()?) as usize;
        if count == 0 || count > MAX_FRAME / PAYLOAD || index >= count {
            return None;
        }
        self.pending.retain(|p| {
            p.start.elapsed() < Duration::from_millis(500) && p.sequence > self.completed
        });
        if !self.pending.iter().any(|p| p.sequence == sequence) {
            if self.pending.len() >= 4 {
                self.pending.remove(0);
            }
            self.pending.push(Partial {
                sequence,
                count,
                chunks: vec![None; count],
                start: Instant::now(),
            });
        }
        let p = self.pending.iter_mut().find(|p| p.sequence == sequence)?;
        if p.count != count {
            return None;
        }
        p.chunks[index] = Some(bytes[HEADER..].to_vec());
        if p.chunks.iter().any(Option::is_none) {
            return None;
        }
        let data: Vec<_> = p
            .chunks
            .iter()
            .flat_map(|p| p.as_ref().unwrap().iter().copied())
            .collect();
        let frame = codec::decode(&data)?;
        if !frame.validate() {
            return None;
        }
        self.completed = sequence;
        self.pending.retain(|p| p.sequence > sequence);
        Some(frame)
    }
}
