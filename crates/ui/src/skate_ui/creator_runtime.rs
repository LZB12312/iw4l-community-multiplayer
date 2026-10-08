use super::{
    apt_host::{self, MovieHost},
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
    creator_menu::{Choice, Focus, Item, Page, UndoScope},
};
use sim::character::CharacterProfile;

pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
}

#[derive(Clone, Copy)]
struct Position {
    page: Page,
    index: usize,
    first: usize,
}

pub struct Bindings {
    pub movie: Movie,
    pub profile: CharacterProfile,
    pub focus: Focus,
    pub rotation_reset: u64,
    snapshot: CharacterProfile,
    position: Position,
    parents: Vec<Position>,
    controller: Option<usize>,
    navigation: Option<Page>,
    edit: Option<Vec<String>>,
    pending: Option<u64>,
    pending_snapshot: bool,
    error: Option<String>,
    pub closed: bool,
}

impl Bindings {
    fn items(&self) -> impl Iterator<Item = &'static Item> + '_ {
        self.position
            .page
            .menu()
            .items
            .iter()
            .filter(|item| self.profile.male || !item.male_only)
    }

    fn item(&self, index: usize) -> Result<&'static Item, String> {
        self.items().nth(index).ok_or("Invalid creator item".into())
    }

    fn enabled(&self, index: usize) -> bool {
        self.pending.is_none()
            && self.item(index).is_ok_and(|item| {
                matches!(
                    item.choice,
                    Choice::Page(_) | Choice::Gender | Choice::Morph(_) | Choice::Undo(_)
                )
            })
    }

    fn morph_value(&self, target: &str) -> Result<(f32, assets::character::MorphRange), String> {
        let library = assets::character::local_library().ok_or("Missing character library")?;
        let range = library.morph_range(target)?;
        let value = self
            .profile
            .morphs
            .get(target)
            .ok_or("Missing character morph")?
            .value();
        if !range.min.is_finite() || !range.max.is_finite() || range.max <= range.min {
            return Err("Invalid character morph range".into());
        }
        Ok((
            ((value - range.min) / (range.max - range.min)).clamp(0., 1.),
            range,
        ))
    }
}

impl MovieHost for Bindings {
    fn movie(&mut self) -> &mut Movie {
        &mut self.movie
    }
}

impl Host for Bindings {
    fn property(&mut self, vm: &Vm, id: usize, key: &str) -> Result<Value, String> {
        self.movie.property(vm, id, key)
    }

    fn property_changed(&mut self, vm: &mut Vm, id: usize, key: &str) -> Result<(), String> {
        if matches!(key, "text" | "autoSize" | "wordWrap" | "multiline") {
            self.movie.text_changed(vm, id)?;
        }
        Ok(())
    }

