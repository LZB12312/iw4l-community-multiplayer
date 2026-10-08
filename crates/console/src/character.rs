use crate::{CommandSpec, ConsoleCommand, ConsoleRegistry};
use bevy::prelude::*;
use frame::{UiCharacterEdit, UiCharacterEditResult, UiMenuDvars, UiMenuRequest};

const PROFILE: &str = "iw4l-character.txt";
const FULL_PROFILE: &str = "iw4l-character-profile.json";
const FIELDS: [&str; 5] = ["skin", "shirt", "pants", "hair", "board"];

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedCharacter {
    version: u8,
    selections: [u8; 6],
    profile: Option<sim::character::CharacterProfile>,
}

impl SavedCharacter {
    fn appearance(self) -> Option<sim::CharacterAppearance> {
        if self.version != 1 || self.selections[0] > 1 {
            return None;
        }
        let mut a = appearance(self.selections);
        a.profile = self.profile.map(Into::into);
        a.valid().then_some(a)
    }
}

fn choices(a: &sim::CharacterAppearance) -> [u8; 6] {
    [a.skater.into(), a.skin, a.shirt, a.pants, a.hair, a.board]
}

fn appearance(v: [u8; 6]) -> sim::CharacterAppearance {
    sim::CharacterAppearance {
        skater: v[0] == 1,
        skin: v[1],
        shirt: v[2],
        pants: v[3],
        hair: v[4],
        board: v[5],
        profile: None,
    }
}

fn load(mut character: ResMut<net::LocalCharacter>) {
    character.0.skater = assets::bot_model::local_characters().is_some();
    if let Ok(text) = std::fs::read_to_string(PROFILE) {
        let values: Vec<_> = text
            .split_whitespace()
            .filter_map(|v| v.parse::<u8>().ok())
            .collect();
        if let Ok(v) = <[u8; 6]>::try_from(values.as_slice())
            && v[0] <= 1
            && v[1..].iter().all(|v| *v <= 15)
        {
            character.0 = appearance(v);
        }
    }
    if let Ok(bytes) = std::fs::read(FULL_PROFILE) {
        match serde_json::from_slice::<SavedCharacter>(&bytes) {
            Ok(saved) => match saved.appearance() {
                Some(a) => character.0 = a,
                None => diag::warn!(Console, "character profile has invalid selections"),
            },
            Err(error) => diag::warn!(Console, "character profile could not be loaded: {error}"),
        }
    }
}

fn save(character: &sim::CharacterAppearance) -> std::io::Result<()> {
    if !character.valid() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid character selections",
        ));
    }
    let saved = SavedCharacter {
        version: 1,
        selections: choices(character),
        profile: character.profile.as_deref().cloned(),
    };
    let bytes = serde_json::to_vec_pretty(&saved).map_err(std::io::Error::other)?;
    let temporary = format!("{FULL_PROFILE}.tmp");
    std::fs::write(&temporary, bytes)?;
    if let Err(error) = std::fs::rename(&temporary, FULL_PROFILE) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn edit_profile(
    current: &sim::CharacterAppearance,
    args: &[String],
) -> Result<sim::CharacterAppearance, String> {
    use assets::character::CharacterLibrary;
    use sim::character::CharacterAsset;
    let library = assets::character::local_library()
        .ok_or("Full Skate 3 character data has not been prepared.")?;
    let mut next = current.clone();
    let operation = args
        .first()
        .map(String::as_str)
        .ok_or("Missing character edit.")?;
    let profile = match operation {
        "default" | "gender" => {
            let male = match args.get(1).map(String::as_str) {
                Some("male") => true,
                Some("female") => false,
                None if operation == "default" => current.profile.as_ref().is_none_or(|p| p.male),
                _ => return Err("Choose male or female.".into()),
            };
            library.default_profile(male)?
        }
        "restore" | "restore_morphs" => {
            let encoded = args.get(1).ok_or("Missing character snapshot.")?;
            if encoded.len() > 65536 {
                return Err("Character snapshot is too large.".into());
            }
            let saved: sim::character::CharacterProfile =
                serde_json::from_str(encoded).map_err(|_| "Invalid character snapshot.")?;
            library.validate(&saved)?;
            let profile = current
                .profile
                .as_deref()
                .ok_or("Missing current character profile.")?;
            if profile.male != saved.male {
                return Err("Character snapshot has a different gender.".into());
            }
            if operation == "restore_morphs" {
                library.with_saved_morphs(profile, &saved)?
            } else {
                saved
            }
        }
        "model" | "material" | "morph" => {
            let profile = current
                .profile
                .as_deref()
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| library.default_profile(true))?;
            let name = args.get(1).ok_or("Missing character part or morph.")?;
            if operation == "morph" {
                let value = args
                    .get(2)
                    .ok_or("Missing morph value.")?
                    .parse::<f32>()
                    .map_err(|_| "Invalid morph value.")?;
                library.with_morph(&profile, name, value)?
            } else {
                let slot = CharacterLibrary::slot(name).ok_or("Unknown character part.")?;
                if operation == "model" {
                    let asset = CharacterAsset::try_from(
                        args.get(2).ok_or("Missing character model.")?.clone(),
                    )?;
                    library.with_model(&profile, slot, asset)?
                } else {
                    let group = args
                        .get(2)
                        .ok_or("Missing material group.")?
                        .parse::<usize>()
                        .map_err(|_| "Invalid material group.")?;
                    let asset = CharacterAsset::try_from(
                        args.get(3).ok_or("Missing character material.")?.clone(),
                    )?;
                    library.with_material(&profile, slot, group, asset)?
                }
            }
        }
        _ => return Err("Unknown character edit.".into()),
    };
    next.skater = true;
    next.profile = Some(profile.into());
    Ok(next)
}

