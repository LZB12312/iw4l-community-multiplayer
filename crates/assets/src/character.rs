use crate::bot_model::{BotJoint, BotModel, BotSurface, BotTexture, BotVertex};
use crate::skate_board::{Glb, index};
use bevy::{
    asset::RenderAssetUsages,
    prelude::{Image, Mat4, Vec3},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use serde::Deserialize;
use sim::character::{
    CharacterAsset, CharacterPart, CharacterProfile, CharacterScalar, CharacterSlot,
};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Deserialize)]
pub struct CharacterModelInfo {
    pub slot: String,
    pub name: String,
    pub flags: BTreeMap<String, String>,
    pub groups: Vec<Vec<CharacterAsset>>,
    scene: String,
}

#[derive(Deserialize)]
pub struct CharacterMaterialInfo {
    pub name: String,
    pub flags: BTreeMap<String, String>,
    diffuse: String,
    #[serde(default)]
    opacity: Option<String>,
    tint: [f32; 3],
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    models: BTreeMap<String, CharacterModelInfo>,
    materials: BTreeMap<String, CharacterMaterialInfo>,
    morphs: Vec<String>,
    #[serde(default)]
    defaults: BTreeMap<String, PreparedProfile>,
}

#[derive(Deserialize)]
struct PreparedProfile {
    selections: BTreeMap<String, PreparedSelection>,
    truck: f32,
    wheel: f32,
    posture: u32,
}

#[derive(Deserialize)]
struct PreparedSelection {
    asset_id: CharacterAsset,
    material_id: CharacterAsset,
}

#[derive(Clone, Copy, Deserialize)]
pub struct MorphRange {
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Deserialize)]
struct MorphParameter {
    target: String,
    branch_a: MorphRange,
    branch_b: MorphRange,
}

#[derive(Deserialize)]
struct NativeParameters {
    version: u32,
    morphs: Vec<MorphParameter>,
    collections: Vec<NativeCollection>,
}

#[derive(Deserialize)]
struct NativeCollection {
    #[serde(rename = "class")]
    class: String,
    key: String,
    parent: String,
    fields: BTreeMap<String, serde_json::Value>,
}

pub struct CharacterColour {
    pub key: String,
    pub swatch: [f32; 3],
    material: String,
}

type CachedCharacter = Result<Arc<Vec<CharacterMeshPart>>, String>;
type CharacterMeshCache = VecDeque<(CharacterProfile, CachedCharacter)>;

pub struct CharacterLibrary {
    root: PathBuf,
    manifest: Manifest,
    textures: Mutex<BTreeMap<String, std::sync::Weak<BotTexture>>>,
    meshes: Mutex<CharacterMeshCache>,
    parameters: OnceLock<Result<NativeParameters, String>>,
}

pub struct CharacterMeshPart {
    pub slot: CharacterSlot,
    pub asset: CharacterAsset,
    pub materials: Vec<CharacterAsset>,
    pub native: BotModel,
    pub walking: BotModel,
    xray: OnceLock<BotModel>,
}

impl CharacterMeshPart {
    pub fn board(&self) -> bool {
        matches!(
            self.slot,
            CharacterSlot::Deck | CharacterSlot::Trucks | CharacterSlot::Wheels
        )
    }
    pub fn xray(&self) -> &BotModel {
        self.xray.get_or_init(|| {
            let mut model = self.native.clone();
            for surface in &mut model.surfaces {
                surface.material.push_str("/xray_body");
            }
            model
        })
    }
    pub fn texture_keys(&self) -> impl Iterator<Item = &str> {
        self.native
            .surfaces
            .iter()
            .filter_map(|surface| self.native.textures[surface.texture].image_key.as_deref())
    }
    pub fn texture_image(&self, key: &str) -> Option<Image> {
        let tex = self
            .native
            .textures
            .iter()
            .find(|t| t.image_key.as_deref() == Some(key))?;
        Some(Image::new(
            Extent3d {
                width: u32::from(tex.width),
                height: u32::from(tex.height),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            tex.rgba.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ))
    }
}

pub fn local_library() -> Option<&'static CharacterLibrary> {
    static LIBRARY: OnceLock<Option<CharacterLibrary>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let root = std::env::var_os("IW4L_SKATE_ASSETS")?;
            CharacterLibrary::load(Path::new(&root))
                .map_err(|error| diag::warn!(World, "character library: {error}"))
                .ok()
        })
        .as_ref()
}

