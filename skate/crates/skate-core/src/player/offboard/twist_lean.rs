#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observation {
    /// Actual PhysOutSkeleton+496 and+500, produced by active-state Fill.
    pub hips_right_angle: f32,
    pub hips_up_angle: f32,
    /// Live ISkaterAnim virtual28, not a physical stance snapshot.
    pub mirrored: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub twist: f32,
    pub lean: f32,
}

impl State {
    pub fn begin(&mut self, observation: Option<Observation>) {
        if let Some(observation) = observation {
            *self = read(observation);
        }
    }

    /// The default publishes captured values; exactly update="always" rereads.
    /// The live branch initializes its local values to zero before the read.
    pub fn update(&self, always: bool, observation: Option<Observation>) -> Self {
        if always {
            observation.map(read).unwrap_or_default()
        } else {
            *self
        }
    }
}

fn read(observation: Observation) -> State {
    State {
        twist: if observation.mirrored {
            -observation.hips_up_angle
        } else {
            observation.hips_up_angle
        },
        lean: observation.hips_right_angle,
    }
}
