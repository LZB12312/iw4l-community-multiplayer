pub trait PostOutputServices {
    fn finish_player_output_82909510(&mut self, player_field_1392: &mut PlayerOutputField1392);

    fn finish_skeleton_output_82be2ba8(&mut self);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerOutputField1392;

/// Runs the complete TU3 PostOutput phase.
pub fn run_post_output(
    player_field_1392: &mut PlayerOutputField1392,
    services: &mut impl PostOutputServices,
) {
    services.finish_player_output_82909510(player_field_1392);
    services.finish_skeleton_output_82be2ba8();
}
