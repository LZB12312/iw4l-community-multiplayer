//! Optional local, preconverted character mesh. Assets remain outside the game archives.
use asset_core::AssetRef;
use asset_material::{AuthoredImage, MaterialCatalog};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use serde::Deserialize;
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Clone, Deserialize)]
pub struct BotModel {
    #[serde(default = "default_lighting_gain")]
    pub lighting_gain: f32,
    pub joints: Vec<BotJoint>,
    pub surfaces: Vec<BotSurface>,
    pub(crate) textures: Vec<Arc<BotTexture>>,
}
fn default_lighting_gain() -> f32 {
    1.0
}

#[derive(Clone, Deserialize)]
pub struct BotJoint {
    pub target: String,
    pub origin: [f32; 3],
    pub end: Option<[f32; 3]>,
    pub target_child: Option<String>,
    #[serde(default)]
    pub inverse_bind: Option<[f32; 16]>,
}
#[derive(Clone, Deserialize)]
pub struct BotSurface {
    pub material: String,
    pub texture: usize,
    pub vertices: Vec<BotVertex>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Deserialize)]
pub struct BotVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: u32,
    #[serde(default)]
    pub uv2: Option<[f32; 2]>,
    pub joints: [usize; 4],
    pub weights: [f32; 4],
}
#[derive(Clone, Deserialize)]
pub(crate) struct BotTexture {
    #[serde(default)]
    pub(crate) image_key: Option<String>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) rgba: Vec<u8>,
}

pub fn local_bot_model() -> Option<&'static BotModel> {
    static MODEL: OnceLock<Option<BotModel>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let path = std::env::var_os("IW4L_BOT_MODEL")?;
            match load(Path::new(&path)) {
                Ok(model) => Some(model),
                Err(error) => {
                    diag::info!(World, "local bot model: {error}");
                    None
                }
            }
        })
        .as_ref()
}

fn load(path: &Path) -> Result<BotModel, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let model: BotModel = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    validate(&model)?;
    Ok(model)
}

pub(crate) fn validate(model: &BotModel) -> Result<(), String> {
    if !model.lighting_gain.is_finite()
        || !(0.25..=4.0).contains(&model.lighting_gain)
        || model.joints.is_empty()
        || model.joints.iter().any(|j| {
            j.origin.iter().any(|v| !v.is_finite())
                || j.end.is_some_and(|end| end.iter().any(|v| !v.is_finite()))
                || j.inverse_bind.is_some_and(|bind| {
                    let matrix = Mat4::from_cols_array(&bind);
                    !matrix.is_finite() || matrix.determinant().abs() < 1e-10
                })
        })
        || model.surfaces.is_empty()
        || model.textures.iter().any(|t| {
            t.width == 0
                || t.height == 0
                || t.rgba.len() != usize::from(t.width) * usize::from(t.height) * 4
        })
        || model.surfaces.iter().any(|s| {
            s.texture >= model.textures.len()
                || s.indices.len() % 3 != 0
                || s.indices.iter().any(|&i| i as usize >= s.vertices.len())
                || s.vertices.iter().any(|v| {
                    v.joints.iter().any(|&j| j >= model.joints.len())
                        || v.uv2.is_some_and(|uv| uv.iter().any(|v| !v.is_finite()))
                        || v.position
                            .iter()
                            .chain(&v.normal)
                            .chain(&v.weights)
                            .any(|x| !x.is_finite())
                })
        })
    {
        return Err("invalid mesh, weights or texture dimensions".into());
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CharacterPart {
    pub category: String,
    pub label: String,
    pub index: u8,
    pub slots: Vec<String>,
    pub walking: BotModel,
    pub native: BotModel,
    #[serde(skip)]
    xray: OnceLock<BotModel>,
}

impl CharacterPart {
    pub fn xray(&self) -> &BotModel {
        self.xray.get_or_init(|| {
            let mut model = self.native.clone();
            for surface in &mut model.surfaces {
                surface.material.push_str("/xray_body");
            }
            model
        })
    }
}

pub fn local_characters() -> Option<&'static Vec<CharacterPart>> {
    static PARTS: OnceLock<Option<Vec<CharacterPart>>> = OnceLock::new();
    PARTS
        .get_or_init(|| {
            let root = std::env::var_os("IW4L_SKATE_ASSETS")?;
            let data = std::fs::read(Path::new(&root).join("characters.json")).ok()?;
            let parts: Vec<CharacterPart> = serde_json::from_slice(&data)
                .map_err(|e| diag::warn!(World, "characters: {e}"))
                .ok()?;
            for part in &parts {
                if part.index > 15
                    || validate(&part.native).is_err()
                    || validate(&part.walking).is_err()
                {
                    diag::warn!(World, "characters: invalid part {}", part.label);
                    return None;
                }
            }
            Some(parts)
        })
        .as_ref()
}

pub fn character_parts(selections: [u8; 5], native: bool) -> Vec<&'static BotModel> {
    let Some(parts) = local_characters() else {
        return Vec::new();
    };
    let index = |category: &str| match category {
        "skin" => selections[0],
        "shirt" => selections[1],
        "pants" => selections[2],
        "hair" => selections[3],
        "board" => selections[4],
        _ => 0,
    };
    let chosen: Vec<_> = parts
        .iter()
        .filter(|p| p.category != "base" && p.index == index(&p.category) && p.index != 0)
        .collect();
    let replaced: Vec<_> = chosen.iter().flat_map(|p| p.slots.iter()).collect();
    let mut models = Vec::new();
    for p in parts.iter().filter(|p| p.category == "base") {
        if !p.slots.iter().any(|s| replaced.contains(&s)) {
            models.push(if native { &p.native } else { &p.walking });
        }
    }
    models.extend(
        chosen
            .into_iter()
            .filter(|p| p.category != "board")
            .map(|p| if native { &p.native } else { &p.walking }),
    );
    models
}

