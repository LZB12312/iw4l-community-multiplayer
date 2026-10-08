use super::{
    apt_host::{self, MovieHost},
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
};

#[derive(Clone, Default, PartialEq)]
pub struct Input {
    pub score: u32,
    pub speed: f32,
    pub duration: f32,
}
pub struct Bindings {
    pub movie: Movie,
    pub input: Input,
}
pub struct Runtime {
    pub vm: Vm,
    pub bindings: Bindings,
    controller: usize,
    visible: bool,
}
fn array(vm: &mut Vm, values: &[Value]) -> Result<Value, String> {
    let id = vm.object(ObjectKind::Plain);
    for (index, value) in values.iter().enumerate() {
        vm.set(id, index.to_string(), value.clone())?;
    }
    vm.set(id, "length", Value::Number(values.len() as f64))?;
    Ok(Value::Object(id))
}
fn number(value: f64) -> String {
    let digits = (value.max(0.) as u64).to_string();
    let mut output = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            output.push(',');
        }
        output.push(c);
    }
    output
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
        let native = match &vm.objects.get(id).ok_or("Missing UI object")?.kind {
            ObjectKind::Native(name) => name.as_str(),
            _ => "global",
        };
        let first = || args.first().ok_or("Missing UI argument");
        match (native, method) {
            ("global", "ASSetPropFlags" | "trace") | ("ScreenManager", "OnLoaded") => {
                Ok(Value::Undefined)
            }
            ("Audio", "PlaySound") => Ok(Value::Undefined),
            ("FELanguage", "GetLocalizedFormatNumberString") => {
                Ok(Value::Text(number(first()?.number())))
            }
            ("FELanguage", "GetLocalizedTime") => {
                Ok(Value::Text(format!("{:.2}", first()?.number())))
            }
            ("FELanguage", "GetLocalizedFormatStringCustom") => {
                let label = first()?.text();
                let value = args.get(1).ok_or("Missing metric value")?.number();
                let template = self.movie.text_assets.localize(&label);
                let text = template
                    .replace("{0:speed}", &format!("{:.0} km/h", value * 3.6))
                    .replace("{0:distance}", &format!("{value:.1} m"));
                Ok(Value::Text(format!("#{text}")))
            }
            ("HUDComponents", "HOM_GetScore") => Ok(Value::Number(self.input.score as f64)),
            ("HUDComponents", "HOM_GetBonusData") => array(
                vm,
                &[
                    Value::Number(self.input.speed as f64),
                    Value::Number(0.),
                    Value::Number(0.),
                    Value::Number(self.input.duration as f64),
                    Value::Number(0.),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::Bool(false),
                ],
            ),
            ("HUDComponents", "HOM_GetBonusScores") => array(
                vm,
                &[
                    Value::Number((self.input.speed * 10.) as f64),
                    Value::Number(0.),
                    Value::Number(0.),
                    Value::Number((self.input.duration * 10.) as f64),
                    Value::Number(0.),
                ],
            ),
            ("HUDComponents", "HOM_GetBonusCollisionData")
            | ("HUDComponents", "HOM_GetBonusCollisionScore") => array(
                vm,
                &[
                    Value::Number(0.),
                    Value::Number(0.),
                    Value::Bool(false),
                    Value::Number(0.),
                    Value::Number(0.),
                ],
            ),
            _ => Err(format!(
                "Unsupported Hall of Meat binding {native}.{method}"
            )),
        }
    }
}
impl Runtime {
    pub fn load(source: &serde_json::Value) -> Result<Self, String> {
        let mut vm = Vm::new();
        vm.set(vm.global, "_global", Value::Object(vm.global))?;
        for name in [
            "MovieClip",
            "Object",
            "TextFormat",
            "HUDComponents",
            "FELanguage",
            "Math",
            "Audio",
            "ScreenManager",
        ] {
            let id = vm.object(ObjectKind::Native(name.into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(id, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(id))?;
        }
        vm.set(vm.global, "Screen_EdgeOffset", Value::Number(0.))?;
        let mut bindings = Bindings {
            movie: Movie::load(source)?,
            input: Input::default(),
        };
        let initial: Vec<u32> = if let Some(initial) = source.get("initial_actions") {
            serde_json::from_value(initial.clone()).map_err(|e| e.to_string())?
        } else {
            bindings
                .movie
                .characters
                .values()
                .flat_map(|c| &c.frames)
                .flat_map(|f| &f.controls)
                .filter(|c| c.type_name == "do_init_action")
                .map(|c| c.actions_offset)
                .collect()
        };
        for offset in initial {
            let code = bindings
                .movie
                .actions
                .get(&offset.to_string())
                .ok_or("Missing UI initialization")?
                .clone();
            vm.run(&code, &mut bindings)?;
        }
        bindings.movie.initialize(&mut vm)?;
        let mut runtime = Self {
            vm,
            bindings,
            controller: usize::MAX,
            visible: false,
        };
        runtime.drain()?;
        let Value::Object(controller) = runtime.vm.get(runtime.bindings.movie.root, "screen")
        else {
            return Err("Hall of Meat controller absent".into());
        };
        runtime.controller = controller;
        runtime.vm.call_method(
            controller,
            "Show",
            vec![Value::Bool(false)],
            &mut runtime.bindings,
        )?;
        runtime.drain()?;
        Ok(runtime)
    }
    fn drain(&mut self) -> Result<(), String> {
        apt_host::drain(&mut self.bindings, &mut self.vm)
    }
    pub fn update(
        &mut self,
        input: Input,
        visible: bool,
        new_impact: bool,
        frames: usize,
    ) -> Result<(), String> {
        self.vm.begin_update();
        let changed = self.bindings.input != input;
        self.bindings.input = input;
        if visible != self.visible || new_impact {
            if new_impact && self.visible {
                self.vm.call_method(
                    self.controller,
                    "ResetBonusMetrics",
                    vec![],
                    &mut self.bindings,
                )?;
                self.vm
                    .call_method(self.controller, "ResetBonuses", vec![], &mut self.bindings)?;
            }
            self.vm.call_method(
                self.controller,
                "Show",
                vec![Value::Bool(visible)],
                &mut self.bindings,
            )?;
            self.visible = visible;
            self.drain()?;
        }
        if visible && (changed || new_impact) {
            self.vm
                .call_method(self.controller, "UpdateScoring", vec![], &mut self.bindings)?;
            self.drain()?;
        }
        for _ in 0..frames.min(6) {
            self.bindings.movie.advance(&mut self.vm)?;
            self.drain()?;
        }
        self.vm
            .collect(self.bindings.movie.roots().chain([self.controller]))?;
        Ok(())
    }
}