pub fn register(app: &mut App) {
    app.add_systems(Startup, load)
        .add_systems(Update, route.in_set(frame::ClientSet::Ui));
}

pub fn commands(registry: &mut ConsoleRegistry) {
    for name in [
        "ui_character_open",
        "ui_skate_creator_open",
        "ui_character_cycle",
        "character",
    ] {
        registry.register(CommandSpec::new(name));
    }
}

fn route(
    mut events: MessageReader<ConsoleCommand>,
    mut edits: MessageReader<UiCharacterEdit>,
    mut results: MessageWriter<UiCharacterEditResult>,
    mut character: ResMut<net::LocalCharacter>,
    mut dvars: ResMut<UiMenuDvars>,
    mut menus: MessageWriter<UiMenuRequest>,
    creator: Res<frame::SkateCreatorState>,
) {
    for edit in edits.read() {
        let result = commit_profile(&mut character.0, &mut dvars, &edit.args);
        results.write(UiCharacterEditResult {
            request_id: edit.request_id,
            result,
        });
    }
    for command in events.read() {
        if command.name == "ui_character_open" {
            dvars.set(
                "ui_character_status",
                "Choices apply to both walking and skating, and are visible to other players.",
            );
            menus.write(UiMenuRequest::Open(if creator.available {
                "character_options".into()
            } else {
                "character_creator".into()
            }));
        } else if command.name == "ui_skate_creator_open" {
            menus.write(UiMenuRequest::Open(if creator.available {
                frame::SKATE_CREATOR_MENU.into()
            } else {
                "character_creator".into()
            }));
        } else if command.name == "ui_character_cycle" || command.name == "character" {
            if command.args.first().map(String::as_str) == Some("native") {
                let _ = commit_profile(&mut character.0, &mut dvars, &command.args[1..]);
                continue;
            }
            let mut values = choices(&character.0);
            let mut profile = character.0.profile.clone();
            match command.args.first().map(String::as_str) {
                Some("kind") | Some("soldier") | Some("skater") => {
                    let skater = match command.args.first().map(String::as_str) {
                        Some("soldier") => false,
                        Some("skater") => true,
                        _ => !character.0.skater,
                    };
                    if skater && assets::bot_model::local_characters().is_none() {
                        dvars.set(
                            "ui_character_status",
                            "Skate 3 character assets have not been prepared.",
                        );
                        continue;
                    }
                    values[0] = skater.into();
                }
                Some(field) => {
                    let Some(i) = FIELDS.iter().position(|f| *f == field) else {
                        continue;
                    };
                    values[i + 1] = (values[i + 1] + 1..=15)
                        .find(|n| assets::bot_model::character_option(field, *n).is_some())
                        .unwrap_or(0);
                    profile = None;
                }
                None => continue,
            }
            let mut next = appearance(values);
            next.profile = profile;
            if let Err(error) = save(&next) {
                dvars.set(
                    "ui_character_status",
                    format!("Could not save character: {error}"),
                );
                continue;
            }
            character.0 = next;
        }
    }
    dvars.set(
        "ui_character_kind",
        if character.0.skater {
            "Skate 3 skater"
        } else {
            "MW2 soldier"
        },
    );
    for (field, index) in FIELDS.iter().zip(&choices(&character.0)[1..]) {
        let label = if let Some(profile) = &character.0.profile {
            use sim::character::CharacterSlot;
            let slot = match *field {
                "skin" => CharacterSlot::Face,
                "shirt" => CharacterSlot::OuterTop,
                "pants" => CharacterSlot::Pants,
                "hair" => CharacterSlot::Hair,
                _ => CharacterSlot::Deck,
            };
            let part = &profile.parts[slot as usize];
            if part.model.0 == 0 {
                "None"
            } else {
                assets::character::local_library()
                    .and_then(|library| {
                        if *field == "skin" {
                            part.materials
                                .first()
                                .and_then(|id| library.materials().get(&String::from(*id)))
                                .map(|m| m.name.as_str())
                        } else {
                            library
                                .models()
                                .get(&String::from(part.model))
                                .map(|m| m.name.as_str())
                        }
                    })
                    .unwrap_or("Unavailable")
            }
        } else {
            assets::bot_model::character_option(field, *index)
                .map_or("Default", |p| p.label.as_str())
        };
        dvars.set(&format!("ui_character_{field}"), label);
    }
}

fn commit_profile(
    current: &mut sim::CharacterAppearance,
    dvars: &mut UiMenuDvars,
    args: &[String],
) -> Result<(), String> {
    let result = edit_profile(current, args).and_then(|next| {
        save(&next).map_err(|error| format!("Could not save character: {error}"))?;
        *current = next;
        Ok(())
    });
    dvars.set(
        "ui_character_status",
        match &result {
            Ok(()) => "Character saved. Changes apply to walking and skating.".into(),
            Err(error) => error.clone(),
        },
    );
    result
}
