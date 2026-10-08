//! Complete the grind FillOut write to native PhysOut pointer+12's 48-byte
//! block. CURRENT already represents it as PhysicalPlayerInput::physics.
use crate::physics::SkaterRuntime;

/// Call immediately after grind FillOut sets pending_wipeout_impulse, before
/// the completed physical packet is consumed by the next input phase.
pub(crate) fn publish(skater: &mut SkaterRuntime) {
    if let Some(impulse) = skater.grind.pending_wipeout_impulse.take() {
        let output = &mut skater.player_input.physical.physics;
        output.vector_16 = impulse.map(f32::to_bits);
        output.flag_32 = 1;
    }
}
