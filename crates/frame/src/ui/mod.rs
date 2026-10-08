use bevy::prelude::*;

pub mod audio;
pub mod input;
pub mod menu;

pub use audio::{UiPlayMusic, UiPlaySound, UiStopMusic};
pub use input::{UiBindRequest, UiBindingCapture};
pub use menu::{
    HostMatchRules, SKATE_CREATOR_MENU, SkateCreatorState, UiCharacterEdit, UiCharacterEditResult,
    UiCharacterInput, UiExecCommand, UiMenuDvars, UiMenuKey, UiMenuRequest, UiPartyState,
};

pub fn register_ui_contracts(app: &mut App) {
    app.init_resource::<UiMenuDvars>()
        .init_resource::<UiBindingCapture>()
        .init_resource::<UiPartyState>()
        .init_resource::<SkateCreatorState>()
        .add_message::<UiBindRequest>()
        .add_message::<UiPlaySound>()
        .add_message::<UiPlayMusic>()
        .add_message::<UiStopMusic>()
        .add_message::<UiExecCommand>()
        .add_message::<UiCharacterEdit>()
        .add_message::<UiCharacterEditResult>()
        .add_message::<UiCharacterInput>()
        .add_message::<UiMenuRequest>();
}
