#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatePhaseFields {
    /// PhysicalPlayer+1344.
    pub elapsed_1344: f32,
    /// ProcessedPhysIn+2604.
    pub timestep_2604: f32,
    /// SkateboardController+448.
    pub controller_state_448: u32,
    /// SkateboardController+452.
    pub controller_system_on_452: bool,
}

pub trait StatePhaseServices {
    /// Current-state vtable slot `+8`.
    fn update_current_state_vtable_8(&mut self);

    fn update_controller_82d75f00(&mut self);
    fn update_controller_mode_1_82d751d8(&mut self);
    fn update_controller_mode_2_82d750f0(&mut self);
    fn update_controller_mode_3_82d75bb8(&mut self);
    fn update_controller_mode_4_82d75d58(&mut self);

    fn touch_skateboard_vtable_116(&mut self);
    fn apply_skateboard_force_queue_82c03718(&mut self);
    fn update_skateboard_fixed_step_cache_82db61f0(&mut self);
}

pub fn run_state_phase(fields: &mut StatePhaseFields, services: &mut impl StatePhaseServices) {
    services.update_current_state_vtable_8();

    if fields.controller_system_on_452 {
        services.update_controller_82d75f00();
        match fields.controller_state_448 {
            1 => services.update_controller_mode_1_82d751d8(),
            2 => services.update_controller_mode_2_82d750f0(),
            3 => services.update_controller_mode_3_82d75bb8(),
            4 => services.update_controller_mode_4_82d75d58(),
            _ => {}
        }
    }

    services.touch_skateboard_vtable_116();
    services.apply_skateboard_force_queue_82c03718();
    services.update_skateboard_fixed_step_cache_82db61f0();
    fields.elapsed_1344 += fields.timestep_2604;
}