impl CharacterLibrary {
    pub fn load(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let bytes = std::fs::read(root.join("private/customisation/library.json"))
            .map_err(|e| e.to_string())?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if manifest.version != 3 || manifest.models.is_empty() || manifest.materials.is_empty() {
            return Err("unsupported character library".into());
        }
        Ok(Self {
            root,
            manifest,
            textures: Mutex::new(BTreeMap::new()),
            meshes: Mutex::new(VecDeque::new()),
            parameters: OnceLock::new(),
        })
    }

    pub fn models(&self) -> &BTreeMap<String, CharacterModelInfo> {
        &self.manifest.models
    }
    pub fn materials(&self) -> &BTreeMap<String, CharacterMaterialInfo> {
        &self.manifest.materials
    }

    fn parameters(&self) -> Result<&NativeParameters, String> {
        self.parameters
            .get_or_init(|| {
                let bytes = std::fs::read(self.path("private/customisation/native.json")?)
                    .map_err(|e| e.to_string())?;
                let data: NativeParameters =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if data.version != 1
                    || data.morphs.is_empty()
                    || data.morphs.iter().any(|m| {
                        !self.manifest.morphs.contains(&m.target)
                            || [m.branch_a, m.branch_b].into_iter().any(|r| {
                                !r.min.is_finite()
                                    || !r.max.is_finite()
                                    || !r.default.is_finite()
                                    || r.min < 0.
                                    || r.max > 1.
                                    || r.min > r.max
                                    || !(r.min..=r.max).contains(&r.default)
                            })
                    })
                {
                    return Err("invalid character parameter catalogue".into());
                }
                Ok(data)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn default_profile(&self, male: bool) -> Result<CharacterProfile, String> {
        let preset = self
            .manifest
            .defaults
            .get(if male { "male" } else { "female" })
            .ok_or("missing character default")?;
        let mut profile = CharacterProfile {
            male,
            skin_tint: [CharacterScalar::ONE; 3],
            hair_tint: [CharacterScalar::ONE; 3],
            trucks: CharacterScalar::new(preset.truck).ok_or("invalid default trucks")?,
            wheels: CharacterScalar::new(preset.wheel).ok_or("invalid default wheels")?,
            posture: CharacterAsset(u64::from(preset.posture)),
            ..CharacterProfile::default()
        };
        for (slot, name) in slots() {
            if let Some(selection) = preset.selections.get(name) {
                let model = self
                    .manifest
                    .models
                    .get(&String::from(selection.asset_id))
                    .ok_or("missing default character model")?;
                let materials = model
                    .groups
                    .iter()
                    .map(|group| {
                        if group.contains(&selection.material_id) {
                            Ok(selection.material_id)
                        } else {
                            group.first().copied().ok_or("empty default material group")
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                profile.parts[slot as usize] = CharacterPart {
                    model: selection.asset_id,
                    materials,
                    ..CharacterPart::default()
                };
            }
        }
        for name in &self.manifest.morphs {
            profile
                .morphs
                .insert(name.clone(), CharacterScalar::default());
        }
        for parameter in &self.parameters()?.morphs {
            profile.morphs.insert(
                parameter.target.clone(),
                CharacterScalar::new(parameter.branch_a.default).ok_or("invalid default morph")?,
            );
        }
        self.validate(&profile)?;
        Ok(profile)
    }

    pub fn morph_range(&self, name: &str) -> Result<MorphRange, String> {
        let parameter = self
            .parameters()?
            .morphs
            .iter()
            .find(|p| p.target == name)
            .ok_or("unknown editable character morph")?;
        if parameter.branch_a.min != parameter.branch_b.min
            || parameter.branch_a.max != parameter.branch_b.max
        {
            return Err("character morph has differing parameter ranges".into());
        }
        Ok(parameter.branch_a)
    }

    pub fn with_model(
        &self,
        profile: &CharacterProfile,
        slot: CharacterSlot,
        asset: CharacterAsset,
    ) -> Result<CharacterProfile, String> {
        let mut next = profile.clone();
        let part = &mut next.parts[slot as usize];
        if asset.0 == 0 {
            *part = CharacterPart::default();
        } else {
            let model = self
                .manifest
                .models
                .get(&String::from(asset))
                .ok_or("unknown character model")?;
            let materials = model
                .groups
                .iter()
                .enumerate()
                .map(|(index, group)| {
                    part.materials
                        .get(index)
                        .copied()
                        .filter(|id| group.contains(id))
                        .or_else(|| group.first().copied())
                        .ok_or("empty character material group")
                })
                .collect::<Result<Vec<_>, _>>()?;
            part.model = asset;
            part.materials = materials;
        }
        self.validate(&next)?;
        Ok(next)
    }

    pub fn with_material(
        &self,
        profile: &CharacterProfile,
        slot: CharacterSlot,
        group: usize,
        asset: CharacterAsset,
    ) -> Result<CharacterProfile, String> {
        let mut next = profile.clone();
        let material = next.parts[slot as usize]
            .materials
            .get_mut(group)
            .ok_or("unknown character material group")?;
        *material = asset;
        self.validate(&next)?;
        Ok(next)
    }

    pub fn with_morph(
        &self,
        profile: &CharacterProfile,
        name: &str,
        value: f32,
    ) -> Result<CharacterProfile, String> {
        let range = self.morph_range(name)?;
        if !value.is_finite() || !(range.min..=range.max).contains(&value) {
            return Err("character morph is outside its range".into());
        }
        let mut next = profile.clone();
        next.morphs.insert(
            name.into(),
            CharacterScalar::new(value).ok_or("invalid character morph")?,
        );
        self.validate(&next)?;
        Ok(next)
    }

    pub fn with_saved_morphs(
        &self,
        profile: &CharacterProfile,
        saved: &CharacterProfile,
    ) -> Result<CharacterProfile, String> {
        self.validate(profile)?;
        self.validate(saved)?;
        if profile.male != saved.male {
            return Err("Character snapshot has a different gender".into());
        }
        let mut next = profile.clone();
        for parameter in &self.parameters()?.morphs {
            let value = saved
                .morphs
                .get(&parameter.target)
                .ok_or("Character snapshot is missing a morph")?;
            let range = self.morph_range(&parameter.target)?;
            next.morphs.insert(
                parameter.target.clone(),
                CharacterScalar::new(value.value().clamp(range.min, range.max))
                    .ok_or("Invalid character snapshot morph")?,
            );
        }
        self.validate(&next)?;
        Ok(next)
    }

    pub fn eye_colours(&self) -> Result<Vec<CharacterColour>, String> {
        let mut colours = Vec::new();
        for row in &self.parameters()?.collections {
            if row.class != "cac_colour" || row.parent != "default_eye" {
                continue;
            }
            let data = |name: &str, kind: &str| -> Result<&str, String> {
                let field = row
                    .fields
                    .get(name)
                    .ok_or("Missing character colour field")?;
                if field["type"].as_str() != Some(kind) {
                    return Err("Invalid character colour field type".into());
                }
                field["data"]
                    .as_str()
                    .ok_or("Missing character colour data".into())
            };
            let order = data("Hash_9C2DC17266699DD9", "EA::Reflection::Int32")?;
            if order.len() != 8 {
                return Err("Invalid character colour order".into());
            }
            let order = u32::from_str_radix(order, 16)
                .map_err(|_| "Invalid character colour order")? as i32;
            let vector = data("Hash_7827ED970A88B70C", "Math::Vector4")?;
            if !vector.is_ascii() || vector.len() != 32 {
                return Err("Invalid character colour swatch".into());
            }
            let mut swatch = [0.; 3];
            for (i, component) in swatch.iter_mut().enumerate() {
                *component = f32::from_bits(
                    u32::from_str_radix(&vector[i * 8..i * 8 + 8], 16)
                        .map_err(|_| "Invalid character colour swatch")?,
                );
                if !component.is_finite() || !(0. ..=1.).contains(component) {
                    return Err("Character colour swatch is outside its range".into());
                }
            }
            let material = data("Hash_AB0E65FA2F485ED8", "EA::Reflection::Text")?.to_owned();
            if material.is_empty()
                || row.key.is_empty()
                || row.key.len() > 64
                || !row
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err("Invalid character colour identity".into());
            }
            colours.push((
                order,
                CharacterColour {
                    key: row.key.clone(),
                    swatch,
                    material,
                },
            ));
        }
        colours.sort_by_key(|(order, _)| *order);
        if colours.is_empty() || colours.len() > 64 || colours.windows(2).any(|w| w[0].0 == w[1].0)
        {
            return Err("Invalid character eye palette".into());
        }
        Ok(colours.into_iter().map(|(_, colour)| colour).collect())
    }

    pub fn eye_colour_index(&self, profile: &CharacterProfile) -> Result<usize, String> {
        self.validate(profile)?;
        let part = &profile.parts[CharacterSlot::Eyes as usize];
        let colour = part
            .materials
            .iter()
            .find_map(|id| {
                self.manifest
                    .materials
                    .get(&String::from(*id))?
                    .flags
                    .get("EyeColour")
            })
            .ok_or("Character has no eye colour material")?;
        self.eye_colours()?
            .iter()
            .position(|entry| &entry.material == colour)
            .ok_or("Character eye colour is outside its palette".into())
    }

    pub fn with_eye_colour(
        &self,
        profile: &CharacterProfile,
        key: &str,
    ) -> Result<CharacterProfile, String> {
        self.validate(profile)?;
        let colour = self
            .eye_colours()?
            .into_iter()
            .find(|c| c.key == key)
            .ok_or("Unknown character eye colour")?;
        let mut next = profile.clone();
        let part = &mut next.parts[CharacterSlot::Eyes as usize];
        let model = self
            .manifest
            .models
            .get(&String::from(part.model))
            .ok_or("Missing character eye model")?;
        let mut changed = false;
        for (group, material) in model.groups.iter().zip(&mut part.materials) {
            let current = self
                .manifest
                .materials
                .get(&String::from(*material))
                .ok_or("Missing character eye material")?;
            if !current.flags.contains_key("EyeColour") {
                continue;
            }
            *material = group
                .iter()
                .copied()
                .find(|id| {
                    self.manifest
                        .materials
                        .get(&String::from(*id))
                        .is_some_and(|info| {
                            info.flags.get("EyeColour") == Some(&colour.material)
                                && current
                                    .flags
                                    .iter()
                                    .filter(|(name, _)| {
                                        !matches!(name.as_str(), "EyeColour" | "IsDefault")
                                    })
                                    .all(|(name, value)| info.flags.get(name) == Some(value))
                        })
                })
                .ok_or("Eye colour has no compatible character material")?;
            changed = true;
        }
        if !changed {
            return Err("Character has no eye colour material group".into());
        }
        part.palette = Some(colour.key);
        self.validate(&next)?;
        Ok(next)
    }

    pub fn slot(name: &str) -> Option<CharacterSlot> {
        slots()
            .into_iter()
            .find(|(_, key)| key.eq_ignore_ascii_case(name))
            .map(|(slot, _)| slot)
    }

    pub fn material_names(&self) -> impl Iterator<Item = String> + '_ {
        self.manifest.materials.keys().flat_map(|id| {
            let name = format!("iw4l_character_library/{id}");
            [name.clone(), format!("{name}/xray_body")]
        })
    }

    pub fn material_image_key(&self, id: &str) -> Option<String> {
        let info = self.manifest.materials.get(id)?;
        Some(image_key(info))
    }

    pub(crate) fn material_placeholders(&self) -> BotModel {
        let mut model = BotModel {
            lighting_gain: 1.,
            joints: Vec::new(),
            surfaces: Vec::new(),
            textures: Vec::new(),
        };
        let mut variants = BTreeMap::new();
        for (id, info) in &self.manifest.materials {
            let key = image_key(info);
            let texture = *variants.entry(key.clone()).or_insert_with(|| {
                let index = model.textures.len();
                model.textures.push(Arc::new(BotTexture {
                    image_key: Some(key),
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                }));
                index
            });
            let name = format!("iw4l_character_library/{id}");
            for material in [name.clone(), format!("{name}/xray_body")] {
                model.surfaces.push(BotSurface {
                    material,
                    texture,
                    vertices: Vec::new(),
                    indices: Vec::new(),
                });
            }
        }
        model
    }

    /// Reuses both mode meshes while a profile is unchanged. A small LRU also
    /// remembers failures so unavailable local assets are not retried per frame.
    pub fn assemble_cached(&self, profile: &CharacterProfile) -> CachedCharacter {
        const CAPACITY: usize = 32;
        let mut cache = self
            .meshes
            .lock()
            .map_err(|_| "character mesh cache unavailable")?;
        if let Some(index) = cache.iter().position(|(key, _)| key == profile) {
            let entry = cache.remove(index).expect("cached character index");
            let result = entry.1.clone();
            cache.push_back(entry);
            return result;
        }
        let result = self.assemble(profile).map(Arc::new);
        cache.push_back((profile.clone(), result.clone()));
        while cache.len() > CAPACITY {
            cache.pop_front();
        }
        result
    }

    pub fn validate(&self, profile: &CharacterProfile) -> Result<(), String> {
        if !profile.valid()
            || profile
                .morphs
                .keys()
                .any(|name| !self.manifest.morphs.contains(name))
        {
            return Err("invalid character profile for this library".into());
        }
        for (i, (_, name)) in slots().into_iter().enumerate() {
            let selected = &profile.parts[i];
            if selected.model.0 == 0 {
                continue;
            }
            let key = String::from(selected.model);
            let model = self
                .manifest
                .models
                .get(&key)
                .ok_or("unknown character model")?;
            if model.slot != name
                || model.groups.len() != selected.materials.len()
                || model
                    .groups
                    .iter()
                    .zip(&selected.materials)
                    .any(|(group, id)| !group.contains(id))
                || selected
                    .materials
                    .iter()
                    .any(|id| !self.manifest.materials.contains_key(&String::from(*id)))
            {
                return Err(format!("invalid character material or slot: {name}"));
            }
            let gender = model.flags.get("Gender").map(String::as_str).unwrap_or("");
            if !matches!(gender, "" | "unisex")
                && gender != if profile.male { "male" } else { "female" }
            {
                return Err(format!("character gender does not fit {name}"));
            }
        }
        Ok(())
    }

    pub fn assemble(&self, profile: &CharacterProfile) -> Result<Vec<CharacterMeshPart>, String> {
        self.validate(profile)?;
        let mut parts = Vec::new();
        for (i, (slot, _)) in slots().into_iter().enumerate() {
            let selected = &profile.parts[i];
            if selected.model.0 == 0 {
                continue;
            }
            let model = &self.manifest.models[&String::from(selected.model)];
            let bytes = std::fs::read(self.path(&model.scene)?).map_err(|e| e.to_string())?;
            let glb = Glb::parse(&bytes)?;
            let mut native = geometry(&glb, profile, &selected.materials)?;
            for surface in &mut native.surfaces {
                let material = selected
                    .materials
                    .get(surface.texture)
                    .ok_or("missing material group")?;
                surface.material = format!(
                    "iw4l_character_library/{material_id}",
                    material_id = String::from(*material)
                );
            }
            for material in &selected.materials {
                let info = self
                    .manifest
                    .materials
                    .get(&String::from(*material))
                    .ok_or("unknown character material")?;
                native.textures.push(self.texture(info)?);
            }
            crate::bot_model::validate(&native)?;
            let walking = walking(&native, &glb)?;
            crate::bot_model::validate(&walking)?;
            parts.push(CharacterMeshPart {
                slot,
                asset: selected.model,
                materials: selected.materials.clone(),
                native,
                walking,
                xray: OnceLock::new(),
            });
        }
        Ok(parts)
    }

    fn texture(&self, info: &CharacterMaterialInfo) -> Result<Arc<BotTexture>, String> {
        let key = serde_json::to_string(&(&info.diffuse, &info.opacity, info.tint))
            .map_err(|e| e.to_string())?;
        let mut cache = self
            .textures
            .lock()
            .map_err(|_| "character texture cache unavailable")?;
        if let Some(texture) = cache.get(&key).and_then(std::sync::Weak::upgrade) {
            return Ok(texture);
        }
        // Weak references let an evicted profile release its decoded pixels.
        cache.retain(|_, texture| texture.strong_count() != 0);
        let opacity = info.opacity.as_deref().map(|p| self.path(p)).transpose()?;
        let mut decoded = texture(&self.path(&info.diffuse)?, opacity.as_deref(), info.tint)?;
        decoded.image_key = Some(image_key(info));
        let texture = Arc::new(decoded);
        cache.insert(key, Arc::downgrade(&texture));
        Ok(texture)
    }

    fn path(&self, relative: &str) -> Result<PathBuf, String> {
        let relative = Path::new(relative);
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err("character asset path is not relative".into());
        }
        let path = self
            .root
            .join(relative)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(&self.root) {
            return Err("character asset leaves its library".into());
        }
        Ok(path)
    }
}

fn slots() -> [(CharacterSlot, &'static str); 18] {
    use CharacterSlot::*;
    [
        (Arm, "Arm"),
        (Leg, "Leg"),
        (Hair, "Hair"),
        (Face, "Rostral"),
        (Feet, "Feet"),
        (Accessory, "Accessory"),
        (Eyes, "Organ"),
        (Deck, "SkateBoard"),
        (Trucks, "SkateTruck"),
        (Wheels, "SkateWheel"),
        (Pants, "Pants"),
        (Socks, "Sock"),
        (Hat, "Hat"),
        (Glasses, "Glasses"),
        (InnerTop, "InnerTorso"),
        (OuterTop, "OuterTorso"),
        (Jewellery, "Jewellery"),
        (Wrist, "WristItem"),
    ]
}

fn image_key(info: &CharacterMaterialInfo) -> String {
    let source = serde_json::to_vec(&(&info.diffuse, &info.opacity, info.tint))
        .expect("finite library image metadata");
    format!("iw4l_character_image/{:016x}", crate::fnv1a64(&source))
}

fn geometry(
    glb: &Glb<'_>,
    profile: &CharacterProfile,
    materials: &[CharacterAsset],
) -> Result<BotModel, String> {
    let skin = &glb.json["skins"][0];
    let inverse = glb.floats(index(&skin["inverseBindMatrices"])?)?;
    let joint_nodes = skin["joints"].as_array().ok_or("character has no joints")?;
    if inverse.len() != joint_nodes.len() * 16
        || joint_nodes.is_empty()
        || joint_nodes.len() > sim::presentation::MAX_SKATE_BONES
    {
        return Err("invalid character bind matrices".into());
    }
    let mut joints = Vec::new();
    for (i, node) in joint_nodes.iter().enumerate() {
        let name = glb.json["nodes"][index(node)?]["name"]
            .as_str()
            .ok_or("character joint has no name")?;
        let bind: [f32; 16] = inverse[i * 16..(i + 1) * 16]
            .try_into()
            .map_err(|_| "missing bind matrix")?;
        let matrix = Mat4::from_cols_array(&bind);
        if !matrix.is_finite() || matrix.determinant().abs() < 1e-10 {
            return Err("invalid character bind matrix".into());
        }
        joints.push(BotJoint {
            target: name.into(),
            origin: [0.; 3],
            end: None,
            target_child: None,
            inverse_bind: Some(bind),
        });
    }
    let mut surfaces = Vec::new();
    for mesh in glb.json["meshes"]
        .as_array()
        .ok_or("character has no meshes")?
    {
        let targets = mesh["extras"]["targetNames"]
            .as_array()
            .ok_or("character has no morph names")?;
        for primitive in mesh["primitives"]
            .as_array()
            .ok_or("character has no primitives")?
        {
            if primitive["mode"].as_u64().unwrap_or(4) != 4 {
                return Err("character primitive is not triangles".into());
            }
            let attributes = &primitive["attributes"];
            let mut positions = glb.floats(index(&attributes["POSITION"])?)?;
            let mut normals = glb.floats(index(&attributes["NORMAL"])?)?;
            let uv = glb.floats(index(&attributes["TEXCOORD_0"])?)?;
            let uv2 = attributes
                .get("TEXCOORD_1")
                .map(|value| glb.floats(index(value)?))
                .transpose()?;
            let ids = glb.integers(index(&attributes["JOINTS_0"])?)?;
            let weights = glb.floats(index(&attributes["WEIGHTS_0"])?)?;
            let count = positions.len() / 3;
            if count == 0
                || positions.len() != count * 3
                || normals.len() != count * 3
                || uv.len() != count * 2
                || ids.len() != count * 4
                || weights.len() != count * 4
                || uv2.as_ref().is_some_and(|uv| uv.len() != count * 2)
            {
                return Err("character vertex streams disagree".into());
            }
            let deltas = primitive["targets"]
                .as_array()
                .ok_or("character has no morph streams")?;
            if targets.len() != deltas.len() {
                return Err("character morph streams disagree".into());
            }
            for (i, (name, delta)) in targets.iter().zip(deltas).enumerate() {
                let name = name.as_str().ok_or("invalid morph name")?;
                let value = profile
                    .morphs
                    .get(name)
                    .map(|v| v.value())
                    .unwrap_or(mesh["weights"][i].as_f64().unwrap_or(0.) as f32);
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err("invalid morph weight".into());
                }
                if value == 0. {
                    continue;
                }
                let pos = glb.floats(index(&delta["POSITION"])?)?;
                let normal = glb.floats(index(&delta["NORMAL"])?)?;
                if pos.len() != positions.len() || normal.len() != normals.len() {
                    return Err("invalid morph stream length".into());
                }
                for (v, d) in positions.iter_mut().zip(pos) {
                    *v += d * value;
                }
                for (v, d) in normals.iter_mut().zip(normal) {
                    *v += d * value;
                }
            }
            let group = index(&primitive["material"])?;
            if group >= materials.len() {
                return Err("missing character primitive material".into());
            }
            let vertices = (0..count)
                .map(|i| {
                    let normal = Vec3::from_slice(&normals[i * 3..i * 3 + 3]).normalize_or(Vec3::Y);
                    let u = half::f16::from_f32(uv[i * 2]).to_bits();
                    let v = half::f16::from_f32(uv[i * 2 + 1]).to_bits();
                    BotVertex {
                        position: positions[i * 3..i * 3 + 3]
                            .try_into()
                            .expect("vertex width"),
                        normal: normal.to_array(),
                        uv: (u32::from(u) << 16) | u32::from(v),
                        uv2: uv2.as_ref().map(|uv| [uv[i * 2], uv[i * 2 + 1]]),
                        joints: std::array::from_fn(|j| ids[i * 4 + j] as usize),
                        weights: std::array::from_fn(|j| weights[i * 4 + j]),
                    }
                })
                .collect();
            let indices = glb.integers(index(&primitive["indices"])?)?;
            if indices.len() % 3 != 0 {
                return Err("invalid character triangles".into());
            }
            surfaces.push(BotSurface {
                material: String::new(),
                texture: group,
                vertices,
                indices: indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|t| [t[0], t[2], t[1]])
                    .collect(),
            });
        }
    }
    Ok(BotModel {
        lighting_gain: 1.,
        joints,
        surfaces,
        textures: Vec::new(),
    })
}

fn target(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match name.trim_end_matches("_REPARENTED") {
        "HIPS" => ("j_mainroot", Some("SPINE")),
        "SPINE" => ("j_spinelower", Some("SPINE1")),
        "SPINE1" | "SPINE2" => ("j_spineupper", Some("SPINE3")),
        "SPINE3" => ("j_spine4", Some("NECK")),
        "NECK" | "NECK1" => ("j_neck", Some("HEAD")),
        "HEAD" => ("j_head", None),
        "LEFTSHOULDER" => ("j_clavicle_le", Some("LEFTARM")),
        "LEFTARM" => ("j_shoulder_le", Some("LEFTFOREARM")),
        "LEFTFOREARM" => ("j_elbow_le", Some("LEFTHAND")),
        "LEFTHAND" => ("j_wrist_le", None),
        "RIGHTSHOULDER" => ("j_clavicle_ri", Some("RIGHTARM")),
        "RIGHTARM" => ("j_shoulder_ri", Some("RIGHTFOREARM")),
        "RIGHTFOREARM" => ("j_elbow_ri", Some("RIGHTHAND")),
        "RIGHTHAND" => ("j_wrist_ri", None),
        "LEFTUPLEG" => ("j_hip_le", Some("LEFTLEG")),
        "LEFTLEG" => ("j_knee_le", Some("LEFTFOOT")),
        "LEFTFOOT" => ("j_ankle_le", Some("LEFTTOEBASE")),
        "LEFTTOEBASE" => ("j_ball_le", None),
        "RIGHTUPLEG" => ("j_hip_ri", Some("RIGHTLEG")),
        "RIGHTLEG" => ("j_knee_ri", Some("RIGHTFOOT")),
        "RIGHTFOOT" => ("j_ankle_ri", Some("RIGHTTOEBASE")),
        "RIGHTTOEBASE" => ("j_ball_ri", None),
        _ => return None,
    })
}
fn walking(native: &BotModel, glb: &Glb<'_>) -> Result<BotModel, String> {
    let basis = |v: Vec3| Vec3::new(v.z, v.x, v.y);
    let mut model = native.clone();
    let nodes = glb.json["nodes"].as_array().ok_or("missing joint nodes")?;
    let mut parents = vec![None; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                let child = index(child)?;
                let p = parents.get_mut(child).ok_or("invalid joint parent")?;
                if p.replace(parent).is_some() {
                    return Err("multiple joint parents".into());
                }
            }
        }
    }
    let bind_origin = |j: &BotJoint| -> Result<Vec3, String> {
        let bind = j.inverse_bind.as_ref().ok_or("missing native bind")?;
        Ok(basis(Mat4::from_cols_array(bind).inverse().w_axis.truncate()) / 0.0254)
    };
    for (i, joint) in model.joints.iter_mut().enumerate() {
        let source = &native.joints[i];
        let mut node = index(&glb.json["skins"][0]["joints"][i])?;
        let mut mapped = target(&source.target);
        let mut depth = 0;
        while mapped.is_none() {
            depth += 1;
            if depth > nodes.len() {
                return Err("cyclic character skeleton".into());
            }
            let Some(parent) = parents[node] else {
                break;
            };
            node = parent;
            mapped = nodes[node]["name"].as_str().and_then(target);
        }
        let (name, child) = mapped.unwrap_or(("j_mainroot", None));
        let origin = glb.json["skins"][0]["joints"]
            .as_array()
            .and_then(|joints| joints.iter().position(|n| n.as_u64() == Some(node as u64)))
            .and_then(|i| native.joints.get(i))
            .unwrap_or(source);
        joint.target = name.into();
        joint.origin = bind_origin(origin)?.to_array();
        joint.inverse_bind = None;
        joint.end = None;
        joint.target_child = None;
        if let Some(child) = child.and_then(|name| {
            native
                .joints
                .iter()
                .find(|j| j.target.trim_end_matches("_REPARENTED") == name)
        }) && let Some((child_target, _)) = target(&child.target).filter(|(n, _)| *n != name)
        {
            joint.end = Some(bind_origin(child)?.to_array());
            joint.target_child = Some(child_target.into());
        }
    }
    for surface in &mut model.surfaces {
        for vertex in &mut surface.vertices {
            vertex.position = (basis(Vec3::from_array(vertex.position)) / 0.0254).to_array();
            vertex.normal = basis(Vec3::from_array(vertex.normal)).to_array();
        }
    }
    Ok(model)
}