pub fn character_option(category: &str, index: u8) -> Option<&'static CharacterPart> {
    local_characters()?
        .iter()
        .find(|p| p.category == category && p.index == index)
}

pub fn local_skate_board() -> Option<&'static BotModel> {
    static MODEL: OnceLock<Option<BotModel>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let root = std::env::var_os("IW4L_SKATE_ASSETS")?;
            load(&Path::new(&root).join("board.json"))
                .map_err(|e| diag::warn!(World, "skate board: {e}"))
                .ok()
        })
        .as_ref()
}

pub fn meat_skeleton() -> Option<&'static [BotModel; 2]> {
    static MODEL: OnceLock<Option<[BotModel; 2]>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let root = std::env::var_os("IW4L_SKATE_ASSETS")?;
            let normal = load(&Path::new(&root).join("skeleton.json"))
                .map_err(|e| diag::warn!(World, "Hall of Meat skeleton: {e}"))
                .ok()?;
            let mut injured = normal.clone();
            for surface in &mut injured.surfaces {
                surface.material.push_str("/injured");
            }
            for texture in &mut injured.textures {
                let texture = Arc::make_mut(texture);
                for pixel in texture.rgba.chunks_exact_mut(4) {
                    pixel[1] = (pixel[1] as f32 * 0.12) as u8;
                    pixel[2] = (pixel[2] as f32 * 0.12) as u8;
                }
            }
            Some([normal, injured])
        })
        .as_ref()
}

pub(crate) fn install_local_bot_materials(catalog: &mut MaterialCatalog) {
    for model in [local_bot_model(), local_skate_board()]
        .into_iter()
        .flatten()
    {
        install_materials(catalog, model);
    }
    if let Some(parts) = local_characters() {
        for part in parts {
            install_materials(catalog, &part.native);
            install_materials(catalog, part.xray());
        }
    }
    if let Some(models) = meat_skeleton() {
        for model in models {
            install_materials(catalog, model);
        }
    }
    if let Some(library) = crate::character::local_library() {
        install_materials(catalog, &library.material_placeholders());
    }
}