    fn call(
        &mut self,
        vm: &mut Vm,
        id: usize,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if let Some(value) = apt_host::call(self, vm, id, method, &args)? {
            return Ok(value);
        }
        let native = match &vm.objects.get(id).ok_or("Missing creator object")?.kind {
            ObjectKind::Native(name) => name.as_str(),
            _ => "global",
        };
        let argument = |index: usize| args.get(index).ok_or("Missing creator argument");
        let index = |argument_number: usize| -> Result<usize, String> {
            let number = argument(argument_number)?.number();
            if !number.is_finite() || number < 0. || number.fract() != 0. {
                return Err("Invalid creator index".into());
            }
            Ok(number as usize)
        };
        match (native, method) {
            ("ScreenManager", "OnLoaded") => {
                let Some(Value::Object(controller)) = args.get(1) else {
                    return Err("Missing creator controller".into());
                };
                self.controller = Some(*controller);
                Ok(Value::Undefined)
            }
            ("ScreenManager", "IntroComplete")
            | ("LetterBox", "EnableLetterBox")
            | ("Audio", "PlaySound") => Ok(Value::Undefined),
            ("Game", "CAC_GetReceiptBalance") => Ok(Value::Number(0.)),
            ("Game", "CAC_GetNumItems") => Ok(Value::Number(self.items().count() as f64)),
            ("Game", "CAC_GetTitleText") => Ok(Value::Text(self.position.page.menu().title.into())),
            ("Game", "CAC_GetFilterText") => Ok(Value::Text("#".into())),
            ("Game", "CAC_GetItemText") => {
                let item = self.item(index(0)?)?;
                Ok(Value::Text(
                    if matches!(item.choice, Choice::Gender) {
                        if self.profile.male {
                            "ID_CAC_CHOOSE_GENDER_MALE"
                        } else {
                            "ID_CAC_CHOOSE_GENDER_FEMALE"
                        }
                    } else {
                        item.label
                    }
                    .into(),
                ))
            }
            ("Game", "CAC_GetOptionType") => Ok(Value::Text(self.item(index(0)?)?.kind.into())),
            ("Game", "CAC_GetIntegerValue" | "CAC_GetStringValue") => {
                if let Choice::Morph(target) = self.item(index(0)?)?.choice {
                    if method != "CAC_GetIntegerValue" {
                        return Err("Character morph has no text value".into());
                    }
                    return Ok(Value::Number(
                        (self.morph_value(target)?.0 * 122.).round() as f64
                    ));
                }
                let (value, labels) = match self.item(index(0)?)?.choice {
                    Choice::Trucks => (
                        self.profile.trucks.value(),
                        [
                            "ID_CAC_EDIT_TRUCKS_LOOSE",
                            "ID_CAC_EDIT_TRUCKS_MEDIUM",
                            "ID_CAC_EDIT_TRUCKS_TIGHT",
                        ],
                    ),
                    Choice::Wheels => (
                        self.profile.wheels.value(),
                        [
                            "ID_CAC_EDIT_WHEELS_SOFT",
                            "ID_CAC_EDIT_WHEELS_MEDIUM",
                            "ID_CAC_EDIT_WHEELS_HARD",
                        ],
                    ),
                    _ => return Err("Creator item has no board value".into()),
                };
                if method == "CAC_GetIntegerValue" {
                    Ok(Value::Number((value * 10.).round() as f64))
                } else {
                    Ok(Value::Text(
                        labels[if value < 0.35 {
                            0
                        } else if value < 0.75 {
                            1
                        } else {
                            2
                        }]
                        .into(),
                    ))
                }
            }
            ("Game", "CAC_GetInfoPanelType") => Ok(Value::Text(
                if self.error.is_some() {
                    "blurb"
                } else {
                    self.position.page.menu().info
                }
                .into(),
            )),
            ("Game", "CAC_GetDescriptionText") => {
                Ok(Value::Text(self.error.as_ref().map_or_else(
                    || self.item(index(0)?).map(|item| item.description.to_owned()),
                    |error| Ok(format!("#{error}")),
                )?))
            }
            ("Game", "CAC_GetCurrentMenuIndex") => Ok(Value::Number(self.position.index as f64)),
            ("Game", "CAC_GetCurrentMenuMinWindowIndex") => {
                Ok(Value::Number(self.position.first as f64))
            }
            ("Game", "CAC_UpdateMenuIndices") => {
                let next = index(0)?;
                self.item(next)?;
                let first = argument(1)?.number();
                if !first.is_finite() || first < 0. || first.fract() != 0. || first > next as f64 {
                    return Err("Invalid creator window".into());
                }
                self.position.index = next;
                self.position.first = first as usize;
                Ok(Value::Undefined)
            }
            ("Game", "CAC_GetButtonHelpTextA") => Ok(Value::Text(
                if self.enabled(self.position.index)
                    && self.item(self.position.index).is_ok_and(|item| {
                        matches!(
                            item.choice,
                            Choice::Page(_) | Choice::Gender | Choice::Undo(_)
                        )
                    })
                {
                    "ID_COMMON_SELECT"
                } else {
                    "#"
                }
                .into(),
            )),
            ("Game", "CAC_GetButtonHelpTextB") => Ok(Value::Text(
                if self.position.page == Page::Main {
                    "ID_CAC_EXIT"
                } else {
                    "ID_COMMON_BACK"
                }
                .into(),
            )),
            ("Game", "OnMenuGetItemEnabled") => {
                Ok(Value::Bool(index(1).is_ok_and(|index| self.enabled(index))))
            }
            ("Game", "OnMenuHilight") => Ok(Value::Undefined),
            ("Game", "OnMenuSelect") => {
                let selected = index(1)?;
                if self.enabled(selected) {
                    match self.item(selected)?.choice {
                        Choice::Page(page) => self.navigation = Some(page),
                        Choice::Gender => {
                            self.edit = Some(vec![
                                "gender".into(),
                                if self.profile.male { "female" } else { "male" }.into(),
                            ])
                        }
                        Choice::Undo(scope) => {
                            self.edit = Some(vec![
                                match scope {
                                    UndoScope::Appearance => "restore",
                                    UndoScope::Morphs => "restore_morphs",
                                }
                                .into(),
                                serde_json::to_string(&self.snapshot).map_err(|e| e.to_string())?,
                            ]);
                        }
                        _ => {}
                    }
                }
                Ok(Value::Undefined)
            }
            _ => Err(format!("Unsupported creator binding {native}.{method}")),
        }
    }
}