fn texture(path: &Path, opacity: Option<&Path>, tint: [f32; 3]) -> Result<BotTexture, String> {
    if tint.iter().any(|v| !v.is_finite() || *v < 0. || *v > 4.) {
        return Err("invalid character material tint".into());
    }
    let mut diffuse = image(path)?;
    if let Some(path) = opacity {
        let mask = image(path)?;
        let has_alpha = mask.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
        let sample = |x: usize, y: usize| {
            let p = &mask.rgba[(y * usize::from(mask.width) + x) * 4..];
            if has_alpha {
                f32::from(p[3])
            } else {
                // The owned opacity textures may store their mask in RGB.
                ((299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2]) + 500)
                    / 1000) as f32
            }
        };
        for y in 0..usize::from(diffuse.height) {
            for x in 0..usize::from(diffuse.width) {
                let sx = ((x as f32 + 0.5) * f32::from(mask.width) / f32::from(diffuse.width)
                    - 0.5)
                    .clamp(0., f32::from(mask.width - 1));
                let sy = ((y as f32 + 0.5) * f32::from(mask.height) / f32::from(diffuse.height)
                    - 0.5)
                    .clamp(0., f32::from(mask.height - 1));
                let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                let (x1, y1) = (
                    (x0 + 1).min(usize::from(mask.width) - 1),
                    (y0 + 1).min(usize::from(mask.height) - 1),
                );
                let (dx, dy) = (sx - x0 as f32, sy - y0 as f32);
                let a = sample(x0, y0) * (1. - dx) + sample(x1, y0) * dx;
                let b = sample(x0, y1) * (1. - dx) + sample(x1, y1) * dx;
                let at = (y * usize::from(diffuse.width) + x) * 4 + 3;
                diffuse.rgba[at] =
                    (f32::from(diffuse.rgba[at]) * (a * (1. - dy) + b * dy) / 255.).floor() as u8;
            }
        }
    }
    for p in diffuse.rgba.as_chunks_mut::<4>().0 {
        for i in 0..3 {
            p[i] = (f32::from(p[i]) * tint[i]).round().clamp(0., 255.) as u8;
        }
    }
    Ok(diffuse)
}

fn image(path: &Path) -> Result<BotTexture, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("character image is too large")?
    ];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    let width = u16::try_from(frame.width).map_err(|_| "character texture is too wide")?;
    let height = u16::try_from(frame.height).map_err(|_| "character texture is too high")?;
    let channels = frame.color_type.samples();
    if width == 0 || height == 0 {
        return Err("empty character image".into());
    }
    let rgba = pixels[..frame.buffer_size()]
        .chunks_exact(channels)
        .flat_map(|p| match channels {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    Ok(BotTexture {
        image_key: None,
        width,
        height,
        rgba,
    })
}