fn install_materials(catalog: &mut MaterialCatalog, model: &BotModel) {
    let Some(template) = catalog
        .materials
        .iter()
        .filter(|m| {
            m.name.as_str().starts_with("mc/")
                && !m.name.as_str().contains("gfx_")
                && !m.name.as_str().contains("fx_")
                && catalog.takes_model_lighting(m) == Some(true)
                && m.textures
                    .iter()
                    .any(|t| t.semantic == 2 && t.image.is_some())
        })
        .min_by_key(|m| {
            // Prefer a plain diffuse model shader. In particular, do not inherit
            // metallic reflection and tint settings from effects such as brass.
            let maps = m.textures.iter().filter(|t| t.semantic != 2).count();
            let character = ["body", "head", "skin", "soldier", "militia"]
                .iter()
                .any(|part| m.name.as_str().contains(part));
            (!character, maps, m.name.as_str().to_owned())
        })
        .cloned()
    else {
        return;
    };
    let image_template = template
        .textures
        .iter()
        .find(|t| t.semantic == 2)
        .and_then(|t| t.image)
        .and_then(|i| catalog.images.get(i))
        .cloned();
    let Some(image_template) = image_template else {
        return;
    };
    // IW4's packed normal format stores its neutral XY in alpha and green.
    // Neither the donor's normal map nor its metal specular map belongs to CJ.
    let mut neutral_maps = Vec::new();
    for (semantic, label, rgba) in [
        (5, "normal", [255, 130, 255, 130]),
        (8, "specular", [0, 0, 0, 255]),
    ] {
        let image = AuthoredImage {
            name: AssetRef::Real(format!("iw4l_bot_override/{label}")),
            semantic,
            width: 1,
            height: 1,
            depth: 1,
            level_count: 1,
            decoded: Some(Arc::new(Image::new(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba.to_vec(),
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::RENDER_WORLD,
            ))),
            payload: Arc::new(Vec::new()),
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
            common_owned: false,
            use_srgb_reads: false,
            ..image_template.clone()
        };
        neutral_maps.push((semantic, catalog.link_image(image)));
    }
    for surface in &model.surfaces {
        let tex = &model.textures[surface.texture];
        let decoded = Image::new(
            Extent3d {
                width: u32::from(tex.width),
                height: u32::from(tex.height),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            tex.rgba.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        let image = AuthoredImage {
            name: AssetRef::Real(
                tex.image_key
                    .clone()
                    .unwrap_or_else(|| format!("{}/diffuse", surface.material)),
            ),
            width: tex.width,
            height: tex.height,
            depth: 1,
            level_count: 1,
            decoded: Some(Arc::new(decoded)),
            payload: Arc::new(Vec::new()),
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
            common_owned: false,
            use_srgb_reads: true,
            ..image_template.clone()
        };
        let image_index = catalog.link_image(image);
        let mut material = template.clone();
        material.extended_sort = tex.image_key.is_some();
        material.name = AssetRef::Real(surface.material.clone());
        if surface.material.ends_with("/xray_body") {
            // The donor pixel shader writes opaque alpha. Blend the selected
            // character around its animated bones independently of that output.
            material.blend_constant = Some([0.3; 4]);
            let blend_bits_mask = 0x07ff_3fff;
            for state in &mut material.state_bits {
                let blend = 14 | (15 << 4) | (1 << 8);
                state[0] = (state[0] & !blend_bits_mask)
                    | blend
                    | (blend << 16)
                    | asset_iw4::GFXS0_ATEST_DISABLE;
                state[1] &= !asset_iw4::GFXS1_DEPTHWRITE;
            }
            material.sort_key = 40;
            material.camera_region = asset_iw4::CAMERA_REGION_LIT_TRANS;
            if let Some(route) = &mut material.route {
                route.primary_sort_key = material.sort_key;
            }
        }
        for binding in &mut material.textures {
            if binding.semantic == 2 {
                binding.image = Some(image_index);
            }
            if let Some((_, image)) = neutral_maps
                .iter()
                .find(|(semantic, _)| *semantic == binding.semantic)
            {
                binding.image = Some(*image);
            }
        }
        catalog.link_material(material);
    }
    diag::info!(
        World,
        "local bot model: installed {} surfaces using {}",
        model.surfaces.len(),
        template.name.as_str()
    );
}