pub struct Runtime {
    pub vm: Vm,
    pub bindings: Bindings,
    controller: usize,
}

impl Runtime {
    pub fn load(source: &serde_json::Value, profile: CharacterProfile) -> Result<Self, String> {
        if !profile.valid() {
            return Err("Invalid creator profile".into());
        }
        let mut vm = Vm::new();
        vm.set(vm.global, "_global", Value::Object(vm.global))?;
        vm.set(vm.global, "Screen_EdgeOffset", Value::Number(0.))?;
        for key in [
            "AptUp",
            "AptDown",
            "AptLeft",
            "AptRight",
            "AptNext",
            "AptBack",
            "FE_SOUND_CORE_NAV_UP",
            "FE_SOUND_CORE_NAV_DOWN",
            "FE_SOUND_BLUE_BOX_NAV",
        ] {
            vm.set(vm.global, key, Value::Text(key.into()))?;
        }
        for name in [
            "MovieClip",
            "Object",
            "TextFormat",
            "FELanguage",
            "Math",
            "Audio",
            "ScreenManager",
            "FE",
            "LetterBox",
            "Game",
            "CAS",
            "MenuPicker",
            "ButtonHelp",
        ] {
            let id = vm.object(ObjectKind::Native(name.into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(id, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(id))?;
        }
        let mut bindings = Bindings {
            movie: Movie::load(source)?,
            snapshot: profile.clone(),
            profile,
            focus: Focus::Standing,
            rotation_reset: 0,
            position: Position {
                page: Page::Main,
                index: 0,
                first: 0,
            },
            parents: Vec::new(),
            controller: None,
            navigation: None,
            edit: None,
            pending: None,
            pending_snapshot: false,
            error: None,
            closed: false,
        };
        let initial: Vec<u32> =
            serde_json::from_value(source["initial_actions"].clone()).map_err(|e| e.to_string())?;
        for offset in initial {
            let code = bindings
                .movie
                .actions
                .get(&offset.to_string())
                .ok_or("Missing creator initialization")?
                .clone();
            vm.run(&code, &mut bindings)?;
        }
        bindings.movie.initialize(&mut vm)?;
        for _ in 0..60 {
            vm.begin_update();
            apt_host::drain(&mut bindings, &mut vm)?;
            bindings.movie.advance(&mut vm)?;
        }
        apt_host::drain(&mut bindings, &mut vm)?;
        let controller = bindings.controller.ok_or("Creator did not load")?;
        vm.set(controller, "Id", Value::Number(0.))?;
        let mut runtime = Self {
            vm,
            bindings,
            controller,
        };
        runtime.refresh()?;
        runtime.vm.call_method(
            runtime.bindings.movie.root,
            "gotoAndPlay",
            vec![Value::Text("intro".into())],
            &mut runtime.bindings,
        )?;
        for _ in 0..30 {
            runtime.advance(1)?;
        }
        Ok(runtime)
    }

    fn refresh(&mut self) -> Result<(), String> {
        self.vm.begin_update();
        self.vm.call_method(
            self.controller,
            "SetState",
            vec![Value::Number(
                self.bindings.position.page.menu().kind as f64,
            )],
            &mut self.bindings,
        )?;
        apt_host::drain(&mut self.bindings, &mut self.vm)
    }

    pub fn input(&mut self, key: Key, request_id: u64) -> Result<Option<Vec<String>>, String> {
        if self.bindings.closed || self.bindings.pending.is_some() {
            return Ok(None);
        }
        if matches!(key, Key::Back) {
            if let Some(parent) = self.bindings.parents.pop() {
                if matches!(parent.page, Page::Main | Page::Body | Page::Merchandise) {
                    self.bindings.focus = Focus::Standing;
                }
                self.bindings.rotation_reset = self.bindings.rotation_reset.wrapping_add(1);
                self.bindings.snapshot = self.bindings.profile.clone();
                self.bindings.position = parent;
                self.bindings.error = None;
                self.refresh()?;
            } else {
                self.bindings.closed = true;
            }
            return Ok(None);
        }
        self.vm.begin_update();
        let key = match key {
            Key::Up => "AptUp",
            Key::Down => "AptDown",
            Key::Left => "AptLeft",
            Key::Right => "AptRight",
            Key::Select => "AptNext",
            Key::Back => unreachable!(),
        };
        self.vm.call_method(
            self.controller,
            "UpdateInput",
            vec![Value::Text(key.into()), Value::Number(0.)],
            &mut self.bindings,
        )?;
        apt_host::drain(&mut self.bindings, &mut self.vm)?;
        if matches!(key, "AptLeft" | "AptRight") {
            if let Choice::Morph(target) = self.bindings.item(self.bindings.position.index)?.choice
            {
                let (value, range) = self.bindings.morph_value(target)?;
                let next = (value + if key == "AptLeft" { -0.01 } else { 0.01 }).clamp(0., 1.);
                if next != value {
                    self.bindings.edit = Some(vec![
                        "morph".into(),
                        target.into(),
                        (range.min + next * (range.max - range.min)).to_string(),
                    ]);
                }
            }
        }
        if let Some(page) = self.bindings.navigation.take() {
            self.bindings.snapshot = self.bindings.profile.clone();
            if let Some(focus) = page.focus() {
                self.bindings.focus = focus;
                self.bindings.rotation_reset = self.bindings.rotation_reset.wrapping_add(1);
            }
            self.bindings.parents.push(self.bindings.position);
            self.bindings.position = Position {
                page,
                index: 0,
                first: 0,
            };
            self.bindings.error = None;
            self.refresh()?;
        }
        if let Some(edit) = self.bindings.edit.take() {
            self.bindings.pending_snapshot = matches!(
                edit.first().map(String::as_str),
                Some("gender" | "restore" | "restore_morphs")
            );
            self.bindings.pending = Some(request_id);
            return Ok(Some(edit));
        }
        Ok(None)
    }

    pub fn acknowledge(
        &mut self,
        request_id: u64,
        result: &Result<(), String>,
        profile: &CharacterProfile,
    ) -> Result<(), String> {
        if self.bindings.pending != Some(request_id) {
            return Ok(());
        }
        self.bindings.pending = None;
        if result.is_ok() {
            self.bindings.profile = profile.clone();
            if self.bindings.pending_snapshot {
                self.bindings.snapshot = profile.clone();
            }
        }
        self.bindings.pending_snapshot = false;
        self.bindings.error = result.as_ref().err().cloned();
        self.bindings.position.index = self
            .bindings
            .position
            .index
            .min(self.bindings.items().count().saturating_sub(1));
        self.bindings.position.first = self
            .bindings
            .position
            .first
            .min(self.bindings.position.index);
        self.refresh()
    }

    pub fn advance(&mut self, frames: usize) -> Result<(), String> {
        for _ in 0..frames.min(6) {
            self.vm.begin_update();
            self.bindings.movie.advance(&mut self.vm)?;
            apt_host::drain(&mut self.bindings, &mut self.vm)?;
        }
        self.vm
            .collect(self.bindings.movie.roots().chain([self.controller]))?;
        Ok(())
    }

    pub fn sync_profile(&mut self, profile: &CharacterProfile) -> Result<(), String> {
        if self.bindings.pending.is_none() && self.bindings.profile != *profile {
            if !profile.valid() {
                return Err("Invalid creator profile".into());
            }
            if self.bindings.profile.male != profile.male {
                self.bindings.snapshot = profile.clone();
            }
            self.bindings.profile = profile.clone();
            self.bindings.position.index = self
                .bindings
                .position
                .index
                .min(self.bindings.items().count().saturating_sub(1));
            self.bindings.position.first = self
                .bindings
                .position
                .first
                .min(self.bindings.position.index);
            self.refresh()?;
        }
        Ok(())
    }
}
