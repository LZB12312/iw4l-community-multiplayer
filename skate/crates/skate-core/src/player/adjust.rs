#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdjustFields {
    /// ProcessedPhysIn+2512.
    pub processed_word_2512: u32,
    /// ProcessedPhysIn+2604.
    pub timestep_2604: f32,
    /// SkateboardController+448.
    pub controller_state_448: u32,
    /// Player+1876 output staging +42.
    pub output_flag_42: bool,
    /// Player+1876 output staging +144.
    pub output_scalar_144: f32,
    /// Player+1876 output staging +200.
    pub output_counter_200: u32,
}

pub trait AdjustServices {
    fn board_condition_82c02138(&mut self) -> u32;

    /// BoardBody vtable `+4`, byte `+868`. Called only in controller mode 1.
    fn controller_mode_one_board_flag_868(&mut self) -> bool;

    fn adjust_skeleton_82bd80d8(&mut self, board_flag: bool, timestep: f32);

    /// Current physical-state vtable slot `+40`.
    fn update_current_state_post_physics_vtable_40(&mut self);

    fn player_output_selector_82db9100(&mut self) -> u32;

    fn finish_skeleton_adjustment_82bd83e0(&mut self, selector: u32);
}

/// Runs the complete TU3 Adjust wrapper after solver completion.
pub fn run_adjust(fields: &mut AdjustFields, services: &mut impl AdjustServices) {
    if services.board_condition_82c02138() == 1 && fields.processed_word_2512 != 500 {
        fields.output_flag_42 = true;
        fields.output_scalar_144 = 0.0;
        fields.output_counter_200 = fields.output_counter_200.wrapping_add(1);
    }

    let board_flag = if fields.controller_state_448 == 1 {
        services.controller_mode_one_board_flag_868()
    } else {
        false
    };
    services.adjust_skeleton_82bd80d8(board_flag, fields.timestep_2604);
    services.update_current_state_post_physics_vtable_40();
    let selector = services.player_output_selector_82db9100();
    services.finish_skeleton_adjustment_82bd83e0(selector);
}
