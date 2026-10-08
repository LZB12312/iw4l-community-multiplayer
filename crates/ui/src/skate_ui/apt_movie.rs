use super::{
    apt_display::{Control, DisplayList, Placement},
    apt_vm::{Instruction, ObjectKind, Value, Vm},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Deserialize)]
pub struct Frame {
    pub controls: Vec<Control>,
}
#[derive(Clone, Deserialize)]
pub struct Character {
    pub id: i32,
    pub type_name: String,
    #[serde(default)]
    pub frames: Vec<Frame>,
    pub text: Option<serde_json::Value>,
    pub bounds: Option<[f32; 4]>,
    pub source_bundle: Option<String>,
}
#[derive(Clone)]
pub struct Instance {
    pub character: i32,
    pub frame: usize,
    pub playing: bool,
    pub children: BTreeMap<i32, usize>,
    pub placement: Option<Placement>,
    attached: bool,
}
pub struct Movie {
    pub characters: BTreeMap<i32, Character>,
    pub instances: BTreeMap<usize, Instance>,
    pub actions: BTreeMap<String, Vec<Instruction>>,
    pub pending: VecDeque<(usize, u32)>,
    pub root: usize,
    pub text_assets: super::apt_text::TextAssets,
    states: BTreeMap<i32, Vec<DisplayList>>,
    exports: BTreeMap<String, i32>,
    bundle_exports: BTreeMap<String, BTreeMap<String, i32>>,
    classes: BTreeMap<i32, usize>,
    initialized: BTreeSet<usize>,
    bounds: super::apt_bounds::Bounds,
}
impl Movie {
    pub fn load(json: &serde_json::Value) -> Result<Self, String> {
        let characters: Vec<Character> =
            serde_json::from_value(json["characters"].clone()).map_err(|e| e.to_string())?;
        let mut states = BTreeMap::new();
        for c in &characters {
            let mut list = DisplayList::default();
            let mut frames = Vec::new();
            for frame in &c.frames {
                for control in &frame.controls {
                    list.apply(control)?;
                }
                frames.push(list.clone());
            }
            states.insert(c.id, frames);
        }
        Ok(Self {
            characters: characters.into_iter().map(|c| (c.id, c)).collect(),
            instances: BTreeMap::new(),
            actions: serde_json::from_value(json["actions"].clone()).map_err(|e| e.to_string())?,
            pending: VecDeque::new(),
            root: usize::MAX,
            text_assets: super::apt_text::TextAssets::load(json)?,
            states,
            exports: json
                .get("exports")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(BTreeMap::new),
            bundle_exports: json
                .get("bundle_exports")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(BTreeMap::new),
            classes: BTreeMap::new(),
            initialized: BTreeSet::new(),
            bounds: super::apt_bounds::authored(json)?,
        })
    }
    pub fn initialize(&mut self, vm: &mut Vm) -> Result<(), String> {
        if self.root != usize::MAX {
            return Err("Movie already initialized".into());
        }
        self.root = self.create(vm, 0, None, 0)?;
        vm.set(self.root, "_root", Value::Object(self.root))?;
        for id in self.instances.keys() {
            vm.set(*id, "_root", Value::Object(self.root))?;
        }
        Ok(())
    }
    fn create(
        &mut self,
        vm: &mut Vm,
        character: i32,
        parent: Option<usize>,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > 32 || self.instances.len() >= 16384 {
            return Err(format!(
                "APT movie hierarchy limit at character {character}, nesting {depth}, instances {}",
                self.instances.len()
            ));
        }
        let c = self
            .characters
            .get(&character)
            .ok_or("APT unknown character")?
            .clone();
        let id = vm.object(ObjectKind::Native(format!("movie:{character}")));
        if self.root == usize::MAX && parent.is_none() {
            self.root = id;
        }
        if self.root != usize::MAX {
            vm.set(id, "_root", Value::Object(self.root))?;
        }
        if let Some(parent) = parent {
            vm.set(id, "_parent", Value::Object(parent))?;
        }
        vm.set(id, "_x", Value::Number(0.0))?;
        vm.set(id, "_y", Value::Number(0.0))?;
        vm.set(id, "_visible", Value::Bool(true))?;
        vm.set(id, "_alpha", Value::Number(100.0))?;
        if let Some(text) = &c.text {
            vm.set(
                id,
                "multiline",
                Value::Bool(text["multiline"].as_bool().unwrap_or(false)),
            )?;
            vm.set(
                id,
                "wordWrap",
                Value::Bool(text["word_wrap"].as_bool().unwrap_or(false)),
            )?;
            vm.set(
                id,
                "text",
                Value::Text(text["initial_text"].as_str().unwrap_or("").into()),
            )?;
        }
        self.instances.insert(
            id,
            Instance {
                character,
                frame: 0,
                playing: !c.frames.is_empty(),
                children: BTreeMap::new(),
                placement: None,
                attached: false,
            },
        );
        self.text_changed(vm, id)?;
        if !c.frames.is_empty() {
            self.seek(vm, id, 0, depth)?;
        }
        Ok(id)
    }
    pub fn text_style(&self, vm: &Vm, id: usize) -> Result<(i32, f32, u32, i64), String> {
        let instance = self.instances.get(&id).ok_or("Text instance missing")?;
        let text = self.characters[&instance.character]
            .text
            .as_ref()
            .ok_or("Text definition missing")?;
        let mut font = text["font_id"].as_i64().ok_or("Text font missing")? as i32;
        let mut height = text["font_height"].as_f64().ok_or("Text height missing")? as f32;
        let mut color = u32::from_str_radix(
            text["color_argb"]
                .as_str()
                .ok_or("Text color missing")?
                .trim_start_matches('#'),
            16,
        )
        .map_err(|e| e.to_string())?;
        let mut align = text["alignment"].as_i64().unwrap_or(0);
        if let Value::Object(format) = vm.get(id, "__textFormat") {
            if let Value::Text(family) = vm.get(format, "font") {
                font = self.text_assets.apt_font(&family)?;
            }
            if let Value::Number(size) = vm.get(format, "size") {
                if !size.is_finite() || size <= 0. {
                    return Err("Invalid text format size".into());
                }
                height = size as f32;
            }
            if let Value::Number(rgb) = vm.get(format, "color") {
                if !rgb.is_finite() || !(0. ..=16777215.).contains(&rgb) {
                    return Err("Invalid text format color".into());
                }
                color = (color & 0xff000000) | rgb as u32;
            }
            if let Value::Text(value) = vm.get(format, "align") {
                align = match value.as_str() {
                    "left" => 0,
                    "right" => 1,
                    "center" => 2,
                    _ => return Err("Unknown native text alignment".into()),
                };
            }
        }
        Ok((font, height, color, align))
    }
    pub fn text_changed(&self, vm: &mut Vm, id: usize) -> Result<(), String> {
        let Some(instance) = self.instances.get(&id) else {
            return Ok(());
        };
        let character = &self.characters[&instance.character];
        let Some(_text) = &character.text else {
            return Ok(());
        };
        let value = self.text_assets.localize(&vm.get(id, "text").text());
        vm.set(id, "_displayText", Value::Text(value.clone()))?;
        let (font_id, height, _, _) = self.text_style(vm, id)?;
        let font = self
            .text_assets
            .fonts
            .get(&font_id)
            .ok_or("Missing original text font")?;

        let bounds = character.bounds.ok_or("Missing text bounds")?;
        let lines = font.lines(
            &value,
            height,
            vm.get(id, "multiline").truth(),
            vm.get(id, "wordWrap")
                .truth()
                .then_some(bounds[2] - bounds[0]),
        );
        let width = lines.iter().map(|line| line.width).fold(0., f32::max);
        vm.set(
            id,
            "textHeight",
            Value::Number((font.line_height * font.scale[1] * height * lines.len() as f32) as f64),
        )?;
        vm.set(id, "textWidth", Value::Number(width as f64))?;
        let autosize = vm.get(id, "autoSize").text();
        vm.set(
            id,
            "_width",
            Value::Number(if !vm.get(id, "wordWrap").truth()
                && (autosize == "left" || autosize == "right" || autosize == "center")
            {
                width
            } else {
                bounds[2] - bounds[0]
            } as f64),
        )?;
        Ok(())
    }
    fn remove(&mut self, vm: &mut Vm, id: usize) {
        if let Some(instance) = self.instances.remove(&id) {
            for child in instance.children.values() {
                self.remove(vm, *child);
            }
        }
        self.initialized.remove(&id);
        if let Value::Object(parent) = vm.get(id, "_parent") {
            let name = vm.get(id, "_name").text();
            if vm.get(parent, &name) == Value::Object(id) {
                vm.objects[parent].fields.remove(&name);
            }
        }
        let _ = vm.set(id, "_visible", Value::Bool(false));
        self.pending
            .retain(|(object, _)| self.instances.contains_key(object));
    }
    pub fn export(&self, parent: usize, symbol: &str) -> Result<i32, String> {
        let instance = self.instances.get(&parent).ok_or("Absent movie parent")?;
        let bundle = self.characters[&instance.character]
            .source_bundle
            .as_deref();
        bundle
            .and_then(|bundle| self.bundle_exports.get(bundle))
            .and_then(|exports| exports.get(symbol))
            .or_else(|| self.exports.get(symbol))
            .copied()
            .ok_or_else(|| format!("Unknown movie symbol {symbol}"))
    }
    pub fn register_class(&mut self, symbol: &str, class: usize) -> Result<(), String> {
        let character = self
            .exports
            .get(symbol)
            .copied()
            .ok_or_else(|| format!("Class lacks export {symbol}"))?;
        self.classes.insert(character, class);
        Ok(())
    }
    pub fn prepare_constructor(&mut self, vm: &mut Vm, id: usize) -> Result<bool, String> {
        let Some(instance) = self.instances.get(&id) else {
            return Ok(false);
        };
        let Some(class) = self.classes.get(&instance.character).copied() else {
            return Ok(false);
        };
        if !self.initialized.insert(id) {
            return Ok(false);
        }
        if let Value::Object(prototype) = vm.get(class, "prototype") {
            vm.objects[id].prototype = Some(prototype);
        }
        vm.set(id, "constructor", Value::Object(class))?;
        Ok(true)
    }
    pub fn prepare_constructors(&mut self, vm: &mut Vm) -> Result<Vec<usize>, String> {
        let ids: Vec<_> = self.instances.keys().copied().collect();
        let mut pending = Vec::new();
        for id in ids {
            if self.prepare_constructor(vm, id)? {
                pending.push(id);
            }
        }
        Ok(pending)
    }
    pub fn roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.instances
            .keys()
            .copied()
            .chain(self.classes.values().copied())
    }
    pub fn property(&self, vm: &Vm, id: usize, key: &str) -> Result<Value, String> {
        let value = vm.get(id, key);
        if value != Value::Undefined
            || !matches!(key, "_width" | "_height")
            || !self.instances.contains_key(&id)
        {
            return Ok(value);
        }
        let bounds = super::apt_bounds::local(self, vm, &self.bounds, id, 0)?;
        Ok(Value::Number(bounds.map_or(0., |r| {
            if key == "_width" {
                r[2] - r[0]
            } else {
                r[3] - r[1]
            }
        })))
    }
    pub fn attach(
        &mut self,
        vm: &mut Vm,
        parent: usize,
        character: i32,
        name: String,
        depth: i32,
        init: Option<usize>,
    ) -> Result<usize, String> {
        let mut nesting = 0;
        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if nesting >= 32 {
                return Err("Attached movie nesting limit".into());
            }
            self.instances
                .get(&id)
                .ok_or("Absent attached movie ancestor")?;
            nesting += 1;
            ancestor = match vm.get(id, "_parent") {
                Value::Object(id) => Some(id),
                _ => None,
            };
        }
        if let Some(old) = self
            .instances
            .get_mut(&parent)
            .ok_or("Absent attachMovie parent")?
            .children
            .remove(&depth)
        {
            self.remove(vm, old);
        }
        let id = self.create(vm, character, Some(parent), nesting)?;
        let placement = Placement {
            character,
            name: name.clone(),
            matrix: [1., 0., 0., 1., 0., 0.],
            color: [255, 255, 255, 255, 0, 0, 0, 0],
            clip_events: vec![],
            blend_mode: 0,
            clip_depth: -1,
            ratio: 0.,
        };
        let instance = self.instances.get_mut(&id).unwrap();
        instance.placement = Some(placement);
        instance.attached = true;
        self.instances
            .get_mut(&parent)
            .unwrap()
            .children
            .insert(depth, id);
        vm.set(parent, name.clone(), Value::Object(id))?;
        vm.set(id, "_name", Value::Text(name))?;
        if let Some(object) = init {
            let fields = vm.objects[object].fields.clone();
            for (key, value) in fields {
                vm.set(id, key, value)?;
            }
        }
        Ok(id)
    }
    pub fn seek(
        &mut self,
        vm: &mut Vm,
        id: usize,
        frame: usize,
        nesting: usize,
    ) -> Result<(), String> {
        let instance = self
            .instances
            .get(&id)
            .ok_or("APT absent movie instance")?
            .clone();
        let character = self
            .characters
            .get(&instance.character)
            .ok_or("APT absent movie character")?;
        if frame >= character.frames.len() {
            return Err(format!(
                "APT frame {frame} outside character {}",
                character.id
            ));
        }
        let frame_count = character.frames.len();
        let actions: Vec<_> = character.frames[frame]
            .controls
            .iter()
            .filter(|c| c.type_name == "do_action" && c.actions_offset != 0)
            .map(|c| c.actions_offset)
            .collect();
        let list = self.states[&instance.character][frame].clone();
        let mut children = BTreeMap::new();
        for (depth, placement) in list.depths {
            let previous = instance.children.get(&depth).copied();
            if let Some(old) =
                previous.filter(|old| self.instances.get(old).is_some_and(|i| i.attached))
            {
                children.insert(depth, old);
                continue;
            }
            if let Some(name) = previous
                .and_then(|old| self.instances.get(&old))
                .and_then(|i| i.placement.as_ref())
                .map(|p| p.name.clone())
            {
                if !name.is_empty() && name != placement.name {
                    vm.objects[id].fields.remove(&name);
                }
            }
            let mut pending_start = None;
            let child = if let Some(old) = previous.filter(|old| {
                self.instances
                    .get(old)
                    .is_some_and(|i| i.character == placement.character)
            }) {
                old
            } else {
                if let Some(old) = previous {
                    self.remove(vm, old);
                }
                pending_start = Some(self.pending.len());
                self.create(vm, placement.character, Some(id), nesting + 1)?
            };
            let old = self.instances[&child].placement.as_ref();
            if old.is_none_or(|old| old.matrix != placement.matrix) {
                vm.set(child, "_x", Value::Number(placement.matrix[4] as f64))?;
                vm.set(child, "_y", Value::Number(placement.matrix[5] as f64))?;
            }
            if !placement.name.is_empty() {
                vm.set(id, &placement.name, Value::Object(child))?;
            }
            vm.set(child, "_name", Value::Text(placement.name.clone()))?;
            if let Some(pending_start) = pending_start {
                let mut initialization = VecDeque::new();
                for event in &placement.clip_events {
                    if event.flags & 0x40000 != 0 {
                        initialization.push_back((child, event.actions_offset));
                    }
                }
                let tail = self.pending.split_off(pending_start);
                self.pending.append(&mut initialization);
                self.pending.extend(tail);
                for event in &placement.clip_events {
                    if event.flags & 0x80 != 0 {
                        self.pending.push_back((child, event.actions_offset));
                    }
                }
            }
            self.instances.get_mut(&child).unwrap().placement = Some(placement);
            children.insert(depth, child);
        }
        for (depth, child) in &instance.children {
            if !children.contains_key(depth) {
                if self.instances.get(child).is_some_and(|i| i.attached) {
                    children.insert(*depth, *child);
                    continue;
                }
                if let Some(name) = self
                    .instances
                    .get(child)
                    .and_then(|i| i.placement.as_ref())
                    .map(|p| p.name.clone())
                {
                    if !name.is_empty() {
                        vm.objects[id].fields.remove(&name);
                    }
                }
                self.remove(vm, *child);
            }
        }
        let current = self.instances.get_mut(&id).unwrap();
        current.frame = frame;
        current.children = children;
        vm.set(id, "_currentframe", Value::Number((frame + 1) as f64))?;
        vm.set(id, "_totalframes", Value::Number(frame_count as f64))?;
        for offset in actions {
            self.pending.push_back((id, offset));
        }
        Ok(())
    }
    pub fn advance(&mut self, vm: &mut Vm) -> Result<(), String> {
        let playing: Vec<_> = self
            .instances
            .iter()
            .filter(|(_, i)| i.playing)
            .map(|(id, i)| (*id, i.character, i.frame))
            .collect();
        for (id, character, frame) in playing {
            if !self.instances.contains_key(&id) {
                continue;
            }
            let count = self.characters[&character].frames.len();
            if count > 1 {
                self.seek(vm, id, (frame + 1) % count, 0)?;
            }
        }
        Ok(())
    }
    pub fn method(
        &mut self,
        vm: &mut Vm,
        id: usize,
        method: &str,
        args: &[Value],
    ) -> Result<bool, String> {
        if !self.instances.contains_key(&id) {
            return Ok(false);
        }
        match method {
            "setTextFormat" => {
                let format = args.first().ok_or("Missing text format")?;
                if !matches!(format, Value::Object(_)) {
                    return Err("Invalid text format".into());
                }
                vm.set(id, "__textFormat", format.clone())?;
                self.text_changed(vm, id)?;
            }
            "removeMovieClip" => {
                if id != self.root {
                    if let Value::Object(parent) = vm.get(id, "_parent") {
                        if let Some(instance) = self.instances.get_mut(&parent) {
                            instance.children.retain(|_, child| *child != id);
                        }
                        let name = vm.get(id, "_name").text();
                        if vm.get(parent, &name) == Value::Object(id) {
                            vm.objects[parent].fields.remove(&name);
                        }
                    }
                    self.remove(vm, id);
                    self.pending
                        .retain(|(object, _)| self.instances.contains_key(object));
                }
            }
            "stop" => self.instances.get_mut(&id).unwrap().playing = false,
            "play" => self.instances.get_mut(&id).unwrap().playing = true,
            "gotoAndPlay" | "gotoAndStop" => {
                let c = &self.characters[&self.instances[&id].character];
                let frame = match args.first().ok_or("APT goto requires frame")? {
                    Value::Text(label) => c
                        .frames
                        .iter()
                        .position(|f| {
                            f.controls.iter().any(|control| {
                                control.type_name == "frame_label"
                                    && control.label.as_deref() == Some(label)
                            })
                        })
                        .ok_or_else(|| format!("APT character {} lacks label {label}", c.id))?,
                    value => {
                        let frame = value.number();
                        if !frame.is_finite() || frame < 0.0 {
                            return Err(format!(
                                "Invalid APT frame {frame} in {} on character {}",
                                method, c.id
                            ));
                        }
                        (frame as usize).saturating_sub(1)
                    }
                };
                self.seek(vm, id, frame, 0)?;
                self.instances.get_mut(&id).unwrap().playing = method == "gotoAndPlay";
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
