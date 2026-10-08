//! The five board-owned persistent fields used by CalcManualEffect.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ManualState {
    /// Board+264: 0.95/0.05 filtered clamped angle error.
    pub filtered_angle_error: f32,
    /// Board+268: normalized requested angle for this tick.
    pub target_angle: f32,
    /// Board+272: normalized measured deck angle, retained for the next derivative.
    pub measured_angle: f32,
    /// Board+276: accumulated controller output, scaled to angular displacement.
    pub angular_correction: f32,
    /// Board+280: zero also identifies the first active controller tick.
    pub elapsed: f32,
}

impl ManualState {
    pub fn reset(&mut self) {
        self.filtered_angle_error = 0.0;
        self.target_angle = 0.0;
        self.measured_angle = 0.0;
        self.elapsed = 0.0;
        self.angular_correction = 0.0;
    }

    pub fn enter_ground(
        &mut self,
        previous_category: u32,
        powerslide_exit_scale: f32,
    ) -> ManualEntryContinuation {
        if previous_category == 100 {
            // +268 target, +272 measurement and +280 timer are preserved.
            self.angular_correction *= powerslide_exit_scale;
            self.filtered_angle_error *= powerslide_exit_scale;
            ManualEntryContinuation::Continue
        } else {
            self.filtered_angle_error = 0.0;
            self.target_angle = 0.0;
            self.measured_angle = 0.0;
            self.angular_correction = 0.0;
            self.elapsed = 0.0;
            ManualEntryContinuation::RemoveVelocityIntoGround
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum ManualEntryContinuation {
    Continue,
    RemoveVelocityIntoGround,
}
